//! Configuration directory resolution: profiles + dev/prod layering.
//!
//! The *policy* (which directory is the shipped baseline, which is the
//! writable overlay) lives here as pure functions over already-resolved
//! base directories, so it is unit-tested without Tauri or the filesystem.
//!
//! The base directories themselves are fetched per-binary, each with its
//! own correct tool:
//! - the Tauri app uses `app.path()` (`resource_dir`, `app_config_dir`,
//!   `document_dir`) inside `setup()`,
//! - the headless binary uses the `dirs` crate (it has no Tauri runtime).
//!
//! Both feed the same policy here. No hand-rolled `%APPDATA%`/`~/.config`
//! logic; no silent fallbacks — a directory that can't be resolved is a
//! hard error naming what was missing.

use std::path::{Path, PathBuf};

/// Which writable config overlay is active on top of the shipped baseline.
/// Selected explicitly (env var or build-mode default) so it is never
/// ambiguous which configuration is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Committed, repo-shared development config (`<repo>/config/dev`).
    /// Shared across the team via git.
    Dev,
    /// Machine-local per-user/per-rig config (platform app-config dir).
    User,
}

impl Profile {
    /// Resolve the active profile. Explicit `OPENISI_PROFILE=dev|user`
    /// wins; otherwise default to the build/run mode (`dev` in a dev run,
    /// `user` in an installed build). An unrecognized value is a hard
    /// error — never a silent fall-through to a guessed profile.
    pub fn resolve(default_is_dev: bool) -> Result<Profile, String> {
        let env = std::env::var("OPENISI_PROFILE").ok();
        Self::from_env_value(env.as_deref(), default_is_dev)
    }

    /// Pure core of [`Profile::resolve`], factored out for testing.
    pub fn from_env_value(value: Option<&str>, default_is_dev: bool) -> Result<Profile, String> {
        match value {
            None => Ok(if default_is_dev { Profile::Dev } else { Profile::User }),
            Some(s) => match s.trim().to_ascii_lowercase().as_str() {
                "dev" => Ok(Profile::Dev),
                "user" => Ok(Profile::User),
                other => Err(format!(
                    "OPENISI_PROFILE must be 'dev' or 'user', got '{other}'"
                )),
            },
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Dev => "dev",
            Profile::User => "user",
        }
    }
}

/// Resolved directories for the two-layer registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLayout {
    /// Read-only shipped baseline holding `rig.toml` / `experiment.toml` /
    /// `analysis.toml`.
    pub shipped_dir: PathBuf,
    /// Writable active overlay (sparse user/dev overrides), same filenames.
    pub user_dir: PathBuf,
}

/// Resolve the shipped baseline + writable overlay from candidate base
/// directories. Each candidate is `Option` because its source may
/// legitimately be unavailable (e.g. no `resource_dir` in a dev run, no
/// `app_config_dir` if the OS lookup failed).
///
/// Policy (fail-loud — a missing required directory is an error):
/// - **shipped baseline**: dev run → `repo_config`; installed → `resource_config`.
/// - **active overlay**: `Dev` profile → `repo_config/dev`; `User` → `app_config`.
pub fn resolve_layout(
    is_dev: bool,
    profile: Profile,
    repo_config: Option<&Path>,
    resource_config: Option<&Path>,
    app_config: Option<&Path>,
) -> Result<ConfigLayout, String> {
    let shipped_dir = if is_dev {
        repo_config
            .ok_or_else(|| "dev run: could not locate the repo `config` directory".to_string())?
            .to_path_buf()
    } else {
        resource_config
            .ok_or_else(|| {
                "installed build: could not resolve the bundle resource `config` directory"
                    .to_string()
            })?
            .to_path_buf()
    };

    let user_dir = match profile {
        Profile::Dev => repo_config
            .ok_or_else(|| {
                "dev profile: could not locate the repo `config` directory for the \
                 committed dev overlay"
                    .to_string()
            })?
            .join("dev"),
        Profile::User => app_config
            .ok_or_else(|| {
                "user profile: could not resolve the platform app-config directory".to_string()
            })?
            .to_path_buf(),
    };

    Ok(ConfigLayout { shipped_dir, user_dir })
}

/// The default data directory when the user has not set one:
/// `<Documents>/OpenISI`. Returned as an explicit value to be **persisted
/// into the config on first run** (and surfaced in the UI) — it is a
/// deliberate, visible default, not an ongoing implicit fallback. `None`
/// when the Documents directory can't be resolved; the caller decides how
/// to surface that.
pub fn default_data_dir(documents: Option<&Path>) -> Option<PathBuf> {
    documents.map(|d| d.join("OpenISI"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_env_explicit_wins() {
        assert_eq!(Profile::from_env_value(Some("dev"), false).unwrap(), Profile::Dev);
        assert_eq!(Profile::from_env_value(Some("user"), true).unwrap(), Profile::User);
        // Case/whitespace tolerant.
        assert_eq!(Profile::from_env_value(Some("  DEV "), false).unwrap(), Profile::Dev);
    }

    #[test]
    fn profile_default_follows_build_mode_when_unset() {
        assert_eq!(Profile::from_env_value(None, true).unwrap(), Profile::Dev);
        assert_eq!(Profile::from_env_value(None, false).unwrap(), Profile::User);
    }

    #[test]
    fn profile_unrecognized_is_fatal() {
        let err = Profile::from_env_value(Some("prod"), true).unwrap_err();
        assert!(err.contains("must be 'dev' or 'user'"), "got: {err}");
        assert!(err.contains("prod"));
    }

    #[test]
    fn layout_dev_uses_repo_baseline_and_dev_overlay() {
        let repo = PathBuf::from("/repo/config");
        let layout = resolve_layout(
            true,
            Profile::Dev,
            Some(&repo),
            None,
            Some(Path::new("/home/u/.config/com.openisi.app")),
        )
        .unwrap();
        assert_eq!(layout.shipped_dir, repo);
        assert_eq!(layout.user_dir, repo.join("dev"));
    }

    #[test]
    fn layout_prod_user_uses_resource_baseline_and_app_config_overlay() {
        let resource = PathBuf::from("/Applications/OpenISI.app/Contents/Resources/config");
        let app_cfg = PathBuf::from("/home/u/.config/com.openisi.app");
        let layout =
            resolve_layout(false, Profile::User, None, Some(&resource), Some(&app_cfg)).unwrap();
        assert_eq!(layout.shipped_dir, resource);
        assert_eq!(layout.user_dir, app_cfg);
    }

    #[test]
    fn layout_missing_shipped_baseline_is_fatal() {
        // installed build but no resource_dir resolved.
        let err = resolve_layout(false, Profile::User, None, None, Some(Path::new("/x"))).unwrap_err();
        assert!(err.contains("bundle resource"), "got: {err}");
    }

    #[test]
    fn layout_user_profile_missing_app_config_is_fatal() {
        let repo = PathBuf::from("/repo/config");
        let err = resolve_layout(true, Profile::User, Some(&repo), None, None).unwrap_err();
        assert!(err.contains("app-config"), "got: {err}");
    }

    #[test]
    fn default_data_dir_appends_openisi() {
        assert_eq!(
            default_data_dir(Some(Path::new("/home/u/Documents"))),
            Some(PathBuf::from("/home/u/Documents/OpenISI"))
        );
        assert_eq!(default_data_dir(None), None);
    }
}
