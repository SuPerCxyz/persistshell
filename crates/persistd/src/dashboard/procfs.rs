use std::collections::{HashMap, HashSet};
use std::io;
use std::time::Instant;

use persist_ipc::{CollectionStatus, Completeness};

use super::model::{RawCounters, RawSessionSample};

pub(super) const MAX_PROC_ENTRIES: usize = 262_144;
pub(super) const MAX_PROC_FILE_BYTES: u64 = 4 * 1024;

pub(super) trait ProcSource {
    fn list_pids(&self, max_entries: usize) -> io::Result<(Vec<u32>, bool)>;
    fn read_stat(&self, pid: u32) -> io::Result<String>;
    fn read_io(&self, pid: u32) -> io::Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionRoot {
    pub(crate) session_id: u32,
    pub(crate) root_pid: u32,
    pub(crate) foreground_pid: Option<u32>,
    pub(crate) writer_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProcSnapshot {
    pub completeness: Completeness,
    pub daemon: Option<RawCounters>,
    pub sessions: Vec<RawSessionSample>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ParsedProcess {
    pub pid: u32,
    pub ppid: u32,
    pub counters: RawCounters,
}

#[derive(Debug, Clone, Copy)]
struct ProcessRecord {
    process: ParsedProcess,
    io_complete: bool,
}

pub(super) fn parse_stat(value: &str, page_size: u64) -> Option<ParsedProcess> {
    if page_size == 0 {
        return None;
    }
    let (pid, fields) = value.split_once(" (")?;
    let (_, fields) = fields.rsplit_once(") ")?;
    let values = fields.split_whitespace().collect::<Vec<_>>();
    if values.len() < 22 {
        return None;
    }
    let rss_pages = values[21].parse::<u64>().ok()?;
    let rss_kib =
        (u128::from(rss_pages) * u128::from(page_size) / 1024).min(u128::from(u64::MAX)) as u64;
    Some(ParsedProcess {
        pid: pid.parse().ok()?,
        ppid: values[1].parse().ok()?,
        counters: RawCounters {
            user_ticks: values[11].parse().ok()?,
            system_ticks: values[12].parse().ok()?,
            rss_kib,
            read_bytes: 0,
            write_bytes: 0,
            process_count: 1,
        },
    })
}

pub(super) fn parse_io(value: &str) -> Option<(u64, u64)> {
    let field = |name| {
        value
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|number| number.trim().parse().ok())
    };
    Some((field("read_bytes:")?, field("write_bytes:")?))
}

pub(super) fn collect_procfs(
    source: &dyn ProcSource,
    daemon_pid: u32,
    roots: &[SessionRoot],
    page_size: u64,
    max_entries: usize,
) -> ProcSnapshot {
    collect_procfs_until(source, daemon_pid, roots, page_size, max_entries, None)
}

pub(super) fn collect_procfs_until(
    source: &dyn ProcSource,
    daemon_pid: u32,
    roots: &[SessionRoot],
    page_size: u64,
    max_entries: usize,
    deadline: Option<Instant>,
) -> ProcSnapshot {
    let Ok((pids, truncated)) = source.list_pids(max_entries) else {
        return unavailable_snapshot(roots);
    };
    let mut records = HashMap::with_capacity(pids.len());
    let mut stat_incomplete = false;
    let mut timed_out = false;
    for pid in pids {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            timed_out = true;
            break;
        }
        let Ok(stat) = source.read_stat(pid) else {
            stat_incomplete = true;
            continue;
        };
        let Some(mut process) = parse_stat(&stat, page_size).filter(|item| item.pid == pid) else {
            stat_incomplete = true;
            continue;
        };
        let io_complete = source
            .read_io(pid)
            .ok()
            .and_then(|value| parse_io(&value))
            .map(|(read, write)| {
                process.counters.read_bytes = read;
                process.counters.write_bytes = write;
            })
            .is_some();
        records.insert(
            pid,
            ProcessRecord {
                process,
                io_complete,
            },
        );
    }

    let mut root_owners = HashMap::with_capacity(roots.len());
    let mut duplicate_roots = HashSet::new();
    for (index, root) in roots.iter().enumerate() {
        if root.root_pid == 0 || root_owners.contains_key(&root.root_pid) {
            duplicate_roots.insert(index);
        } else {
            root_owners.insert(root.root_pid, index);
        }
    }

    let mut aggregates = vec![RawCounters::default(); roots.len()];
    let mut statuses = vec![CollectionStatus::Complete; roots.len()];
    for duplicate in duplicate_roots {
        statuses[duplicate] = CollectionStatus::Unavailable;
    }
    for record in records.values() {
        let Some(owner) = find_owner(record.process.pid, &records, &root_owners) else {
            continue;
        };
        if statuses[owner] == CollectionStatus::Unavailable {
            continue;
        }
        add_counters(&mut aggregates[owner], record.process.counters);
        if !record.io_complete {
            statuses[owner] = CollectionStatus::Partial;
        }
    }

    let global_partial = stat_incomplete || truncated || timed_out;
    for (index, root) in roots.iter().enumerate() {
        if !records.contains_key(&root.root_pid) {
            statuses[index] = CollectionStatus::Unavailable;
        } else if global_partial && statuses[index] == CollectionStatus::Complete {
            statuses[index] = CollectionStatus::Partial;
        }
    }
    let sessions = roots
        .iter()
        .enumerate()
        .map(|(index, root)| RawSessionSample {
            session_id: root.session_id,
            counters: aggregates[index],
            foreground_pid: root.foreground_pid,
            writer_active: root.writer_active,
            collection_status: statuses[index],
        })
        .collect::<Vec<_>>();
    let daemon_record = records.get(&daemon_pid);
    let daemon = daemon_record.map(|record| record.process.counters);
    let daemon_complete = daemon_record.is_some_and(|record| record.io_complete);
    let all_sessions_unavailable = sessions
        .iter()
        .all(|session| session.collection_status == CollectionStatus::Unavailable);
    let completeness = if daemon.is_none() && all_sessions_unavailable {
        Completeness::Unavailable
    } else if global_partial
        || !daemon_complete
        || sessions
            .iter()
            .any(|session| session.collection_status != CollectionStatus::Complete)
    {
        Completeness::Partial
    } else {
        Completeness::Complete
    };
    ProcSnapshot {
        completeness,
        daemon,
        sessions,
        truncated: truncated || timed_out,
    }
}

fn find_owner(
    pid: u32,
    records: &HashMap<u32, ProcessRecord>,
    roots: &HashMap<u32, usize>,
) -> Option<usize> {
    let mut current = pid;
    let mut visited = HashSet::new();
    while current != 0 && visited.insert(current) {
        if let Some(owner) = roots.get(&current) {
            return Some(*owner);
        }
        let record = records.get(&current)?;
        if record.process.ppid == current {
            return None;
        }
        current = record.process.ppid;
    }
    None
}

fn add_counters(total: &mut RawCounters, value: RawCounters) {
    total.user_ticks = total.user_ticks.saturating_add(value.user_ticks);
    total.system_ticks = total.system_ticks.saturating_add(value.system_ticks);
    total.rss_kib = total.rss_kib.saturating_add(value.rss_kib);
    total.read_bytes = total.read_bytes.saturating_add(value.read_bytes);
    total.write_bytes = total.write_bytes.saturating_add(value.write_bytes);
    total.process_count = total.process_count.saturating_add(1);
}

fn unavailable_snapshot(roots: &[SessionRoot]) -> ProcSnapshot {
    ProcSnapshot {
        completeness: Completeness::Unavailable,
        daemon: None,
        sessions: roots
            .iter()
            .map(|root| RawSessionSample {
                session_id: root.session_id,
                counters: RawCounters::default(),
                foreground_pid: root.foreground_pid,
                writer_active: root.writer_active,
                collection_status: CollectionStatus::Unavailable,
            })
            .collect(),
        truncated: false,
    }
}
