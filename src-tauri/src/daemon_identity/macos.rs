use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
};

const SOCKET: &str = ".daemon-ipc/s";
const INSTANCE: &str = "daemon.instance.json";
const CONFIG_ID: &str = "daemon.config-id";
const LOCK: &str = ".daemon-pid.lock";

fn lock(dir: &Path) -> Result<fs::File> {
    use fs2::FileExt;
    use std::time::{Duration, Instant};
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(LOCK))?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.uid() != unsafe { libc::geteuid() } {
        bail!("invalid daemon PID lock file");
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                bail!("timed out acquiring daemon PID lock; records retained")
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub version: u32,
    pub instance_id: String,
    pub pid: u32,
    pub config_id: String,
    pub endpoint: Endpoint,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub kind: String,
    pub path: String,
}

fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| anyhow::anyhow!("identity entropy unavailable: {e}"))?;
    Ok(hex::encode(bytes))
}

// ACL allow entries can grant rights beyond the POSIX mode. Deny entries only
// narrow access; unknown ACL state is refused.
fn no_acl_grants(path: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    extern "C" {
        fn acl_get_file(path: *const libc::c_char, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_get_entry(
            acl: *mut libc::c_void,
            index: libc::c_int,
            entry: *mut *mut libc::c_void,
        ) -> libc::c_int;
        fn acl_get_tag_type(entry: *mut libc::c_void, tag: *mut libc::c_int) -> libc::c_int;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    let name = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: name is a live C string; a returned ACL is freed once below.
    let acl = unsafe { acl_get_file(name.as_ptr(), 0x100) };
    if acl.is_null() {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ENOENT) && fs::symlink_metadata(path).is_ok() {
            return Ok(());
        }
        return Err(e).context("cannot verify directory ACL");
    }
    let result = (|| -> Result<()> {
        let mut index = 0; // ACL_FIRST_ENTRY
        loop {
            let mut entry = std::ptr::null_mut();
            if unsafe { acl_get_entry(acl, index, &mut entry) } != 0 {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(libc::EINVAL) {
                    break;
                }
                return Err(e).context("cannot inspect directory ACL");
            }
            let mut tag = 0;
            if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("cannot inspect directory ACL entry");
            }
            if tag != 2 {
                bail!("directory ACL grants or has unknown access; daemon IPC refused");
            }
            index = -1; // ACL_NEXT_ENTRY
        }
        Ok(())
    })();
    unsafe {
        acl_free(acl);
    }
    result
}

fn owner_dir(path: &Path) -> Result<PathBuf> {
    let md = fs::symlink_metadata(path).context("cannot inspect daemon config directory")?;
    if !md.file_type().is_dir() || md.uid() != unsafe { libc::geteuid() } || md.mode() & 0o022 != 0
    {
        bail!("daemon IPC requires a non-symlink configuration directory owned by the current user and not writable by others");
    }
    let dir = fs::canonicalize(path).context("cannot resolve daemon config directory")?;
    for ancestor in dir.ancestors() {
        let metadata = fs::metadata(ancestor)?;
        let owner = metadata.uid();
        if (owner != 0 && owner != unsafe { libc::geteuid() })
            || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
        {
            bail!("daemon configuration path has an untrusted writable or owned ancestor");
        }
        no_acl_grants(ancestor)?;
    }
    Ok(dir)
}

/// Reject unsafe paths before loading or creating the administrative token.
pub fn validate_config(path: &Path) -> Result<()> {
    owner_dir(path).map(|_| ())
}

/// Create only the final directory after validating its existing parent; never
/// walk through an untrusted path while recursively creating directories.
pub fn prepare_config(path: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            owner_dir(parent).context("cannot safely create daemon config directory")?;
            match fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(e) => return Err(e.into()),
    }
    validate_config(path)
}

