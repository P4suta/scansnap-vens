use std::{net::SocketAddrV4, num::NonZeroU8};

use vens_core::discovery::{Probe, SearchReply};

/// Sends each probe three times, as section 5.1 of the protocol document does.
pub const DOCUMENTED_ATTEMPTS: NonZeroU8 = NonZeroU8::MIN.saturating_add(2);
const DATAGRAMS_PER_ATTEMPT: u8 = 16;
const DATAGRAM_CAPACITY: usize = 512;

pub trait Datagram {
    type Error;

    /// # Errors
    ///
    /// Reports the port's own failure to send.
    fn send(&mut self, frame: &[u8], target: SocketAddrV4) -> Result<(), Self::Error>;

    /// Returns `None` when no datagram arrives before the port's deadline.
    ///
    /// # Errors
    ///
    /// Reports the port's own failure to receive.
    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchError<E> {
    Port(E),
    NoReply { attempts: u8 },
}

fn listen<D: Datagram>(port: &mut D) -> Result<Option<SearchReply>, D::Error> {
    let mut buffer = [0; DATAGRAM_CAPACITY];
    for _ in 0..DATAGRAMS_PER_ATTEMPT {
        let Some(length) = port.receive(&mut buffer)? else {
            return Ok(None);
        };
        if let Some(Ok(reply)) = buffer.get(..length).map(SearchReply::decode) {
            return Ok(Some(reply));
        }
    }
    Ok(None)
}

/// Sends the probe up to `attempts` times and returns the first valid search reply.
///
/// # Errors
///
/// Fails when the port fails or when no valid reply follows any of the probes.
pub fn search<D: Datagram>(
    port: &mut D,
    probe: Probe,
    target: SocketAddrV4,
    attempts: NonZeroU8,
) -> Result<SearchReply, SearchError<D::Error>> {
    let frame = probe.encode();
    for _ in 0..attempts.get() {
        port.send(&frame, target).map_err(SearchError::Port)?;
        if let Some(reply) = listen(port).map_err(SearchError::Port)? {
            return Ok(reply);
        }
    }
    Err(SearchError::NoReply {
        attempts: attempts.get(),
    })
}
