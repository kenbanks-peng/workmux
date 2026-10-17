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
) -> anyhow::Result<(PathBuf, PathBuf, bool)> {
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
    if suffix.is_some() {
        anyhow::ensure!(
            resolved.0.exists(),
            "extra_mounts: expanded host path does not exist: {}",
            resolved.0.display()
        );
    }
    Ok(resolved)
}
