//! Container extra-mount expansion for host project roots and guest homes.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::config::ExtraMount;

/// Expand `{project_root}` in host prefixes and `{container_home}` in guest
/// prefixes, then resolve tilde expansion, guest defaults, and path validation.
/// Ordinary mounts do not require a Git lookup.
pub(super) fn resolve(
    mount: &ExtraMount,
    worktree_root: &Path,
    container_home: &str,
) -> anyhow::Result<Option<(PathBuf, PathBuf, bool)>> {
    let host_path = match mount {
        ExtraMount::Path(path) => path,
        ExtraMount::Spec { host_path, .. } => host_path,
    };
    let suffix = if host_path == "{project_root}" {
        Some("")
    } else {
        host_path.strip_prefix("{project_root}/")
    };

    let mut resolved_mount = mount.clone();
    if let Some(suffix) = suffix {
        let project_root = crate::git::get_main_worktree_root_in(Some(worktree_root))
            .context("extra_mounts: failed to resolve {project_root}")?;
        // Append as text so a second slash cannot discard the root.
        let expanded = if suffix.is_empty() {
            project_root.to_string_lossy().into_owned()
        } else {
            format!("{}/{}", project_root.display(), suffix)
        };
        match &mut resolved_mount {
            ExtraMount::Path(path) => *path = expanded,
            ExtraMount::Spec { host_path, .. } => *host_path = expanded,
        }
    }
    if let ExtraMount::Spec {
        guest_path: Some(guest),
        ..
    } = &mut resolved_mount
    {
        if guest == "{container_home}" {
            *guest = container_home.to_string();
        } else if let Some(suffix) = guest.strip_prefix("{container_home}/") {
            *guest = format!("{container_home}/{suffix}");
        }
    }
    let resolved = resolved_mount.resolve()?;
    if resolved_mount.skip_missing(&resolved.0)? {
        return Ok(None);
    }
    if suffix.is_some() {
        anyhow::ensure!(
            resolved.0.exists(),
            "extra_mounts: expanded host path does not exist: {}",
            resolved.0.display()
        );
    }
    Ok(Some(resolved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_extra_mount_validates_paths_before_skipping() {
        let temp = tempfile::tempdir().unwrap();
        for (host, guest) in [
            ("relative".to_string(), "/guest"),
            (
                temp.path().join("missing").display().to_string(),
                "relative",
            ),
        ] {
            let mount = ExtraMount::Spec {
                host_path: host,
                guest_path: Some(guest.into()),
                writable: None,
                optional: true,
            };
            assert!(resolve(&mount, temp.path(), "/home/dev").is_err());
        }
    }

    #[test]
    fn optional_extra_mount_skips_missing_literal_path_without_git() {
        let temp = tempfile::tempdir().unwrap();
        let host = temp.path().join("missing");
        let mount = ExtraMount::Spec {
            host_path: host.display().to_string(),
            guest_path: None,
            writable: None,
            optional: true,
        };
        assert!(resolve(&mount, temp.path(), "/home/dev").unwrap().is_none());
        assert!(!host.exists());
        std::fs::create_dir(&host).unwrap();
        assert_eq!(
            resolve(&mount, temp.path(), "/home/dev").unwrap(),
            Some((host.clone(), host, true))
        );
    }
}
