use std::{
    net::Ipv4Addr,
    path::Path,
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::Duration,
};

use vens_core::{control::ReserveRefusal, trigger::RecentPresses};
use vens_escl::bridge::Refusal;
use vens_net::{
    control::ControlError,
    host::SystemNetwork,
    operation::{self, ReservedScanner, Trigger},
    session::{JobFailure, PrepareError},
};

use crate::{
    capture::Pdf,
    job::{CREATION_HEADROOM_MILLIS, JobEvent, Report, run_reserved},
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

type Scanner = ReservedScanner<SystemNetwork>;

fn acquire(network: &mut SystemNetwork, target: Ipv4Addr) -> Result<Scanner, (Refusal, String)> {
    let unavailable = |detail: String| (Refusal::Unreachable, detail);
    let mut connected =
        operation::open(network, target).map_err(|error| unavailable(format!("{error:?}")))?;
    let prepared = connected.prepare().map_err(|error| {
        let refusal = match error {
            PrepareError::NotRegistered => Refusal::Unregistered,
            PrepareError::Command(_) | PrepareError::StatusRefused(_) | PrepareError::Status(_) => {
                Refusal::Unreachable
            }
        };
        (refusal, format!("{error:?}"))
    })?;
    connected.reserve(prepared).map_err(|error| {
        let refusal = match error {
            ControlError::Reserve(ReserveRefusal::Occupied) => Refusal::Busy,
            ControlError::Port(_)
            | ControlError::Greeting(_)
            | ControlError::Frame(_)
            | ControlError::Reserve(_)
            | ControlError::Release(_) => Refusal::Unreachable,
        };
        (refusal, format!("{error:?}"))
    })
}

enum Work<T> {
    Pressed,
    Escl(T),
    Reconnect(Option<T>),
    Stop,
}

fn await_work<T: Report>(
    scanner: &mut Scanner,
    recent: &mut RecentPresses,
    jobs: &Receiver<T>,
    log: &impl Fn(&str),
) -> Work<T> {
    loop {
        match scanner.poll_trigger(recent) {
            Ok(Trigger::Waiting) => {}
            Ok(Trigger::Pressed) => return Work::Pressed,
            Ok(Trigger::PoweredOff) => {
                log("the scanner is turning off");
                return Work::Reconnect(None);
            }
            Ok(Trigger::ReserveExpired) => {
                log("the scanner stopped answering keepalive probes");
                return Work::Reconnect(None);
            }
            Err(error) => {
                log(&format!("the button socket failed: {error}"));
                return Work::Reconnect(None);
            }
        }
        match jobs.recv_timeout(POLL_INTERVAL) {
            Ok(mut report) => {
                if report.client_waiting() {
                    return if scanner.renew_for(CREATION_HEADROOM_MILLIS) {
                        Work::Escl(report)
                    } else {
                        Work::Reconnect(Some(report))
                    };
                }
                report.report(JobEvent::Abandoned { detail: None });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Work::Stop,
        }
    }
}

fn acquire_for_job<T: Report>(network: &mut SystemNetwork, target: Ipv4Addr, mut report: T) {
    if !report.client_waiting() {
        return report.report(JobEvent::Abandoned { detail: None });
    }
    match acquire(network, target) {
        Ok(scanner) => run_reserved(scanner, &mut report),
        Err((refusal, detail)) => report.report(JobEvent::Refused { refusal, detail }),
    }
}

fn release(scanner: Scanner, log: &impl Fn(&str)) {
    if let Err(error) = scanner.release() {
        log(&format!("release after listening failed: {error:?}"));
    }
}

fn save(scanner: Scanner, folder: &Path, log: &impl Fn(&str)) {
    log("Scan button pressed");
    let mut pdf = Pdf::default();
    let run = scanner.scan(&mut pdf);
    if let Some(error) = run.keepalive_failure {
        log(&format!("keepalive failed: {error}"));
    }
    match run.outcome {
        Ok(summary) if summary.sheets == 0 => return log("the scanner fed no sheet"),
        Ok(_) => {}
        Err(JobFailure::Release { failure, .. }) => {
            log(&format!(
                "release after the button scan failed: {failure:?}"
            ));
        }
        Err(error @ JobFailure::Scan { .. }) => {
            return log(&format!("the button scan failed: {error:?}"));
        }
    }
    match pdf.publish(folder) {
        Ok(path) => log(&format!("saved {}", path.display())),
        Err(error) => log(&format!(
            "the PDF couldn't be saved in {}: {error}",
            folder.display()
        )),
    }
}

pub(crate) fn run<T: Report>(
    mut network: SystemNetwork,
    target: Ipv4Addr,
    folder: &Path,
    jobs: &Receiver<T>,
    log: impl Fn(&str),
) {
    let mut recent = RecentPresses::default();
    let mut selected = None;
    loop {
        let mut scanner = match acquire(&mut network, target) {
            Ok(scanner) => scanner,
            Err((_, detail)) => {
                log(&format!("can't wait for the Scan button: {detail}"));
                match jobs.recv_timeout(RETRY_INTERVAL) {
                    Ok(report) => acquire_for_job(&mut network, target, report),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                continue;
            }
        };
        let device = Some(scanner.scanner_mac());
        if selected != device {
            recent = RecentPresses::default();
            selected = device;
        }
        log("waiting for the Scan button");
        match await_work(&mut scanner, &mut recent, jobs, &log) {
            Work::Pressed => save(scanner, folder, &log),
            Work::Escl(mut report) => run_reserved(scanner, &mut report),
            Work::Reconnect(job) => {
                release(scanner, &log);
                recent = RecentPresses::default();
                if let Some(report) = job {
                    acquire_for_job(&mut network, target, report);
                }
            }
            Work::Stop => return release(scanner, &log),
        }
    }
}
