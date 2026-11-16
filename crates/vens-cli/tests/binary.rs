//! The binary prints the usage for help, names a usage problem with exit code 2, and names any other failure with exit code 1.

use std::{env, fs, process, process::Command};

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
fn help_prints_the_whole_usage_and_succeeds() {
    assert_eq!(
        run(&["help"]),
        Some((Some(0), vens_cli::report::usage().to_owned(), String::new()))
    );
}

#[test]
fn a_usage_problem_names_itself_and_exits_with_two() {
    assert_eq!(
        run(&["print"]),
        Some((
            Some(2),
            String::new(),
            format!(
                "vens: unknown subcommand `print`\n{}",
                vens_cli::report::usage()
            )
        ))
    );
}

#[test]
fn a_folder_that_cannot_be_created_fails_with_one_before_any_network_use() {
    let file = env::temp_dir().join(format!("vens-cli-binary-{}", process::id()));
    let _ = fs::write(&file, b"a regular file");
    let target = file.join("scans");
    let outcome = target
        .to_str()
        .and_then(|target| run(&["scan", target, "--address", "192.0.2.7"]));
    let _ = fs::remove_file(&file);
    assert_eq!(
        outcome.map(|(code, out, error)| (code, out, error.starts_with("vens: the folder "))),
        Some((Some(1), String::new(), true))
    );
}
