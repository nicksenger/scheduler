use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::sink::SinkExt;
use iced::futures::stream::{BoxStream, StreamExt};
use iced::futures::{self, FutureExt};
use iced::subscription::{self, Subscription};
use tonic::transport::Channel;
use tonic::Status;

use schema::proto::server::server_client::{self, ServerClient};
use schema::{StatusUpdate, ToFromProto};

type SchedulerClient = server_client::ServerClient<Channel>;
type UpdatesStream = BoxStream<'static, StatusUpdate>;

const RECONNECT_DELAY: Duration = Duration::from_secs(5);

pub fn connect(server_uri: String) -> Subscription<Event> {
    struct Connect;

    subscription::channel(
        std::any::TypeId::of::<Connect>(),
        100,
        |events| async move {
            let (sender, receiver) = mpsc::channel(100);
            let state = State::Disconnected {
                receiver,
                sender,
                events,
                server_uri,
            };

            futures::stream::unfold(state, |state| async move {
                match state {
                    mut state @ State::Disconnected { .. } => {
                        match ServerClient::connect(state.server_uri().to_string()).await {
                            Ok(client) => {
                                let _ = state
                                    .events()
                                    .send(Event::Connected(Client::Connected {
                                        client: client.clone(),
                                        sender: state.sender(),
                                    }))
                                    .await;

                                Some(((), state.connected()))
                            }
                            Err(e) => {
                                log::warn!("connection failed: {:?}", e);
                                tokio::time::sleep(RECONNECT_DELAY).await;
                                let _ = state.events().send(Event::Disconnected).await;

                                Some(((), state.disconnected()))
                            }
                        }
                    }

                    State::Connected {
                        mut receiver,
                        sender,
                        events,
                        server_uri,
                        ..
                    } => match receiver.next().await {
                        Some(connection) => {
                            log::info!("subscribed");
                            connection
                                .map(|update| {
                                    log::info!("received status update");
                                    let mut events = events.clone();

                                    async move {
                                        let _ = events.send(Event::StatusUpdate(update)).await;
                                    }
                                })
                                .buffered(1)
                                .collect::<()>()
                                .await;

                            log::info!("disconnected");
                            Some((
                                (),
                                State::Disconnected {
                                    receiver,
                                    sender,
                                    events,
                                    server_uri,
                                },
                            ))
                        }

                        None => {
                            log::info!("disconnected");
                            Some((
                                (),
                                State::Disconnected {
                                    receiver,
                                    sender,
                                    events,
                                    server_uri,
                                },
                            ))
                        }
                    },
                }
            })
            .collect::<()>()
            .await;

            unreachable!()
        },
    )
}

enum State {
    Connected {
        receiver: mpsc::Receiver<UpdatesStream>,
        sender: mpsc::Sender<UpdatesStream>,
        events: mpsc::Sender<Event>,
        server_uri: String,
    },
    Disconnected {
        receiver: mpsc::Receiver<UpdatesStream>,
        sender: mpsc::Sender<UpdatesStream>,
        events: mpsc::Sender<Event>,
        server_uri: String,
    },
}

impl State {
    fn connected(self) -> Self {
        match self {
            Self::Disconnected {
                receiver,
                sender,
                events,
                server_uri,
            } => Self::Connected {
                receiver,
                sender,
                events,
                server_uri,
            },
            x => x,
        }
    }

    fn disconnected(self) -> Self {
        match self {
            Self::Connected {
                receiver,
                sender,
                events,
                server_uri,
                ..
            } => Self::Disconnected {
                receiver,
                sender,
                events,
                server_uri,
            },
            x => x,
        }
    }

    fn server_uri(&self) -> &str {
        match self {
            Self::Connected { server_uri, .. } | Self::Disconnected { server_uri, .. } => {
                server_uri.as_str()
            }
        }
    }

    fn events(&mut self) -> mpsc::Sender<Event> {
        match self {
            Self::Connected { events, .. } | Self::Disconnected { events, .. } => events.clone(),
        }
    }

    fn sender(&self) -> mpsc::Sender<UpdatesStream> {
        match self {
            Self::Connected { sender, .. } | Self::Disconnected { sender, .. } => sender.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Connected(Client),
    Disconnected,
    StatusUpdate(StatusUpdate),
}

#[derive(Debug, Clone)]
pub enum Client {
    Pending,
    Connected {
        client: SchedulerClient,
        sender: mpsc::Sender<UpdatesStream>,
    },
}

impl Client {
    pub fn monitor(&self) -> impl futures::Future<Output = Result<(), Status>> {
        log::info!("attempt subscription");
        let Client::Connected { client, sender, .. } = self else {
            log::warn!("no connection");
            return futures::future::ready(Err(Status::unavailable("no connection"))).boxed();
        };
        let mut client = client.clone();
        let mut sender = sender.clone();

        async move {
            match client.monitor(()).await.map(tonic::Response::into_inner) {
                Ok(stream) => match sender
                    .send(
                        stream
                            .filter_map(|proto| async move {
                                proto
                                    .ok()
                                    .and_then(|proto| StatusUpdate::try_from_proto(proto))
                            })
                            .boxed(),
                    )
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        log::warn!("failed to process update stream: {:?}", e);
                        Err(Status::unavailable("failed to process update stream"))
                    }
                },
                Err(status) => {
                    log::warn!("sender error: {:?}", status);
                    Err(status)
                }
            }
        }
        .boxed()
    }
}
