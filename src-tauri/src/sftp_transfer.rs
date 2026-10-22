//! SFTP transfer robustness: cancellation + resume (K6).
//!
//! Before K6 transfers were a single `std::io::copy` with no way to stop a
//! large transfer mid-flight and no way to recover from an interruption without
//! re-sending from byte 0. This module adds:
//!
//! - a process-global **cancellation registry** keyed by an opaque transfer id,
//!   so a caller can flip a flag that the copy loop observes between chunks; and
//! - a **resume offset** helper that decides where a transfer should pick up
//!   given how many bytes already landed on the destination.
//!
//! ## A4: `walk_local_dir` — safe directory traversal
//!
//! When uploading a directory tree to a remote host, the client needs to
//! enumerate all files within the tree. A naive recursive `read_dir` has
//! three problems:
//!
//! 1. **Symlink loops**: a symlink that points back to an ancestor directory
//!    causes infinite recursion. We use BFS with a depth cap and detect
//!    symlink-to-directory targets.
//! 2. **Unbounded depth**: a malicious or broken directory structure could
//!    be thousands of levels deep, exhausting stack or memory. The depth cap
//!    (`LOCAL_WALK_DEPTH_CAP`) prevents this.
//! 3. **Broken symlinks**: `symlink_metadata` on a broken symlink succeeds
//!    (it returns the link's own metadata), but `metadata` (which follows
//!    the link) fails. We skip broken symlinks silently rather than aborting
//!    the entire walk.
//!
//! Design borrowed from rssh's `walk_local_dir` function.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use crate::app_state::app_state;

/// Maximum directory depth for `walk_local_dir`. This prevents infinite
/// recursion and bounds memory usage. A typical project rarely exceeds
/// 20 levels; 64 is generous while still safe.
const LOCAL_WALK_DEPTH_CAP: u32 = 64;

/// A file entry discovered by `walk_local_dir`.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// Path relative to the walk root.
    pub relative: PathBuf,
    /// File size in bytes (0 if unknown).
    pub size: u64,
}

/// Walk a local directory tree, returning all regular files found.
///
/// Uses BFS with a depth cap to prevent infinite recursion from symlink
/// loops. Symlinks to directories are skipped (not followed) to prevent
/// cycles. Broken symlinks are silently skipped.
///
/// Returns entries sorted by breadth-first order (parent directories first).
pub fn walk_local_dir(root: &Path) -> io::Result<Vec<WalkEntry>> {
    let mut entries = Vec::new();
    let mut queue: VecDeque<(PathBuf, PathBuf, u32)> = VecDeque::new();

    // Start with the root directory.
    // Use `symlink_metadata` to check if the root itself is a symlink.
    let root_meta = std::fs::symlink_metadata(root)?;
    if root_meta.is_file() {
        // Root is a single file — return it directly.
        entries.push(WalkEntry {
            path: root.to_path_buf(),
            relative: PathBuf::new(),
            size: root_meta.len(),
        });
        return Ok(entries);
    }

    if !root_meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a file or directory", root.display()),
        ));
    }

    queue.push_back((root.to_path_buf(), PathBuf::new(), 0));

    while let Some((dir, relative_prefix, depth)) = queue.pop_front() {
        if depth >= LOCAL_WALK_DEPTH_CAP {
            let _ = crate::diagnostics::append_diagnostic_log(
                "warn",
                "sftp_transfer",
                "walk_local_dir depth cap reached",
                Some(serde_json::json!({
                    "dir": dir.display().to_string(),
                    "depth": depth,
                    "cap": LOCAL_WALK_DEPTH_CAP,
                })),
            );
            continue;
        }

        let read_dir = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(error) => {
                // Log but continue — a single unreadable subdirectory
                // shouldn't abort the entire walk.
                let _ = crate::diagnostics::append_diagnostic_log(
                    "warn",
                    "sftp_transfer",
                    "walk_local_dir: cannot read directory",
                    Some(serde_json::json!({
                        "dir": dir.display().to_string(),
                        "error": error.to_string(),
                    })),
                );
                continue;
            }
        };

        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            let relative = relative_prefix.join(entry.file_name());

            // Use `symlink_metadata` to get the entry's own metadata
            // (not following symlinks). This lets us detect symlinks.
            let meta = match entry.metadata() {
                // `metadata()` follows symlinks — gives us the target's
                // metadata. This is a single syscall.
                Ok(m) => m,
                Err(_) => {
                    // Broken symlink or permission issue — skip silently.
                    // Try `symlink_metadata` to log if it's a symlink.
                    if std::fs::symlink_metadata(&path)
                        .map(|m| m.file_type().is_symlink())
                        .unwrap_or(false)
                    {
                        let _ = crate::diagnostics::append_diagnostic_log(
                            "debug",
                            "sftp_transfer",
                            "walk_local_dir: skipping broken symlink",
                            Some(serde_json::json!({
                                "path": path.display().to_string(),
                            })),
                        );
                    }
                    continue;
                }
            };

            if meta.is_file() {
                entries.push(WalkEntry {
                    path: path.clone(),
                    relative: relative.clone(),
                    size: meta.len(),
                });
            } else if meta.is_dir() {
                // Check if this directory entry is actually a symlink to
                // a directory. If so, skip it to prevent cycles.
                // `entry.metadata()` already followed the symlink, so if
                // we got here with `is_dir()`, it's a real directory or a
                // symlink pointing to a directory.
                //
                // To detect the symlink case, check `symlink_metadata`.
                if std::fs::symlink_metadata(&path)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    // This is a symlink to a directory — skip to prevent
                    // potential cycles.
                    let _ = crate::diagnostics::append_diagnostic_log(
                        "debug",
                        "sftp_transfer",
                        "walk_local_dir: skipping symlink-to-directory",
                        Some(serde_json::json!({
                            "path": path.display().to_string(),
                        })),
                    );
                    continue;
                }

                queue.push_back((path, relative, depth + 1));
            }
            // Symlinks to files are not followed — they're treated as
            // regular files if `metadata()` succeeded, or skipped if it
            // failed (broken symlink). This is intentional: uploading a
            // symlink's target content is safer than uploading the link
            // itself.
        }
    }

    Ok(entries)
}

