use std::collections::HashMap;

use once_cell::sync::Lazy;

pub static ORIGIN: Lazy<Destination> = Lazy::new(|| Destination {
    name: DestinationName("ORIGIN".to_string()),
    north_m: 0,
    east_m: 0,
});

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Emergency,
    #[default]
    Resupply,
}

impl<'a> TryFrom<&'a str> for Priority {
    type Error = String;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        match s {
            s if s == "Emergency" => Ok(Self::Emergency),
            s if s == "Resupply" => Ok(Self::Resupply),
            _ => Err("invalid priority".to_string()),
        }
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DestinationName(String);

impl DestinationName {
    pub fn from_str(s: &str) -> Self {
        Self(s.to_string())
    }

    pub fn to_string(&self) -> String {
        self.0.clone()
    }
}

/// A `Destination` to which carriers will deliver orders
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    /// The name of the destination
    pub name: DestinationName,
    /// Destination's y-offset from the origin/nest in meters
    pub north_m: i64,
    /// Destination's x-offset from the origin/nest in meters
    pub east_m: i64,
}

impl Destination {
    pub fn from_csv(path: &str) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        let csv_bytes = std::fs::read(path)?;
        let mut destinations = vec![];

        for line in String::from_utf8(csv_bytes)?.lines() {
            let values = line.split(", ").collect::<Vec<_>>();
            destinations.push(Self {
                name: DestinationName(values[0].to_string()),
                north_m: values[1].parse::<i64>()?,
                east_m: values[2].parse::<i64>()?,
            });
        }

        Ok(destinations)
    }

    /// Returns the destination's distance from somewhere else in meters
    fn distance_from(&self, other_north: i64, other_east: i64) -> f32 {
        // TODO: in real-world applications the precision may become important here,
        // we'd probably want to use a decimal type for speeds, distances, etc
        (((self.north_m.abs() - other_north.abs()).pow(2)
            + (self.east_m.abs() - other_east.abs()).pow(2)) as f32)
            .sqrt()
    }

    /// Returns the destination's distance from another destination in meters
    pub fn distance_from_other(&self, other: &Self) -> f32 {
        self.distance_from(other.north_m, other.east_m)
    }

    /// Returns the destination's distance from the origin in meters
    pub fn distance_from_origin(&self) -> f32 {
        self.distance_from_other(&ORIGIN)
    }
}

/// An `Order` is a request for delivery of _something_ to a particular `Destination`
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Order {
    /// Time in __seconds__ _since midnight_ that the order was placed
    pub time: u64,
    /// Unique-ish identifier for the destination
    pub destination: DestinationName,
    /// Priority of the order, used by scheduling logic
    pub priority: Priority,
}

impl Order {
    pub fn from_csv(path: &str) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        let csv_bytes = std::fs::read(path)?;
        let mut orders = vec![];

        for line in String::from_utf8(csv_bytes)?.lines() {
            let values = line.split(", ").collect::<Vec<_>>();
            orders.push(Self {
                time: values[0].parse::<u64>()?,
                destination: DestinationName(values[1].to_string()),
                priority: values[2].try_into()?,
            });
        }

        Ok(orders)
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Flight {
    /// Time in __seconds__ _since midnight_ that the flight was launched
    pub launch_time: u64,
    /// Orders carried by the flight
    pub orders: Vec<Order>,
}

impl Flight {
    /// Returns the total distance that will be traveled by the flight
    fn total_distance(&self, destinations: &HashMap<DestinationName, Destination>) -> f32 {
        self.orders
            .iter()
            .map(|order| destinations.get(&order.destination).expect("destination"))
            .chain(std::iter::once(Lazy::force(&ORIGIN)))
            .fold((0.0, Lazy::force(&ORIGIN)), |(traveled, prev), cur| {
                (traveled + cur.distance_from_other(prev), cur)
            })
            .0
    }

    /// Returns current east/north pos & orders based on the time since launch (x, y, order_num)
    /// TODO: Make a proper `Point` type
    pub fn current_position(
        &self,
        destinations: &HashMap<DestinationName, Destination>,
        current_time: u64,
        speed_mps: u64,
    ) -> (f32, f32, usize) {
        let seconds = current_time - self.launch_time;

        let total_distance_traveled = seconds * speed_mps;
        let mut distance = total_distance_traveled;
        let mut prev = Lazy::force(&ORIGIN);
        for (i, dest) in self
            .orders
            .iter()
            .map(|order| destinations.get(&order.destination).expect("destination"))
            .chain(std::iter::once(Lazy::force(&ORIGIN)))
            .enumerate()
        {
            let dist_between = dest.distance_from_other(prev) as u64;

            match distance.saturating_sub(dist_between) {
                d if d == 0 => {
                    // Point is on this path
                    let f = distance as f32 / dist_between as f32;
                    let north_comp = dest.north_m - prev.north_m;
                    let east_comp = dest.east_m - prev.east_m;

                    return (
                        (east_comp as f32 * f) + prev.east_m as f32,
                        (north_comp as f32 * f) + prev.north_m as f32,
                        self.orders.len() - i,
                    );
                }
                d => {
                    distance = d;
                }
            }

            prev = dest;
        }

        (0.0, 0.0, self.orders.len())
    }

    /// Returns the time that the flight will arrive back at the origin
    pub fn end_time(
        &self,
        destinations: &HashMap<DestinationName, Destination>,
        speed_mps: u64,
    ) -> u64 {
        self.launch_time + self.total_distance(destinations) as u64 / speed_mps
    }
}
