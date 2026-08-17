//! Local asciicast v2 terminal recordings.
//!
//! Recordings contain raw terminal output and may therefore contain secrets.
//! They live under `~/.agent2ssh/recordings`, which is deliberately absent from
//! the WebDAV allow-list. IDs are UUIDs rather than user-controlled filenames.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use uuid::Uuid;

use crate::store::{config_dir, restrict_file_to_owner};

const CONFIG_FILE: &str = "recording.json";
const RECORDINGS_DIR: &str = "recordings";
const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingConfig {
    /// Recording is intentionally opt-in because output may contain secrets.
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingInfo {
    pub id: String,
    pub host: String,
    pub created_at: DateTime<Utc>,
    pub duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingContent {
    pub info: RecordingInfo,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CastHeader {
    version: u8,
    width: u32,
    height: u32,
    timestamp: i64,
    host: String,
}

pub struct Recorder {
    id: Uuid,
    writer: BufWriter<File>,
    start: Instant,
    pending: Vec<u8>,
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

pub fn recordings_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join(RECORDINGS_DIR))
}

fn ensure_recordings_dir() -> Result<PathBuf> {
    let path = recordings_dir()?;
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create recording directory {}", path.display()))?;
    restrict_file_to_owner(&path)?;
    Ok(path)
}

pub fn load_recording_config() -> RecordingConfig {
    let Ok(path) = config_path() else {
        return RecordingConfig::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return RecordingConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_recording_config(config: &RecordingConfig) -> Result<()> {
    let path = config_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("recording config path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!("{CONFIG_FILE}.tmp.{}", std::process::id()));
    let raw = serde_json::to_vec_pretty(config)?;
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(&raw)?;
    file.sync_all()?;
    restrict_file_to_owner(&temp)?;
    fs::rename(&temp, &path)?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

fn parse_id(id: &str) -> Result<Uuid> {
    let parsed = Uuid::parse_str(id).map_err(|_| anyhow!("invalid recording id"))?;
    if parsed.to_string() != id {
        return Err(anyhow!("recording id must be a canonical UUID"));
    }
    Ok(parsed)
}

fn recording_path(id: &str) -> Result<PathBuf> {
    let id = parse_id(id)?;
    Ok(recordings_dir()?.join(format!("{id}.cast")))
}

fn checked_existing_path(id: &str) -> Result<PathBuf> {
    let path = recording_path(id)?;
    let metadata =
        fs::symlink_metadata(&path).with_context(|| format!("recording '{id}' does not exist"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow!("recording path is not a regular file"));
    }
    Ok(path)
}

impl Recorder {
    pub fn new(host: &str, cols: u32, rows: u32) -> Result<Self> {
        let directory = ensure_recordings_dir()?;
        let id = Uuid::new_v4();
        let path = directory.join(format!("{id}.cast"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        restrict_file_to_owner(&path)?;
        let mut writer = BufWriter::new(file);
        let header = CastHeader {
            version: 2,
            width: cols,
            height: rows,
            timestamp: Utc::now().timestamp(),
            host: host.to_string(),
        };
        writeln!(writer, "{}", serde_json::to_string(&header)?)?;
        writer.flush()?;
        Ok(Self {
            id,
            writer,
            start: Instant::now(),
            pending: Vec::new(),
        })
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Record bytes while preserving UTF-8 code points split across SSH chunks.
    pub fn record_output(&mut self, data: &[u8]) -> Result<()> {
        self.pending.extend_from_slice(data);
        let split = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) => match error.error_len() {
                None => error.valid_up_to(),
                Some(_) => self.pending.len(),
            },
        };
        if split == 0 {
            return Ok(());
        }
        let chunk = String::from_utf8_lossy(&self.pending[..split]).into_owned();
        self.write_event("o", &chunk)?;
        self.pending.drain(..split);
        Ok(())
    }

    pub fn record_resize(&mut self, cols: u32, rows: u32) -> Result<()> {
        self.write_event("r", &format!("{cols}x{rows}"))
    }

    fn write_event(&mut self, event_type: &str, data: &str) -> Result<()> {
        let event = serde_json::json!([self.start.elapsed().as_secs_f64(), event_type, data]);
        writeln!(self.writer, "{event}")?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        if !self.pending.is_empty() {
            let chunk = String::from_utf8_lossy(&self.pending).into_owned();
            self.write_event("o", &chunk)?;
            self.pending.clear();
        }
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(())
    }
}

fn inspect_recording(path: &Path) -> Result<RecordingInfo> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow!("recording path is not a regular file"));
    }
    let id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("recording filename is invalid"))?;
    parse_id(id)?;
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let header_line = lines
        .next()
        .ok_or_else(|| anyhow!("recording is empty"))??;
    let header: CastHeader = serde_json::from_str(&header_line)?;
    if header.version != 2 {
        return Err(anyhow!("unsupported asciicast version"));
    }
    let mut duration_seconds = 0.0_f64;
    for line in lines {
        let Ok(line) = line else { continue };
        let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(timestamp) = event.get(0).and_then(|value| value.as_f64()) {
            if timestamp.is_finite() {
                duration_seconds = duration_seconds.max(timestamp);
            }
        }
    }
    let created_at = DateTime::from_timestamp(header.timestamp, 0)
        .ok_or_else(|| anyhow!("recording timestamp is invalid"))?;
    Ok(RecordingInfo {
        id: id.to_string(),
        host: header.host,
        created_at,
        duration_seconds,
        width: header.width,
        height: header.height,
        size_bytes: metadata.len(),
    })
}

pub fn list_recordings() -> Result<Vec<RecordingInfo>> {
    let directory = ensure_recordings_dir()?;
    let mut recordings = Vec::new();
    for entry in fs::read_dir(directory)? {
        let Ok(entry) = entry else { continue };
        if entry.path().extension().and_then(|value| value.to_str()) != Some("cast") {
            continue;
        }
        if let Ok(info) = inspect_recording(&entry.path()) {
            recordings.push(info);
        }
    }
    recordings.sort_by_key(|info| std::cmp::Reverse(info.created_at));
    Ok(recordings)
}

pub fn read_recording(id: &str) -> Result<RecordingContent> {
    let path = checked_existing_path(id)?;
    let metadata = fs::metadata(&path)?;
    if metadata.len() > MAX_READ_BYTES {
        return Err(anyhow!(
            "recording is too large to replay in the desktop (maximum {} MiB)",
            MAX_READ_BYTES / 1024 / 1024
        ));
    }
    let info = inspect_recording(&path)?;
    let content = fs::read_to_string(path).context("recording is not valid UTF-8")?;
    Ok(RecordingContent { info, content })
}

pub fn delete_recording(id: &str, confirmed: bool) -> Result<RecordingInfo> {
    if !confirmed {
        return Err(anyhow!("recording deletion requires explicit confirmation"));
    }
    let path = checked_existing_path(id)?;
    let info = inspect_recording(&path)?;
    fs::remove_file(path)?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_events(path: &Path) -> Vec<serde_json::Value> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn split_utf8_is_recovered_and_timestamps_are_monotonic() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.cast");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let file = options.open(&path).unwrap();
        let mut recorder = Recorder {
            id: Uuid::new_v4(),
            writer: BufWriter::new(file),
            start: Instant::now(),
            pending: Vec::new(),
        };
        recorder.record_output(&[0xE4, 0xB8]).unwrap();
        recorder.record_output(&[0xAD]).unwrap();
        recorder.record_output(b"x").unwrap();
        recorder.finish().unwrap();
        let events = parse_events(&path);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0][2], "中");
        assert_eq!(events[1][2], "x");
        assert!(events[1][0].as_f64().unwrap() >= events[0][0].as_f64().unwrap());
    }

    #[test]
    fn finish_flushes_incomplete_utf8_lossily() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.cast");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let mut recorder = Recorder {
            id: Uuid::new_v4(),
            writer: BufWriter::new(file),
            start: Instant::now(),
            pending: Vec::new(),
        };
        recorder.record_output(&[0xE4, 0xB8]).unwrap();
        recorder.finish().unwrap();
        assert!(parse_events(&path)[0][2]
            .as_str()
            .unwrap()
            .contains('\u{FFFD}'));
    }

    #[test]
    fn rejects_path_traversal_ids() {
        assert!(recording_path("../audit").is_err());
        assert!(recording_path("not-a-uuid").is_err());
    }
}
