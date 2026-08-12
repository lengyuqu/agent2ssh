//! Command snippets store (B38).
//!
//! Frequently used commands can be saved as named snippets for quick recall.
//! Snippets are stored as a JSON file (`snippets.json`) in the config directory.
//! This mirrors rssh's `db/snippet.rs` pattern, adapted to file-based storage.
//!
//! ## Sync
//!
//! Snippets are syncable via WebDAV. The merge strategy is additive by name:
//! matching names overwrite the command, new names are appended, local-only
//! snippets are kept. Never deletes — the user may have intentionally removed
//! a snippet from the sync payload.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;

use crate::store::{
    config_dir, ensure_config_dir, lock_config_file, restrict_file_to_owner, FileLockGuard,
};

/// The JSON file name for command snippets.
const SNIPPETS_FILE: &str = "snippets.json";
const SNIPPETS_LOCK_FILE: &str = ".snippets.lock";
const MAX_NAME_CHARS: usize = 128;
const MAX_COMMAND_BYTES: usize = 64 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 4 * 1024;

/// A named command snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// Unique name for the snippet (user-defined).
    pub name: String,
    /// The command text (may be multi-line).
    pub command: String,
    /// Optional description/notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn snippet_write_lock() -> Result<FileLockGuard> {
    lock_config_file(SNIPPETS_LOCK_FILE)
}

/// Validate and normalize a snippet received from a user-facing transport.
/// The command is deliberately excluded from error messages because snippets
/// commonly contain credentials or other sensitive shell arguments.
pub fn validate_snippet(mut snippet: Snippet) -> Result<Snippet> {
    snippet.name = snippet.name.trim().to_string();
    if snippet.name.is_empty() {
        return Err(anyhow!("snippet name must not be empty"));
    }
    if snippet.name.chars().count() > MAX_NAME_CHARS {
        return Err(anyhow!(
            "snippet name exceeds the maximum length of {MAX_NAME_CHARS} characters"
        ));
    }
    if snippet.name.chars().any(char::is_control) {
        return Err(anyhow!("snippet name must not contain control characters"));
    }
    if snippet.command.trim().is_empty() {
        return Err(anyhow!("snippet command must not be empty"));
    }
    if snippet.command.len() > MAX_COMMAND_BYTES {
        return Err(anyhow!(
            "snippet command exceeds the maximum size of {MAX_COMMAND_BYTES} bytes"
        ));
    }
    snippet.description = snippet.description.and_then(|description| {
        let trimmed = description.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    });
    if snippet
        .description
        .as_ref()
        .is_some_and(|description| description.chars().count() > MAX_DESCRIPTION_CHARS)
    {
        return Err(anyhow!(
            "snippet description exceeds the maximum length of {MAX_DESCRIPTION_CHARS} characters"
        ));
    }
    Ok(snippet)
}

/// Load snippets from the config file. Returns an empty vec if the file
/// doesn't exist; returns an error if the file exists but fails to parse
/// (fail-fast, not silent clearing).
pub fn load_snippets() -> Result<Vec<Snippet>> {
    let path = config_dir()?.join(SNIPPETS_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read snippets file {}", path.display()))?;
    let snippets: Vec<Snippet> = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse snippets file {}", path.display()))?;
    Ok(snippets)
}

/// Save snippets to the config file.
pub fn save_snippets(snippets: &[Snippet]) -> Result<()> {
    let _guard = snippet_write_lock()?;
    save_snippets_unlocked(snippets)
}

fn save_snippets_unlocked(snippets: &[Snippet]) -> Result<()> {
    ensure_config_dir()?;
    let path = config_dir()?.join(SNIPPETS_FILE);
    let validated = snippets
        .iter()
        .cloned()
        .map(validate_snippet)
        .collect::<Result<Vec<_>>>()?;
    let mut names = std::collections::HashSet::new();
    if let Some(duplicate) = validated
        .iter()
        .map(|snippet| snippet.name.as_str())
        .find(|name| !names.insert((*name).to_string()))
    {
        return Err(anyhow!("duplicate snippet name: {duplicate}"));
    }
    let json = serde_json::to_string_pretty(&validated)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("snippets path has no parent directory"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .context("failed to create temporary snippets file")?;
    temp.write_all(json.as_bytes())
        .context("failed to write temporary snippets file")?;
    temp.as_file_mut()
        .sync_all()
        .context("failed to flush temporary snippets file")?;
    restrict_file_to_owner(temp.path())?;
    temp.persist(&path)
        .map_err(|error| anyhow!("failed to replace snippets file: {}", error.error))?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

/// Add a snippet, or update if the name already exists.
pub fn add_snippet(name: &str, command: &str, description: Option<&str>) -> Result<Vec<Snippet>> {
    let snippet = validate_snippet(Snippet {
        name: name.to_string(),
        command: command.to_string(),
        description: description.map(str::to_string),
    })?;
    let _guard = snippet_write_lock()?;
    let mut snippets = load_snippets()?;
    if let Some(existing) = snippets.iter_mut().find(|item| item.name == snippet.name) {
        *existing = snippet;
    } else {
        snippets.push(snippet);
    }
    save_snippets_unlocked(&snippets)?;
    Ok(snippets)
}

/// Remove a snippet by name. Returns true if a snippet was removed.
pub fn remove_snippet(name: &str) -> Result<bool> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("snippet name must not be empty"));
    }
    if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(anyhow!("invalid snippet name"));
    }
    let _guard = snippet_write_lock()?;
    let mut snippets = load_snippets()?;
    let before = snippets.len();
    snippets.retain(|s| s.name != name);
    let removed = snippets.len() < before;
    if removed {
        save_snippets_unlocked(&snippets)?;
    }
    Ok(removed)
}

