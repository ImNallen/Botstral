#[cfg(all(feature = "nonproduction-test-worker", not(debug_assertions)))]
compile_error!("nonproduction-test-worker is forbidden in release builds");

mod http;
mod local_state;
pub mod manager;
mod owner_http;
mod public_http;
mod worker_http;

pub use http::{serve, serve_with_worker};
pub use local_state::{LocalState, protect_process};
pub use public_http::PublicTls;
pub use worker_http::WorkerTls;
