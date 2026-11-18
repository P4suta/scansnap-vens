use std::{fmt::Debug, mem, net::Ipv4Addr};

use vens_core::{
    control::ReserveRefusal,
    job::JobError,
    jpeg::{self, JpegError},
    scan,
    sense::Condition,
    status::PaperPath,
};
use vens_escl::bridge::{Ending, Fault, Refusal, Side};
use vens_net::{
    control::ControlError,
    network::Network,
    operation::{self, JobRun, ReservedScanner},
    scan::{PageSink, ScanError},
    session::{JobFailure, PrepareError},
};

/// What the job thread tells the owner, in order: a refusal, or the reserve, each page, and the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEvent {
    Reserved,
    Abandoned {
        detail: Option<String>,
    },
    Refused {
        refusal: Refusal,
        detail: String,
    },
    Page {
        side: Side,
        page: Vec<u8>,
    },
    Finished {
        ending: Ending,
        detail: Option<String>,
    },
}

/// Carries the events of the job thread to the owner.
pub trait Report {
    fn report(&mut self, event: JobEvent);

    fn client_waiting(&self) -> bool {
        true
    }

    fn created(&mut self) -> bool {
        self.report(JobEvent::Reserved);
        true
    }
}

struct Pages<'a, T> {
    report: &'a mut T,
    image: Vec<u8>,
}

impl<T: Report> PageSink for Pages<'_, T> {
    type Error = JpegError;

    fn append(&mut self, _: u32, _: scan::Side, data: &[u8]) -> Result<(), JpegError> {
        self.image.extend_from_slice(data);
        Ok(())
    }

    fn finish(&mut self, _: u32, side: scan::Side, height: u16) -> Result<(), JpegError> {
        let mut page = mem::take(&mut self.image);
        jpeg::set_height(&mut page, height)?;
        let side = match side {
            scan::Side::Front => Side::Front,
            scan::Side::Back => Side::Back,
        };
        self.report.report(JobEvent::Page { side, page });
        Ok(())
    }
}

fn refuse<T: Report>(report: &mut T, refusal: Refusal, detail: &impl Debug) {
    report.report(JobEvent::Refused {
        refusal,
        detail: format!("{detail:?}"),
    });
}

fn ending<E: Debug>(run: JobRun<E, JpegError>) -> (Ending, Option<String>) {
    let (ending, failure) = match run.outcome {
        Ok(_) => (Ending::Completed, None),
        Err(JobFailure::Release { failure, .. }) => (
            Ending::Completed,
            Some(format!("release failed: {failure:?}")),
        ),
        Err(JobFailure::Scan { failure, release }) => {
            let fault = match failure {
                ScanError::Job(JobError::Condition(Condition::PaperJam)) => Fault::Jam,
                ScanError::Job(JobError::Condition(Condition::DoubleFeed)) => Fault::DoubleFeed,
                ScanError::Channel(_) | ScanError::Sink(_) | ScanError::Job(_) => Fault::Scanner,
            };
            (
                Ending::Failed(fault),
                Some(format!("{failure:?}; release: {release:?}")),
            )
        }
    };
    let keepalive = run
        .keepalive_failure
        .map(|failure| format!("keepalive failed: {failure:?}"));
    let detail = match (failure, keepalive) {
        (Some(failure), Some(keepalive)) => Some(format!("{failure}; {keepalive}")),
        (failure, keepalive) => failure.or(keepalive),
    };
    (ending, detail)
}

/// Runs one job on the scanner at `target`, which may be the broadcast address, and reports each step.
///
/// The job refuses an empty top chute before it reserves the scanner, and it reports a scanner held by another host as busy.
pub fn run_job<N, T>(network: &mut N, target: Ipv4Addr, report: &mut T)
where
    N: Network,
    N::Error: Debug,
    T: Report,
{
    if !report.client_waiting() {
        return report.report(JobEvent::Abandoned { detail: None });
    }
    let mut connected = match operation::open(network, target) {
        Ok(connected) => connected,
        Err(failure) => return refuse(report, Refusal::Unreachable, &failure),
    };
    let prepared = match connected.prepare() {
        Ok(prepared) => prepared,
        Err(PrepareError::NotRegistered) => {
            return refuse(
                report,
                Refusal::Unregistered,
                &PrepareError::<N::Error>::NotRegistered,
            );
        }
        Err(failure) => return refuse(report, Refusal::Unreachable, &failure),
    };
    match prepared.paper() {
        PaperPath::TopChute => {}
        paper @ (PaperPath::FrontSlot | PaperPath::Absent) => {
            return refuse(report, Refusal::NoPaper, &paper);
        }
    }
    if !report.client_waiting() {
        return report.report(JobEvent::Abandoned { detail: None });
    }
    let reserved = match connected.reserve(prepared) {
        Ok(reserved) => reserved,
        Err(ControlError::Reserve(ReserveRefusal::Occupied)) => {
            return refuse(report, Refusal::Busy, &ReserveRefusal::Occupied);
        }
        Err(failure) => return refuse(report, Refusal::Unreachable, &failure),
    };
    scan_reserved(reserved, report);
}

/// Runs an eSCL job on the scanner that the button worker holds.
pub fn run_reserved<N, T>(mut scanner: ReservedScanner<N>, report: &mut T)
where
    N: Network,
    N::Error: Debug,
    T: Report,
{
    match scanner.prepare().map(|prepared| prepared.paper()) {
        Ok(PaperPath::TopChute) => scan_reserved(scanner, report),
        Ok(paper @ (PaperPath::FrontSlot | PaperPath::Absent)) => {
            let release = scanner.release();
            refuse(report, Refusal::NoPaper, &(paper, release));
        }
        Err(failure) => {
            let release = scanner.release();
            refuse(report, Refusal::Unreachable, &(failure, release));
        }
    }
}

fn scan_reserved<N, T>(reserved: ReservedScanner<N>, report: &mut T)
where
    N: Network,
    N::Error: Debug,
    T: Report,
{
    if !report.created() {
        let detail = reserved
            .release()
            .err()
            .map(|error| format!("release failed: {error:?}"));
        return report.report(JobEvent::Abandoned { detail });
    }
    let run = reserved.scan(&mut Pages {
        report: &mut *report,
        image: Vec::new(),
    });
    let (ending, detail) = ending(run);
    report.report(JobEvent::Finished { ending, detail });
}