/// Chunk size for cancellable copies. Large enough to keep throughput high,
/// small enough that a cancel is observed within a few hundred KB.
const COPY_CHUNK: usize = 64 * 1024;

/// Largest write that is safe against every SFTP server measured, and the floor
/// below which a refused write is a real error.
///
/// Dropbear is the reason. Its SFTP channel takes packets of at most 32 KiB, and
/// a write above that limit came back as zero bytes written: with the previous
/// transport, 1 MiB uploads hung with the remote file stuck at 131072 bytes and
/// completed once writes were cut to 16 KiB (measured against Dropbear 2024.85).
/// OpenSSH has no such limit, and 64 KiB writes are about 2.6x faster than 16 KiB
/// ones against it (28 vs 11 MB/s over loopback), so the small size is used only
/// where it is needed — see [`upload_chunk_for_server`].
///
/// The limit is the server's, so it survives the transport swap; what changed is
/// the client-side packing that turned it into a stall. 16 KiB is still the
/// conservative choice, but it has not been re-measured against Dropbear on the
/// current transport — if that run happens, this number is what to revisit.
const SFTP_SAFE_WRITE: usize = 16 * 1024;

/// Write size to use for an upload to the server that sent `banner`.
pub fn upload_chunk_for_server(banner: Option<&str>) -> usize {
    match banner {
        Some(banner) if banner.to_ascii_lowercase().contains("dropbear") => SFTP_SAFE_WRITE,
        _ => COPY_CHUNK,
    }
}

fn registry() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    &app_state().transfer_cancels
}

/// Register a cancellable transfer, returning its shared cancel flag. The copy
/// loop is handed this flag; [`cancel_transfer`] flips it.
pub fn register(transfer_id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    registry()
        .lock()
        .unwrap()
        .insert(transfer_id.to_string(), flag.clone());

    // Reserve + activate a lifecycle entry so this transfer is tracked.
    let lifecycle = crate::app_state::lifecycle();
    if let Ok(reservation) = crate::lifecycle::LifecycleRegistry::reserve(
        &lifecycle,
        transfer_id,
        crate::app_state::ResourceKind::Transfer,
        crate::app_state::ResourceOwner::Headless(Uuid::new_v4()),
    ) {
        let _ = reservation.activate();
    }

    flag
}

/// Remove a transfer from the registry once it finishes (success or failure).
pub fn unregister(transfer_id: &str) {
    registry().lock().unwrap().remove(transfer_id);

    // Mark the lifecycle entry as Closed.
    let _ = crate::app_state::lifecycle()
        .lock()
        .unwrap()
        .close(transfer_id, None);
}

