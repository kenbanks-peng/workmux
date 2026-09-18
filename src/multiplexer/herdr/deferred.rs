//! Shell commands for an embedded, independent deferred worker.
//! No workmux command dispatch or project configuration is used by the worker.
use super::identity::ProcessIdentity;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;

const WORKER: &str = include_str!("deferred_worker.py");

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Action {
    CloseTab,
    CloseWorkspace,
    FocusTab,
    FocusWorkspace,
}

impl Action {
    pub(super) fn workspace(self) -> bool {
        matches!(self, Self::CloseWorkspace | Self::FocusWorkspace)
    }

    pub(super) fn close(self) -> bool {
        matches!(self, Self::CloseTab | Self::CloseWorkspace)
    }
}

#[derive(Serialize)]
pub(super) struct Operation {
    pub endpoint: String,
    pub boot: String,
    pub action: Action,
    pub target: String,
    pub terminals: HashMap<String, ProcessIdentity>,
}

impl Operation {
    pub fn command(&self) -> Result<String> {
        worker_command(&serde_json::to_string(self)?)
    }
}

fn worker_command(payload: &str) -> Result<String> {
    // Resolve now, not after the caller exits or the script changes PATH.
    let python =
        which::which("python3").context("Herdr deferred operations require Python 3 on PATH")?;
    let python = std::path::absolute(python)?;
    Ok(format!(
        "{} -I -c {} {}",
        super::agent::shell_quote(python.to_str().context("Non-UTF-8 Python path")?),
        super::agent::shell_quote(WORKER),
        super::agent::shell_quote(payload),
    ))
}

#[cfg(test)]
pub(super) fn run(payload: &str) -> Result<()> {
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &worker_command(payload)?])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_is_independent_of_path_project_imports_and_shell_payload() {
        let directory = tempfile::tempdir().unwrap();
        // Neither the current directory nor PYTHONPATH may supply worker imports.
        for name in ["json.py", "sitecustomize.py"] {
            std::fs::write(
                directory.path().join(name),
                "raise RuntimeError('project import')",
            )
            .unwrap();
        }
        let marker = directory.path().join("injected");
        let payload = format!("'; touch {}; #", marker.display());
        let command = worker_command(&payload).unwrap();
        assert!(!command.contains("_herdr-deferred"));
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .current_dir(directory.path())
            .env("PATH", "")
            .env("PYTHONPATH", directory.path())
            .env("PYTHONHOME", directory.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("Herdr deferred operation failed:"),
            "{error}"
        );
        assert!(!error.contains("project import"), "{error}");
        assert!(!marker.exists());
    }
}
