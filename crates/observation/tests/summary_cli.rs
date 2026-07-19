use std::path::{Path, PathBuf};
use std::process::Command;

const RUN_ID: &str = "00000000-0000-4000-8000-000000000001";
const USAGE: &str = "usage: observation-summary <run-directory>\n";

fn golden_run_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/golden/v2")
        .join(RUN_ID)
}

#[test]
fn prints_the_deterministic_summary_for_the_golden_run() {
    let output = Command::new(env!("CARGO_BIN_EXE_observation-summary"))
        .arg(golden_run_dir())
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        include_str!("expected/summary.txt")
    );
}

#[test]
fn rejects_missing_run_directory_argument() {
    let output = Command::new(env!("CARGO_BIN_EXE_observation-summary"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8(output.stderr).unwrap(), USAGE);
}

#[test]
fn rejects_multiple_run_directory_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_observation-summary"))
        .arg(golden_run_dir())
        .arg(golden_run_dir())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8(output.stderr).unwrap(), USAGE);
}