/// Request cancellation of an in-flight transfer. Returns true if a transfer
/// with that id was registered.
pub fn cancel_transfer(transfer_id: &str) -> bool {
    match registry().lock().unwrap().get(transfer_id) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Number of currently-registered (in-flight) transfers.
pub fn active_count() -> usize {
    registry().lock().unwrap().len()
}

/// `write_all` that survives a peer which accepts only smaller writes.
///
/// A second line of defence behind [`upload_chunk_for_server`], for servers that
/// are not recognised by banner.
///
/// A write that reports zero bytes is not progress, and for a plain file it would
/// mean the disk is gone; for an SFTP handle it can also mean "this write is
/// larger than the server's packet size". On `Ok(0)` the limit is halved (down to
/// [`SFTP_SAFE_WRITE`]) and the same data retried, and the smaller limit sticks in
/// `limit` so the rest of the transfer does not repeat the discovery.
fn write_all_shrinking<W: Write>(
    writer: &mut W,
    mut data: &[u8],
    limit: &mut usize,
) -> io::Result<()> {
    while !data.is_empty() {
        let take = data.len().min(*limit);
        match writer.write(&data[..take]) {
            Ok(0) if *limit > SFTP_SAFE_WRITE => *limit = (*limit / 2).max(SFTP_SAFE_WRITE),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write whole buffer",
                ))
            }
            Ok(n) => data = &data[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Copy `reader` into `writer` in chunks, aborting with an `Interrupted` error
/// as soon as `cancel` is set. Returns the number of bytes copied this call (the
/// caller adds any pre-existing resume offset to get the file's total length).
pub fn copy_cancellable<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    cancel: &AtomicBool,
) -> io::Result<u64> {
    copy_cancellable_with_chunk(reader, writer, cancel, COPY_CHUNK)
}

/// [`copy_cancellable`] with an explicit chunk size, for peers that need smaller
/// writes than the default (see [`upload_chunk_for_server`]).
pub fn copy_cancellable_with_chunk<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    cancel: &AtomicBool,
    chunk: usize,
) -> io::Result<u64> {
    let chunk = chunk.max(1);
    let mut buf = vec![0u8; chunk];
    let mut write_limit = chunk;
    let mut total: u64 = 0;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "transfer cancelled",
            ));
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        write_all_shrinking(writer, &buf[..n], &mut write_limit)?;
        total += n as u64;
    }
    writer.flush()?;
    Ok(total)
}

/// Decide where a resumed transfer should start.
///
/// `existing` is how many bytes already exist on the destination; `total` is the
/// full size of the source (when known). Returns the byte offset to resume from:
/// - `Some(0)` when not resuming, or when the destination is empty, or when the
///   destination is already as large as (or larger than) the source — in which
///   case the caller should restart from scratch to avoid a corrupt/over-long
///   result;
/// - `Some(existing)` when a partial transfer can be continued.
pub fn resume_offset(resume: bool, existing: u64, total: Option<u64>) -> u64 {
    if !resume || existing == 0 {
        return 0;
    }
    match total {
        // Destination already complete or longer than source: don't append, redo.
        Some(t) if existing >= t => 0,
        _ => existing,
    }
}

