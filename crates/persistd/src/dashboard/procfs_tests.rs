use std::collections::{BTreeMap, HashSet};
use std::io::{self, BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_ipc::{CollectionStatus, Completeness};

use super::proc_source::RealProcSource;
use super::procfs::*;

#[derive(Default)]
struct FakeProc {
    stat: BTreeMap<u32, String>,
    io: BTreeMap<u32, String>,
    stat_failures: HashSet<u32>,
    io_failures: HashSet<u32>,
}

impl FakeProc {
    fn add(&mut self, pid: u32, ppid: u32, ticks: u64, rss_pages: u64, io: u64) {
        self.stat
            .insert(pid, stat_line(pid, ppid, ticks, ticks + 1, rss_pages));
        self.io.insert(
            pid,
            format!("rchar: 1\nread_bytes: {io}\nwrite_bytes: {}\n", io * 2),
        );
    }
}

impl ProcSource for FakeProc {
    fn list_pids(&self, max_entries: usize) -> io::Result<(Vec<u32>, bool)> {
        let truncated = self.stat.len() > max_entries;
        Ok((
            self.stat.keys().copied().take(max_entries).collect(),
            truncated,
        ))
    }

    fn read_stat(&self, pid: u32) -> io::Result<String> {
        if self.stat_failures.contains(&pid) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "stat"));
        }
        self.stat
            .get(&pid)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "stat"))
    }

    fn read_io(&self, pid: u32) -> io::Result<String> {
        if self.io_failures.contains(&pid) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "io"));
        }
        self.io
            .get(&pid)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "io"))
    }
}

fn stat_line(pid: u32, ppid: u32, user: u64, system: u64, rss: u64) -> String {
    let mut fields = vec!["0".to_owned(); 22];
    fields[0] = "S".to_owned();
    fields[1] = ppid.to_string();
    fields[11] = user.to_string();
    fields[12] = system.to_string();
    fields[21] = rss.to_string();
    format!("{pid} (worker (test)) {}", fields.join(" "))
}

fn root(session_id: u32, root_pid: u32) -> SessionRoot {
    SessionRoot {
        session_id,
        root_pid,
        foreground_pid: Some(root_pid),
        writer_active: false,
    }
}

#[test]
fn stat_and_io_parsers_handle_comm_and_required_fields() {
    let stat = parse_stat(&stat_line(10, 2, 11, 7, 3), 4_096).unwrap();
    assert_eq!(stat.pid, 10);
    assert_eq!(stat.ppid, 2);
    assert_eq!(stat.counters.user_ticks, 11);
    assert_eq!(stat.counters.system_ticks, 7);
    assert_eq!(stat.counters.rss_kib, 12);
    let io = parse_io("read_bytes: 100\nwrite_bytes: 250\n").unwrap();
    assert_eq!(io, (100, 250));
    assert!(parse_stat("broken", 4_096).is_none());
    assert!(parse_io("read_bytes: 1\n").is_none());
}

#[test]
fn one_scan_aggregates_multiple_session_trees() {
    let mut proc = FakeProc::default();
    for (pid, ppid) in [(100, 1), (10, 1), (11, 10), (12, 11), (20, 1), (21, 20)] {
        proc.add(pid, ppid, u64::from(pid), 1, u64::from(pid) * 10);
    }
    let snapshot = collect_procfs(&proc, 100, &[root(1, 10), root(2, 20)], 4_096, 100);
    assert_eq!(snapshot.completeness, Completeness::Complete);
    assert_eq!(snapshot.daemon.unwrap().process_count, 1);
    assert_eq!(snapshot.sessions[0].counters.process_count, 3);
    assert_eq!(snapshot.sessions[1].counters.process_count, 2);
    assert_eq!(snapshot.sessions[0].counters.user_ticks, 33);
    assert_eq!(snapshot.sessions[0].counters.read_bytes, 330);
}

#[test]
fn nearest_nested_root_owns_each_process_once() {
    let mut proc = FakeProc::default();
    proc.add(100, 1, 1, 1, 1);
    proc.add(10, 1, 1, 1, 1);
    proc.add(11, 10, 1, 1, 1);
    proc.add(12, 11, 1, 1, 1);
    let snapshot = collect_procfs(&proc, 100, &[root(1, 10), root(2, 11)], 4_096, 100);
    assert_eq!(snapshot.sessions[0].counters.process_count, 1);
    assert_eq!(snapshot.sessions[1].counters.process_count, 2);
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .map(|session| session.counters.process_count)
            .sum::<u32>(),
        3
    );
}

