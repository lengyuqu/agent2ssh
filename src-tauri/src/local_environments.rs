//! User-defined local environments — `~/.agent2ssh/environments.toml`.
//!
//! The built-in local environments are the shells of the machine's own
//! operating system: Windows offers `pwsh`, `cmd`, `gitbash`, `vs` and `wsl`;
//! macOS offers `zsh`, `bash`, `sh` and `pwsh`; Linux offers `bash`, `sh`,
//! `dash`, `zsh` and `pwsh`. This file lets the person who owns the machine add their own
//! under the same `local:<id>` addressing: a base shell plus a working
//! directory, process environment variables, and a one-line prelude that runs in
//! the shell before every command.
//!
//! ```toml
//! [[environment]]
//! id = "py-ml"                      # addressed as `local:py-ml`
//! base = "gitbash"                  # a built-in shell id for this OS, or "default"
//! cwd = "D:/work/ml"                # optional, absolute (a leading `~/` is expanded)
//! prelude = "source .venv/Scripts/activate"
//!
//! [environment.env]
//! CUDA_VISIBLE_DEVICES = "0"
//! ```
//!
//! Because the host id is the only thing that names an environment, scope
//! allow-lists, approval-policy host patterns and the audit log can all single
//! one out without learning anything new.
//!
//! ## Why this file is a trust boundary
//!
//! The prelude and the variables run *before* the command and are not part of
//! it, so the risk classifier — which only sees the command — never looks at
//! them. An environment definition is therefore a way to execute something the
//! authorization chain did not review, and the file that holds it has to be as
//! hard for an agent to influence as the policy file. Two rules follow:
//!
//! * **Re-read when the file changes.** The next command that resolves an
//!   environment sees a new or edited definition without a daemon restart, so
//!   an agent can write the file and use `local:<id>` in the same task. The
//!   prelude is classified at that read, the same way it is at startup.
//! * **A prelude is classified at load time** with the same built-in classifier a
//!   command gets, and a definition whose prelude rates `high` or `blocked` is
//!   rejected rather than loaded. That is a floor, not a proof — user policy
//!   rules are not consulted here — but it keeps an obviously destructive setup
//!   step out of the one place the per-command checks cannot see. The classifier
//!   reads bash, so this applies to the POSIX-family bases only (`gitbash`,
//!   `wsl`, `zsh`, `bash`, `sh`, `dash`, and `default` off Windows). For
//!   `pwsh`, `cmd` and `vs` it is skipped:
//!   run over PowerShell or cmd it rejects harmless lines and passes harmful
//!   ones, so a prelude on those bases runs as written once the file is saved.
//!
//! A definition that fails validation is rejected on its own; the rest still
//! load. A file that does not parse at all disables only the custom
//! environments — the built-in ones never depend on it. Naming a rejected
//! environment is refused with the reason, never silently treated as a plain
//! shell.

use anyhow::Result;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use crate::{
    container_runtime::ContainerRuntime,
    core::classify_risk,
    local_exec::{LocalEnvironment, DEPTH_ENV},
    store::config_dir,
    types::RiskLevel,
};

/// File name under the config directory.
pub const ENVIRONMENTS_FILE: &str = "environments.toml";

const MAX_ID_LEN: usize = 32;
const MAX_ENV_VARS: usize = 64;
const MAX_ENV_VALUE_LEN: usize = 8 * 1024;
const MAX_PRELUDE_LEN: usize = 4 * 1024;

/// One `[[environment]]` table as written. Unknown keys are an error so a typo
/// such as `envs` cannot leave a definition silently missing its variables.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvironment {
    id: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    distro: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    prelude: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    environment: Vec<RawEnvironment>,
}

/// A validated user-defined environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomEnvironment {
    pub id: String,
    pub description: Option<String>,
    /// The built-in environment this one is layered on.
    pub base: LocalEnvironment,
    /// The WSL distribution, only ever set when `base` is [`LocalEnvironment::Wsl`].
    pub distro: Option<String>,
    pub cwd: Option<PathBuf>,
    /// Runs in the shell before every command, after the base's own setup.
    pub prelude: Option<String>,
    /// Process environment variables, in name order.
    pub env: Vec<(String, String)>,
}