#[cfg(test)]
mod tests {
    /// A writer that, like an SFTP handle on a server with small packets, accepts
    /// nothing at all when handed more than `max` bytes.
    struct PickyWriter {
        max: usize,
        written: Vec<u8>,
    }
    impl std::io::Write for PickyWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if buf.len() > self.max {
                return Ok(0);
            }
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn copy_adapts_to_a_peer_that_refuses_large_writes() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let mut writer = PickyWriter {
            max: 32_000,
            written: Vec::new(),
        };
        let flag = AtomicBool::new(false);
        let n = copy_cancellable(&mut data.as_slice(), &mut writer, &flag).unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(writer.written, data);
    }

    #[test]
    fn dropbear_gets_small_upload_writes_and_everything_else_the_default() {
        assert_eq!(
            upload_chunk_for_server(Some("SSH-2.0-dropbear_2024.85")),
            16 * 1024
        );
        assert_eq!(
            upload_chunk_for_server(Some("SSH-2.0-OpenSSH_10.3")),
            64 * 1024
        );
        assert_eq!(upload_chunk_for_server(None), 64 * 1024);
    }

    #[test]
    fn copy_still_fails_when_even_small_writes_are_refused() {
        let mut writer = PickyWriter {
            max: 0,
            written: Vec::new(),
        };
        let flag = AtomicBool::new(false);
        let err = copy_cancellable(&mut [1u8; 10].as_slice(), &mut writer, &flag).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::WriteZero);
    }

    use super::*;
    use std::io::Cursor;

    #[test]
    fn copy_cancellable_copies_all_when_not_cancelled() {
        let data = vec![7u8; 200 * 1024];
        let mut reader = Cursor::new(data.clone());
        let mut writer: Vec<u8> = Vec::new();
        let flag = AtomicBool::new(false);
        let n = copy_cancellable(&mut reader, &mut writer, &flag).unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(writer, data);
    }

    #[test]
    fn copy_cancellable_aborts_when_flag_set() {
        let data = vec![0u8; 1024];
        let mut reader = Cursor::new(data);
        let mut writer: Vec<u8> = Vec::new();
        let flag = AtomicBool::new(true); // already cancelled
        let err = copy_cancellable(&mut reader, &mut writer, &flag).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn registry_cancel_roundtrip() {
        let id = "k6-test-transfer";
        let flag = register(id);
        assert!(!flag.load(Ordering::SeqCst));
        assert!(cancel_transfer(id));
        assert!(flag.load(Ordering::SeqCst));
        unregister(id);
        assert!(!cancel_transfer(id)); // gone after unregister
    }

    #[test]
    fn resume_offset_decisions() {
        // Not resuming -> always from 0.
        assert_eq!(resume_offset(false, 500, Some(1000)), 0);
        // Resuming, partial -> continue from existing.
        assert_eq!(resume_offset(true, 500, Some(1000)), 500);
        // Resuming but empty destination -> from 0.
        assert_eq!(resume_offset(true, 0, Some(1000)), 0);
        // Resuming but destination already complete/over -> restart from 0.
        assert_eq!(resume_offset(true, 1000, Some(1000)), 0);
        assert_eq!(resume_offset(true, 1200, Some(1000)), 0);
        // Resuming with unknown total -> trust existing.
        assert_eq!(resume_offset(true, 500, None), 500);
    }

    // ── A4: walk_local_dir tests ──────────────────────────────────────────

    #[test]
    fn walk_local_dir_single_file() {
        let temp = std::env::temp_dir().join(format!("agent2ssh-walk-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let file = temp.join("hello.txt");
        std::fs::write(&file, b"hello world").unwrap();

        let entries = walk_local_dir(&file).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, file);
        assert_eq!(entries[0].size, 11);

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn walk_local_dir_flat_directory() {
        let temp = std::env::temp_dir().join(format!("agent2ssh-walk-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(temp.join("a.txt"), b"aaa").unwrap();
        std::fs::write(temp.join("b.txt"), b"bbbb").unwrap();
        std::fs::write(temp.join("c.txt"), b"cc").unwrap();

        let entries = walk_local_dir(&temp).unwrap();
        assert_eq!(entries.len(), 3);
        // All entries should have correct sizes.
        let sizes: std::collections::HashSet<u64> = entries.iter().map(|e| e.size).collect();
        assert!(sizes.contains(&3));
        assert!(sizes.contains(&4));
        assert!(sizes.contains(&2));

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn walk_local_dir_nested_directories() {
        let temp = std::env::temp_dir().join(format!("agent2ssh-walk-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(temp.join("sub1").join("sub2")).unwrap();
        std::fs::write(temp.join("root.txt"), b"root").unwrap();
        std::fs::write(temp.join("sub1").join("mid.txt"), b"mid").unwrap();
        std::fs::write(temp.join("sub1").join("sub2").join("deep.txt"), b"deep").unwrap();

        let entries = walk_local_dir(&temp).unwrap();
        assert_eq!(entries.len(), 3);

        // Verify relative paths (normalize separators for cross-platform).
        let relative_paths: std::collections::HashSet<String> = entries
            .iter()
            .map(|e| e.relative.display().to_string().replace('\\', "/"))
            .collect();
        assert!(relative_paths.contains("root.txt"));
        assert!(relative_paths.contains("sub1/mid.txt"));
        assert!(relative_paths.contains("sub1/sub2/deep.txt"));

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn walk_local_dir_skips_broken_symlinks() {
        let temp = std::env::temp_dir().join(format!("agent2ssh-walk-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(temp.join("real.txt"), b"real").unwrap();

        // Create a broken symlink.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink("/nonexistent/target", temp.join("broken")).unwrap();
        }

        let entries = walk_local_dir(&temp).unwrap();
        // Should find only the real file, skip the broken symlink.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, temp.join("real.txt"));

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn walk_local_dir_skips_symlink_to_directory() {
        let temp = std::env::temp_dir().join(format!("agent2ssh-walk-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(temp.join("real_dir")).unwrap();
        std::fs::write(temp.join("real_dir/file.txt"), b"file").unwrap();
        std::fs::write(temp.join("top.txt"), b"top").unwrap();

        // Create a symlink pointing to a directory within the tree.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(temp.join("real_dir"), temp.join("link_dir")).unwrap();
        }

        let entries = walk_local_dir(&temp).unwrap();

        // Should find: top.txt and real_dir/file.txt.
        // Should NOT recurse into the symlinked directory (would cause
        // duplicate real_dir/file.txt entries).
        assert_eq!(entries.len(), 2);

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn walk_local_dir_empty_directory() {
        let temp = std::env::temp_dir().join(format!("agent2ssh-walk-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();

        let entries = walk_local_dir(&temp).unwrap();
        assert!(entries.is_empty());

        std::fs::remove_dir_all(&temp).unwrap();
    }
}
