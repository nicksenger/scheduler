use prost::Message as ProstMessage;

mod entities;
mod runner;
mod scheduler;

pub mod proto {
    pub mod server;
}

pub use entities::{Destination, DestinationName, Flight, Order, Priority, ORIGIN};
pub use runner::{Runner, Speed};
pub use scheduler::Scheduler;

pub const SAMPLE_DESTINATIONS_CSV_PATH: &'static str = "./test_data/destinations.csv";
pub const SAMPLE_ORDERS_CSV_PATH: &'static str = "./test_data/orders.csv";

pub trait ToFromProto<Proto>: Sized
where
    Proto: ProstMessage,
{
    fn try_from_proto(proto: Proto) -> Option<Self>;
    fn into_proto(self) -> Proto;
}

impl<T, Proto> ToFromProto<Proto> for T
where
    Proto: ProstMessage,
    T: TryFrom<Proto> + Into<Proto>,
{
    fn try_from_proto(proto: Proto) -> Option<Self> {
        proto.try_into().ok()
    }

    fn into_proto(self) -> Proto {
        self.into()
    }
}

#[derive(Clone, Debug)]
pub struct StatusUpdate {
    pub time: u64,
    pub flights: Vec<Flight>,
    pub speed: runner::Speed,
}

impl ToFromProto<proto::server::StatusUpdate> for StatusUpdate {
    fn into_proto(self) -> proto::server::StatusUpdate {
        proto::server::StatusUpdate {
            time: self.time as i64,
            flights: self.flights.into_iter().map(Flight::into_proto).collect(),
            speed: self.speed.to_i32(),
        }
    }

    fn try_from_proto(message: proto::server::StatusUpdate) -> Option<Self> {
        Some(Self {
            time: message.time as u64,
            flights: message
                .flights
                .into_iter()
                .filter_map(|flight| Flight::try_from_proto(flight))
                .collect(),
            speed: runner::Speed::from_i32(message.speed),
        })
    }
}

impl ToFromProto<proto::server::Flight> for Flight {
    fn into_proto(self) -> proto::server::Flight {
        proto::server::Flight {
            launch_time: self.launch_time as i64,
            orders: self.orders.into_iter().map(Order::into_proto).collect(),
        }
    }

    fn try_from_proto(message: proto::server::Flight) -> Option<Self> {
        Some(Self {
            launch_time: message.launch_time as u64,
            orders: message
                .orders
                .into_iter()
                .filter_map(|order| Order::try_from_proto(order))
                .collect(),
        })
    }
}

impl ToFromProto<proto::server::Order> for Order {
    fn into_proto(self) -> proto::server::Order {
        proto::server::Order {
            time: self.time as i64,
            destination: self.destination.to_string(),
            priority: match self.priority {
                Priority::Emergency => proto::server::Priority::Emergency.into(),
                Priority::Resupply => proto::server::Priority::Resupply.into(),
            },
        }
    }

    fn try_from_proto(message: proto::server::Order) -> Option<Self> {
        Some(Self {
            time: message.time as u64,
            destination: DestinationName::from_str(&message.destination),
            priority: match message.priority() {
                proto::server::Priority::Emergency => Priority::Emergency,
                proto::server::Priority::Resupply => Priority::Resupply,
            },
        })
    }
}