/// Everything loaded from `environments.toml`.
#[derive(Debug, Default)]
pub struct EnvironmentRegistry {
    environments: Vec<CustomEnvironment>,
    /// `(id as written, reason)` for definitions that failed validation.
    rejected: Vec<(String, String)>,
    /// Set when the file could not be read or parsed as a whole.
    load_error: Option<String>,
}

impl EnvironmentRegistry {
    /// Parse and validate the text of an `environments.toml`.
    pub fn parse(text: &str) -> Self {
        let file: RawFile = match toml::from_str(text) {
            Ok(file) => file,
            Err(error) => {
                return Self {
                    load_error: Some(error.to_string()),
                    ..Self::default()
                }
            }
        };

        let mut registry = Self::default();
        let mut seen = HashSet::new();
        for raw in file.environment {
            let written_id = raw.id.clone();
            match validate(raw) {
                Err(reason) => registry.rejected.push((written_id, reason)),
                Ok(environment) if !seen.insert(environment.id.clone()) => {
                    registry.rejected.push((
                        written_id,
                        "duplicate id (the first definition is kept)".to_string(),
                    ));
                }
                Ok(environment) => registry.environments.push(environment),
            }
        }
        registry
    }

    /// The definition named `id`, matched exactly: unlike the built-ins, a
    /// custom id is not case-folded, because a token scope or approval pattern
    /// that names `local:py-ml` should not also match `local:PY-ML`.
    pub fn get(&self, id: &str) -> Option<&CustomEnvironment> {
        self.environments
            .iter()
            .find(|environment| environment.id == id)
    }

    /// Why `id` was rejected, if a definition by that name failed validation.
    pub fn rejection(&self, id: &str) -> Option<&str> {
        self.rejected
            .iter()
            .find(|(rejected, _)| rejected == id)
            .map(|(_, reason)| reason.as_str())
    }

    pub fn ids(&self) -> Vec<&str> {
        self.environments
            .iter()
            .map(|environment| environment.id.as_str())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.environments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.environments.is_empty()
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// One line per problem, for the daemon to log at start so a bad definition
    /// is noticed before an agent trips over it.
    pub fn problems(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(error) = &self.load_error {
            lines.push(format!(
                "{ENVIRONMENTS_FILE} could not be loaded, so no custom environments are \
                 available: {error}"
            ));
        }
        for (id, reason) in &self.rejected {
            lines.push(format!(
                "environment '{id}' in {ENVIRONMENTS_FILE} was rejected: {reason}"
            ));
        }
        lines
    }

    /// Add what this file knows to the error for a `local:<kind>` that no
    /// built-in environment recognised, so the caller sees what *is* available.
    pub fn annotate_unknown(&self, kind: &str, error: anyhow::Error) -> anyhow::Error {
        if is_reserved_id(kind) {
            return error;
        }
        let mut notes = Vec::new();
        if !self.environments.is_empty() {
            notes.push(format!(
                "configured in {ENVIRONMENTS_FILE}: {}",
                self.ids().join(", ")
            ));
        }
        if let Some(load_error) = &self.load_error {
            notes.push(format!(
                "{ENVIRONMENTS_FILE} could not be loaded: {load_error}"
            ));
        }
        if notes.is_empty() {
            error
        } else {
            anyhow::anyhow!("{error} ({})", notes.join("; "))
        }
    }
}

// ── Process-wide registry ───────────────────────────────────────────────────

struct RegistryCache {
    /// `None` when the file is absent. A later create or edit changes this and
    /// the next lookup reads the file again.
    fingerprint: Option<(SystemTime, u64)>,
    registry: Arc<EnvironmentRegistry>,
}

static REGISTRY: Mutex<Option<RegistryCache>> = Mutex::new(None);

fn registry_slot() -> std::sync::MutexGuard<'static, Option<RegistryCache>> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `(mtime, length)` of `environments.toml`, or `None` when it is not there.
fn file_fingerprint() -> Option<(SystemTime, u64)> {
    let path = config_dir().ok()?.join(ENVIRONMENTS_FILE);
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn read_from_disk() -> EnvironmentRegistry {
    let path = match config_dir() {
        Ok(dir) => dir.join(ENVIRONMENTS_FILE),
        Err(error) => {
            return EnvironmentRegistry {
                load_error: Some(error.to_string()),
                ..EnvironmentRegistry::default()
            }
        }
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => EnvironmentRegistry::parse(&text),
        // No file is the normal case: nobody has defined anything.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            EnvironmentRegistry::default()
        }
        Err(error) => EnvironmentRegistry {
            load_error: Some(format!("{}: {error}", path.display())),
            ..EnvironmentRegistry::default()
        },
    }
}

/// One entry of the local-environment listing surfaced in the desktop UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalEnvironmentInfo {
    /// Addressed as `local:<id>` in exec targets, approval host patterns and
    /// the audit log.
    pub id: String,
    pub builtin: bool,
    /// Custom environments only: the built-in shell this one layers on.
    pub base: Option<String>,
    pub distro: Option<String>,
    pub cwd: Option<String>,
    pub env_count: usize,
    pub description: Option<String>,
    /// The operating system this catalog belongs to: `Windows`, `macOS` or
    /// `Linux`. Every row of one listing carries the same value, so the desktop
    /// can label the card without guessing from the ids.
    pub platform: String,
}

