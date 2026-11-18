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
    job::{JobEvent, Report, run_reserved},
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
    Reconnect,
    Stop,
}

fn await_work<T>(
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
                *recent = RecentPresses::default();
                log("the scanner is turning off");
                return Work::Reconnect;
            }
            Ok(Trigger::ReserveExpired) => {
                log("the scanner stopped answering keepalive probes");
                return Work::Reconnect;
            }
            Err(error) => {
                log(&format!("the button socket failed: {error}"));
                return Work::Reconnect;
            }
        }
        match jobs.recv_timeout(POLL_INTERVAL) {
            Ok(report) => return Work::Escl(report),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Work::Stop,
        }
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
    loop {
        let mut scanner = match acquire(&mut network, target) {
            Ok(scanner) => scanner,
            Err((refusal, detail)) => {
                log(&format!("can't wait for the Scan button: {detail}"));
                match jobs.recv_timeout(RETRY_INTERVAL) {
                    Ok(mut report) => report.report(JobEvent::Refused { refusal, detail }),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                continue;
            }
        };
        log("waiting for the Scan button");
        match await_work(&mut scanner, &mut recent, jobs, &log) {
            Work::Pressed => save(scanner, folder, &log),
            Work::Escl(mut report) => run_reserved(scanner, &mut report),
            work @ (Work::Reconnect | Work::Stop) => {
                if let Err(error) = scanner.release() {
                    log(&format!("release after listening failed: {error:?}"));
                }
                if matches!(work, Work::Stop) {
                    return;
                }
            }
        }
    }
}
