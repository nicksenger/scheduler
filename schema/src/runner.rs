use std::num::NonZeroU8;
use std::time::Duration;

use futures::Future;

use crate::Scheduler;

/// A `Runner` dispatches carriers to fulfill orders using a provided `Scheduler`.
/// It returns a `Response` future, which may be polled to drive its operation
/// until completion of all deliveries.
pub trait Runner<S: Scheduler> {
    type Response: Future<Output = Result<Self::Success, Self::Error>>;
    type Success;
    type Error;

    /// Initialize the `Runner` to fulfill orders using the provided `Scheduler`.
    fn run(&self, scheduler: S) -> Self::Response;
}

/// Allows running in fast-forward or slow-motion instead of real-time
#[derive(Default, Clone, Copy, Debug)]
pub enum Speed {
    #[default]
    RealTime,
    /// Speed up the runner by the provided multiplier (e.g. `2` gives double speed)
    FastForward(NonZeroU8),
    /// Slow down the runner by the provided multiplier (e.g. `2` gives half speed)
    #[allow(unused)]
    SlowMotion(NonZeroU8),
}

impl Speed {
    pub fn fast_forward(rate: u8) -> Option<Self> {
        NonZeroU8::new(rate).map(Self::FastForward)
    }

    pub fn adjust_duration(&self, duration: Duration) -> Duration {
        match self {
            Self::RealTime => duration,
            Self::FastForward(x) => duration / x.get() as u32,
            Self::SlowMotion(x) => duration * x.get() as u32,
        }
    }

    pub(crate) fn to_i32(&self) -> i32 {
        match self {
            Self::RealTime => 0,
            Self::FastForward(x) => x.get() as i32,
            Self::SlowMotion(x) => -1 * x.get() as i32,
        }
    }

    pub(crate) fn from_i32(n: i32) -> Self {
        match n {
            n if n == 0 => Self::RealTime,
            n if n > 0 => Self::FastForward(NonZeroU8::new(n as u8).expect("speed")),
            _ => Self::SlowMotion(NonZeroU8::new(n as u8).expect("speed")),
        }
    }
}