fn host_platform() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

/// The built-in `local` / `local:<id>` rows for this operating system, in the
/// order the desktop lists them.
///
/// Windows, macOS and Linux do not share a shell set. Offering `cmd`, Git Bash,
/// the Visual Studio shell and WSL on a Mac describes a machine that is not
/// this one, and a Linux box is not a Mac: Dash is a Linux shell, zsh is the
/// macOS login shell.
fn builtin_catalog() -> Vec<(&'static str, String)> {
    let platform = host_platform();
    let default = match crate::shell_list::default_shell() {
        Some(shell) => format!("{platform} default shell ({})", shell.name),
        None => format!("{platform} default shell"),
    };
    let mut rows = vec![("local", default)];
    #[cfg(windows)]
    rows.extend([
        ("local:pwsh", "PowerShell".to_string()),
        ("local:cmd", "Windows Command Prompt".to_string()),
        ("local:gitbash", "Git Bash".to_string()),
        ("local:vs", "Visual Studio developer shell".to_string()),
        ("local:wsl", "WSL".to_string()),
    ]);
    #[cfg(target_os = "macos")]
    rows.extend([
        ("local:zsh", "Zsh".to_string()),
        ("local:bash", "Bash".to_string()),
        ("local:sh", "POSIX sh".to_string()),
        ("local:pwsh", "PowerShell".to_string()),
    ]);
    #[cfg(all(unix, not(target_os = "macos")))]
    rows.extend([
        ("local:bash", "Bash".to_string()),
        ("local:sh", "POSIX sh".to_string()),
        ("local:dash", "Dash".to_string()),
        ("local:zsh", "Zsh".to_string()),
        ("local:pwsh", "PowerShell".to_string()),
    ]);
    rows
}

/// The built-in environments plus everything loaded from
/// `environments.toml`, for the desktop host-management listing. Rejected
/// definitions and a load error are surfaced so a hand-edited file that
/// failed validation is visible instead of silently absent.
pub fn list_environments() -> Vec<LocalEnvironmentInfo> {
    let platform = host_platform().to_string();
    let builtins = builtin_catalog()
        .into_iter()
        .map(|(id, description)| LocalEnvironmentInfo {
            id: id.to_string(),
            builtin: true,
            base: None,
            distro: None,
            cwd: None,
            env_count: 0,
            description: Some(description),
            platform: platform.clone(),
        });

    let registry = current();
    let custom = registry.ids().into_iter().filter_map(|id| {
        let env = registry.get(id)?;
        Some(LocalEnvironmentInfo {
            id: format!("local:{id}"),
            builtin: false,
            base: Some(env.base.id().to_string()),
            distro: env.distro.clone(),
            cwd: env.cwd.as_ref().map(|path| path.display().to_string()),
            env_count: env.env.len(),
            description: env.description.clone(),
            platform: platform.clone(),
        })
    });

    builtins.chain(custom).collect()
}

