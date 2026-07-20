use std::process::Command;

use super::*;

#[test]
fn proc_stat_parser_handles_closing_parenthesis_in_name() {
    let fields = (1..=19)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let stat = parse_process_stat(&format!("42 (holder ) worker) S {fields}")).unwrap();
    assert_eq!(stat.state, b'S');
    assert_eq!(stat.start_time, 19);
}

#[test]
fn procfs_watcher_reports_live_process_and_identity_change() {
    let pid = std::process::id();
    let watcher = ProcessExit::watch_procfs(pid).unwrap();
    assert!(!watcher.has_exited().unwrap());

    let stat = read_process_stat(pid).unwrap();
    let changed = ProcessIdentity {
        pid,
        start_time: stat.start_time.wrapping_add(1),
    };
    assert!(changed.has_exited().unwrap());
}

#[test]
fn procfs_watcher_waits_for_child_exit_without_pidfd() {
    let mut child = Command::new("sh")
        .args(["-c", "sleep 0.05"])
        .spawn()
        .unwrap();
    let watcher = ProcessExit::watch_procfs(child.id()).unwrap();
    watcher.wait_for(Duration::from_secs(1)).unwrap();
    child.wait().unwrap();
    assert!(watcher.has_exited().unwrap());
}

#[test]
fn procfs_watcher_wait_is_bounded() {
    let watcher = ProcessExit::watch_procfs(std::process::id()).unwrap();
    let started = Instant::now();
    assert!(watcher.wait_for(Duration::from_millis(30)).is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn pidfd_fallback_only_accepts_unsupported_or_denied_errors() {
    for code in [libc::ENOSYS, libc::EINVAL, libc::EPERM] {
        assert!(pidfd_fallback_allowed(&std::io::Error::from_raw_os_error(
            code
        )));
    }
    assert!(!pidfd_fallback_allowed(&std::io::Error::from_raw_os_error(
        libc::EMFILE
    )));
}
