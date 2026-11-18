//! The `vensd` bridge: offer the scanner to eSCL clients over HTTP and run each job on the scanner.

pub mod advertise;
mod button;
pub mod capture;
pub mod framing;
pub mod http;
pub mod invocation;
pub mod job;
pub mod request;
pub mod server;
