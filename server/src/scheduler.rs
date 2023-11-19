use std::{
    cmp::{Ordering, Reverse},
    collections::HashMap,
    slice,
};

use itertools::{Either, Itertools};
use once_cell::sync::Lazy;
use schema::{Destination, DestinationName, Flight, Order, Priority, Scheduler};

/// A naive scheduler which sorts the incoming orders by priority
/// and packs them into the available carriers.
/// __WARNING:__ this scheduler uses a naive algorithm which I pretty much made up as I went along.
/// Its packing & scheduling quality, as well as its performance characteristics, are relatively
/// untested, and likely poor.
pub struct NaiveScheduler {
    /// `Destination`s serviced by this `Scheduler`
    destinations: HashMap<DestinationName, Destination>,
    /// Number of carriers controlled by this `Scheduler`
    num_carriers: usize, // TODO: identifier & data for individual carriers
    /// Total number of orders that can be held by carriers controlled by this scheduler
    max_orders_per_carrier: usize,
    /// Speed in meters per second for carriers controlled by this scheduler
    carrier_speed_mps: u64,
    /// Max range in meters that carriers controlled by this scheduler can travel
    carrier_range_m: u64,
    /// Orders that have not yet been fulfilled
    unfulfilled_orders: Vec<Order>,
    /// Orders that are currently in-flight
    active_flights: Vec<Flight>,
}

impl NaiveScheduler {
    /// Number of carriers to keep in reserve for emergency orders
    const NUM_RESERVE_CARRIERS: usize = 2;

    pub fn new(
        destinations: HashMap<DestinationName, Destination>,
        num_carriers: usize,
        max_orders_per_carrier: usize,
        carrier_speed_mps: u64,
        carrier_range_m: u64,
    ) -> Self {
        Self {
            destinations,
            num_carriers,
            max_orders_per_carrier,
            carrier_speed_mps,
            carrier_range_m,
            unfulfilled_orders: Vec::new(),
            active_flights: Vec::new(),
        }
    }

    pub fn active_flights(&self) -> impl Iterator<Item = &Flight> {
        self.active_flights.iter()
    }

    /// Returns the number of carriers available to make deliveries
    fn available_carriers(&self) -> usize {
        self.num_carriers - self.active_flights.len()
    }

    /// Mark as landed those available those carriers which are no longer in flight
    fn process_landings(&mut self, current_time: u64) {
        let active_flights = std::mem::take(&mut self.active_flights);
        let (_finished, still_active): (Vec<Flight>, Vec<Flight>) =
            active_flights.into_iter().partition_map(|flight| {
                use std::cmp::Ordering::*;

                match flight
                    .end_time(&self.destinations, self.carrier_speed_mps)
                    .cmp(&current_time)
                {
                    Less | Equal => Either::Left(flight),
                    Greater => Either::Right(flight),
                }
            });

        self.active_flights = still_active;
    }
}

impl Scheduler for NaiveScheduler {
    type UnfulfilledOrders<'a> = slice::Iter<'a, Order>;
    type LaunchedFlights<'a> = slice::Iter<'a, Flight>;

    fn unfulfilled_orders(&self) -> Self::UnfulfilledOrders<'_> {
        self.unfulfilled_orders.iter()
    }

    fn queue_order(&mut self, order: Order) {
        self.unfulfilled_orders.push(order);
    }

    fn launch_flights(&mut self, current_time: u64) -> slice::Iter<'_, Flight> {
        self.process_landings(current_time);

        #[derive(Debug)]
        struct Bin {
            distance_allocated: u64,
            orders: Vec<Order>,
        }

        // Reserve a certain number of carriers to use for emergency orders
        let mut available_carriers = self.available_carriers();
        if self
            .unfulfilled_orders
            .iter()
            .find(|x| matches!(x.priority, Priority::Emergency))
            .is_none()
        {
            available_carriers = available_carriers.saturating_sub(Self::NUM_RESERVE_CARRIERS);
        }

        let mut bins = (0..available_carriers)
            .map(|_| Bin {
                distance_allocated: 0,
                orders: vec![],
            })
            .collect::<Vec<_>>();

        // Sort the unfilled orders so that any `Emergency` orders are prioritized
        self.unfulfilled_orders
            .sort_unstable_by(|a, b| match (a.priority, b.priority) {
                (Priority::Emergency, Priority::Resupply) => Ordering::Greater,
                (Priority::Resupply, Priority::Emergency) => Ordering::Less,
                // TODO: further sorting by descending distance from origin here should improve packing
                _ => Ordering::Equal,
            });

        // Pack orders into the bins until reaching an order that doesn't fit
        loop {
            let Some(order) = self.unfulfilled_orders.pop() else {
                break;
            };

            let destination = self
                .destinations
                .get(&order.destination)
                .expect("destination");

            // Sort the bins based on the priority of the order
            match order.priority {
                // For emergencies: sort to minimize delivery time (least full first)
                Priority::Emergency => bins.sort_by_key(|bin| bin.distance_allocated),
                // For resupplies: sort to maximize utilization (most full first)
                Priority::Resupply => bins.sort_by_key(|bin| Reverse(bin.orders.len())),
            }
            let Some((bin, distance)) = bins.iter_mut().find_map(|bin| {
                (bin.orders.len() < self.max_orders_per_carrier)
                    .then(|| {
                        let last_stop = bin
                            .orders
                            .last()
                            .and_then(|x| self.destinations.get(&x.destination))
                            .unwrap_or_else(|| Lazy::force(&schema::ORIGIN));

                        let distance = destination.distance_from_other(last_stop) as u64;
                        (distance <= (self.carrier_range_m - bin.distance_allocated))
                            .then(|| (bin, distance))
                    })
                    .flatten()
            }) else {
                break;
            };

            bin.orders.push(order);
            bin.distance_allocated += distance;
        }

        let num_in_flight = self.active_flights.len();

        // Map packed bins to flights and add them to the active list
        self.active_flights
            .extend(bins.into_iter().filter_map(|bin| {
                (bin.distance_allocated > 0).then(|| Flight {
                    launch_time: current_time,
                    orders: bin.orders,
                })
            }));
        self.active_flights[num_in_flight..].iter()
    }
}
