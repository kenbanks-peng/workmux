//! Private deferred operations, executed by a detached workmux process.
use super::client::{Client, Pane, Snapshot};
use super::identity::ProcessIdentity;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
pub(super) struct Operation {
    pub endpoint: String,
    pub boot: String,
    pub action: Action,
    pub target: String,
    pub terminals: HashMap<String, ProcessIdentity>,
}

impl Operation {
    pub fn command(&self) -> Result<String> {
        let executable = std::env::current_exe().context("Locate workmux executable")?;
        // Unit-test executables cannot dispatch CLI commands. The isolated
        // integration runner supplies the normal binary built alongside them.
        #[cfg(test)]
        let executable = std::env::var_os("WORKMUX_HERDR_TEST_EXECUTABLE")
            .map(std::path::PathBuf::from)
            .unwrap_or(executable);
        Ok(format!(
            "{} _herdr-deferred {}",
            super::agent::shell_quote(executable.to_str().context("Non-UTF-8 workmux path")?),
            super::agent::shell_quote(&serde_json::to_string(self)?),
        ))
    }

    fn contains(&self, pane: &Pane) -> bool {
        if self.action.workspace() {
            pane.workspace_id == self.target
        } else {
            pane.tab_id == self.target
        }
    }

    fn target_exists(&self, snapshot: &Snapshot) -> bool {
        if self.action.workspace() {
            snapshot
                .workspaces
                .iter()
                .any(|w| w.workspace_id == self.target)
        } else {
            snapshot.tabs.iter().any(|t| t.tab_id == self.target)
        }
    }

    fn verify_contents(&self, snapshot: &Snapshot) -> Result<()> {
        let live: HashSet<_> = snapshot
            .panes
            .iter()
            .filter(|p| self.contains(p))
            .map(|p| &p.terminal_id)
            .collect();
        ensure!(
            self.terminals
                .keys()
                .all(|terminal| live.contains(terminal)),
            "Herdr target contents changed; refusing deferred close"
        );
        Ok(())
    }

    fn verify_process(client: &Client, pane: &Pane, shell: &ProcessIdentity) -> Result<()> {
        let process = client.request("pane.process_info", json!({"pane_id": pane.pane_id}))?;
        ensure!(
            process["process_info"]["shell_pid"].as_u64() == Some(u64::from(shell.pid))
                && shell.is_live(),
            "Herdr terminal process changed"
        );
        Ok(())
    }

    fn run(self) -> Result<()> {
        let client = Client::for_lifetime(self.endpoint.clone(), self.boot.clone());
        let snapshot = client.snapshot()?;
        ensure!(
            self.target_exists(&snapshot),
            "Herdr target no longer exists"
        );
        if !self.action.close() {
            let (method, params) = if self.action.workspace() {
                ("workspace.focus", json!({"workspace_id": self.target}))
            } else {
                ("tab.focus", json!({"tab_id": self.target}))
            };
            client.request(method, params)?;
            return Ok(());
        }
        ensure!(
            self.action.workspace() || !self.terminals.is_empty(),
            "Herdr cleanup target has no owned terminals"
        );
        self.verify_contents(&snapshot)?;
        for pane in snapshot.panes.iter().filter(|p| self.contains(p)) {
            if let Some(shell) = self.terminals.get(&pane.terminal_id) {
                Self::verify_process(&client, pane, shell)?;
            }
        }
        self.verify_contents(&client.snapshot()?)?;
        // Match tmux: extra panes belong to the target being closed. Never
        // follow a captured terminal into a different tab or workspace.
        let (method, params) = if self.action.workspace() {
            ("workspace.close", json!({"workspace_id": self.target}))
        } else {
            ("tab.close", json!({"tab_id": self.target}))
        };
        client.request(method, params)?;
        Ok(())
    }
}

pub fn run(payload: &str) -> Result<()> {
    serde_json::from_str::<Operation>(payload)
        .context("Invalid Herdr deferred operation")?
        .run()
}
