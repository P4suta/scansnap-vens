use std::process::Command;

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
