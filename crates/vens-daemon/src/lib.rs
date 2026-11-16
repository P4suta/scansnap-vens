//! The `vensd` bridge: offer the scanner to eSCL clients over HTTP and run each job on the scanner.

pub mod framing;
pub mod http;
pub mod job;
pub mod request;
