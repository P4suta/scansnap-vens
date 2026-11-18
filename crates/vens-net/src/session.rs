use vens_core::{
    HostId, Ipv4Address, MacAddress, Port,
    command::ScsiStatus,
    control::{LocalTime, Release, Reserve, ReserveMode, UtcOffset},
    job::Summary,
    scan::Command,
    status::{HardwareStatus, PaperPath, StatusError},
};

use crate::{
    channel::{ChannelError, CommandChannel, Stream},
    control::{ControlChannel, ControlError},
    keepalive::Keepalive,
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

/// The registered host identifier and paper position that a status read finds before a reserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prepared {
    host_id: HostId,
    paper: PaperPath,
}

impl Prepared {
    #[must_use]
    pub const fn host_id(&self) -> HostId {
        self.host_id
    }

    #[must_use]
    pub const fn paper(&self) -> PaperPath {
        self.paper
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PrepareError<P> {
    Command(ChannelError<P>),
    StatusRefused(ScsiStatus),
    Status(StatusError),
    NotRegistered,
}

/// The scanner held for this host until `scan` releases it.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "the scanner stays reserved until `scan` releases it"]
pub struct Reserved {
    mac: MacAddress,
}

#[derive(Debug, PartialEq, Eq)]
pub enum JobFailure<P, K> {
    Scan {
        failure: ScanError<P, K>,
        release: Result<(), ControlError<P>>,
    },
    Release {
        summary: Summary,
        failure: ControlError<P>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum SessionError<P, K> {
    Prepare(PrepareError<P>),
    Reserve(ControlError<P>),
    Job(JobFailure<P, K>),
}

/// Reads the registered host identifier and the paper position without a reserve.
///
/// # Errors
///
/// Fails when the status read fails, is refused, or is malformed, or when no host is registered.
pub fn prepare<S: Stream>(
    command: &mut CommandChannel<S>,
) -> Result<Prepared, PrepareError<S::Error>> {
    let mut buffer = [0; STATUS_CAPACITY];
    let reply = command
        .execute(&Command::hardware_status(), &mut buffer)
        .map_err(PrepareError::Command)?;
    if reply.status != ScsiStatus::Good {
        return Err(PrepareError::StatusRefused(reply.status));
    }
    let status = HardwareStatus::decode(reply.data).map_err(PrepareError::Status)?;
    let host_id = status
        .registered_host()
        .ok_or(PrepareError::NotRegistered)?;
    Ok(Prepared {
        host_id,
        paper: status.paper(),
    })
}

/// Reserves the scanner with the identifier that `prepare` read.
///
/// # Errors
///
/// Fails when the control channel fails or the scanner refuses the reserve.
pub fn reserve<T: Stream>(
    control: &mut ControlChannel<T>,
    host: Host,
    prepared: Prepared,
) -> Result<Reserved, ControlError<T::Error>> {
    control.reserve(Reserve {
        mode: ReserveMode::Scan,
        mac: host.mac,
        host: host.address,
        trigger_port: host.trigger_port,
        time: host.time,
        host_id: prepared.host_id,
        offset: UtcOffset::from_seconds(0),
    })?;
    Ok(Reserved { mac: host.mac })
}

impl Reserved {
    /// Releases the scanner without starting a job.
    ///
    /// # Errors
    ///
    /// Fails when the control channel fails or the scanner refuses the release.
    pub fn release<T: Stream>(
        self,
        control: &mut ControlChannel<T>,
    ) -> Result<(), ControlError<T::Error>> {
        control.release(Release::new(self.mac))
    }

    /// Runs one job and releases the scanner after it, whatever the job's outcome.
    ///
    /// # Errors
    ///
    /// A job failure carries the release outcome, and a release failure after a good job carries the job summary.
    pub fn scan<S, T, A, C, K>(
        self,
        command: &mut CommandChannel<S>,
        control: &mut ControlChannel<T>,
        keepalive: &mut A,
        clock: &mut C,
        sink: &mut K,
        buffer: &mut [u8],
    ) -> Result<Summary, JobFailure<S::Error, K::Error>>
    where
        S: Stream,
        T: Stream<Error = S::Error>,
        A: Keepalive,
        C: Clock,
        K: PageSink,
    {
        let scanned = run(command, clock, keepalive, sink, buffer);
        let released = self.release(control);
        match (scanned, released) {
            (Ok(summary), Ok(())) => Ok(summary),
            (Ok(summary), Err(failure)) => Err(JobFailure::Release { summary, failure }),
            (Err(failure), release) => Err(JobFailure::Scan { failure, release }),
        }
    }
}

/// Reserves the scanner with its registered host identifier, runs one job, and releases the scanner.
///
/// # Errors
///
/// Fails when the identifier can't be read, the reserve is refused, the job fails, or the release is refused.
pub fn scan_once<S, T, A, C, K>(
    command: &mut CommandChannel<S>,
    control: &mut ControlChannel<T>,
    keepalive: &mut A,
    host: Host,
    clock: &mut C,
    sink: &mut K,
    buffer: &mut [u8],
) -> Result<Summary, SessionError<S::Error, K::Error>>
where
    S: Stream,
    T: Stream<Error = S::Error>,
    A: Keepalive,
    C: Clock,
    K: PageSink,
{
    let prepared = prepare(command).map_err(SessionError::Prepare)?;
    let reserved = reserve(control, host, prepared).map_err(SessionError::Reserve)?;
    reserved
        .scan(command, control, keepalive, clock, sink, buffer)
        .map_err(SessionError::Job)
}
