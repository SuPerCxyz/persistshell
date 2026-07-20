use std::collections::VecDeque;
use std::io;
use std::os::fd::RawFd;

use persist_core::{PersistError, Result};

const MAX_PROXY_QUEUE: usize = 1024 * 1024;

pub(super) struct PendingWrites {
    items: VecDeque<Vec<u8>>,
    offset: usize,
    bytes: usize,
}

impl PendingWrites {
    pub(super) fn new() -> Self {
        Self {
            items: VecDeque::new(),
            offset: 0,
            bytes: 0,
        }
    }

    pub(super) fn push(&mut self, bytes: Vec<u8>) -> Result<()> {
        let total = self.bytes.saturating_add(bytes.len());
        if total > MAX_PROXY_QUEUE {
            return Err(PersistError::invalid_argument(
                "public holder proxy queue limit exceeded",
            ));
        }
        self.bytes = total;
        self.items.push_back(bytes);
        Ok(())
    }

    pub(super) fn has_data(&self) -> bool {
        !self.items.is_empty()
    }

    pub(super) fn flush(&mut self, fd: RawFd) -> Result<()> {
        while let Some(front) = self.items.front() {
            let count = unsafe {
                libc::write(
                    fd,
                    front[self.offset..].as_ptr().cast(),
                    front.len() - self.offset,
                )
            };
            if count < 0 {
                let source = io::Error::last_os_error();
                if source.kind() == io::ErrorKind::WouldBlock {
                    return Ok(());
                }
                return Err(PersistError::Io {
                    operation: "write public holder proxy",
                    source,
                });
            }
            self.offset += count as usize;
            self.bytes -= count as usize;
            if self.offset == front.len() {
                self.items.pop_front();
                self.offset = 0;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_rejects_bytes_above_hard_limit() {
        let mut queue = PendingWrites::new();
        queue.push(vec![0; MAX_PROXY_QUEUE]).unwrap();
        assert!(queue.push(vec![0]).is_err());
    }
}
