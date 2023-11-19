use std::{collections::HashMap, future::Future, pin::Pin};

use futures::{channel::mpsc, Stream};
use schema::{Destination, DestinationName, Order, Runner, Scheduler, Speed, StatusUpdate};

use crate::NaiveScheduler;

type Success = <CsvRunner as Runner<NaiveScheduler>>::Success;
type Error = <CsvRunner as Runner<NaiveScheduler>>::Error;
type Response = Pin<Box<dyn Future<Output = Result<Success, Error>>>>;

// We will emit max 2 updates every second regardless of whether we are fast-forwarding
// TODO: find an appropriate number for this
const MAX_UPDATES_PER_SECOND: u64 = 4;

/// Simulation runner which exercises a `Scheduler` using data provided by a CSV
pub struct CsvRunner {
    speed: Speed,
    destinations: HashMap<DestinationName, Destination>,
    orders: Vec<Order>,
    status_updates_sender: mpsc::UnboundedSender<StatusUpdate>,
    status_updates_receiver: Option<mpsc::UnboundedReceiver<StatusUpdate>>,
}

impl CsvRunner {
    const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

    pub fn from_csv_paths(
        destinations_csv_path: &str,
        orders_csv_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let destinations = Destination::from_csv(destinations_csv_path)?;
        let destinations: HashMap<DestinationName, Destination> = destinations
            .into_iter()
            .map(|dest| (dest.name.clone(), dest))
            .collect();

        let orders = Order::from_csv(orders_csv_path)?;

        let (tx, rx) = mpsc::unbounded();

        Ok(Self {
            speed: Default::default(),
            destinations: destinations.clone(),
            orders,
            status_updates_sender: tx,
            status_updates_receiver: Some(rx),
        })
    }

    /// Run with the provided `Speed`
    pub fn with_speed(mut self, speed: Speed) -> Self {
        self.speed = speed;
        self
    }

    /// Returns a stream of status updates
    /// TODO: refactor runner to manage subscriptions in addition to gateway server
    pub fn stream_updates(&mut self) -> Option<impl Stream<Item = StatusUpdate>> {
        self.status_updates_receiver.take()
    }

    /// Run with the default inputs & carrier parameters
    pub fn run_with_defaults(&self) -> Response {
        let scheduler = NaiveScheduler::new(self.destinations.clone(), 10, 3, 30, 160_000);
        self.run(scheduler)
    }

    async fn run_inner(
        speed: Speed,
        mut updates: mpsc::UnboundedSender<StatusUpdate>,
        mut orders: Vec<Order>,
        mut scheduler: NaiveScheduler,
    ) -> Result<Success, Error> {
        orders.sort_by_key(|order| order.time);
        let first_launch_time = orders
            .first()
            .map(|order| order.time)
            .ok_or_else(|| "No orders".to_string())?;

        let mut orders_iter = orders.into_iter().peekable();

        enum Event {
            Idle(u64),
            Order(Order, u64),
            Launch {
                order: Option<Order>,
                current_time: u64,
            },
        }

        impl Event {
            fn current_time(&self) -> u64 {
                match self {
                    Self::Idle(t)
                    | Self::Order(_, t)
                    | Self::Launch {
                        current_time: t, ..
                    } => *t,
                }
            }
        }

        // Map orders/launches into events happening every second
        let events = (first_launch_time..=Self::SECONDS_PER_DAY).map(|current_time| {
            match (orders_iter.peek(), current_time) {
                // Launch every minute
                (Some(Order { time, .. }), current_time) if current_time % 60 == 0 => {
                    // Launch may occur on the same second as an incoming order
                    Event::Launch {
                        order: (*time == current_time).then(|| orders_iter.next().expect("order")),
                        current_time,
                    }
                }
                (_, current_time) if current_time % 60 == 0 => Event::Launch {
                    order: None,
                    current_time,
                },

                // Queue orders at the appropriate time
                (Some(Order { time, .. }), _) if *time == current_time => {
                    Event::Order(orders_iter.next().expect("order"), current_time)
                }

                // Otherwise just idling until the next second
                _ => Event::Idle(current_time),
            }
        });

        let adjusted_sleep_duration = speed.adjust_duration(std::time::Duration::from_secs(1));
        let update_interval_seconds = match speed {
            Speed::FastForward(factor) => factor.get() as u64 / MAX_UPDATES_PER_SECOND,
            _ => 1,
        };

        for event in events {
            let current_time = event.current_time();

            match event {
                Event::Launch {
                    order,
                    current_time,
                } => {
                    if let Some(order) = order {
                        scheduler.queue_order(order);
                    }

                    let _launched = scheduler.launch_flights(current_time).collect::<Vec<_>>();
                }

                Event::Order(order, _) => {
                    scheduler.queue_order(order);
                }

                Event::Idle(_) => {}
            }

            if current_time % update_interval_seconds == 0 {
                log::info!("sending update to channel");
                let _ = updates.start_send(StatusUpdate {
                    time: current_time,
                    flights: scheduler.active_flights().cloned().collect(),
                    speed,
                });
            }

            tokio::time::sleep(adjusted_sleep_duration).await;
        }

        Ok(scheduler.unfulfilled_orders().count())
    }
}

impl Runner<NaiveScheduler> for CsvRunner {
    type Response = Response;
    /// Number of undelivered packages
    type Success = usize;
    /// Description of what went wrong
    type Error = String;

    fn run(&self, scheduler: NaiveScheduler) -> Self::Response {
        let orders = self.orders.clone();
        let speed = self.speed;
        let updates = self.status_updates_sender.clone();
        Box::pin(async move { Self::run_inner(speed, updates, orders, scheduler).await })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const DEST_PATH: &'static str = "../test_data/destinations.csv";
    const ORDER_PATH: &'static str = "../test_data/orders.csv";

    #[tokio::test(start_paused = true)]
    async fn test_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let runner = CsvRunner::from_csv_paths(DEST_PATH, ORDER_PATH)?;
        let unfulfilled_orders = runner.run_with_defaults().await?;

        assert_eq!(unfulfilled_orders, 0);

        Ok(())
    }
}
