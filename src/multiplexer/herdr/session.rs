//! Session reopening must not adopt layouts restored by Herdr after a restart.
use super::*;

impl HerdrBackend {
    pub(super) fn workspace_owned(&self, workspace: &str) -> Result<bool> {
        match std::fs::read_to_string(self.record_path(workspace)?) {
            Ok(boot) => Ok(boot == self.client.boot()?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn session_open_name(&self, prefix: &str, name: &str) -> Result<String> {
        let snapshot = self.client.snapshot()?;
        let full_name = util::prefixed(prefix, name);
        let matches: Vec<_> = snapshot
            .workspaces
            .iter()
            .filter(|workspace| workspace.label == full_name)
            .collect();
        ensure!(
            matches.len() <= 1,
            "Herdr workspace '{full_name}' is ambiguous ({} matches); use a unique workspace name",
            matches.len()
        );
        let Some(workspace) = matches.first() else {
            return Ok(name.into());
        };
        if self.workspace_owned(&workspace.workspace_id)? {
            return Ok(name.into());
        }
        // Never reuse even an owned suffix by name alone. It may belong to a
        // different worktree. The shared workflow persists the selected name
        // before setup, so partial setup and subsequent opens use that target.
        for suffix in 2_u64.. {
            let candidate = format!("{name}-{suffix}");
            let full_candidate = util::prefixed(prefix, &candidate);
            if !snapshot
                .workspaces
                .iter()
                .any(|w| w.label == full_candidate)
            {
                return Ok(candidate);
            }
        }
        bail!("No free Herdr workspace name")
    }
}
