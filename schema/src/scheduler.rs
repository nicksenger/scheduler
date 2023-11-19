use crate::{Flight, Order};

/// A flight scheduler for processing incoming orders
pub trait Scheduler {
    /// Pending orders queued for processing by the scheduler
    type UnfulfilledOrders<'a>: Iterator<Item = &'a Order>
    where
        Self: 'a;
    /// Carriers which have been launched by this scheduler
    type LaunchedFlights<'a>: Iterator<Item = &'a Flight>
    where
        Self: 'a;

    /// Returns a list of any orders queued for processing by this scheduler,
    /// but which have not yet been fulfilled.
    fn unfulfilled_orders<'a>(&'a self) -> Self::UnfulfilledOrders<'a>;

    /// Schedule an order to be delivered by a carrier controlled by this scheduler
    fn queue_order(&mut self, order: Order);

    /// Return a list of all flights that should be launched at the given time
    fn launch_flights<'a>(&'a mut self, current_time: u64) -> Self::LaunchedFlights<'a>;
}