pub fn current() -> Arc<EnvironmentRegistry> {
    let fingerprint = file_fingerprint();
    let mut slot = registry_slot();
    if let Some(cached) = slot.as_ref() {
        if cached.fingerprint == fingerprint {
            return Arc::clone(&cached.registry);
        }
    }
    let registry = Arc::new(read_from_disk());
    let loaded = Arc::clone(&registry);
    *slot = Some(RegistryCache {
        fingerprint,
        registry,
    });
    loaded
}

/// Re-read the file and replace the process's registry.
///
/// [`current`] already does this when the file's mtime or length changed.
/// This forces that read, which is what startup and the tests use.
pub fn reload() -> Arc<EnvironmentRegistry> {
    let registry = Arc::new(read_from_disk());
    let loaded = Arc::clone(&registry);
    *registry_slot() = Some(RegistryCache {
        fingerprint: file_fingerprint(),
        registry,
    });
    loaded
}

// ── Validation ──────────────────────────────────────────────────────────────

/// Whether `id` is spoken for by a built-in environment, a container runtime or
/// the plain `local` target — and so can never name a custom one.
pub fn is_reserved_id(id: &str) -> bool {
    let folded = id.trim().to_ascii_lowercase();
    LocalEnvironment::from_id(&folded).is_some()
        || ContainerRuntime::from_id(&folded).is_some()
        || matches!(folded.as_str(), "local" | "default")
}

