use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// A single-slot, `(mtime, len)`-keyed cache for a config file parsed from disk.
///
/// Several config files under `~/.agent2ssh/` are read on hot paths — every
/// exec (risk rules, limits), every authenticated daemon request (scoped
/// tokens), every fired event or error diagnostic (webhook, anomaly). Re-reading
/// and re-parsing TOML on each call is wasted I/O and CPU. `ConfigCache`
/// memoizes the parsed value and only re-runs the loader when the backing file's
/// modification time or length changes, so external edits are still picked up
/// promptly without paying for a read on every call.
///
/// One slot suffices: each cache instance is dedicated to a single config type,
/// which in production maps to one fixed path. If the path changes (e.g. tests
/// overriding `AGENT2SSH_CONFIG_DIR`), the signature mismatch simply forces a
/// reload — always correct, just not cached across alternating paths.
pub struct ConfigCache<T> {
    inner: Mutex<Option<Entry<T>>>,
}

struct Entry<T> {
    path: PathBuf,
    signature: Option<(SystemTime, u64)>,
    value: T,
}

/// `None` when the file is absent or its metadata is unreadable — both treated
/// as "reload"; a missing file is the common case for optional configs.
fn file_signature(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

impl<T: Clone> ConfigCache<T> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Return the cached value if the file is unchanged; otherwise run `loader`,
    /// cache its result, and return it. `loader` performs the actual read/parse
    /// and is responsible for handling a missing file (e.g. returning a default).
    pub fn load_with(&self, path: &Path, loader: impl FnOnce() -> Result<T>) -> Result<T> {
        let signature = file_signature(path);
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = guard.as_ref() {
            if entry.path == path && entry.signature == signature {
                return Ok(entry.value.clone());
            }
        }
        let value = loader()?;
        *guard = Some(Entry {
            path: path.to_path_buf(),
            signature,
            value: value.clone(),
        });
        Ok(value)
    }

    /// Drop any cached value, forcing the next `load_with` to re-read. Call this
    /// after an in-process write so the new value is observed immediately,
    /// regardless of filesystem mtime granularity.
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}

impl<T: Clone> Default for ConfigCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reloads_only_when_file_changes() {
        let dir = std::env::temp_dir().join(format!("a2s-cfgcache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfg.txt");
        let cache: ConfigCache<String> = ConfigCache::new();

        // Missing file -> loader runs and returns a default.
        let loads = std::cell::Cell::new(0);
        let load = |path: &Path| {
            cache.load_with(path, || {
                loads.set(loads.get() + 1);
                Ok(std::fs::read_to_string(path).unwrap_or_else(|_| "default".into()))
            })
        };
        assert_eq!(load(&path).unwrap(), "default");
        assert_eq!(loads.get(), 1);

        // Second call with no change -> served from cache, loader not re-run.
        assert_eq!(load(&path).unwrap(), "default");
        assert_eq!(loads.get(), 1);

        // Writing the file changes the signature -> reload.
        std::fs::write(&path, "v1-content").unwrap();
        assert_eq!(load(&path).unwrap(), "v1-content");
        assert_eq!(loads.get(), 2);
        assert_eq!(load(&path).unwrap(), "v1-content");
        assert_eq!(loads.get(), 2);

        // Explicit invalidation forces a reload even when unchanged.
        cache.invalidate();
        assert_eq!(load(&path).unwrap(), "v1-content");
        assert_eq!(loads.get(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
