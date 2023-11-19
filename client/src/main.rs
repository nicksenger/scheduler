use std::collections::HashMap;
use std::env;
use std::time::Duration;

use iced::executor;
use iced::widget::{column, container, text};
use iced::{theme, Application, Command, Element, Length, Settings, Theme};
use schema::{Destination, DestinationName, Speed, StatusUpdate};

mod client;
mod map;
use client::Client;

const CLIENT_FRAME_RATE: u64 = 20;

pub fn main() -> iced::Result {
    dotenv::dotenv().ok();
    env_logger::init();

    let gateway_uri = env::var("SERVER_URI").unwrap_or("http://localhost:50051".to_string());

    Gui::run(Settings {
        flags: gateway_uri,
        ..Default::default()
    })
}

struct Gui {
    gateway_uri: String,
    client: Client,
    destinations: HashMap<DestinationName, Destination>,
    latest_update: Option<StatusUpdate>,
    perceived_time_millis: u64,
    is_monitoring: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    StatusUpdate(StatusUpdate),
    IncrementPerceivedTime,
    MonitorRequestSuccess,
    MonitorRequestFailed,
    Connected(Client),
    Disconnected,
}

impl Application for Gui {
    type Message = Message;
    type Theme = Theme;
    type Executor = executor::Default;
    type Flags = String;

    fn new(gateway_uri: String) -> (Gui, Command<Message>) {
        (
            Gui {
                gateway_uri,
                client: Client::Pending,
                destinations: Destination::from_csv(schema::SAMPLE_DESTINATIONS_CSV_PATH)
                    .expect("destinations")
                    .into_iter()
                    .map(|d| (d.name.clone(), d))
                    .collect(),
                latest_update: None,
                perceived_time_millis: 0,
                is_monitoring: false,
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("Scheduler - Monitoring Client")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::StatusUpdate(update) => {
                self.perceived_time_millis = update.time * 1000;
                self.latest_update = Some(update);

                Command::none()
            }

            Message::IncrementPerceivedTime => {
                if let Some(update) = self.latest_update.as_ref() {
                    self.perceived_time_millis += match update.speed {
                        Speed::RealTime => 50,
                        Speed::FastForward(n) => n.get() as u64 * 50,
                        Speed::SlowMotion(n) => 50 / (n.get() as u64),
                    }
                }

                Command::none()
            }

            Message::MonitorRequestSuccess => {
                self.is_monitoring = true;

                Command::none()
            }

            Message::MonitorRequestFailed => {
                if matches!(&self.client, Client::Connected { .. }) {
                    let monitor_fut = self.client.monitor();
                    Command::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            monitor_fut.await
                        },
                        |res| match res {
                            Ok(_) => Message::MonitorRequestSuccess,
                            Err(_) => Message::MonitorRequestFailed,
                        },
                    )
                } else {
                    Command::none()
                }
            }

            Message::Connected(client) => {
                log::info!("client connected");
                self.client = client;

                Command::perform(self.client.monitor(), |res| match res {
                    Ok(_) => Message::MonitorRequestSuccess,
                    Err(_) => Message::MonitorRequestFailed,
                })
            }

            Message::Disconnected => {
                log::info!("client disconnected");
                self.client = Client::Pending;

                Command::none()
            }
        }
    }

    fn view(&self) -> Element<Message> {
        let content: Element<Message> = match &self.latest_update {
            Some(update) => map::view(&self.destinations, update, self.perceived_time_millis),
            None => text("Waiting for update…").into(),
        };
        let with_connection_status: Element<Message> = match &self.client {
            Client::Pending => text("Client disconnected, attempting to connect…").into(),
            Client::Connected { .. } => column![
                text("Connected to server"),
                container(content).padding(20).style(theme::Container::Box)
            ]
            .align_items(iced::Alignment::Center)
            .into(),
        };

        container(with_connection_status)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::Subscription::batch(vec![
            client::connect(self.gateway_uri.to_string()).map(Into::into),
            iced::time::every(Duration::from_millis(
                1000 / (CLIENT_FRAME_RATE - (CLIENT_FRAME_RATE / 10)),
            ))
            .map(|_| Message::IncrementPerceivedTime),
        ])
    }
}

impl From<client::Event> for Message {
    fn from(event: client::Event) -> Self {
        match event {
            client::Event::Connected(sender) => Self::Connected(sender),
            client::Event::Disconnected => Self::Disconnected,
            client::Event::StatusUpdate(update) => Self::StatusUpdate(update),
        }
    }
}
