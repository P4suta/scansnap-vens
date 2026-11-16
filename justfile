set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

check_runner := env("CARGO_TARGET_DIR", "target") / "check-runner"

default:
    @just --list

check:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- check

fmt:
    cargo fmt --all

hardware-probe *target:
    cargo run --locked -p vens-net --example probe -- {{ target }}

hardware-scan directory *address_and_trace:
    cargo run --locked -p vens-net --example scan -- {{ quote(directory) }} {{ address_and_trace }}
