set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

check_runner := env("CARGO_TARGET_DIR", "target") / "check-runner"

default:
    @just --list

check:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- check

fmt:
    cargo fmt --all

vens *arguments:
    cargo run --locked -p vens-cli -- {{ arguments }}

vensd *arguments:
    cargo run --locked -p vens-daemon --bin vensd -- {{ arguments }}

redact-capture raw fixture:
    cargo run --locked -p xtask -- redact-capture {{ quote(raw) }} {{ quote(fixture) }}
