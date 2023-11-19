use std::collections::HashMap;
use std::env;
use std::pin::Pin;

use futures::channel::mpsc;
use futures::{Stream, StreamExt};
use schema::proto::server::server_server::{Server, ServerServer};
use schema::{Speed, StatusUpdate, ToFromProto};
use tonic::transport::Server as TonicServer;
use tonic::{Response, Status};
use ulid::Ulid;

use server::CsvRunner;

// TODO: name server proto something other than "server", as it gets confusing here
#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    env_logger::init();

    let addr = env::var("SERVER_SOCKET")
        .unwrap_or_else(|_| "0.0.0.0:50051".to_string())
        .parse()?;

    let mut runner = CsvRunner::from_csv_paths(
        schema::SAMPLE_DESTINATIONS_CSV_PATH,
        schema::SAMPLE_ORDERS_CSV_PATH,
    )?
    .with_speed(Speed::fast_forward(200).expect("speed")); // run demo in fast-forward
    let subscriptions = HashMap::<Ulid, mpsc::UnboundedSender<StatusUpdate>>::new();
    let updates = runner.stream_updates().expect("update stream");
    let (subscriptions_sender, subscriptions_receiver) = mpsc::unbounded();
    let server = ServerServer::new(ServerService {
        subscriptions_sender,
    });

    #[derive(Debug)]
    enum Event {
        Update(StatusUpdate),
        NewSubscription(Ulid, mpsc::UnboundedSender<StatusUpdate>),
    }

    let updates = updates.map(Event::Update).boxed();
    let new_subscriptions = subscriptions_receiver
        .map(|(ulid, tx)| Event::NewSubscription(ulid, tx))
        .boxed();

    let event_stream = futures::stream::select_all(vec![updates, new_subscriptions]).fuse();
    let stream_process = event_stream
        .scan(subscriptions, |subscriptions, event| {
            log::info!("processing event");
            let fut = match event {
                // Send each update to all of the subscribers
                Event::Update(update) => {
                    let mut disconnected = vec![];
                    for (id, tx) in subscriptions.iter() {
                        match tx.clone().start_send(update.clone()) {
                            Err(e) if e.is_disconnected() => {
                                disconnected.push(*id);
                            }
                            _ => {}
                        }
                    }

                    // Remove any disconnected subscribers
                    for id in disconnected {
                        subscriptions.remove(&id);
                    }

                    futures::future::ready(()) // Leave open the possibility of doing some other async work in response to each event
                }

                // Track any new subscriptions in the map
                Event::NewSubscription(id, tx) => {
                    subscriptions.insert(id, tx);

                    futures::future::ready(())
                }
            };

            futures::future::ready(Some(fut))
        })
        .boxed()
        .buffer_unordered(100) // For if there was other async work to be done
        .collect::<()>();

    log::info!("running server on {}", addr);

    let _ = futures::join!(
        TonicServer::builder().add_service(server).serve(addr),
        stream_process,
        runner.run_with_defaults()
    );

    Ok(())
}

struct ServerService {
    subscriptions_sender: mpsc::UnboundedSender<(Ulid, mpsc::UnboundedSender<StatusUpdate>)>,
}

#[tonic::async_trait]
impl Server for ServerService {
    type MonitorStream =
        Pin<Box<dyn Stream<Item = Result<schema::proto::server::StatusUpdate, Status>> + Send>>;

    async fn monitor(
        &self,
        _request: tonic::Request<()>,
    ) -> Result<Response<Self::MonitorStream>, Status> {
        let subscription_id = Ulid::new();
        log::info!("received monitor request: {}", subscription_id);
        let (tx, rx) = mpsc::unbounded();
        self.subscriptions_sender
            .clone()
            .start_send((subscription_id, tx))
            .map_err(|_| Status::internal("send subscription"))?;

        let resp = rx
            .map(|update| Ok::<schema::proto::server::StatusUpdate, Status>(update.into_proto()))
            .boxed();

        Ok(tonic::Response::new(resp))
    }
}
