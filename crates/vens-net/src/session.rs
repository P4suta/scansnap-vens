use vens_core::{
    HostId, Ipv4Address, MacAddress, Port,
    command::ScsiStatus,
    control::{LocalTime, Release, Reserve, ReserveMode, UtcOffset},
    job::Summary,
    scan::Command,
    status::{HardwareStatus, StatusError},
};

use crate::{
    channel::{ChannelError, CommandChannel, Stream},
    control::{ControlChannel, ControlError},
    scan::{Clock, PageSink, ScanError, run},
};

const STATUS_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    pub mac: MacAddress,
    pub address: Ipv4Address,
    pub trigger_port: Port,
    pub time: LocalTime,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SessionError<P, K> {
    Command(ChannelError<P>),
    StatusRefused(ScsiStatus),
    Status(StatusError),
    NotRegistered,
    Reserve(ControlError<P>),
    Scan {
        failure: ScanError<P, K>,
        release: Result<(), ControlError<P>>,
    },
    Release {
        summary: Summary,
        failure: ControlError<P>,
    },
}

fn registered_host<S: Stream, K>(
    command: &mut CommandChannel<S>,
) -> Result<HostId, SessionError<S::Error, K>> {
    let mut buffer = [0; STATUS_CAPACITY];
    let reply = command
        .execute(&Command::hardware_status(), &mut buffer)
        .map_err(SessionError::Command)?;
    if reply.status != ScsiStatus::Good {
        return Err(SessionError::StatusRefused(reply.status));
    }
    HardwareStatus::decode(reply.data)
        .map_err(SessionError::Status)?
        .registered_host()
        .ok_or(SessionError::NotRegistered)
}

/// Reserves the scanner with its registered host identifier, runs one job, and releases the scanner.
///
/// # Errors
///
/// Fails when the identifier can't be read, the reserve is refused, the job fails, or the release is refused.
/// A job failure carries the release outcome, and a release failure after a good job carries the job summary.
pub fn scan_once<S, T, C, K>(
    command: &mut CommandChannel<S>,
    control: &mut ControlChannel<T>,
    host: Host,
    clock: &mut C,
    sink: &mut K,
    buffer: &mut [u8],
) -> Result<Summary, SessionError<S::Error, K::Error>>
where
    S: Stream,
    T: Stream<Error = S::Error>,
    C: Clock,
    K: PageSink,
{
    let host_id = registered_host(command)?;
    control
        .reserve(Reserve {
            mode: ReserveMode::Scan,
            mac: host.mac,
            host: host.address,
            trigger_port: host.trigger_port,
            time: host.time,
            host_id,
            offset: UtcOffset::from_seconds(0),
        })
        .map_err(SessionError::Reserve)?;
    let scanned = run(command, clock, sink, buffer);
    let released = control.release(Release::new(host.mac));
    match (scanned, released) {
        (Ok(summary), Ok(())) => Ok(summary),
        (Ok(summary), Err(failure)) => Err(SessionError::Release { summary, failure }),
        (Err(failure), release) => Err(SessionError::Scan { failure, release }),
    }
}