/// Create credentials with owner-only permissions from the first write. The
/// PID lock also serializes concurrent daemon token initialization.
pub fn load_or_create_token(config: &Path) -> Result<String> {
    let dir = owner_dir(config)?;
    let _lock = lock(&dir)?;
    let path = dir.join("daemon.token");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .context("cannot open daemon token securely")?;
    let md = file.metadata()?;
    if !md.file_type().is_file()
        || md.uid() != unsafe { libc::geteuid() }
        || md.nlink() != 1
        || md.len() > super::MAX_FRAME as u64
    {
        bail!("daemon token is not a safe owner-controlled regular file");
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    no_acl_grants(&path).context("daemon token ACL is not private")?;
    let observed = fs::symlink_metadata(&path)?;
    if !observed.file_type().is_file() || (observed.dev(), observed.ino()) != (md.dev(), md.ino()) {
        bail!("daemon token path changed while opening");
    }
    let mut token = String::new();
    file.read_to_string(&mut token)?;
    let token = token.trim().to_string();
    if !token.is_empty() {
        return Ok(token);
    }
    let token = uuid::Uuid::new_v4().to_string();
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    Ok(token)
}

fn private_socket_dir(dir: &Path, create: bool) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let path = dir.join(".daemon-ipc");
    if create {
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let md = fs::symlink_metadata(&path)?;
    if !md.file_type().is_dir() || md.uid() != unsafe { libc::geteuid() } || md.mode() & 0o077 != 0
    {
        bail!(
            "daemon socket directory must be owned by the current user and mode 0700 (or stricter)"
        );
    }
    no_acl_grants(&path)?;
    Ok(())
}

fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => bail!(
            "{} already exists; retained for explicit recovery",
            path.display()
        ),
    }
}

// Install without following or replacing any destination. The temporary file is
// on the same filesystem and is never used as an authority on its own.
fn install(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().context("missing parent")?;
    let tmp = dir.join(format!(".daemon-identity-{}.tmp", new_id()?));
    let result = (|| -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::hard_link(&tmp, path)
            .with_context(|| format!("cannot publish {} without replacement", path.display()))?;
        Ok(())
    })();
    let _ = fs::remove_file(tmp);
    result
}

fn config_id(dir: &Path) -> Result<String> {
    let path = dir.join(CONFIG_ID);
    if matches!(fs::symlink_metadata(&path), Err(ref e) if e.kind() == std::io::ErrorKind::NotFound)
    {
        install(&path, new_id()?.as_bytes())?;
    }
    read_config_id(dir)
}

fn read_config_id(dir: &Path) -> Result<String> {
    let path = dir.join(CONFIG_ID);
    let md = fs::symlink_metadata(&path)?;
    if !md.file_type().is_file()
        || md.uid() != unsafe { libc::geteuid() }
        || md.mode() & 0o077 != 0
        || md.len() != 32
    {
        bail!("invalid daemon config identifier; retained");
    }
    let id = fs::read_to_string(path)?;
    if !super::valid_id(&id) {
        bail!("invalid daemon config identifier; retained");
    }
    Ok(id)
}