/// Merge snippets additively by name (used by sync import).
/// Matching names overwrite the command; new names are appended; local-only
/// snippets are kept. Never deletes.
pub fn merge_snippets(local: &[Snippet], incoming: &[Snippet]) -> Vec<Snippet> {
    let mut result = local.to_vec();
    for inc in incoming {
        if let Some(s) = result.iter_mut().find(|s| s.name == inc.name) {
            s.command = inc.command.clone();
            s.description = inc.description.clone();
        } else {
            result.push(inc.clone());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(label: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("agent2ssh-snip-{}-{}", label, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_empty_when_no_file() {
        let dir = unique_dir("empty");
        let snippets = load_snippets().unwrap();
        assert!(snippets.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = unique_dir("roundtrip");
        let snippets = vec![
            Snippet {
                name: "check-logs".into(),
                command: "tail -f /var/log/syslog".into(),
                description: Some("Follow syslog".into()),
            },
            Snippet {
                name: "disk-usage".into(),
                command: "df -h".into(),
                description: None,
            },
        ];
        save_snippets(&snippets).unwrap();

        let loaded = load_snippets().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "check-logs");
        assert_eq!(loaded[0].command, "tail -f /var/log/syslog");
        assert_eq!(loaded[0].description.as_deref(), Some("Follow syslog"));
        assert_eq!(loaded[1].name, "disk-usage");
        assert!(loaded[1].description.is_none());

        cleanup(&dir);
    }

    #[test]
    fn add_new_snippet() {
        let dir = unique_dir("add");
        add_snippet("hello", "echo hello", None).unwrap();
        add_snippet("world", "echo world", Some("Print world")).unwrap();

        let snippets = load_snippets().unwrap();
        assert_eq!(snippets.len(), 2);
        assert_eq!(snippets[1].description.as_deref(), Some("Print world"));

        cleanup(&dir);
    }

    #[test]
    fn add_updates_existing_by_name() {
        let dir = unique_dir("update");
        add_snippet("greet", "echo hi", None).unwrap();
        add_snippet("greet", "echo hello", Some("Updated")).unwrap();

        let snippets = load_snippets().unwrap();
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].command, "echo hello");
        assert_eq!(snippets[0].description.as_deref(), Some("Updated"));

        cleanup(&dir);
    }

    #[test]
    fn remove_by_name() {
        let dir = unique_dir("remove");
        add_snippet("keep", "echo keep", None).unwrap();
        add_snippet("delete", "echo delete", None).unwrap();

        let removed = remove_snippet("delete").unwrap();
        assert!(removed);

        let snippets = load_snippets().unwrap();
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "keep");

        // Removing non-existent returns false.
        let removed_again = remove_snippet("delete").unwrap();
        assert!(!removed_again);

        cleanup(&dir);
    }

    #[test]
    fn load_fails_on_corrupt_json() {
        let dir = unique_dir("corrupt");
        let path = dir.join(SNIPPETS_FILE);
        std::fs::write(&path, "{{corrupt").unwrap();

        let result = load_snippets();
        assert!(
            result.is_err(),
            "corrupt JSON must fail, not silently clear"
        );

        cleanup(&dir);
    }

    #[test]
    fn merge_additive_by_name() {
        let local = vec![Snippet {
            name: "a".into(),
            command: "cmd-a".into(),
            description: None,
        }];
        let incoming = vec![
            Snippet {
                name: "a".into(),
                command: "cmd-a-updated".into(),
                description: Some("updated".to_string()),
            },
            Snippet {
                name: "b".into(),
                command: "cmd-b".into(),
                description: None,
            },
        ];
        let merged = merge_snippets(&local, &incoming);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "a");
        assert_eq!(merged[0].command, "cmd-a-updated");
        assert_eq!(merged[1].name, "b");
        assert_eq!(merged[1].command, "cmd-b");
    }

    #[test]
    fn merge_never_deletes_local_only() {
        let local = vec![
            Snippet {
                name: "local-only".into(),
                command: "cmd-local".into(),
                description: None,
            },
            Snippet {
                name: "shared".into(),
                command: "old".into(),
                description: None,
            },
        ];
        let incoming = vec![Snippet {
            name: "shared".into(),
            command: "new".into(),
            description: None,
        }];
        let merged = merge_snippets(&local, &incoming);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|s| s.name == "local-only"));
        assert!(merged
            .iter()
            .any(|s| s.name == "shared" && s.command == "new"));
    }

    #[test]
    fn snippet_serialization_roundtrip() {
        let s = Snippet {
            name: "test".into(),
            command: "echo test".into(),
            description: Some("A test".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Snippet = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.command, "echo test");
        assert_eq!(back.description.as_deref(), Some("A test"));
    }

    #[test]
    fn snippet_without_description_serializes_without_field() {
        let s = Snippet {
            name: "test".into(),
            command: "echo test".into(),
            description: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("description"),
            "None description must be skipped"
        );
    }

    #[test]
    fn validates_and_normalizes_user_input() {
        let normalized = validate_snippet(Snippet {
            name: "  disk  ".into(),
            command: "df -h".into(),
            description: Some("  Disk usage  ".into()),
        })
        .unwrap();
        assert_eq!(normalized.name, "disk");
        assert_eq!(normalized.description.as_deref(), Some("Disk usage"));

        for snippet in [
            Snippet {
                name: "".into(),
                command: "true".into(),
                description: None,
            },
            Snippet {
                name: "bad\nname".into(),
                command: "true".into(),
                description: None,
            },
            Snippet {
                name: "ok".into(),
                command: "   ".into(),
                description: None,
            },
        ] {
            assert!(validate_snippet(snippet).is_err());
        }
    }
}