#[test]
fn missing_roots_and_io_failures_have_explicit_status() {
    let mut proc = FakeProc::default();
    proc.add(100, 1, 1, 1, 1);
    proc.add(10, 1, 1, 1, 1);
    proc.add(11, 10, 1, 1, 1);
    proc.io_failures.insert(11);
    let snapshot = collect_procfs(&proc, 100, &[root(1, 10), root(2, 99)], 4_096, 100);
    assert_eq!(snapshot.completeness, Completeness::Partial);
    assert_eq!(
        snapshot.sessions[0].collection_status,
        CollectionStatus::Partial
    );
    assert_eq!(
        snapshot.sessions[1].collection_status,
        CollectionStatus::Unavailable
    );
    assert_eq!(snapshot.sessions[0].counters.process_count, 2);
}

#[test]
fn scan_limit_and_unattributable_stat_failure_mark_partial() {
    let mut proc = FakeProc::default();
    proc.add(10, 1, 1, 1, 1);
    proc.add(11, 10, 1, 1, 1);
    proc.stat_failures.insert(11);
    let snapshot = collect_procfs(&proc, 10, &[root(1, 10)], 4_096, 100);
    assert_eq!(snapshot.completeness, Completeness::Partial);
    assert_eq!(
        snapshot.sessions[0].collection_status,
        CollectionStatus::Partial
    );

    let truncated = collect_procfs(&proc, 10, &[root(1, 10)], 4_096, 1);
    assert_eq!(truncated.completeness, Completeness::Partial);
    assert!(truncated.truncated);
}

#[test]
fn expired_deadline_stops_before_reading_process_files() {
    let mut proc = FakeProc::default();
    proc.add(10, 1, 1, 1, 1);
    let snapshot = collect_procfs_until(
        &proc,
        10,
        &[root(1, 10)],
        4_096,
        100,
        Some(Instant::now() - Duration::from_millis(1)),
    );
    assert!(snapshot.truncated);
    assert_eq!(snapshot.completeness, Completeness::Unavailable);
    assert_eq!(
        snapshot.sessions[0].collection_status,
        CollectionStatus::Unavailable
    );
}

#[test]
fn duplicate_session_roots_do_not_double_count() {
    let mut proc = FakeProc::default();
    proc.add(10, 1, 1, 1, 1);
    let snapshot = collect_procfs(&proc, 10, &[root(1, 10), root(2, 10)], 4_096, 10);
    assert_eq!(snapshot.sessions[0].counters.process_count, 1);
    assert_eq!(
        snapshot.sessions[1].collection_status,
        CollectionStatus::Unavailable
    );
    assert_eq!(snapshot.completeness, Completeness::Partial);
}

#[test]
fn real_source_limits_files_and_filters_pid_directories() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("12")).unwrap();
    std::fs::create_dir(temp.path().join("self")).unwrap();
    std::fs::write(temp.path().join("12/stat"), stat_line(12, 1, 1, 1, 1)).unwrap();
    std::fs::write(temp.path().join("12/io"), "read_bytes: 1\nwrite_bytes: 2\n").unwrap();
    let source = RealProcSource::new(temp.path().to_path_buf());
    assert_eq!(source.list_pids(10).unwrap(), (vec![12], false));
    assert!(source.read_stat(12).is_ok());
    std::fs::write(
        temp.path().join("12/stat"),
        vec![b'x'; MAX_PROC_FILE_BYTES as usize + 1],
    )
    .unwrap();
    assert_eq!(
        source.read_stat(12).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

struct ChildTree {
    shell: Child,
    leaf_pid: u32,
}

impl Drop for ChildTree {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.leaf_pid as libc::pid_t, libc::SIGKILL);
        }
        let _ = self.shell.kill();
        let _ = self.shell.wait();
    }
}

fn spawn_child_tree() -> ChildTree {
    let mut shell = Command::new("sh")
        .args(["-c", "sleep 30 & echo $!; wait"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(shell.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    ChildTree {
        shell,
        leaf_pid: line.trim().parse().unwrap(),
    }
}

#[test]
fn real_linux_source_observes_shell_child_tree() {
    let tree = spawn_child_tree();
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    let source = RealProcSource::system();
    let snapshot = collect_procfs(
        &source,
        std::process::id(),
        &[root(1, tree.shell.id())],
        page_size,
        MAX_PROC_ENTRIES,
    );
    assert!(snapshot.daemon.is_some());
    assert!(snapshot.sessions[0].counters.process_count >= 2);
    assert_ne!(
        snapshot.sessions[0].collection_status,
        CollectionStatus::Unavailable
    );
}