/// Check published discovery records for a freshly spawned child. This is
/// only a consistency check; it does not bind a socket to a process object.
pub fn published_for(config: &Path, pid: u32) -> Result<bool> {
    let dir = owner_dir(config)?;
    private_socket_dir(&dir, false)?;
    let pid_path = dir.join("daemon.pid");
    let record_path = dir.join(INSTANCE);
    let pid_md = fs::symlink_metadata(&pid_path)?;
    let record_md = fs::symlink_metadata(&record_path)?;
    let socket_md = fs::symlink_metadata(dir.join(SOCKET))?;
    if !pid_md.file_type().is_file()
        || pid_md.len() > 20
        || !record_md.file_type().is_file()
        || record_md.len() > 4096
        || !socket_md.file_type().is_socket()
        || pid_md.uid() != unsafe { libc::geteuid() }
        || record_md.uid() != unsafe { libc::geteuid() }
    {
        return Ok(false);
    }
    let record: Instance = serde_json::from_slice(&fs::read(record_path)?)?;
    Ok(fs::read(pid_path)? == pid.to_string().as_bytes()
        && record.version == super::VERSION
        && record.pid == pid
        && super::valid_id(&record.instance_id)
        && super::valid_id(&record.config_id)
        && record.config_id == read_config_id(&dir)?
        && record.endpoint.kind == "unix"
        && record.endpoint.path == SOCKET)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DiscoverySnapshot {
    pub(crate) socket_path: PathBuf,
    pub(crate) instance_id: String,
    dir: PathBuf,
    pid: u32,
    pid_bytes: Vec<u8>,
    record_bytes: Vec<u8>,
    pid_inode: (u64, u64),
    record_inode: (u64, u64),
    socket_inode: (u64, u64),
    config_inode: (u64, u64),
}

/// Presence of any IPC record is not identity proof; it does mean the legacy
/// compatibility path must not ignore an inconsistent new-protocol instance.
pub fn has_ipc_artifacts(config: &Path) -> Result<bool> {
    let dir = owner_dir(config)?;
    let _lock = lock(&dir)?;
    let ipc_dir = dir.join(".daemon-ipc");
    match fs::symlink_metadata(&ipc_dir) {
        Ok(md) if !md.file_type().is_dir() => bail!("unsafe IPC socket directory"),
        Ok(_) => private_socket_dir(&dir, false)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    for path in [dir.join(INSTANCE), dir.join(SOCKET)] {
        match fs::symlink_metadata(path) {
            Ok(_) => return Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(false)
}

/// Snapshot short file transactions under the existing publication lock.
/// Contents and metadata are discovery evidence only and never authorize stop.
pub(crate) fn discovery_snapshot(config: &Path, expected_pid: u32) -> Result<DiscoverySnapshot> {
    let dir = owner_dir(config)?;
    let _lock = lock(&dir)?;
    private_socket_dir(&dir, false)?;
    let pid_path = dir.join("daemon.pid");
    let record_path = dir.join(INSTANCE);
    let socket_path = dir.join(SOCKET);
    let config_path = dir.join(CONFIG_ID);
    let pid_md = fs::symlink_metadata(&pid_path)?;
    let record_md = fs::symlink_metadata(&record_path)?;
    let socket_md = fs::symlink_metadata(&socket_path)?;
    let config_md = fs::symlink_metadata(&config_path)?;
    let owner = unsafe { libc::geteuid() };
    if !pid_md.file_type().is_file()
        || pid_md.len() > 20
        || pid_md.uid() != owner
        || !record_md.file_type().is_file()
        || record_md.len() > 4096
        || record_md.uid() != owner
        || !config_md.file_type().is_file()
        || !socket_md.file_type().is_socket()
        || socket_md.uid() != owner
        || socket_md.mode() & 0o077 != 0
    {
        bail!("unsafe or invalid IPC discovery records");
    }
    let pid_bytes = fs::read(&pid_path)?;
    let record_bytes = fs::read(&record_path)?;
    let record: Instance =
        serde_json::from_slice(&record_bytes).context("invalid IPC instance record")?;
    if expected_pid == 0
        || pid_bytes != expected_pid.to_string().as_bytes()
        || record.version != super::VERSION
        || record.pid != expected_pid
        || !super::valid_id(&record.instance_id)
        || !super::valid_id(&record.config_id)
        || record.config_id != read_config_id(&dir)?
        || record.endpoint.kind != "unix"
        || record.endpoint.path != SOCKET
    {
        bail!("IPC discovery records conflict");
    }
    Ok(DiscoverySnapshot {
        socket_path,
        instance_id: record.instance_id,
        dir,
        pid: expected_pid,
        pid_bytes,
        record_bytes,
        pid_inode: (pid_md.dev(), pid_md.ino()),
        record_inode: (record_md.dev(), record_md.ino()),
        socket_inode: (socket_md.dev(), socket_md.ino()),
        config_inode: (config_md.dev(), config_md.ino()),
    })
}

pub(crate) fn verify_discovery_snapshot(previous: &DiscoverySnapshot) -> Result<()> {
    if discovery_snapshot(&previous.dir, previous.pid)? != *previous {
        bail!("IPC records or socket changed during discovery");
    }
    Ok(())
}

pub struct Publication {
    pub listener: UnixListener,
    pub instance: Instance,
    dir: PathBuf,
    record: Vec<u8>,
    socket_inode: (u64, u64),
}

/// Bind before publication but do not accept connections until this succeeds.
/// Interrupted publication intentionally leaves evidence rather than guessing.
pub fn publish(config: &Path, pid: u32) -> Result<Publication> {
    let dir = owner_dir(config)?;
    let _lock = lock(&dir)?;
    for name in ["daemon.pid", INSTANCE] {
        absent(&dir.join(name))?;
    }
    private_socket_dir(&dir, true)?;
    absent(&dir.join(SOCKET))?;
    let config_id = config_id(&dir)?;
    let instance = Instance {
        version: 1,
        instance_id: new_id()?,
        pid,
        config_id,
        endpoint: Endpoint {
            kind: "unix".into(),
            path: SOCKET.into(),
        },
    };
    let record = serde_json::to_vec(&instance)?;
    let socket_path = dir.join(SOCKET);
    let listener = UnixListener::bind(&socket_path)
        .context("cannot bind private daemon socket (path may be too long)")?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    let md = fs::symlink_metadata(&socket_path)?;
    let socket_inode = (md.dev(), md.ino());
    listener.set_nonblocking(true)?;
    install(&dir.join("daemon.pid"), pid.to_string().as_bytes())?;
    install(&dir.join(INSTANCE), &record)?;
    if let Ok(d) = fs::File::open(&dir) {
        let _ = d.sync_all();
    }
    Ok(Publication {
        listener,
        instance,
        dir,
        record,
        socket_inode,
    })
}

impl Publication {
    /// Called by the daemon only after its listener is closed. A mismatch in
    /// any of the three paths preserves everything for manual inspection.
    pub fn cleanup(self) -> Result<bool> {
        drop(self.listener);
        let _lock = lock(&self.dir)?;
        let pid = self.dir.join("daemon.pid");
        let record = self.dir.join(INSTANCE);
        let socket = self.dir.join(SOCKET);
        let pid_md = fs::symlink_metadata(&pid)?;
        let record_md = fs::symlink_metadata(&record)?;
        let socket_md = fs::symlink_metadata(&socket)?;
        if !pid_md.file_type().is_file()
            || !record_md.file_type().is_file()
            || !socket_md.file_type().is_socket()
            || (socket_md.dev(), socket_md.ino()) != self.socket_inode
            || fs::read(&pid)? != self.instance.pid.to_string().as_bytes()
            || fs::read(&record)? != self.record
        {
            return Ok(false);
        }
        fs::remove_file(socket)?;
        fs::remove_file(record)?;
        fs::remove_file(pid)?;
        if let Ok(d) = fs::File::open(&self.dir) {
            let _ = d.sync_all();
        }
        Ok(true)
    }
}

/// Explicit, operator-directed recovery for a complete crashed publication.
/// ESRCH and ECONNREFUSED are observations, not proof of the old instance;
/// uncooperative legacy writers and inherited descriptors remain a limitation.
pub fn recover(config: &Path, expected_pid: u32, expected_instance: &str) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    if !super::valid_id(expected_instance) {
        bail!("invalid expected instance id");
    }
    let dir = owner_dir(config)?;
    let _lock = lock(&dir)?;
    private_socket_dir(&dir, false)?;
    let pid_path = dir.join("daemon.pid");
    let record_path = dir.join(INSTANCE);
    let sock_path = dir.join(SOCKET);
    let pid_md = fs::symlink_metadata(&pid_path)?;
    let record_md = fs::symlink_metadata(&record_path)?;
    let socket_md = fs::symlink_metadata(&sock_path)?;
    if !pid_md.file_type().is_file()
        || !record_md.file_type().is_file()
        || !socket_md.file_type().is_socket()
        || record_md.len() > 4096
    {
        bail!("identity recovery paths are not expected regular files and socket; retained");
    }
    let raw_pid = fs::read(&pid_path)?;
    let raw_record = fs::read(&record_path)?;
    let record: Instance =
        serde_json::from_slice(&raw_record).context("invalid instance record; retained")?;
    if raw_pid != expected_pid.to_string().as_bytes()
        || record.version != super::VERSION
        || record.pid != expected_pid
        || record.instance_id != expected_instance
        || !super::valid_id(&record.config_id)
        || record.config_id != read_config_id(&dir)?
        || record.endpoint.kind != "unix"
        || record.endpoint.path != SOCKET
    {
        bail!("identity records differ from the expected instance; retained");
    }
    let pid = i32::try_from(expected_pid)
        .ok()
        .filter(|p| *p > 0)
        .context("invalid expected PID")?;
    if unsafe { libc::kill(pid, 0) } != -1
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    {
        bail!("daemon PID exists or cannot be observed; records retained");
    }
    // Nonblocking connect: a live endpoint, a full backlog, or an uncertain
    // error are all refusals. It cannot stall while the file lock is held.
    let path = sock_path.as_os_str().as_bytes();
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if path.len() >= addr.sun_path.len() {
        bail!("socket path too long");
    }
    addr.sun_len = std::mem::size_of_val(&addr) as u8;
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, b) in addr.sun_path.iter_mut().zip(path) {
        *slot = *b as libc::c_char;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        let e = std::io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        return Err(e.into());
    }
    let rc = unsafe {
        libc::connect(
            fd,
            (&addr as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&addr) as libc::socklen_t,
        )
    };
    let error = if rc == -1 {
        std::io::Error::last_os_error().raw_os_error()
    } else {
        None
    };
    unsafe {
        libc::close(fd);
    }
    if error != Some(libc::ECONNREFUSED) {
        bail!("socket may still have a listener; records retained");
    }
    let now = fs::symlink_metadata(&sock_path)?;
    if !now.file_type().is_socket()
        || (now.dev(), now.ino()) != (socket_md.dev(), socket_md.ino())
        || fs::read(&pid_path)? != raw_pid
        || fs::read(&record_path)? != raw_record
    {
        bail!("identity records changed during recovery; retained");
    }
    fs::remove_file(sock_path)?;
    fs::remove_file(record_path)?;
    fs::remove_file(pid_path)?;
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod token_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn token_is_private_from_creation_and_empty_file_is_reinitialized() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let first = load_or_create_token(dir.path()).unwrap();
        let path = dir.path().join("daemon.token");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(load_or_create_token(dir.path()).unwrap(), first);
        fs::write(&path, b" \n").unwrap();
        let replacement = load_or_create_token(dir.path()).unwrap();
        assert_ne!(replacement, first);
        assert_eq!(fs::read_to_string(path).unwrap(), replacement);
    }

    #[test]
    fn token_symlinks_hardlinks_and_acl_grants_are_rejected() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let target = dir.path().join("other");
        let path = dir.path().join("daemon.token");
        fs::write(&target, b"untouched").unwrap();
        symlink(&target, &path).unwrap();
        assert!(load_or_create_token(dir.path()).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&target, &path).unwrap();
        assert!(load_or_create_token(dir.path()).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"synthetic").unwrap();
        let status = std::process::Command::new("chmod")
            .args(["+a", "everyone allow read"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(load_or_create_token(dir.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"synthetic");
    }
}
