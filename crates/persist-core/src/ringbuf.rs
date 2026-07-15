use std::cmp;

/// A fixed-size byte ring buffer.
///
/// Writes append bytes at the current write position. When the buffer is full,
/// oldest bytes are overwritten. Supports reading back all data in order
/// and reading a limited trailing segment for replay.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    buffer: Vec<u8>,
    capacity: usize,
    write_pos: usize,
    filled: bool,
}

impl RingBuffer {
    /// Create a new `RingBuffer` with the given capacity in bytes.
    ///
    /// `capacity` must be > 0.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "RingBuffer capacity must be > 0");
        Self {
            buffer: vec![0u8; capacity],
            capacity,
            write_pos: 0,
            filled: false,
        }
    }

    /// Write bytes into the ring buffer, overwriting oldest data if full.
    pub fn write(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        let mut remaining = data;
        while !remaining.is_empty() {
            let space = self.capacity - self.write_pos;
            let to_write = cmp::min(remaining.len(), space);
            self.buffer[self.write_pos..self.write_pos + to_write]
                .copy_from_slice(&remaining[..to_write]);
            self.write_pos += to_write;
            if self.write_pos >= self.capacity {
                self.write_pos = 0;
                self.filled = true;
            }
            remaining = &remaining[to_write..];
        }
    }

    /// Return total bytes stored (up to capacity).
    pub fn len(&self) -> usize {
        if self.filled {
            self.capacity
        } else {
            self.write_pos
        }
    }

    /// Return all stored bytes in chronological order.
    pub fn read_all(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len());
        if self.filled {
            out.extend_from_slice(&self.buffer[self.write_pos..]);
        }
        out.extend_from_slice(&self.buffer[..self.write_pos]);
        out
    }

    /// Return up to `max_bytes` of the most recent data.
    pub fn read_replay(&self, max_bytes: usize) -> Vec<u8> {
        let total = self.len();
        if total == 0 {
            return Vec::new();
        }
        let skip = total.saturating_sub(max_bytes);
        let data = self.read_all();
        data[skip..].to_vec()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read() {
        let mut rb = RingBuffer::new(16);
        rb.write(b"hello");
        assert_eq!(rb.read_all(), b"hello");
        assert_eq!(rb.len(), 5);
    }

    #[test]
    fn exact_fit() {
        let mut rb = RingBuffer::new(5);
        rb.write(b"hello");
        assert_eq!(rb.read_all(), b"hello");
        assert_eq!(rb.len(), 5);
        assert!(rb.filled);
    }

    #[test]
    fn overwrite_oldest() {
        let mut rb = RingBuffer::new(8);
        rb.write(b"12345678");
        rb.write(b"AB");
        assert_eq!(rb.len(), 8);
        // "12345678" then "AB" overwrites "12" → "345678AB"
        assert_eq!(rb.read_all(), b"345678AB");
    }

    #[test]
    fn large_write() {
        let mut rb = RingBuffer::new(4);
        rb.write(b"abcdefgh");
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.read_all(), b"efgh");
    }

    #[test]
    fn read_replay_returns_trailing() {
        let mut rb = RingBuffer::new(100);
        rb.write(b"Hello World");
        let replay = rb.read_replay(5);
        assert_eq!(replay, b"World");
    }

    #[test]
    fn read_replay_under_capacity() {
        let mut rb = RingBuffer::new(100);
        rb.write(b"hi");
        let replay = rb.read_replay(1024);
        assert_eq!(replay, b"hi");
    }

    #[test]
    fn read_replay_after_wrap() {
        let mut rb = RingBuffer::new(4);
        rb.write(b"abcdef");
        // buffer has "cdef" (d at pos 0, e at pos 1, f at pos 2, c at pos 3...)
        // Actually: write_pos = 2, filled = true, buffer data = [e,f,c,d]?
        // Let's verify: start [0,0,0,0], write_pos=0
        // write "ab" → [a,b,0,0], write_pos=2
        // write "cdef" → [a,b,c,d], write_pos=4 (wrap), then [e,f,c,d], write_pos=2
        // read_all: from write_pos(2) + to write_pos: [c,d] + [e,f] = [c,d,e,f]
        assert_eq!(rb.read_all(), b"cdef");
        assert_eq!(rb.read_replay(2), b"ef");
        assert_eq!(rb.read_replay(3), b"def");
    }

    #[test]
    fn empty_buffer() {
        let rb = RingBuffer::new(8);
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.read_all().len(), 0);
        assert_eq!(rb.read_replay(100).len(), 0);
    }

    #[test]
    fn write_empty_is_noop() {
        let mut rb = RingBuffer::new(8);
        rb.write(b"abc");
        rb.write(b"");
        assert_eq!(rb.read_all(), b"abc");
    }

    #[test]
    fn multi_write_sequential() {
        let mut rb = RingBuffer::new(10);
        rb.write(b"a");
        rb.write(b"b");
        rb.write(b"c");
        assert_eq!(rb.read_all(), b"abc");
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn zero_capacity_panics() {
        let _rb = RingBuffer::new(0);
    }
}
