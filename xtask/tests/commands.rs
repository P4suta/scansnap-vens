use std::{env, fs, io, path::PathBuf, process::Command};

const GREETING: &str = "control < 0000001056454e530000000000000000\n";

fn scratch(name: &str) -> PathBuf {
    env::temp_dir().join(format!("vens-xtask-{}-{name}", std::process::id()))
}

fn redact_capture(test: &str, raw: &str) -> io::Result<(bool, String, String)> {
    let raw_path = scratch(&format!("{test}-raw.trace"));
    let fixture_path = scratch(&format!("{test}-fixture.trace"));
    fs::write(&raw_path, raw)?;
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("redact-capture")
        .arg(&raw_path)
        .arg(&fixture_path)
        .output()?;
    let fixture = fs::read_to_string(&fixture_path).unwrap_or_default();
    let _ = fs::remove_file(&raw_path);
    let _ = fs::remove_file(&fixture_path);
    Ok((
        output.status.success(),
        fixture,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

#[test]
fn redact_capture_writes_the_fixture_of_a_raw_trace() {
    assert_eq!(
        redact_capture("writes", GREETING).ok(),
        Some((true, GREETING.to_owned(), String::new()))
    );
}

#[test]
fn redact_capture_reports_a_refused_trace_and_writes_nothing() {
    assert_eq!(
        redact_capture("refuses", "control < 00\n").ok(),
        Some((
            false,
            String::new(),
            "the raw trace can't be redacted: UnexpectedEvent { index: 0 }\n".to_owned()
        ))
    );
}

#[test]
fn an_unknown_task_fails_and_names_it() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("deploy")
        .output();
    assert!(output.is_ok_and(|output| {
        !output.status.success()
            && String::from_utf8_lossy(&output.stderr).contains("unknown task: deploy")
    }));
}
