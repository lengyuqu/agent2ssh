//! SFTP over `russh-sftp`.
//!
//! The facade is synchronous and `russh-sftp` is not, so every call is one
//! `block_on` on the transport runtime. File handles are the same deal: the
//! wrapper implements `Read`/`Write`/`Seek` by driving the async halves to
//! completion, one call at a time.
//!
//! Two mapping details worth knowing:
//!
//! - `FileAttributes::permissions` already carries the file-type bits
//!   (`russh-sftp` tests `S_IFDIR` against it), so it maps straight onto
//!   [`FileStat::perm`] with no translation. The legacy listing code reads the
//!   same bits.
//! - `FileAttributes::atime`/`mtime` are `u32` seconds, while [`FileStat`] holds
//!   `u64`; the widening is explicit here rather than at every call site.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh_sftp::client::error::Error as RhError;
use russh_sftp::client::fs::File as RhFile;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags as RhOpenFlags};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use super::block_on;
use crate::transport::types::{Error, ErrorKind, FileStat, OpenFlags, OpenType};

impl From<RhError> for Error {
    fn from(error: RhError) -> Self {
        // `russh-sftp` reports both protocol-level status codes and transport
        // failures through one type; neither maps onto the facade's four named
        // kinds, so the message carries the detail.
        Error::new(ErrorKind::Other, error.to_string())
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn file_stat(attributes: FileAttributes) -> FileStat {
    FileStat {
        size: attributes.size,
        uid: attributes.uid,
        gid: attributes.gid,
        perm: attributes.permissions,
        atime: attributes.atime.map(u64::from),
        mtime: attributes.mtime.map(u64::from),
    }
}

fn rh_flags(flags: OpenFlags) -> RhOpenFlags {
    let mut out = RhOpenFlags::empty();
    if flags.contains(OpenFlags::READ) {
        out |= RhOpenFlags::READ;
    }
    if flags.contains(OpenFlags::WRITE) {
        out |= RhOpenFlags::WRITE;
    }
    if flags.contains(OpenFlags::APPEND) {
        out |= RhOpenFlags::APPEND;
    }
    if flags.contains(OpenFlags::CREATE) {
        out |= RhOpenFlags::CREATE;
    }
    if flags.contains(OpenFlags::TRUNCATE) {
        out |= RhOpenFlags::TRUNCATE;
    }
    if flags.contains(OpenFlags::EXCLUSIVE) {
        out |= RhOpenFlags::EXCLUDE;
    }
    out
}

/// Only the permission bits, with everything else left absent.
///
/// `FileAttributes::default()` is *not* usable here: it fills in size 0 and
/// uid/gid 0, which an OPEN would then apply to the file.
fn attributes_with_mode(mode: i32) -> FileAttributes {
    FileAttributes {
        size: None,
        uid: None,
        user: None,
        gid: None,
        group: None,
        permissions: Some(mode as u32),
        atime: None,
        mtime: None,
    }
}

/// An SFTP session.
pub struct Sftp {
    session: Arc<SftpSession>,
}

impl Sftp {
    pub(super) fn new(session: SftpSession) -> Self {
        Self {
            session: Arc::new(session),
        }
    }

    pub fn open(&self, path: &Path) -> Result<File, Error> {
        let session = Arc::clone(&self.session);
        let path = path_string(path);
        Ok(File {
            inner: block_on(async move { session.open(path).await })?,
        })
    }

    pub fn create(&self, path: &Path) -> Result<File, Error> {
        let session = Arc::clone(&self.session);
        let path = path_string(path);
        Ok(File {
            inner: block_on(async move { session.create(path).await })?,
        })
    }

    pub fn open_mode(
        &self,
        path: &Path,
        flags: OpenFlags,
        mode: i32,
        _open_type: OpenType,
    ) -> Result<File, Error> {
        let session = Arc::clone(&self.session);
        let path = path_string(path);
        let flags = rh_flags(flags);
        let attributes = attributes_with_mode(mode);
        Ok(File {
            inner: block_on(async move {
                session
                    .open_with_flags_and_attributes(path, flags, attributes)
                    .await
            })?,
        })
    }

    pub fn stat(&self, path: &Path) -> Result<FileStat, Error> {
        let session = Arc::clone(&self.session);
        let path = path_string(path);
        let attributes = block_on(async move { session.metadata(path).await })?;
        Ok(file_stat(attributes))
    }

    pub fn readdir(&self, path: &Path) -> Result<Vec<(PathBuf, FileStat)>, Error> {
        let session = Arc::clone(&self.session);
        let path = path_string(path);
        let entries = block_on(async move { session.read_dir(path).await })?;
        // `DirEntry::path` composes the parent and the name, which is what the
        // legacy backend returned and what the listing formatter expects.
        Ok(entries
            .map(|entry| (PathBuf::from(entry.path()), file_stat(entry.metadata())))
            .collect())
    }

    pub fn mkdir(&self, path: &Path, _mode: i32) -> Result<(), Error> {
        // `russh-sftp` sends SFTP `mkdir` without attributes; the server applies
        // its own mode. The legacy backend passed `mode` explicitly, so a
        // strict-permission caller would see a difference here.
        let session = Arc::clone(&self.session);
        let path = path_string(path);
        block_on(async move { session.create_dir(path).await })?;
        Ok(())
    }

    pub fn rmdir(&self, path: &Path) -> Result<(), Error> {
        let session = Arc::clone(&self.session);
        let path = path_string(path);
        block_on(async move { session.remove_dir(path).await })?;
        Ok(())
    }

    pub fn rename(&self, src: &Path, dst: &Path, _overwrite: bool) -> Result<(), Error> {
        // `overwrite` has no equivalent: SFTP v3 `rename` fails when the target
        // exists, and asking for POSIX semantics needs an extension this crate
        // does not negotiate. The one call site passes `false`.
        let session = Arc::clone(&self.session);
        let src = path_string(src);
        let dst = path_string(dst);
        block_on(async move { session.rename(src, dst).await })?;
        Ok(())
    }

    pub fn unlink(&self, path: &Path) -> Result<(), Error> {
        let session = Arc::clone(&self.session);
        let path = path_string(path);
        block_on(async move { session.remove_file(path).await })?;
        Ok(())
    }
}

/// An open SFTP file handle.
pub struct File {
    inner: RhFile,
}

impl File {
    pub fn stat(&mut self) -> Result<FileStat, Error> {
        let attributes = block_on(async { self.inner.metadata().await })?;
        Ok(file_stat(attributes))
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // `russh-sftp`'s async reads already report `std::io::Error`, so no
        // translation is needed on this path.
        block_on(async { self.inner.read(buf).await })
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        block_on(async { self.inner.write(buf).await })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        block_on(async { self.inner.flush().await })
    }
}

impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        block_on(async { self.inner.seek(pos).await })
    }
}
