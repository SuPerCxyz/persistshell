use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use super::format::{
    encode_segment_header, frame_record, MinuteRecord, SegmentHeader, SEGMENT_HEADER_SIZE,
};
use super::storage_reader::{next_sequence, parse_segment_name, read_segment};
use super::storage_security::{ensure_private_directory, verify_private_file};

#[derive(Debug, Clone, Copy)]
pub(super) struct StorageLimits {
    pub max_segments: usize,
    pub max_total_bytes: u64,
    pub max_segment_bytes: u64,
    pub max_record_bytes: usize,
    pub max_directory_entries: usize,
}

impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            max_segments: 24,
            max_total_bytes: 128 * 1024 * 1024,
            max_segment_bytes: 128 * 1024 * 1024,
            max_record_bytes: 1024 * 1024,
            max_directory_entries: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LoadReport {
    pub records: Vec<MinuteRecord>,
    pub skipped_segments: usize,
    pub repaired_tails: usize,
}

pub(super) struct MetricStorage {
    dir: PathBuf,
    limits: StorageLimits,
    current: Option<CurrentSegment>,
    last_timestamp_ms: Option<u64>,
}

struct CurrentSegment {
    hour_start_ms: u64,
    path: PathBuf,
    file: File,
    bytes: u64,
}

impl MetricStorage {
    pub(super) fn open(path: &Path, limits: StorageLimits) -> io::Result<Self> {
        validate_limits(limits)?;
        ensure_private_directory(path)?;
        let entry_count = fs::read_dir(path)?.count();
        if entry_count > limits.max_directory_entries {
            return Err(invalid_data("metrics directory has too many entries"));
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if parse_segment_name(&entry.path()).is_some() {
                verify_private_file(&entry.path())?;
            }
        }
        Ok(Self {
            dir: path.to_path_buf(),
            limits,
            current: None,
            last_timestamp_ms: None,
        })
    }

    pub(super) fn segment_paths(&self) -> Vec<PathBuf> {
        let mut paths = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("segment-") && name.ends_with(".pmt"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    pub(super) fn append(&mut self, record: &MinuteRecord) -> io::Result<()> {
        let framed = frame_record(record, self.limits.max_record_bytes)
            .map_err(|_| invalid_data("metric record exceeds format limits"))?;
        if framed.len() as u64 + SEGMENT_HEADER_SIZE as u64 > self.limits.max_segment_bytes
            || framed.len() as u64 + SEGMENT_HEADER_SIZE as u64 > self.limits.max_total_bytes
        {
            return Err(invalid_data("metric record exceeds storage limits"));
        }
        let hour_start_ms = record.timestamp_ms / 3_600_000 * 3_600_000;
        if self.current.is_none() {
            if let Some((current, last_timestamp)) =
                self.resume_segment(hour_start_ms, record.timestamp_ms, framed.len() as u64)?
            {
                self.current = Some(current);
                self.last_timestamp_ms = last_timestamp;
            }
        }
        let needs_new = self.current.as_ref().map_or(true, |current| {
            current.hour_start_ms != hour_start_ms
                || current.bytes + framed.len() as u64 > self.limits.max_segment_bytes
                || self
                    .last_timestamp_ms
                    .is_some_and(|last| record.timestamp_ms < last)
        });
        if needs_new {
            self.current = Some(self.create_segment(hour_start_ms)?);
        }
        let current = self.current.as_mut().expect("segment created");
        current.file.write_all(&framed)?;
        current.bytes += framed.len() as u64;
        self.last_timestamp_ms = Some(record.timestamp_ms);
        self.rotate()?;
        Ok(())
    }

    pub(super) fn load(&mut self) -> io::Result<LoadReport> {
        let mut report = LoadReport {
            records: Vec::new(),
            skipped_segments: 0,
            repaired_tails: 0,
        };
        for path in self.segment_paths() {
            match read_segment(&path, self.limits) {
                Ok((mut records, repaired)) => {
                    report.records.append(&mut records);
                    report.repaired_tails += usize::from(repaired);
                }
                Err(_) => report.skipped_segments += 1,
            }
        }
        report.records.sort_by_key(|record| record.timestamp_ms);
        Ok(report)
    }

    fn create_segment(&self, hour_start_ms: u64) -> io::Result<CurrentSegment> {
        if fs::read_dir(&self.dir)?.count() >= self.limits.max_directory_entries {
            return Err(invalid_data("metrics directory entry limit reached"));
        }
        let sequence = next_sequence(&self.segment_paths(), hour_start_ms);
        let path = self
            .dir
            .join(format!("segment-{hour_start_ms:020}-{sequence:010}.pmt"));
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        file.write_all(&encode_segment_header(SegmentHeader {
            hour_start_ms,
            sequence,
        }))?;
        verify_private_file(&path)?;
        Ok(CurrentSegment {
            hour_start_ms,
            path,
            file,
            bytes: SEGMENT_HEADER_SIZE as u64,
        })
    }

    fn resume_segment(
        &self,
        hour_start_ms: u64,
        timestamp_ms: u64,
        append_bytes: u64,
    ) -> io::Result<Option<(CurrentSegment, Option<u64>)>> {
        for path in self.segment_paths().into_iter().rev() {
            if parse_segment_name(&path).map(|value| value.0) != Some(hour_start_ms) {
                continue;
            }
            let Ok((records, _)) = read_segment(&path, self.limits) else {
                continue;
            };
            let last_timestamp = records.last().map(|record| record.timestamp_ms);
            if last_timestamp.is_some_and(|last| timestamp_ms < last) {
                return Ok(None);
            }
            let bytes = fs::metadata(&path)?.len();
            if bytes + append_bytes > self.limits.max_segment_bytes {
                return Ok(None);
            }
            let file = OpenOptions::new()
                .read(true)
                .append(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path)?;
            return Ok(Some((
                CurrentSegment {
                    hour_start_ms,
                    path,
                    file,
                    bytes,
                },
                last_timestamp,
            )));
        }
        Ok(None)
    }

    fn rotate(&mut self) -> io::Result<()> {
        loop {
            let paths = self.segment_paths();
            let total = paths.iter().try_fold(0_u64, |sum, path| {
                verify_private_file(path)?;
                Ok::<_, io::Error>(sum.saturating_add(fs::metadata(path)?.len()))
            })?;
            if paths.len() <= self.limits.max_segments && total <= self.limits.max_total_bytes {
                return Ok(());
            }
            let current_path = self.current.as_ref().map(|current| &current.path);
            let Some(oldest) = paths.iter().find(|path| current_path != Some(*path)) else {
                return Err(invalid_data("active metric segment exceeds total limit"));
            };
            fs::remove_file(oldest)?;
        }
    }
}

fn validate_limits(limits: StorageLimits) -> io::Result<()> {
    if limits.max_segments == 0
        || limits.max_total_bytes < SEGMENT_HEADER_SIZE as u64
        || limits.max_segment_bytes < SEGMENT_HEADER_SIZE as u64
        || limits.max_record_bytes == 0
        || limits.max_directory_entries < limits.max_segments
    {
        return Err(invalid_data("invalid metric storage limits"));
    }
    Ok(())
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
