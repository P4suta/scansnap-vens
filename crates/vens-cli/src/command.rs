use std::{
    fmt::Display,
    fs::{self, File},
    io::{self, BufWriter, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use vens_core::control::LocalTime;
use vens_net::{
    host::{DirectorySink, SinkError},
    network::{Network, RecordingNetwork},
    operation::{self, ScanRun},
    trace::Event,
};

use crate::{
    invocation::{Invocation, UsageError},
    report,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Success,
    Failure,
    Usage,
}

/// The text for standard output, the text for standard error, and the exit status of one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub exit: Exit,
}

impl Output {
    fn success(stdout: String) -> Self {
        Self {
            stdout,
            stderr: String::new(),
            exit: Exit::Success,
        }
    }

    fn failure(message: &str) -> Self {
        Self {
            stdout: String::new(),
            stderr: problem(message),
            exit: Exit::Failure,
        }
    }
}

fn problem(message: &str) -> String {
    format!("vens: {message}\n")
}

/// Shows a usage problem followed by the usage.
#[must_use]
pub fn usage_failure(error: &UsageError) -> Output {
    Output {
        stdout: String::new(),
        stderr: format!(
            "{}{}",
            problem(&report::usage_error(error)),
            report::usage()
        ),
        exit: Exit::Usage,
    }
}

/// Runs one invocation on a network and collects everything that the user sees.
pub fn run<N>(mut network: N, invocation: Invocation) -> Output
where
    N: Network,
    N::Error: Display,
{
    match invocation {
        Invocation::Help => Output::success(report::usage().to_owned()),
        Invocation::Discover { target } => match operation::discover(&mut network, target) {
            Ok(reply) => Output::success(report::found(&reply)),
            Err(failure) => Output::failure(&report::failure(&failure)),
        },
        Invocation::Status { target } => match operation::status(&mut network, target) {
            Ok(state) => Output::success(report::state(&state)),
            Err(failure) => Output::failure(&report::failure(&failure)),
        },
        Invocation::Scan {
            directory,
            target,
            trace,
        } => scan(network, &directory, target, trace.as_deref()),
    }
}

/// Names the subfolder of one scan after its time, so a later scan never meets its files.
fn subfolder(directory: &Path, time: LocalTime) -> PathBuf {
    directory.join(format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        time.year(),
        time.month(),
        time.day(),
        time.hour(),
        time.minute(),
        time.second()
    ))
}

fn scan<N>(mut network: N, directory: &Path, target: Ipv4Addr, trace: Option<&Path>) -> Output
where
    N: Network,
    N::Error: Display,
{
    let trace_file = match trace.map(|path| (path, File::create(path))) {
        None => None,
        Some((path, Ok(file))) => Some((path, file)),
        Some((path, Err(error))) => {
            return Output::failure(&format!(
                "the trace {} couldn't be created: {error}",
                path.display()
            ));
        }
    };
    let time = match network.local_time() {
        Ok(time) => time,
        Err(error) => {
            return Output::failure(&format!(
                "the system clock can't name the scan ({error}); set the date and time"
            ));
        }
    };
    let folder = subfolder(directory, time);
    if let Err(error) = fs::create_dir_all(directory).and_then(|()| fs::create_dir(&folder)) {
        return Output::failure(&format!(
            "the folder {} couldn't be created: {error}",
            folder.display()
        ));
    }
    let mut sink = DirectorySink::new(folder.clone());
    let (run, trace_failure) = match trace_file {
        None => (operation::scan(&mut network, target, &mut sink), None),
        Some((path, file)) => {
            let (run, failure) = traced(network, file, |recording| {
                operation::scan(recording, target, &mut sink)
            });
            (run, failure.map(|error| (path, error)))
        }
    };
    if sink.written().is_empty() {
        let _ = fs::remove_dir(&folder);
    }
    scan_output(&run, sink.written(), &folder, trace_failure)
}

/// Runs the work on a recording network and streams every event to the file as it happens.
fn traced<N: Network, R>(
    network: N,
    file: File,
    work: impl FnOnce(&mut RecordingNetwork<N>) -> R,
) -> (R, Option<io::Error>) {
    let (events, log) = mpsc::channel::<Event>();
    let (done, finished) = mpsc::channel::<io::Result<()>>();
    let result = thread::scope(|scope| {
        scope.spawn(move || {
            let mut writer = BufWriter::new(file);
            let written = log
                .iter()
                .try_for_each(|event| writeln!(writer, "{event}").and_then(|()| writer.flush()));
            let _ = done.send(written);
        });
        let mut recording = RecordingNetwork::new(network, events);
        let result = work(&mut recording);
        drop(recording);
        result
    });
    let failure = finished.try_recv().ok().and_then(Result::err);
    (result, failure)
}

/// Builds what the user sees after a scan from its run, the written images, and any trace failure.
#[must_use]
pub fn scan_output<E: Display>(
    run: &ScanRun<E, SinkError>,
    written: &[PathBuf],
    folder: &Path,
    trace_failure: Option<(&Path, io::Error)>,
) -> Output {
    let mut stdout = report::paths(written);
    let mut stderr = String::new();
    let mut exit = Exit::Success;
    match &run.outcome {
        Ok(scanned) if scanned.sheets > 0 => {
            stdout = report::scanned(scanned, written);
        }
        Ok(_) => {
            stderr.push_str(&problem(report::no_sheet()));
            exit = Exit::Failure;
        }
        Err(failure) => {
            stderr.push_str(&problem(&report::failure(failure)));
            if !written.is_empty() {
                stderr.push_str(&problem(&report::saved_before_failure(folder)));
            }
            exit = Exit::Failure;
        }
    }
    if let Some(error) = &run.keepalive_failure {
        stderr.push_str(&problem(&format!("the keepalive failed: {error}")));
    }
    if let Some((path, error)) = trace_failure {
        stderr.push_str(&problem(&format!(
            "the trace {} stopped early: {error}",
            path.display()
        )));
        exit = Exit::Failure;
    }
    Output {
        stdout,
        stderr,
        exit,
    }
}