fn validate_id(id: &str) -> Result<(), String> {
    let well_formed = !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'));
    if !well_formed {
        return Err(format!(
            "invalid id '{id}': expected 1 to {MAX_ID_LEN} characters from [a-z0-9_-], \
             starting with a letter or digit"
        ));
    }
    if is_reserved_id(id) {
        return Err(format!(
            "'{id}' is reserved for a built-in environment or container runtime"
        ));
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<(), String> {
    let well_formed = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !well_formed {
        return Err(format!(
            "invalid variable name '{name}': expected [A-Za-z_][A-Za-z0-9_]*"
        ));
    }
    // The recursion guard is how a local exec knows it is already inside one; a
    // definition that could overwrite it would switch the guard off.
    if name.eq_ignore_ascii_case(DEPTH_ENV) {
        return Err(format!("'{name}' is reserved by the recursion guard"));
    }
    Ok(())
}

fn validate_distro(distro: &str) -> Result<(), String> {
    let well_formed = !distro.is_empty()
        && !distro.starts_with('-')
        && distro
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if well_formed {
        Ok(())
    } else {
        Err(format!(
            "invalid distro '{distro}': expected [A-Za-z0-9._-], not starting with '-'"
        ))
    }
}

fn expand_cwd(cwd: &str) -> Result<PathBuf, String> {
    if cwd.contains('\0') || cwd.trim().is_empty() {
        return Err("cwd is empty or contains a NUL byte".to_string());
    }
    let path = match cwd.strip_prefix("~/").or_else(|| cwd.strip_prefix("~\\")) {
        Some(rest) => dirs::home_dir()
            .ok_or_else(|| "cwd starts with '~' but there is no home directory".to_string())?
            .join(rest),
        None => PathBuf::from(cwd),
    };
    if !path.is_absolute() {
        return Err(format!(
            "cwd '{cwd}' is not absolute: a relative path would depend on where the daemon \
             was started"
        ));
    }
    Ok(path)
}

/// Whether the built-in risk classifier reads the shell this base runs. It
/// parses bash, so only the POSIX-family bases qualify; the default shell does
/// off Windows, where it is a POSIX shell.
fn classifier_understands(base: LocalEnvironment) -> bool {
    match base {
        LocalEnvironment::GitBash
        | LocalEnvironment::Wsl
        | LocalEnvironment::Zsh
        | LocalEnvironment::Bash
        | LocalEnvironment::Sh
        | LocalEnvironment::Dash => true,
        LocalEnvironment::Default => !cfg!(windows),
        LocalEnvironment::Pwsh | LocalEnvironment::Cmd | LocalEnvironment::VsDevShell => false,
    }
}

fn validate_prelude(prelude: &str, base: LocalEnvironment) -> Result<String, String> {
    let prelude = prelude.trim();
    if prelude.is_empty() {
        return Err("prelude is empty".to_string());
    }
    if prelude.len() > MAX_PRELUDE_LEN {
        return Err(format!("prelude is longer than {MAX_PRELUDE_LEN} bytes"));
    }
    // The prelude is joined to the command with the shell's own separator, so a
    // line break would mean something different in each shell. One line it is;
    // chain statements with `;` (or `&&` in cmd).
    if prelude.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
        return Err(
            "prelude must be a single line (chain statements with ';', or '&&' for cmd)"
                .to_string(),
        );
    }
    // The classifier parses bash. Run over PowerShell or cmd it answers about the
    // wrong language — measured: `$env:FOO = 'bar'` comes back High while
    // `Remove-Item -Recurse -Force` and `del /s /q` come back Low — so for those
    // bases it would reject harmless lines and wave through the harmful ones. A
    // floor that is wrong in both directions is worse than none, so those bases
    // rest on the file being as trusted as the policy file and the module note
    // says so.
    if !classifier_understands(base) {
        return Ok(prelude.to_string());
    }
    let risk = classify_risk(prelude);
    if matches!(risk, RiskLevel::High | RiskLevel::Blocked) {
        return Err(format!(
            "prelude classifies as {risk:?} risk. The prelude runs before every command \
             without passing through per-command authorization, so a setup step that \
             would itself need approval does not belong here"
        ));
    }
    Ok(prelude.to_string())
}

fn validate(raw: RawEnvironment) -> Result<CustomEnvironment, String> {
    validate_id(&raw.id)?;

    let base = match raw.base.as_deref().map(|b| b.trim().to_ascii_lowercase()) {
        None => LocalEnvironment::Default,
        Some(base) if base == "default" => LocalEnvironment::Default,
        Some(base) => LocalEnvironment::from_id(&base).ok_or_else(|| {
            format!(
                "unknown base '{base}': expected default or one of {}",
                LocalEnvironment::SELECTABLE_IDS.join(", ")
            )
        })?,
    };

    let distro = match raw.distro {
        None => None,
        Some(_) if base != LocalEnvironment::Wsl => {
            return Err("distro only applies to base = \"wsl\"".to_string())
        }
        Some(distro) => {
            validate_distro(&distro)?;
            Some(distro)
        }
    };

    if raw.env.len() > MAX_ENV_VARS {
        return Err(format!("more than {MAX_ENV_VARS} variables"));
    }
    if base == LocalEnvironment::Wsl && !raw.env.is_empty() {
        // `wsl.exe` does not hand its own process environment to the
        // distribution unless `WSLENV` says which names to translate, so the
        // variables would be set and then never seen. Refuse instead of
        // accepting a definition that does nothing.
        return Err(
            "env has no effect on base = \"wsl\" (wsl.exe does not pass its environment into \
             the distribution): put `export NAME=value` in the prelude instead"
                .to_string(),
        );
    }
    let mut env = Vec::with_capacity(raw.env.len());
    for (name, value) in raw.env {
        validate_env_name(&name)?;
        if value.contains('\0') || value.len() > MAX_ENV_VALUE_LEN {
            return Err(format!(
                "value of '{name}' contains a NUL byte or is longer than {MAX_ENV_VALUE_LEN} bytes"
            ));
        }
        env.push((name, value));
    }

    let cwd = raw.cwd.as_deref().map(expand_cwd).transpose()?;
    let prelude = raw
        .prelude
        .as_deref()
        .map(|prelude| validate_prelude(prelude, base))
        .transpose()?;

    Ok(CustomEnvironment {
        id: raw.id,
        description: raw.description,
        base,
        distro,
        cwd,
        prelude,
        env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(body: &str) -> EnvironmentRegistry {
        EnvironmentRegistry::parse(&format!("[[environment]]\n{body}"))
    }

    fn rejection_of(body: &str, id: &str) -> String {
        let registry = one(body);
        registry
            .rejection(id)
            .unwrap_or_else(|| panic!("'{id}' should have been rejected: {registry:?}"))
            .to_string()
    }

    /// An absolute working directory for the platform running the test.
    ///
    /// A POSIX literal such as `/work/ml` is absolute on Unix only: on Windows it
    /// is a root with no prefix, so `Path::is_absolute` is false and `expand_cwd`
    /// rejects the definition — correctly. Two tests in this module asserted one
    /// anyway, which is why they failed on the Windows CI leg alone. Keeping the
    /// discriminator in one helper lets every assertion below stay identical on
    /// both platforms, the pattern `tests/local_exec.rs` uses for named
    /// environments.
    fn absolute_cwd(segment: &str) -> String {
        if cfg!(windows) {
            format!(r"C:\{segment}")
        } else {
            format!("/{segment}")
        }
    }

    #[test]
    fn a_full_definition_loads_with_every_field() {
        let cwd = absolute_cwd("work/ml");
        // A TOML *literal* string (single quotes), because a basic string would
        // read the backslashes in the Windows form as escapes.
        let registry = EnvironmentRegistry::parse(&format!(
            r#"
[[environment]]
id = "py-ml"
description = "ML toolchain"
base = "gitbash"
cwd = '{cwd}'
prelude = "source .venv/bin/activate"

[environment.env]
ZED = "1"
CUDA_VISIBLE_DEVICES = "0"
"#
        ));
        assert!(registry.problems().is_empty(), "{:?}", registry.problems());
        let env = registry.get("py-ml").expect("loaded");
        assert_eq!(env.base, LocalEnvironment::GitBash);
        assert_eq!(env.cwd, Some(PathBuf::from(&cwd)));
        assert_eq!(env.prelude.as_deref(), Some("source .venv/bin/activate"));
        // Name order, not file order.
        assert_eq!(
            env.env,
            vec![
                ("CUDA_VISIBLE_DEVICES".to_string(), "0".to_string()),
                ("ZED".to_string(), "1".to_string())
            ]
        );
    }

    #[test]
    fn base_defaults_to_the_default_shell_and_accepts_its_aliases() {
        assert_eq!(
            one("id = \"a\"").get("a").unwrap().base,
            LocalEnvironment::Default
        );
        assert_eq!(
            one("id = \"a\"\nbase = \"default\"").get("a").unwrap().base,
            LocalEnvironment::Default
        );
        assert_eq!(
            one("id = \"a\"\nbase = \"PowerShell\"")
                .get("a")
                .unwrap()
                .base,
            LocalEnvironment::Pwsh
        );
    }

    #[test]
    fn a_missing_file_section_is_an_empty_registry() {
        let registry = EnvironmentRegistry::parse("");
        assert!(registry.is_empty());
        assert!(registry.problems().is_empty());
    }

    #[test]
    fn the_builtin_catalog_follows_this_operating_system() {
        let listed: Vec<String> = list_environments()
            .into_iter()
            .filter(|env| env.builtin)
            .map(|env| env.id)
            .collect();
        let catalog: Vec<String> = builtin_catalog()
            .into_iter()
            .map(|(id, _)| id.to_string())
            .collect();
        assert_eq!(listed, catalog);

        #[cfg(windows)]
        {
            assert!(listed.iter().any(|id| id == "local:cmd"));
            assert!(listed.iter().any(|id| id == "local:wsl"));
            assert!(!listed.iter().any(|id| id == "local:zsh"));
            assert!(!listed.iter().any(|id| id == "local:dash"));
        }
        #[cfg(target_os = "macos")]
        {
            for absent in [
                "local:cmd",
                "local:gitbash",
                "local:vs",
                "local:wsl",
                "local:dash",
            ] {
                assert!(
                    !listed.iter().any(|id| id == absent),
                    "{absent} is a Windows or Linux shell, listed on macOS: {listed:?}"
                );
            }
            assert!(listed.iter().any(|id| id == "local:zsh"));
            assert!(listed.iter().any(|id| id == "local:bash"));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(listed.iter().any(|id| id == "local:dash"));
            assert!(listed.iter().any(|id| id == "local:bash"));
            assert!(!listed.iter().any(|id| id == "local:cmd"));
            assert!(!listed.iter().any(|id| id == "local:wsl"));
        }

        let rows = list_environments();
        assert!(rows.iter().all(|env| env.platform == host_platform()));
        let default = rows.iter().find(|env| env.id == "local").expect("local");
        let description = default.description.as_deref().unwrap_or("");
        assert!(
            description.starts_with(host_platform()),
            "the default shell must name this OS, got {description}"
        );
    }

    #[test]
    fn ids_that_a_built_in_or_a_runtime_already_owns_are_rejected() {
        for id in [
            "pwsh",
            "PowerShell",
            "cmd",
            "vs",
            "wsl",
            "bash",
            "zsh",
            "sh",
            "dash",
            "docker",
            "wslc",
            "apple",
            "local",
            "default",
        ] {
            let reason = rejection_of(&format!("id = \"{id}\""), id);
            assert!(
                reason.contains("reserved") || reason.contains("invalid id"),
                "{id}: {reason}"
            );
        }
    }

    #[test]
    fn malformed_ids_are_rejected() {
        for id in [
            "",
            "Py-ML",
            "-lead",
            "has space",
            "a:b",
            "x".repeat(33).as_str(),
        ] {
            let registry = EnvironmentRegistry::parse(&format!(
                "[[environment]]\nid = {}\n",
                toml::Value::String(id.to_string())
            ));
            assert!(
                registry.rejection(id).is_some(),
                "'{id}' should be rejected: {registry:?}"
            );
        }
    }

    #[test]
    fn a_duplicate_id_keeps_the_first_definition() {
        // Both must be absolute on this platform, or the second definition would
        // be rejected for its cwd instead of for the duplicate id — and the
        // "first wins" assertion would never run.
        let first = absolute_cwd("one");
        let second = absolute_cwd("two");
        let registry = EnvironmentRegistry::parse(&format!(
            "[[environment]]\nid = \"a\"\ncwd = '{first}'\n[[environment]]\nid = \"a\"\ncwd = '{second}'\n"
        ));
        assert_eq!(registry.get("a").unwrap().cwd, Some(PathBuf::from(&first)));
        assert!(registry.rejection("a").unwrap().contains("duplicate"));
    }

    #[test]
    fn lookup_is_exact_not_case_folded() {
        let registry = one("id = \"py-ml\"");
        assert!(registry.get("py-ml").is_some());
        assert!(registry.get("PY-ML").is_none());
    }

    #[test]
    fn the_recursion_guard_variable_cannot_be_set() {
        let reason = rejection_of(
            &format!("id = \"a\"\n[environment.env]\n{DEPTH_ENV} = \"0\""),
            "a",
        );
        assert!(reason.contains("recursion guard"), "{reason}");
    }

    #[test]
    fn variable_names_and_values_are_checked() {
        assert!(
            rejection_of("id = \"a\"\n[environment.env]\n\"BAD NAME\" = \"1\"", "a")
                .contains("invalid variable name")
        );
        assert!(
            rejection_of("id = \"a\"\n[environment.env]\n\"1X\" = \"1\"", "a")
                .contains("invalid variable name")
        );
        assert!(
            rejection_of("id = \"a\"\n[environment.env]\nX = \"a\\u0000b\"", "a").contains("NUL")
        );
    }

    #[test]
    fn env_on_a_wsl_base_is_refused_rather_than_ignored() {
        let reason = rejection_of(
            "id = \"a\"\nbase = \"wsl\"\n[environment.env]\nX = \"1\"",
            "a",
        );
        assert!(reason.contains("prelude"), "{reason}");
        // The same variable as an export in the prelude is the supported route.
        let ok = one("id = \"a\"\nbase = \"wsl\"\nprelude = \"export X=1\"");
        assert!(ok.get("a").is_some(), "{:?}", ok.problems());
    }

    #[test]
    fn distro_only_applies_to_a_wsl_base() {
        assert!(rejection_of("id = \"a\"\ndistro = \"Ubuntu\"", "a").contains("wsl"));
        let ok = one("id = \"a\"\nbase = \"wsl\"\ndistro = \"Ubuntu-22.04\"");
        assert_eq!(ok.get("a").unwrap().distro.as_deref(), Some("Ubuntu-22.04"));
        // An option-looking name would be read by wsl.exe as a flag.
        assert!(
            rejection_of("id = \"a\"\nbase = \"wsl\"\ndistro = \"--exec\"", "a")
                .contains("invalid distro")
        );
    }

    #[test]
    fn cwd_must_be_absolute() {
        assert!(rejection_of("id = \"a\"\ncwd = \"relative/dir\"", "a").contains("not absolute"));
        let home = one("id = \"a\"\ncwd = \"~/proj\"");
        let cwd = home.get("a").unwrap().cwd.clone().unwrap();
        assert!(cwd.is_absolute() && cwd.ends_with("proj"), "{cwd:?}");
    }

    #[test]
    fn a_prelude_must_be_one_line() {
        let reason = rejection_of("id = \"a\"\nprelude = \"\"\"\na\nb\"\"\"", "a");
        assert!(reason.contains("single line"), "{reason}");
    }

    #[test]
    fn a_destructive_prelude_is_rejected_at_load() {
        for prelude in [
            "rm -rf /",
            "rm -rf ~",
            "sudo systemctl stop sshd",
            "mkfs.ext4 /dev/sda1",
        ] {
            let reason = rejection_of(
                &format!("id = \"a\"\nbase = \"gitbash\"\nprelude = \"{prelude}\""),
                "a",
            );
            assert!(reason.contains("risk"), "{prelude}: {reason}");
        }
    }

    #[test]
    fn ordinary_setup_preludes_are_accepted() {
        for prelude in [
            "source .venv/bin/activate",
            ". ./env.sh",
            "export JAVA_HOME=/opt/jdk-21",
            "conda activate ml",
            "nvm use 20",
            "cd /srv/app",
        ] {
            let registry = one(&format!(
                "id = \"a\"\nbase = \"gitbash\"\nprelude = \"{prelude}\""
            ));
            assert!(
                registry.get("a").is_some(),
                "'{prelude}' should load: {:?}",
                registry.problems()
            );
        }
    }

    #[test]
    fn powershell_and_cmd_preludes_are_not_run_through_the_bash_classifier() {
        // `$env:FOO = 'bar'` is High to a bash parser. These are the setup lines
        // people actually write for the Windows shells, so a base the classifier
        // does not read must not be judged by it.
        for prelude in [
            "$env:FOO = 'bar'",
            ". .\\.venv\\Scripts\\Activate.ps1",
            "& .\\.venv\\Scripts\\Activate.ps1",
            "call venv\\Scripts\\activate.bat",
            "set FOO=bar",
        ] {
            let registry = one(&format!(
                "id = \"a\"\nbase = \"pwsh\"\nprelude = {}",
                toml::Value::String(prelude.to_string())
            ));
            assert!(
                registry.get("a").is_some(),
                "'{prelude}' should load: {:?}",
                registry.problems()
            );
        }
    }

    #[test]
    fn one_bad_definition_does_not_take_the_others_down() {
        let registry = EnvironmentRegistry::parse(
            "[[environment]]\nid = \"good\"\n[[environment]]\nid = \"BAD\"\n",
        );
        assert!(registry.get("good").is_some());
        assert!(registry.rejection("BAD").is_some());
        assert_eq!(registry.problems().len(), 1);
    }

    #[test]
    fn a_file_that_does_not_parse_disables_only_the_custom_environments() {
        for text in [
            "[[environment]\nid = ",
            // A misspelled key must not leave the definition quietly bare.
            "[[environment]]\nid = \"a\"\nenvs = {}\n",
            "[[env]]\nid = \"a\"\n",
        ] {
            let registry = EnvironmentRegistry::parse(text);
            assert!(registry.is_empty(), "{text}");
            assert!(registry.load_error().is_some(), "{text}");
            assert!(registry.problems()[0].contains("no custom environments"));
        }
    }

    #[test]
    fn the_unknown_environment_error_lists_what_is_configured() {
        let registry = EnvironmentRegistry::parse(
            "[[environment]]\nid = \"alpha\"\n[[environment]]\nid = \"beta\"\n",
        );
        let annotated = registry
            .annotate_unknown("nope", anyhow::anyhow!("unknown local environment 'nope'"))
            .to_string();
        assert!(annotated.contains("unknown local environment 'nope'"));
        assert!(annotated.contains("alpha, beta"), "{annotated}");

        // A built-in's own failure is not an "unknown name" and is left alone.
        let untouched = registry
            .annotate_unknown("pwsh", anyhow::anyhow!("no PowerShell found on PATH"))
            .to_string();
        assert_eq!(untouched, "no PowerShell found on PATH");
    }
}
