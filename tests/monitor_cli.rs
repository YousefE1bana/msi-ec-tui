//! CLI integration tests for `mec monitor` against committed fixtures.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use tempfile::tempdir;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_arg(name: &str) -> String {
    manifest_dir()
        .join("tests")
        .join("fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn mec() -> Command {
    Command::cargo_bin("mec").unwrap()
}

#[test]
fn top_help_lists_monitor() {
    mec()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("monitor"));
}

#[test]
fn monitor_help_succeeds() {
    mec()
        .args(["monitor", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--interval"));
}

#[test]
fn monitor_help_documents_interval() {
    mec()
        .args(["monitor", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--interval"));
}

#[test]
fn gf_monitor_reports_startup_contract() {
    let proc = run_monitor_until_sample(&fixture_arg("gf63"), "5s");
    assert!(proc.stdout.contains("MEC Monitor"));
    assert!(proc.stdout.contains("Device: GF63 Thin 11UC"));
    assert!(proc.stdout.contains("Mode: READY"));
    assert!(proc.stdout.contains("Interval: 5s"));
    assert!(proc.stdout.contains(
        "CPU: 63°C | GPU: 51°C | CPU Fan: 42% | GPU Fan: 31% | Battery: 77% | AC: Connected"
    ));
    assert!(proc.stdout.contains("Monitoring stopped."));
}

#[test]
fn monitor_accepts_all_interval_forms() {
    for interval in ["500ms", "1s", "2s", "5s", "500", "1000", "2000", "5000"] {
        let started = Instant::now();
        let proc = run_monitor_until_sample(&fixture_arg("gf63"), interval);
        assert!(proc.stdout.contains("CPU:"), "interval {interval}");
        assert!(proc.status.success(), "interval {interval}");
        // Even the 5s interval must shut down promptly after SIGINT.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "interval {interval} did not stop promptly"
        );
    }
}

#[test]
fn monitor_rejects_invalid_intervals() {
    for interval in ["0", "100ms", "750ms", "3s", "10s", "fast"] {
        mec()
            .args([
                "--sys-root",
                &fixture_arg("gf63"),
                "monitor",
                "--interval",
                interval,
            ])
            .assert()
            .failure();
    }
}

#[test]
fn monitor_rejects_json_flag() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "monitor", "--json"])
        .assert()
        .failure();
}

#[test]
fn monitor_accepts_relative_sys_root() {
    let proc = run_monitor_until_sample_in("./tests/fixtures/gf63", "5s", manifest_dir());
    assert!(proc.stdout.contains("Mode: READY"));
}

#[test]
fn bare_mec_banner_unchanged() {
    mec()
        .assert()
        .success()
        .stdout("MEC — MSI EC Control Center\n");
}

#[test]
fn status_unchanged() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Status"));
}

#[test]
fn status_json_unchanged() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"READY\""));
}

#[test]
fn doctor_unchanged() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Mode: READY"));
}

// --- signal-test helpers (Linux-only) ---

struct MonitorProc {
    stdout: String,
    status: std::process::ExitStatus,
}

/// Runs `mec monitor` until the first sample line appears, then sends
/// SIGINT and waits for prompt, clean shutdown. Always reaps the child,
/// even when an assertion fails, via the guard below.
fn run_monitor_until_sample(sys_root: &str, interval: &str) -> MonitorProc {
    run_monitor_until_sample_in(sys_root, interval, manifest_dir())
}

fn run_monitor_until_sample_in(sys_root: &str, interval: &str, workdir: PathBuf) -> MonitorProc {
    let mut child = ChildGuard::spawn(sys_root, interval, workdir);
    let mut collected = String::new();
    let mut reader = BufReader::new(child.stdout());
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).unwrap();
        assert!(read > 0, "monitor ended before printing a sample");
        collected.push_str(&line);
        if line.contains("CPU:") {
            break;
        }
        assert!(Instant::now() < deadline, "timed out waiting for sample");
    }
    send_sigint(child.id());
    // Keep `reader` open until the child has exited: dropping it early
    // would close the pipe and turn the child's final writes into EPIPE
    // errors. The pipe buffer easily holds the small shutdown output.
    let output = child.wait_bounded();
    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();
    collected.push_str(&rest);
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
    MonitorProc {
        stdout: collected,
        status: output.status,
    }
}

/// Owns a monitor child process and guarantees cleanup on every path,
/// including failed assertions (via `Drop`).
struct ChildGuard {
    child: Option<std::process::Child>,
}

