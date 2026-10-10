//! Fork-local project-root expansion for container extra mounts.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::config::ExtraMount;

/// Expand only the `{project_root}` host-path prefix, then use the existing
/// resolver for tilde expansion, guest-path defaults, and path validation.
/// Ordinary mounts do not require a Git lookup.
pub(super) fn resolve(
    mount: &ExtraMount,
    worktree_root: &Path,
) -> anyhow::Result<(PathBuf, PathBuf, bool)> {
    let host_path = match mount {
        ExtraMount::Path(path) => path,
        ExtraMount::Spec { host_path, .. } => host_path,
    };
    let suffix = if host_path == "{project_root}" {
        ""
    } else if let Some(suffix) = host_path.strip_prefix("{project_root}/") {
        suffix
    } else {
        return mount.resolve();
    };

    let project_root = crate::git::get_main_worktree_root_in(Some(worktree_root))
        .context("extra_mounts: failed to resolve {project_root}")?;
    // Append as text so a second slash cannot make Path::join discard the root.
    let expanded = if suffix.is_empty() {
        project_root.to_string_lossy().into_owned()
    } else {
        format!("{}/{}", project_root.display(), suffix)
    };
    let mut resolved_mount = mount.clone();
    match &mut resolved_mount {
        ExtraMount::Path(path) => *path = expanded,
        ExtraMount::Spec { host_path, .. } => *host_path = expanded,
    }
    let resolved = resolved_mount.resolve()?;
    anyhow::ensure!(
        resolved.0.exists(),
        "extra_mounts: expanded host path does not exist: {}",
        resolved.0.display()
    );
    Ok(resolved)
}
