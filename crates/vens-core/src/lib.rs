#![no_std]

pub mod discovery;
mod identity;
mod wire;

pub use identity::{HostId, Ipv4Address, MacAddress, Port};
