#[cfg(all(feature = "nonproduction-test-worker", not(debug_assertions)))]
compile_error!("nonproduction-test-worker is forbidden in release builds");

mod computer;
mod config;
#[cfg(feature = "nonproduction-test-worker")]
mod effect_provider;
mod journal;
pub mod maintenance;
mod process;
mod session;
#[cfg(feature = "nonproduction-test-worker")]
mod test_worker;
pub mod transport;
mod vibe;
mod worker;

pub use config::{Config, entry};
