#[cfg(all(feature = "nonproduction-test-worker", not(debug_assertions)))]
compile_error!("nonproduction-test-worker is forbidden in release builds");

mod config;
mod session;
#[cfg(feature = "nonproduction-test-worker")]
mod test_worker;
pub mod transport;

pub use config::{Config, entry};
