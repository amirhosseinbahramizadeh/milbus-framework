//! CLI smoke tests proving the integration-test harness works.
//!
//! These use Cargo's `CARGO_BIN_EXE_<name>` environment variable, which points
//! at the binary built for integration tests, so no extra test dependencies
//! are required. Phase 5 adds real attack/capture integration tests here.

use std::process::Command;

/// Path to the `milbus-analyzer` binary built for integration tests.
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_milbus-analyzer"))
}

#[test]
fn help_exits_zero() {
    let output = binary()
        .arg("--help")
        .output()
        .expect("binary runs");
    assert!(output.status.success(), "--help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for subcommand in ["simulate", "inject", "replay"] {
        assert!(
            stdout.contains(subcommand),
            "--help output should mention {subcommand}: {stdout}"
        );
    }
}

#[test]
fn every_subcommand_advertises_help() {
    for subcommand in ["simulate", "inject", "replay"] {
        let output = binary()
            .arg(subcommand)
            .arg("--help")
            .output()
            .expect("binary runs");
        assert!(
            output.status.success(),
            "{subcommand} --help should exit 0 (got {:?})",
            output.status.code()
        );
    }
}

#[test]
fn version_exits_zero() {
    let output = binary()
        .arg("--version")
        .output()
        .expect("binary runs");
    assert!(output.status.success(), "--version should exit 0");
}
