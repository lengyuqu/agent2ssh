//! Bounded capture of a command's output, shared by every exec target.
//!
//! Two targets run commands now — the embedded SSH path
//! (`embedded_ssh::run_command_capture`) and the local path
//! (`local_exec::run_local_command_capture`). Both must bound what they keep and
//! report what they dropped in *exactly* the same way, because `truncated` and
//! `dropped_bytes` are surfaced to agents, written to the audit log, and used to
//! decide whether a result is trustworthy. A second, subtly different
//! accumulator would not be a duplicate implementation — it would be a reporting
//! bug that only shows up on one target.
//!
//! Both types started in `embedded_ssh.rs`. They moved here so the local target
//! can borrow a byte counter and a result shape without depending on the SSH
//! transport to get them.

/// What a captured command produced.
pub(crate) struct CapturedExec {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    /// True when either stream was cut short by `max_bytes`.
    pub(crate) truncated: bool,
    /// Total bytes discarded from both streams.
    pub(crate) dropped_bytes: usize,
}

/// Bounded head+tail accumulator for a single output stream.
///
/// Retains the first `max_bytes / 2` bytes and a rolling window of the last
/// `max_bytes / 2`, while counting every byte it was fed so callers can report
/// how much of the middle was dropped.
pub(crate) struct BoundedCapture {
    head: Vec<u8>,
    tail: Vec<u8>,
    max_bytes: usize,
    head_bytes: usize,
    tail_bytes: usize,
    total: usize,
}

impl BoundedCapture {
    pub(crate) fn new(max_bytes: usize) -> Self {
        let head_bytes = max_bytes / 2;
        Self {
            head: Vec::new(),
            tail: Vec::new(),
            max_bytes,
            head_bytes,
            tail_bytes: max_bytes.saturating_sub(head_bytes),
            total: 0,
        }
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len();
        if self.head.len() < self.head_bytes {
            let remaining = self.head_bytes - self.head.len();
            let take = chunk.len().min(remaining);
            self.head.extend_from_slice(&chunk[..take]);
            if take < chunk.len() {
                append_rolling_tail(&mut self.tail, &chunk[take..], self.tail_bytes);
            }
        } else {
            append_rolling_tail(&mut self.tail, chunk, self.tail_bytes);
        }
    }

    pub(crate) fn truncated(&self) -> bool {
        self.total > self.max_bytes
    }

    pub(crate) fn dropped_bytes(&self) -> usize {
        self.total.saturating_sub(self.max_bytes)
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        let mut out = self.head;
        out.extend_from_slice(&self.tail);
        out
    }
}

/// Append `chunk` to a rolling tail buffer that keeps only the last
/// `max_tail` bytes, discarding the oldest bytes as it grows.
fn append_rolling_tail(tail: &mut Vec<u8>, chunk: &[u8], max_tail: usize) {
    if max_tail == 0 {
        return;
    }
    if chunk.len() >= max_tail {
        tail.clear();
        tail.extend_from_slice(&chunk[chunk.len() - max_tail..]);
    } else {
        tail.extend_from_slice(chunk);
        if tail.len() > max_tail {
            let overflow = tail.len() - max_tail;
            tail.drain(..overflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_capture_keeps_head_and_tail_and_counts_dropped_bytes() {
        // 16 bytes => 8 head + 8 tail.
        let mut capture = BoundedCapture::new(16);
        capture.push(b"0123456789");
        capture.push(b"abcdefghij");
        assert!(capture.truncated());
        assert_eq!(capture.dropped_bytes(), 4);
        assert_eq!(capture.into_bytes(), b"01234567cdefghij");
    }

    #[test]
    fn bounded_capture_leaves_short_output_untouched() {
        let mut capture = BoundedCapture::new(16);
        capture.push(b"short");
        assert!(!capture.truncated());
        assert_eq!(capture.dropped_bytes(), 0);
        assert_eq!(capture.into_bytes(), b"short");
    }

    #[test]
    fn bounded_capture_never_exceeds_its_budget() {
        // Any number of chunks must stay within `max_bytes`, which is what keeps
        // a stderr-heavy remote command from growing this process unbounded.
        let max_bytes = 4096;
        let mut capture = BoundedCapture::new(max_bytes);
        for _ in 0..512 {
            capture.push(&[b'x'; 8192]);
        }
        assert!(capture.truncated());
        assert_eq!(capture.dropped_bytes(), 512 * 8192 - max_bytes);
        assert_eq!(capture.into_bytes().len(), max_bytes);
    }

    #[test]
    fn bounded_capture_zero_budget_keeps_no_bytes() {
        let mut capture = BoundedCapture::new(0);
        capture.push(b"discarded");
        assert!(capture.truncated());
        assert_eq!(capture.dropped_bytes(), 9);
        assert!(capture.into_bytes().is_empty());
    }

    #[test]
    fn bounded_capture_one_byte_budget_keeps_the_newest_byte() {
        // The head takes `max_bytes / 2` and the tail the remainder, so an odd
        // one-byte budget is all tail: it keeps the newest byte, not the oldest.
        let mut capture = BoundedCapture::new(1);
        capture.push(b"ab");
        assert!(capture.truncated());
        assert_eq!(capture.dropped_bytes(), 1);
        assert_eq!(capture.into_bytes(), b"b");
    }
}
