//! The binary prints the usage for help, and names a usage problem on standard error with exit code 2.

use std::process::Command;

fn run(arguments: &[&str]) -> Option<(Option<i32>, String, String)> {
    let output = Command::new(env!("CARGO_BIN_EXE_vens"))
        .args(arguments)
        .output()
        .ok()?;
    Some((
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

#[test]
fn help_prints_the_usage_and_succeeds() {
    let outcome = run(&["help"]);
    assert_eq!(
        outcome.map(|(code, out, error)| (code, out.starts_with("usage: vens"), error)),
        Some((Some(0), true, String::new()))
    );
}

#[test]
fn a_usage_problem_names_itself_and_exits_with_two() {
    let outcome = run(&["print"]);
    assert_eq!(
        outcome.map(|(code, out, error)| (
            code,
            out,
            error.starts_with("vens: unknown subcommand `print`\nusage: vens")
        )),
        Some((Some(2), String::new(), true))
    );
}