impl ChildGuard {
    fn from_child(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    fn spawn(sys_root: &str, interval: &str, workdir: PathBuf) -> Self {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_mec"));
        command
            .args(["--sys-root", sys_root, "monitor", "--interval", interval])
            .current_dir(workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Self {
            child: Some(command.spawn().unwrap()),
        }
    }

    fn stdout(&mut self) -> std::process::ChildStdout {
        self.child.as_mut().unwrap().stdout.take().unwrap()
    }

    fn id(&self) -> u32 {
        self.child.as_ref().unwrap().id()
    }

    /// Waits up to 15 seconds for exit; kills the child on timeout so no
    /// runaway monitor survives a regression.
    fn wait_bounded(mut self) -> std::process::Output {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let child = self.child.as_mut().unwrap();
            match child.try_wait().unwrap() {
                Some(_) => return self.child.take().unwrap().wait_with_output().unwrap(),
                None => {
                    if Instant::now() >= deadline {
                        let _ = self.child.as_mut().unwrap().kill();
                        let _ = self.child.as_mut().unwrap().wait();
                        panic!("monitor did not exit promptly after SIGINT");
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn send_sigint(pid: u32) {
    let status = std::process::Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

/// Spawns `mec monitor` with stdout/stderr redirected to files under `dir`,
/// so output can be polled without pipe-deadlock risk.
fn spawn_monitor_to_files(
    sys_root: &str,
    interval: &str,
    workdir: &Path,
    dir: &Path,
) -> ChildGuard {
    use std::fs::File;
    let out = File::create(dir.join("stdout.log")).unwrap();
    let err = File::create(dir.join("stderr.log")).unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_mec"));
    command
        .args(["--sys-root", sys_root, "monitor", "--interval", interval])
        .current_dir(workdir)
        .stdout(out)
        .stderr(err);
    ChildGuard::from_child(command.spawn().unwrap())
}

fn wait_for_file_contains(path: &Path, needle: &str, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let content = fs::read_to_string(path).unwrap_or_default();
        if content.contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what} ({needle:?})"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn count_in_file(path: &Path, needle: &str) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .matches(needle)
        .count()
}

#[test]
fn broken_fixture_monitor_degrades_without_telemetry() {
    let scratch = tempdir().unwrap();
    let child = spawn_monitor_to_files(
        &fixture_arg("broken-sysfs"),
        "5s",
        &manifest_dir(),
        scratch.path(),
    );
    let out_log = scratch.path().join("stdout.log");
    let err_log = scratch.path().join("stderr.log");
    wait_for_file_contains(&out_log, "MEC Monitor", "monitor header");
    wait_for_file_contains(&err_log, "MEC monitor degraded:", "degraded warning");
    // Give the loop a chance to attempt another tick: no sample may appear.
    std::thread::sleep(Duration::from_millis(600));
    send_sigint(child.id());
    let output = child.wait_bounded();
    assert!(output.status.success());
    let stdout = fs::read_to_string(&out_log).unwrap();
    let stderr = fs::read_to_string(&err_log).unwrap();
    assert!(stdout.contains("MEC Monitor"));
    assert!(stdout.contains("Device: Broken Interface Test Fixture"));
    assert!(stdout.contains("Mode: READ-ONLY"));
    assert_eq!(count_in_file(&err_log, "MEC monitor degraded:"), 1);
    assert!(stdout.contains("Monitoring stopped."));
    assert!(!stdout.contains("CPU:"));
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
}

#[test]
fn transient_backend_recovers_with_single_warnings() {
    let root = tempdir().unwrap();
    let dmi = root.path().join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("sys_vendor"), b"MSI\n").unwrap();
    fs::write(dmi.join("product_name"), b"Recovery Test Fixture\n").unwrap();
    let ec = root.path().join("sys/devices/platform/msi-ec/cpu");
    fs::create_dir_all(&ec).unwrap();
    let temperature = ec.join("realtime_temperature");
    fs::write(&temperature, b"60\n").unwrap();

    let scratch = tempdir().unwrap();
    let sys_root = root.path().to_string_lossy().into_owned();
    let child = spawn_monitor_to_files(&sys_root, "500ms", &manifest_dir(), scratch.path());
    let out_log = scratch.path().join("stdout.log");
    let err_log = scratch.path().join("stderr.log");

    wait_for_file_contains(&out_log, "CPU: 60°C", "initial sample");
    fs::write(&temperature, b"hot\n").unwrap();
    wait_for_file_contains(&err_log, "MEC monitor degraded:", "degraded warning");
    fs::write(&temperature, b"61\n").unwrap();
    wait_for_file_contains(&err_log, "MEC monitor recovered.", "recovery message");
    wait_for_file_contains(&out_log, "CPU: 61°C", "recovered sample");

    send_sigint(child.id());
    let output = child.wait_bounded();
    assert!(output.status.success());
    assert_eq!(count_in_file(&err_log, "MEC monitor degraded:"), 1);
    assert_eq!(count_in_file(&err_log, "MEC monitor recovered."), 1);
    let stdout = fs::read_to_string(&out_log).unwrap();
    assert!(stdout.contains("Monitoring stopped."));
    let stderr = fs::read_to_string(&err_log).unwrap();
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
}
