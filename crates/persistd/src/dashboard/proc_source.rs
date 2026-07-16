use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::procfs::{ProcSource, MAX_PROC_ENTRIES, MAX_PROC_FILE_BYTES};

pub(super) struct RealProcSource {
    root: PathBuf,
}

impl RealProcSource {
    pub(super) fn system() -> Self {
        Self {
            root: PathBuf::from("/proc"),
        }
    }

    #[cfg(test)]
    pub(super) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn read_process_file(&self, pid: u32, name: &str) -> io::Result<String> {
        read_limited(&self.root.join(pid.to_string()).join(name))
    }
}

impl ProcSource for RealProcSource {
    fn list_pids(&self, max_entries: usize) -> io::Result<(Vec<u32>, bool)> {
        let detection_limit = max_entries.saturating_add(1);
        let mut pids = Vec::with_capacity(detection_limit.min(MAX_PROC_ENTRIES));
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok())
            else {
                continue;
            };
            pids.push(pid);
            if pids.len() == detection_limit {
                break;
            }
        }
        let truncated = pids.len() > max_entries;
        pids.truncate(max_entries);
        pids.sort_unstable();
        Ok((pids, truncated))
    }

    fn read_stat(&self, pid: u32) -> io::Result<String> {
        self.read_process_file(pid, "stat")
    }

    fn read_io(&self, pid: u32) -> io::Result<String> {
        self.read_process_file(pid, "io")
    }
}

fn read_limited(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_PROC_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PROC_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "procfs record exceeds limit",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "procfs record is not UTF-8"))
}
