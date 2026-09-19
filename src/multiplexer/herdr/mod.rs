//! Herdr 0.9.0 / protocol 22: workmux sessions are workspaces, windows are tabs.
//! Public focus is native broadcast focus. No implicit endpoint or last-focus fallback.
#[cfg(test)]
mod cleanup_tests;
mod client;
mod deferred;
#[cfg(test)]
mod deferred_tests;
#[cfg(test)]
mod deferred_unit_tests;
#[cfg(test)]
mod detection_tests;
mod identity;
#[cfg(test)]
mod input_keys_tests;
mod pane_launch;
#[cfg(test)]
mod platform_tests;
mod session;
#[cfg(test)]
mod session_tests;
mod setup;
#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod tests;

use super::*;
use anyhow::{bail, ensure};
use client::{Client, Pane, Snapshot, Tab, Workspace};
use identity::ProcessIdentity;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Debug, Deserialize)]
struct PaneDimensions {
    width: u16,
    height: u16,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
struct TerminalOwnership {
    backend: String,
    endpoint: String,
    boot: String,
    terminal: String,
    shell: ProcessIdentity,
    token: Option<String>,
    primary: bool,
}

#[derive(Serialize, Deserialize)]
struct Ownership {
    boot: String,
    tab_id: String,
    token: Option<String>,
    primary: bool,
    terminals: HashSet<String>,
}

pub struct HerdrBackend {
    client: Client,
    // Only freshly allocated panes in this operation may be replaced for launch.
    fresh: Mutex<HashSet<String>>,
    pending_launch: Mutex<Option<Arc<pane_launch::Launch>>>,
    setup_lock: Mutex<()>,
    launch_directory: Mutex<Option<tempfile::TempDir>>,
    initial_launches: Mutex<HashMap<String, Arc<pane_launch::Launch>>>,
    launches: Mutex<HashMap<String, Arc<pane_launch::Launch>>>,
}
impl HerdrBackend {
    pub fn new() -> Self {
        Self::for_socket(&std::env::var("HERDR_SOCKET_PATH").unwrap_or_default())
    }
    pub fn for_socket(endpoint: &str) -> Self {
        let endpoint = if Path::new(endpoint).is_absolute() {
            std::fs::canonicalize(endpoint)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| endpoint.to_string())
        } else {
            endpoint.to_string()
        };
        Self {
            client: Client::new(endpoint),
            fresh: Mutex::new(HashSet::new()),
            pending_launch: Mutex::new(None),
            setup_lock: Mutex::new(()),
            launch_directory: Mutex::new(None),
            initial_launches: Mutex::new(HashMap::new()),
            launches: Mutex::new(HashMap::new()),
        }
    }

    fn pane_dimensions(&self, key: &str) -> Result<PaneDimensions> {
        let pane = self.pane(key)?;
        let result = self
            .client
            .request("pane.layout", json!({"pane_id":pane.pane_id}))?;
        let rect = result["layout"]["panes"]
            .as_array()
            .context("Missing Herdr pane geometry")?
            .iter()
            .find(|p| p["pane_id"] == pane.pane_id)
            .context("Missing Herdr pane rectangle")?;
        serde_json::from_value(rect["rect"].clone()).context("Malformed Herdr pane dimensions")
    }
    fn set_status_state(
        &self,
        pane_id: &str,
        status: AgentStatus,
        icon: &str,
        _: bool,
    ) -> Result<()> {
        let pane = self.pane(pane_id)?;
        // Herdr's protocol has no workmux icon or done state. Its agent report
        // is still useful for native observers: waiting maps to blocked and
        // done maps to idle; the workmux state store remains authoritative for
        // the exact working/waiting/done lifecycle.
        let state = match status {
            AgentStatus::Working => "working",
            AgentStatus::Waiting => "blocked",
            AgentStatus::Done => "idle",
        };
        self.client.request(
            "pane.report_agent",
            json!({
                "pane_id": pane.pane_id,
                "source": "workmux",
                "agent": "workmux",
                "state": state,
                "message": icon,
            }),
        )?;
        Ok(())
    }
    fn key(&self, id: &str) -> Result<String> {
        Ok(format!("{}~{id}", self.client.boot()?))
    }
    fn raw<'a>(&self, key: &'a str) -> Result<&'a str> {
        let (boot, id) = key
            .split_once('~')
            .context("Expected a verified Herdr target identity")?;
        ensure!(
            boot == self.client.boot()?,
            "Herdr target belongs to a previous server lifetime"
        );
        Ok(id)
    }
    fn pane(&self, key: &str) -> Result<Pane> {
        let terminal = self.raw(key)?;
        self.client
            .snapshot()?
            .panes
            .into_iter()
            .find(|p| p.terminal_id == terminal)
            .context("Herdr terminal no longer exists")
    }
    fn session(&self, name: &str) -> Result<Workspace> {
        let snapshot = self.client.snapshot()?;
        let id = if let Some((boot, id)) = name.split_once('~') {
            ensure!(
                boot == self.client.boot()?,
                "Stale Herdr workspace identity"
            );
            Some(id)
        } else {
            None
        };
        let matches: Vec<_> = snapshot
            .workspaces
            .into_iter()
            .filter(|w| id.is_some_and(|id| w.workspace_id == id) || w.label == name)
            .collect();
        ensure!(!matches.is_empty(), "Herdr workspace '{name}' is missing");
        ensure!(
            matches.len() == 1,
            "Herdr workspace '{name}' is ambiguous ({} matches); use a unique workspace name",
            matches.len()
        );
        Ok(matches.into_iter().next().unwrap())
    }
    fn tab(&self, target: &WindowTarget) -> Result<Tab> {
        let s = self.client.snapshot()?;
        let id = target
            .window_id
            .as_deref()
            .map(|id| self.raw(id))
            .transpose()?;
        let parent = target
            .parent_session
            .as_deref()
            .map(|name| self.session(name))
            .transpose()?;
        let matches: Vec<_> = s
            .tabs
            .into_iter()
            .filter(|t| {
                if let Some(id) = id {
                    return t.tab_id == id;
                }
                t.label == target.full_name
                    && parent
                        .as_ref()
                        .is_none_or(|parent| parent.workspace_id == t.workspace_id)
            })
            .collect();
        ensure!(
            matches.len() == 1,
            "Herdr tab '{}' is missing or ambiguous",
            target.full_name
        );
        Ok(matches.into_iter().next().unwrap())
    }
    fn caller(&self) -> Result<Option<Pane>> {
        // An inherited target is a hint, not permission. Resolve from the
        // selected endpoint and actual ancestry, even when native IDs are stale.
        let ancestors = ancestor_pids()?;
        for pane in self.client.snapshot()?.panes {
            let process = match self.process(&pane) {
                Ok(process) => process,
                Err(error) => {
                    // A temporary or unrelated terminal can close during this
                    // read-only scan. Confirm its absence; do not mask a lost
                    // connection or an error on a still-live terminal.
                    if self
                        .client
                        .snapshot()?
                        .panes
                        .iter()
                        .any(|live| live.terminal_id == pane.terminal_id)
                    {
                        return Err(error);
                    }
                    continue;
                }
            };
            let pid: u32 = process["shell_pid"]
                .as_u64()
                .context("Missing Herdr shell PID")?
                .try_into()?;
            if ancestors.contains(&pid) && ProcessIdentity::read(pid)?.is_live() {
                // Herdr creates one child process per terminal. Once that
                // live ancestor is found, unrelated closing panes cannot
                // invalidate this caller's identity.
                return Ok(Some(pane));
            }
        }
        self.popup_caller(&ancestors)
    }

    fn popup_caller(&self, ancestors: &[u32]) -> Result<Option<Pane>> {
        let server_pid = self.client.server_pid()?;
        let Some(index) = ancestors.iter().position(|pid| *pid == server_pid) else {
            return Ok(None);
        };
        let Some(pid) = index.checked_sub(1).and_then(|index| ancestors.get(index)) else {
            return Ok(None);
        };
        let launcher = ProcessIdentity::read(*pid)?;
        let env = identity::launch_environment(&launcher)?;
        if env
            .get("HERDR_SOCKET_PATH")
            .and_then(|path| std::fs::canonicalize(path).ok())
            .as_deref()
            != Some(Path::new(&self.client.endpoint))
        {
            return Ok(None);
        }
        let Some(address) = env.get("HERDR_ACTIVE_PANE_ID") else {
            return Ok(None);
        };
        // Protocol 22 preserves old public addresses as aliases after a move
        // (app/ids.rs::parse_pane_id). The kernel-verified native launcher ties
        // that alias to this server lifetime. It is navigation context only.
        let result = self
            .client
            .request("pane.get", json!({"pane_id":address}))?;
        let pane: Pane = serde_json::from_value(result["pane"].clone())?;
        ensure!(launcher.is_live(), "Native Herdr popup launcher exited");
        Ok(Some(pane))
    }

    fn process(&self, pane: &Pane) -> Result<Value> {
        Ok(self
            .client
            .request("pane.process_info", json!({"pane_id":pane.pane_id}))?["process_info"]
            .clone())
    }
    fn record_path(&self, id: &str) -> Result<PathBuf> {
        // File-per-object avoids lost updates between unrelated concurrent adds.
        let hex = |s: &str| s.bytes().map(|b| format!("{b:02x}")).collect::<String>();
        Ok(crate::xdg::state_dir()?
            .join("herdr")
            .join(hex(&self.client.endpoint))
            .join(self.client.boot()?)
            .join(hex(id)))
    }
    fn terminal_record(&self, terminal: &str) -> Result<Option<TerminalOwnership>> {
        match std::fs::read(self.record_path(&format!("terminal-{terminal}"))?) {
            Ok(data) => Ok(Some(serde_json::from_slice(&data)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn save_terminal_record(&self, record: &TerminalOwnership) -> Result<()> {
        let path = self.record_path(&format!("terminal-{}", record.terminal))?;
        let parent = path.parent().context("Missing Herdr ownership parent")?;
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut file, record)?;
        file.persist(path)?;
        Ok(())
    }

    fn own_terminal(&self, terminal: &str) -> Result<()> {
        let pane = self.pane(&self.key(terminal)?)?;
        let process = self.process(&pane)?;
        self.save_terminal_record(&TerminalOwnership {
            backend: "herdr".into(),
            endpoint: self.instance_id(),
            boot: self.client.boot()?,
            terminal: terminal.into(),
            shell: ProcessIdentity::read(
                process["shell_pid"]
                    .as_u64()
                    .context("Missing Herdr shell PID")?
                    .try_into()?,
            )?,
            token: None,
            primary: false,
        })
    }

    fn verified_terminal(&self, pane: &Pane) -> Result<TerminalOwnership> {
        let record = self
            .terminal_record(&pane.terminal_id)?
            .context("Refusing an unowned Herdr terminal")?;
        ensure!(
            record.backend == "herdr"
                && record.endpoint == self.instance_id()
                && record.boot == self.client.boot()?
                && record.terminal == pane.terminal_id,
            "Foreign or stale Herdr terminal ownership"
        );
        let process = self.process(pane)?;
        ensure!(
            process["shell_pid"].as_u64() == Some(u64::from(record.shell.pid))
                && record.shell.is_live(),
            "Herdr terminal process lifetime changed"
        );
        Ok(record)
    }

    fn record(&self, tab: &str) -> Result<Option<Ownership>> {
        match std::fs::read(self.record_path(tab)?) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn save_record(&self, record: &Ownership) -> Result<()> {
        let path = self.record_path(&record.tab_id)?;
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut file, record)?;
        file.persist(path)?;
        Ok(())
    }
    fn own_tab(&self, tab: &str, terminal: &str) -> Result<()> {
        self.own_terminal(terminal)?;
        self.save_record(&Ownership {
            boot: self.client.boot()?,
            tab_id: tab.into(),
            token: None,
            primary: false,
            terminals: HashSet::from([terminal.into()]),
        })
    }
    fn add_terminal(&self, tab: &str, terminal: &str) -> Result<()> {
        if let Some(mut record) = self.record(tab)? {
            if self.terminal_record(terminal)?.is_none() {
                return Ok(());
            }
            record.terminals.insert(terminal.into());
            self.save_record(&record)?;
        }
        Ok(())
    }

    fn verified_record(&self, tab: &Tab, snapshot: &Snapshot) -> Result<Ownership> {
        let panes: Vec<_> = snapshot
            .panes
            .iter()
            .filter(|p| p.tab_id == tab.tab_id)
            .collect();
        ensure!(!panes.is_empty(), "Herdr tab has no live terminals");
        let mut records = Vec::new();
        for pane in &panes {
            if self.terminal_record(&pane.terminal_id)?.is_some() {
                records.push(self.verified_terminal(pane)?);
            }
        }
        let first = records.first().context("Refusing an unowned Herdr tab")?;
        Ok(Ownership {
            boot: self.client.boot()?,
            tab_id: tab.tab_id.clone(),
            token: first.token.clone(),
            primary: records.iter().any(|r| r.primary),
            terminals: panes.iter().map(|p| p.terminal_id.clone()).collect(),
        })
    }
    fn launch(&self, shell: &str) -> Result<Arc<pane_launch::Launch>> {
        let mut directory = self.launch_directory.lock().unwrap();
        if directory.is_none() {
            *directory = Some(
                tempfile::Builder::new()
                    .prefix("workmux-herdr-launch-")
                    .tempdir()?,
            );
        }
        pane_launch::Launch::new(shell, directory.as_ref().unwrap().path())
    }
    fn new_tab(
        &self,
        workspace: &str,
        name: Option<&str>,
        cwd: &Path,
        command: Option<&str>,
    ) -> Result<String> {
        let shell = self.get_default_shell()?;
        let initial_launch = command.is_none().then(|| self.launch(&shell)).transpose()?;
        let script = command
            .map(str::to_string)
            .unwrap_or_else(|| initial_launch.as_ref().unwrap().initial_shell());
        let argv = vec!["/bin/sh".to_string(), "-c".into(), script];
        // Never pass tab_id: layout.apply on an occupied tab kills its programs.
        let r = self.client.request("layout.apply", json!({"workspace_id":workspace, "tab_label":name, "focus":false, "root":{"type":"pane", "cwd":cwd, "command":argv}}))?;
        let tab_id = r["tab"]["tab_id"]
            .as_str()
            .or_else(|| r["layout"]["tab_id"].as_str())
            .context("Missing created Herdr tab")?;
        let p = self
            .client
            .snapshot()?
            .panes
            .into_iter()
            .find(|p| p.tab_id == tab_id)
            .context("Missing created Herdr pane")?;
        self.own_tab(tab_id, &p.terminal_id)?;
        let key = self.key(&p.terminal_id)?;
        if let Some(launch) = initial_launch {
            self.fresh.lock().unwrap().insert(key.clone());
            self.initial_launches
                .lock()
                .unwrap()
                .insert(key.clone(), launch);
        }
        Ok(key)
    }
    /// Allocate only a new terminal, then move that verified identity. A gate
    /// keeps short commands alive until ownership and the destination are set.
    fn move_launch(
        &self,
        target: &str,
        cwd: &Path,
        command: Option<&str>,
        direction: &str,
        ratio: f64,
        replace: bool,
    ) -> Result<String> {
        let destination = self.pane(target)?;
        let gate = self.launch("/bin/sh")?;
        let key = self.new_tab(
            &destination.workspace_id,
            None,
            cwd,
            Some(&gate.move_gate()),
        )?;
        let result = (|| -> Result<()> {
            let pane = self.pane(&key)?;
            let live_destination = self.pane(target)?;
            ensure!(
                live_destination.tab_id == destination.tab_id
                    && live_destination.pane_id == destination.pane_id,
                "Herdr launch target moved during allocation"
            );
            self.client.request("pane.move", json!({
                "pane_id":pane.pane_id,
                "destination":{"type":"tab", "tab_id":destination.tab_id,
                    "target_pane_id":live_destination.pane_id, "split":direction, "ratio":ratio},
                "focus":false
            }))?;
            // Tab records can be stale after a native move. The live terminal
            // owns the token; only a replacement inherits its primary role.
            if self
                .terminal_record(&live_destination.terminal_id)?
                .is_some()
            {
                let owner = self.verified_terminal(&live_destination)?;
                let mut record = self.verified_terminal(&self.pane(&key)?)?;
                record.token = owner.token;
                record.primary = replace && owner.primary;
                self.save_terminal_record(&record)?;
            }
            self.add_terminal(&destination.tab_id, &pane.terminal_id)?;
            if replace {
                self.kill_pane(target)?;
                self.fresh.lock().unwrap().remove(target);
                self.initial_launches.lock().unwrap().remove(target);
            }
            let mut pending = self.pending_launch.lock().unwrap();
            if pending
                .as_ref()
                .is_some_and(|launch| command == Some(launch.script().as_str()))
            {
                self.launches
                    .lock()
                    .unwrap()
                    .insert(key.clone(), pending.take().unwrap());
            }
            drop(pending);
            let script = command.map(str::to_string).unwrap_or(format!(
                "exec {} -il",
                agent::shell_quote(&self.get_default_shell()?)
            ));
            gate.deliver(&script)
        })();
        if let Err(error) = result {
            gate.cancel();
            self.cancel_pane_launch(&key)?;
            // This terminal was allocated above, not adopted by label. Never
            // close its tab: a concurrent move may have added unrelated panes.
            if let Ok(pane) = self.pane(&key) {
                self.client
                    .request("pane.close", json!({"pane_id":pane.pane_id}))?;
            }
            return Err(error);
        }
        if replace && command.is_none() {
            self.fresh.lock().unwrap().insert(key.clone());
        }
        Ok(key)
    }
    fn create_tab_in_workspace(&self, workspace: &str, p: CreateWindowParams) -> Result<String> {
        // Resolve placement before allocation. An invalid target must not leave a tab.
        let insert_index = p
            .after_window
            .map(|key| -> Result<u32> {
                let id = self.raw(key)?;
                self.client
                    .snapshot()?
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == workspace)
                    .position(|tab| tab.tab_id == id)
                    .context("Invalid Herdr placement target")
                    .and_then(|index| Ok(u32::try_from(index + 1)?))
            })
            .transpose()?;
        let key = self.new_tab(
            workspace,
            Some(&util::prefixed(p.prefix, p.name)),
            p.cwd,
            None,
        )?;
        if let Some(index) = insert_index {
            self.client.request(
                "tab.move",
                json!({"tab_id":self.pane(&key)?.tab_id,"insert_index":index}),
            )?;
        }
        Ok(key)
    }
    fn pane_op(&self, key: &str, method: &str) -> Result<()> {
        let p = self.pane(key)?;
        self.client.request(method, json!({"pane_id":p.pane_id}))?;
        Ok(())
    }
    fn deferred_command(&self, action: deferred::Action, target_id: &str) -> Result<String> {
        let target = self.raw(target_id)?;
        let snapshot = self.client.snapshot()?;
        let workspace = action.workspace();
        let mut terminals = HashMap::new();
        if action.close() {
            for tab in snapshot.tabs.iter().filter(|tab| {
                if workspace {
                    tab.workspace_id == target
                } else {
                    tab.tab_id == target
                }
            }) {
                if !workspace {
                    self.verified_record(tab, &snapshot)?;
                }
                for pane in snapshot
                    .panes
                    .iter()
                    .filter(|pane| pane.tab_id == tab.tab_id)
                {
                    if self.terminal_record(&pane.terminal_id)?.is_some() {
                        terminals.insert(
                            pane.terminal_id.clone(),
                            self.verified_terminal(pane)?.shell,
                        );
                    }
                }
            }
            ensure!(
                workspace || !terminals.is_empty(),
                "Herdr cleanup target has no owned terminals"
            );
            if workspace {
                ensure!(
                    std::fs::read_to_string(self.record_path(target)?)
                        .ok()
                        .as_deref()
                        == Some(&self.client.boot()?),
                    "Refusing to close an unowned Herdr workspace"
                );
            }
        }
        deferred::Operation {
            endpoint: self.client.endpoint.clone(),
            boot: self.client.boot()?,
            action,
            target: target.to_string(),
            terminals,
        }
        .command()
    }
    /// `pane.read` is capped at 1000 rows. Selection reads are read-only and
    /// accept absolute history coordinates. Split a batch only when native copy
    /// unwrapping has joined physical rows. All batches use one content revision.
    fn capture_history_rows(&self, pane: &Pane, lines: u16) -> Result<String> {
        let info = self
            .client
            .request("pane.get", json!({"pane_id":pane.pane_id}))?;
        let scroll = &info["pane"]["scroll"];
        let viewport = scroll["viewport_rows"]
            .as_u64()
            .context("Missing Herdr viewport size")?;
        let total = u32::try_from(
            scroll["max_offset_from_bottom"]
                .as_u64()
                .context("Missing Herdr history size")?
                + viewport,
        )?;
        let start = total.saturating_sub(u32::from(lines) + u32::try_from(viewport)?);
        let mut output = Vec::new();
        let mut revision = None;
        for first in (start..total).step_by(256) {
            self.capture_row_batch(
                &pane.pane_id,
                first,
                (first + 255).min(total - 1),
                &mut revision,
                &mut output,
            )?;
        }
        while output
            .last()
            .is_some_and(|line: &String| line.trim().is_empty())
        {
            output.pop();
        }
        Ok(output.join("\n"))
    }
    fn capture_row_batch(
        &self,
        pane_id: &str,
        first: u32,
        last: u32,
        revision: &mut Option<u64>,
        output: &mut Vec<String>,
    ) -> Result<()> {
        let end = self.client.request(
            "pane.copy_motion",
            json!({
                "pane_id":pane_id, "cursor":{"row":last,"col":0},
                "motion":"line_end", "content_revision":*revision
            }),
        )?;
        *revision = Some(
            end["content_revision"]
                .as_u64()
                .context("Missing Herdr content revision")?,
        );
        let selected = self.client.request(
            "pane.selection.read",
            json!({
                "pane_id":pane_id, "anchor":{"row":first,"col":0},
                "cursor":end["cursor"], "content_revision":*revision
            }),
        )?;
        let text = selected["text"]
            .as_str()
            .context("Missing Herdr selection text")?;
        let rows: Vec<_> = text.split('\n').collect();
        if rows.len() == (last - first + 1) as usize || first == last {
            output.extend(rows.into_iter().map(|row| row.trim_end().to_string()));
        } else {
            let middle = first + (last - first) / 2;
            self.capture_row_batch(pane_id, first, middle, revision, output)?;
            self.capture_row_batch(pane_id, middle + 1, last, revision, output)?;
        }
        Ok(())
    }
    fn info(&self, p: &Pane, s: &Snapshot) -> Result<LivePaneInfo> {
        let process = self.process(p)?;
        let pid = process["shell_pid"]
            .as_u64()
            .context("Missing Herdr shell PID")?;
        Ok(LivePaneInfo {
            pid: Some(u32::try_from(pid)?),
            current_command: process["foreground_processes"]
                .as_array()
                .and_then(|processes| {
                    processes
                        .iter()
                        .find(|p| p["pid"] == process["foreground_process_group_id"])
                        .or_else(|| processes.first())
                })
                .and_then(|p| p["name"].as_str())
                .map(str::to_string),
            working_dir: PathBuf::from(p.foreground_cwd.as_ref().unwrap_or(&p.cwd)),
            title: p.title.clone().or_else(|| p.label.clone()),
            session: s
                .workspaces
                .iter()
                .find(|w| w.workspace_id == p.workspace_id)
                .map(|w| w.label.clone()),
            window: s
                .tabs
                .iter()
                .find(|t| t.tab_id == p.tab_id)
                .map(|t| t.label.clone()),
            session_id: Some(self.key(&p.workspace_id)?),
            window_id: Some(self.key(&p.tab_id)?),
        })
    }
}

/// Preserve nested multiplexer precedence; Herdr replaces only the tmux fallback.
pub(super) fn detect_fallback(detected: BackendType) -> BackendType {
    detect_backend_with_signals(
        detected,
        std::env::var_os("TMUX").is_some(),
        std::env::var_os("HERDR_SOCKET_PATH").is_some(),
    )
}

fn detect_backend_with_signals(detected: BackendType, tmux: bool, herdr: bool) -> BackendType {
    if detected == BackendType::Tmux && !tmux && herdr {
        BackendType::Herdr
    } else {
        detected
    }
}

impl Drop for HerdrBackend {
    fn drop(&mut self) {
        // A successful creation without setup is still a usable shell. On
        // process interruption this destructor does not run: the guards cancel.
        for launch in self.initial_launches.get_mut().unwrap().values() {
            let _ = launch.release();
        }
    }
}

setup::impl_backend! {
    fn setup_panes(
        &self,
        initial_pane_id: &str,
        panes: &[PaneConfig],
        working_dir: &Path,
        options: PaneSetupOptions<'_>,
        config: &Config,
        task_agent: Option<&str>,
    ) -> Result<PaneSetupResult> {
        let _lock = self.setup_lock.lock().unwrap();
        let setup = setup::Setup::new(self, initial_pane_id);
        let result = setup.setup_panes(
            initial_pane_id, panes, working_dir, options, config, task_agent,
        )?;
        setup.finish()?;
        Ok(result)
    }

    fn name(&self) -> &'static str {
        "herdr"
    }

    fn is_running(&self) -> Result<bool> {
        self.client.snapshot().map(|_| true)
    }
    fn instance_id(&self) -> String {
        self.client.endpoint.clone()
    }
    fn resolve_instance_id(&self) -> Result<String> {
        self.client.snapshot()?;
        Ok(self.instance_id())
    }
    fn server_boot_id(&self) -> Result<Option<String>> {
        self.client.snapshot()?;
        self.client.boot().map(Some)
    }
    fn current_pane_id(&self) -> Option<String> {
        self.caller()
            .ok()
            .flatten()
            .and_then(|p| self.key(&p.terminal_id).ok())
    }
    fn active_pane_id(&self) -> Option<String> {
        self.current_pane_id()
    }
    fn current_window_id(&self) -> Result<Option<String>> {
        self.caller()?.map(|p| self.key(&p.tab_id)).transpose()
    }
    fn current_session_id(&self) -> Result<Option<String>> {
        self.caller()?
            .map(|p| self.key(&p.workspace_id))
            .transpose()
    }
    fn current_session(&self) -> Option<String> {
        let p = self.caller().ok()??;
        self.client
            .snapshot()
            .ok()?
            .workspaces
            .into_iter()
            .find(|w| w.workspace_id == p.workspace_id)
            .map(|w| w.label)
    }
    fn current_window_name(&self) -> Result<Option<String>> {
        let Some(p) = self.caller()? else {
            return Ok(None);
        };
        Ok(self
            .client
            .snapshot()?
            .tabs
            .into_iter()
            .find(|t| t.tab_id == p.tab_id)
            .map(|t| t.label))
    }
    fn get_client_active_pane_path(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(
            self.caller()?.context("No verified Herdr caller pane")?.cwd,
        ))
    }
    fn rightmost_window_id(&self) -> Result<Option<String>> {
        let Some(p) = self.caller()? else {
            return Ok(None);
        };
        self.client
            .snapshot()?
            .tabs
            .iter()
            .rev()
            .find(|t| t.workspace_id == p.workspace_id)
            .map(|t| self.key(&t.tab_id))
            .transpose()
    }
    fn create_window(&self, p: CreateWindowParams) -> Result<String> {
        let caller = self
            .caller()?
            .context("No verified Herdr caller; use --parent-session to select a workspace")?;
        self.create_tab_in_workspace(&caller.workspace_id, p)
    }
    fn create_session(&self, p: CreateSessionParams) -> Result<String> {
        let name = util::prefixed(p.prefix, p.name);
        ensure!(
            !self.session_exists(&name)?,
            "Herdr workspace '{name}' already exists"
        );
        let r = self.client.request(
            "workspace.create",
            json!({"label":name,"cwd":p.cwd,"focus":false}),
        )?;
        let pane: Pane = serde_json::from_value(r["root_pane"].clone())?;
        self.own_tab(&pane.tab_id, &pane.terminal_id)?;
        let marker = self.record_path(&pane.workspace_id)?;
        std::fs::write(marker, self.client.boot()?)?;
        if let Some(name) = p.initial_window_name {
            self.client
                .request("tab.rename", json!({"tab_id":pane.tab_id,"label":name}))?;
        }
        let key = self.key(&pane.terminal_id)?;
        self.fresh.lock().unwrap().insert(key.clone());
        self.respawn_pane(&key, p.cwd, None)
    }
    fn create_window_in_session(&self, p: CreateWindowInSessionParams) -> Result<String> {
        self.new_tab(
            &self.session(p.session_name)?.workspace_id,
            p.name,
            p.cwd,
            None,
        )
    }
    fn supports_window_ownership(&self) -> bool {
        true
    }
    fn set_window_ownership(&self, key: &str, token: &str, primary: bool) -> Result<()> {
        let target = self.pane(key)?;
        let snapshot = self.client.snapshot()?;
        let tab = snapshot
            .tabs
            .iter()
            .find(|t| t.tab_id == target.tab_id)
            .context("Herdr tab no longer exists")?;
        let mut record = self.verified_record(tab, &snapshot)?;
        let mut terminals = Vec::new();
        for pane in snapshot.panes.iter().filter(|p| p.tab_id == target.tab_id) {
            let mut record = self.verified_terminal(pane)?;
            ensure!(
                record.token.as_deref().is_none_or(|old| old == token),
                "Herdr terminal has another owner"
            );
            record.token = Some(token.into());
            record.primary = primary && pane.terminal_id == target.terminal_id;
            terminals.push(record);
        }
        for terminal in terminals {
            self.save_terminal_record(&terminal)?;
        }
        record.token = Some(token.into());
        record.primary = primary;
        self.save_record(&record)
    }
    fn owned_window_targets(&self, token: &str) -> Result<Vec<OwnedWindowTarget>> {
        let s = self.client.snapshot()?;
        let mut result = Vec::new();
        for t in &s.tabs {
            let mut owned = false;
            for pane in s.panes.iter().filter(|p| p.tab_id == t.tab_id) {
                if self
                    .terminal_record(&pane.terminal_id)?
                    .is_some_and(|r| r.token.as_deref() == Some(token))
                {
                    self.verified_terminal(pane)?;
                    owned = true;
                }
            }
            if owned {
                let r = self.verified_record(t, &s)?;
                let parent = s
                    .workspaces
                    .iter()
                    .find(|w| w.workspace_id == t.workspace_id)
                    .map(|w| w.label.clone());
                result.push(OwnedWindowTarget {
                    target: WindowTarget::with_id(t.label.clone(), parent, self.key(&t.tab_id)?),
                    is_primary: r.primary,
                });
            }
        }
        Ok(result)
    }
    fn owned_window_tokens(&self) -> Result<HashSet<String>> {
        let mut result = HashSet::new();
        for pane in self.client.snapshot()?.panes {
            if let Some(token) = self
                .terminal_record(&pane.terminal_id)?
                .and_then(|r| r.token)
            {
                self.verified_terminal(&pane)?;
                result.insert(token);
            }
        }
        Ok(result)
    }
    // Default adoption sees no records. A label/cwd is never proof of ownership.
    fn session_exists(&self, name: &str) -> Result<bool> {
        let snapshot = self.client.snapshot()?;
        if let Some((boot, id)) = name.split_once('~') {
            ensure!(
                boot == self.client.boot()?,
                "Stale Herdr workspace identity"
            );
            return Ok(snapshot.workspaces.iter().any(|w| w.workspace_id == id));
        }
        Ok(snapshot.workspaces.iter().any(|w| w.label == name))
    }
    fn resolve_session_open_name(&self, prefix: &str, name: &str) -> Result<String> {
        self.session_open_name(prefix, name)
    }
    fn switch_to_session(&self, prefix: &str, name: &str) -> Result<()> {
        self.client.request(
            "workspace.focus",
            json!({"workspace_id":self.session(&util::prefixed(prefix,name))?.workspace_id}),
        )?;
        Ok(())
    }
    fn kill_session(&self, name: &str) -> Result<()> {
        let w = self.session(name)?;
        ensure!(
            std::fs::read_to_string(self.record_path(&w.workspace_id)?)
                .ok()
                .as_deref()
                == Some(&self.client.boot()?),
            "Refusing to close an unowned Herdr workspace"
        );
        self.client
            .request("workspace.close", json!({"workspace_id": w.workspace_id}))?;
        Ok(())
    }
    fn kill_window(&self, name: &str) -> Result<()> {
        self.kill_window_target(&WindowTarget::new(name.into(), None))
    }
    fn kill_window_target(&self, target: &WindowTarget) -> Result<()> {
        let tab = self.tab(target)?;
        self.verified_record(&tab, &self.client.snapshot()?)?;
        self.client
            .request("tab.close", json!({"tab_id": tab.tab_id}))?;
        Ok(())
    }
    fn rename_window(&self, old: &str, new: &str) -> Result<()> {
        let tab = self.tab(&WindowTarget::new(old.into(), None))?;
        self.client
            .request("tab.rename", json!({"tab_id":tab.tab_id,"label":new}))?;
        Ok(())
    }
    fn rename_window_at_pane(&self, p: &str, new: &str) -> Result<()> {
        self.client.request(
            "tab.rename",
            json!({"tab_id":self.pane(p)?.tab_id,"label":new}),
        )?;
        Ok(())
    }
    fn rename_session(&self, old: &str, new: &str) -> Result<()> {
        let workspace = self.session(old)?;
        ensure!(
            self.workspace_owned(&workspace.workspace_id)?,
            "Refusing to rename an unowned Herdr workspace"
        );
        ensure!(!self.session_exists(new)?, "Herdr workspace '{new}' already exists");
        self.client.request(
            "workspace.rename",
            json!({"workspace_id":workspace.workspace_id,"label":new}),
        )?;
        Ok(())
    }
    fn select_window(&self, prefix: &str, name: &str) -> Result<()> {
        self.select_window_target(&WindowTarget::new(util::prefixed(prefix, name), None))
    }
    fn select_window_target(&self, target: &WindowTarget) -> Result<()> {
        self.client
            .request("tab.focus", json!({"tab_id":self.tab(target)?.tab_id}))?;
        Ok(())
    }
    fn window_target_exists(&self, target: &WindowTarget) -> Result<bool> {
        let s = self.client.snapshot()?;
        if let Some(id) = &target.window_id {
            let id = self.raw(id)?;
            return Ok(s.tabs.iter().any(|t| t.tab_id == id));
        }
        let parent = target
            .parent_session
            .as_deref()
            .map(|name| self.session(name))
            .transpose()?;
        let count = s
            .tabs
            .iter()
            .filter(|t| {
                t.label == target.full_name
                    && parent
                        .as_ref()
                        .is_none_or(|parent| parent.workspace_id == t.workspace_id)
            })
            .count();
        ensure!(count <= 1, "Herdr tab '{}' is ambiguous", target.full_name);
        Ok(count == 1)
    }
    fn get_all_window_names(&self) -> Result<HashSet<String>> {
        Ok(self
            .client
            .snapshot()?
            .tabs
            .into_iter()
            .map(|t| t.label)
            .collect())
    }
    fn get_window_names_in_session(&self, name: &str) -> Result<HashSet<String>> {
        let w = self.session(name)?;
        Ok(self
            .client
            .snapshot()?
            .tabs
            .into_iter()
            .filter(|t| t.workspace_id == w.workspace_id)
            .map(|t| t.label)
            .collect())
    }
    fn get_all_session_names(&self) -> Result<HashSet<String>> {
        Ok(self
            .client
            .snapshot()?
            .workspaces
            .into_iter()
            .map(|w| w.label)
            .collect())
    }
    fn get_all_windows_with_sessions(&self) -> Result<HashSet<(String, String)>> {
        let s = self.client.snapshot()?;
        Ok(s.tabs
            .iter()
            .filter_map(|t| {
                s.workspaces
                    .iter()
                    .find(|w| w.workspace_id == t.workspace_id)
                    .map(|w| (t.label.clone(), w.label.clone()))
            })
            .collect())
    }
    fn select_pane(&self, p: &str) -> Result<()> {
        self.pane_op(p, "pane.focus")
    }
    fn switch_to_pane(&self, p: &str, _session: Option<&str>) -> Result<()> {
        self.select_pane(p)
    }
    fn zoom_pane(&self, p: &str) -> Result<()> {
        let target = self.pane(p)?;
        let before = self.client.snapshot()?;
        let previous = before.panes.iter().find(|pane| {
            pane.focused
                && Some(&pane.tab_id) == before.focused_tab_id.as_ref()
                && pane.tab_id != target.tab_id
        });
        self.client
            .request("pane.zoom", json!({"pane_id":target.pane_id,"mode":"on"}))?;
        // Native zoom selects its tab. Background setup must leave the previous
        // tab visible while retaining the new tab's requested zoom state.
        if let Some(previous) = previous {
            self.select_pane(&self.key(&previous.terminal_id)?)?;
        }
        Ok(())
    }
    fn kill_pane(&self, p: &str) -> Result<()> {
        let pane = self.pane(p)?;
        self.verified_terminal(&pane)?;
        self.client
            .request("pane.close", json!({"pane_id":pane.pane_id}))?;
        Ok(())
    }
    fn respawn_pane(&self, key: &str, cwd: &Path, command: Option<&str>) -> Result<String> {
        ensure!(
            self.fresh.lock().unwrap().contains(key),
            "Herdr can only replace a fresh workmux launch pane"
        );
        self.move_launch(key, cwd, command, "right", 0.5, true)
    }
    fn split_pane(
        &self,
        target: &str,
        direction: &SplitDirection,
        cwd: &Path,
        size: Option<u16>,
        percentage: Option<u8>,
        command: Option<&str>,
    ) -> Result<String> {
        let dir = match direction {
            SplitDirection::Horizontal => "right",
            SplitDirection::Vertical => "down",
            SplitDirection::Stacked => {
                bail!("split: stacked is only supported by the Zellij backend")
            }
        };
        let ratio = if let Some(size) = size {
            let dimensions = self.pane_dimensions(target)?;
            let total = u64::from(if dir == "right" {
                dimensions.width
            } else {
                dimensions.height
            });
            ensure!(
                u64::from(size) < total,
                "Requested pane size exceeds available space"
            );
            (total - u64::from(size)) as f64 / total as f64
        } else {
            let percentage = percentage.unwrap_or(50);
            ensure!(percentage <= 100, "Invalid Herdr split percentage");
            f64::from(100 - percentage) / 100.0
        };
        // Protocol 22 accepts other values but clamps them in TileLayout. Do not
        // allocate a pane and silently give it a different size than requested.
        ensure!(
            (0.1..=0.9).contains(&ratio),
            "Herdr 0.9.0 limits each split to 10–90% of its target; the requested pane size cannot be preserved"
        );
        self.move_launch(target, cwd, command, dir, ratio, false)
    }
    fn create_handshake(&self) -> Result<Box<dyn PaneHandshake>> {
        let launch = self.launch(&self.get_default_shell()?)?;
        *self.pending_launch.lock().unwrap() = Some(launch.clone());
        Ok(Box::new(pane_launch::Handshake::new(launch)))
    }

    fn clear_pane(&self, key: &str) -> Result<()> {
        self.pane(key)?;
        self.launches
            .lock()
            .unwrap()
            .get(key)
            .context("Herdr can only clear a controlled launch, not a running program")?
            .clear()
    }
    fn set_pane_name(&self, p: &str, name: &str) -> Result<()> {
        self.client.request(
            "pane.rename",
            json!({"pane_id":self.pane(p)?.pane_id,"label":name}),
        )?;
        Ok(())
    }
    fn capture_pane(&self, p: &str, lines: u16) -> Option<String> {
        let pane = self.pane(p).ok()?;
        if lines == 0 {
            return Some(String::new());
        }
        let rows = self.pane_dimensions(p).ok()?.height;
        let text = if u32::from(lines) + u32::from(rows) <= 1000 {
            // Native recent output counts the empty cursor row. Request enough
            // additional rows to retain the requested number of nontrailing rows.
            self.client.request("pane.read", json!({
                "pane_id":pane.pane_id,"source":"recent", "lines":u32::from(lines) + u32::from(rows)
            })).ok()?["read"]["text"].as_str()?.to_string()
        } else {
            self.capture_history_rows(&pane, lines).ok()?
        };
        Some(util::tail_lines(&text, lines))
    }
    fn send_text_fragment(&self, p: &str, text: &str) -> Result<()> {
        self.client.request(
            "pane.send_text",
            json!({"pane_id":self.pane(p)?.pane_id,"text":text}),
        )?;
        Ok(())
    }
    fn send_enter(&self, p: &str) -> Result<()> {
        self.send_key(p, "enter")
    }
    fn send_key(&self, p: &str, key: &str) -> Result<()> {
        let key = match key {
            " " => "space",
            "BSpace" => "backspace",
            key => key,
        };
        let key = key
            .strip_prefix("C-")
            .filter(|key| key.len() == 1)
            .map(|key| format!("ctrl+{key}"))
            .unwrap_or_else(|| key.to_string());
        self.client.request(
            "pane.send_keys",
            json!({"pane_id":self.pane(p)?.pane_id,"keys":[key]}),
        )?;
        Ok(())
    }
    fn paste_text(&self, p: &str, text: &str) -> Result<()> {
        self.client.request(
            "pane.send_input",
            json!({"pane_id":self.pane(p)?.pane_id,"text":text}),
        )?;
        Ok(())
    }
    fn paste_and_submit(&self, p: &str, text: &str) -> Result<()> {
        self.client.request(
            "pane.send_input",
            json!({"pane_id":self.pane(p)?.pane_id,"text":text,"keys":["enter"]}),
        )?;
        Ok(())
    }
    fn send_keys(&self, p: &str, text: &str) -> Result<()> {
        let launch = self.launches.lock().unwrap().get(p).cloned();
        if let Some(launch) = launch {
            launch.deliver_to_pane(text, &self.instance_id(), p)?;
            self.launches.lock().unwrap().remove(p);
            return Ok(());
        }
        self.paste_and_submit(p, text)
    }

    fn get_live_pane_info(&self, key: &str) -> Result<Option<LivePaneInfo>> {
        let terminal = self.raw(key)?;
        let s = self.client.snapshot()?;
        s.panes
            .iter()
            .find(|p| p.terminal_id == terminal)
            .map(|p| self.info(p, &s))
            .transpose()
    }
    fn get_all_live_pane_info(&self) -> Result<HashMap<String, LivePaneInfo>> {
        let s = self.client.snapshot()?;
        s.panes
            .iter()
            .map(|p| Ok((self.key(&p.terminal_id)?, self.info(p, &s)?)))
            .collect()
    }
    fn set_status(&self, pane_id: &str, icon: &str, auto_clear: bool) -> Result<()> {
        let config = Config::load(None)?;
        let status = if auto_clear && icon == config.status_icons.done() {
            AgentStatus::Done
        } else if auto_clear {
            AgentStatus::Waiting
        } else {
            AgentStatus::Working
        };
        self.set_status_state(pane_id, status, icon, auto_clear)
    }

    fn clear_status(&self, pane_id: &str) -> Result<()> {
        let pane = self.pane(pane_id)?;
        self.client.request(
            "pane.clear_agent_authority",
            json!({"pane_id":pane.pane_id,"source":"workmux"}),
        )?;
        Ok(())
    }
    fn ensure_status_format(&self, pane_id: &str) -> Result<()> {
        // Herdr renders agent reports itself. Validate the target so callers
        // receive an error for stale identities instead of a false success.
        let _ = self.pane(pane_id)?;
        Ok(())
    }
    fn schedule_window_close(&self, name: &str, delay: Duration) -> Result<()> {
        self.schedule_window_target_close(&WindowTarget::new(name.into(), None), delay)
    }
    fn schedule_window_target_close(&self, t: &WindowTarget, delay: Duration) -> Result<()> {
        let tab = self.tab(t)?;
        let target = self.key(&tab.tab_id)?;
        let command = self.deferred_command(deferred::Action::CloseTab, &target)?;
        self.run_deferred_script(&format!("sleep {}; {command}", delay.as_secs_f64()))
    }
    fn schedule_session_close(&self, name: &str, delay: Duration) -> Result<()> {
        let workspace = self.session(name)?;
        let target = self.key(&workspace.workspace_id)?;
        let command = self.deferred_command(deferred::Action::CloseWorkspace, &target)?;
        self.run_deferred_script(&format!("sleep {}; {command}", delay.as_secs_f64()))
    }
    fn session_close_handles_navigation(&self) -> bool {
        true
    }
    fn wait_until_session_closed(&self, name: &str) -> Result<()> {
        while self.session_exists(name)? {
            thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }
    fn run_deferred_script(&self, script: &str) -> Result<()> {
        util::run_detached_sh_c(script)
    }
    fn shell_close_window_by_id_guard_cmd(&self, id: &str) -> Result<String> {
        let tab_id = self.raw(id)?;
        ensure!(
            self.client
                .snapshot()?
                .tabs
                .iter()
                .any(|tab| tab.tab_id == tab_id),
            "Herdr tab no longer exists"
        );
        self.deferred_command(deferred::Action::CloseTab, id)
    }
    fn shell_close_session_by_id_guard_cmd(&self, id: &str, _pane: Option<&str>) -> Result<String> {
        let workspace_id = self.raw(id)?;
        ensure!(
            self.client
                .snapshot()?
                .workspaces
                .iter()
                .any(|workspace| workspace.workspace_id == workspace_id),
            "Herdr workspace no longer exists"
        );
        self.deferred_command(deferred::Action::CloseWorkspace, id)
    }
    fn shell_select_window_cmd(&self, full_name: &str) -> Result<String> {
        let tab = self.tab(&WindowTarget::new(full_name.into(), None))?;
        self.deferred_command(deferred::Action::FocusTab, &self.key(&tab.tab_id)?)
    }
    fn shell_kill_window_cmd(&self, full_name: &str) -> Result<String> {
        let tab = self.tab(&WindowTarget::new(full_name.into(), None))?;
        self.deferred_command(deferred::Action::CloseTab, &self.key(&tab.tab_id)?)
    }
    fn shell_kill_window_target_cmd(&self, target: &WindowTarget) -> Result<String> {
        let tab = self.tab(target)?;
        self.deferred_command(deferred::Action::CloseTab, &self.key(&tab.tab_id)?)
    }
    fn shell_switch_session_cmd(&self, full_name: &str) -> Result<String> {
        let workspace = self.session(full_name)?;
        self.deferred_command(
            deferred::Action::FocusWorkspace,
            &self.key(&workspace.workspace_id)?,
        )
    }
    fn shell_kill_session_cmd(&self, full_name: &str) -> Result<String> {
        let workspace = self.session(full_name)?;
        self.deferred_command(
            deferred::Action::CloseWorkspace,
            &self.key(&workspace.workspace_id)?,
        )
    }
}

fn ancestor_pids() -> Result<Vec<u32>> {
    // ps is read-only and portable across the supported Unix systems. No shell
    // interpretation or current public pane address participates in resolution.
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid="])
        .output()?;
    ensure!(
        output.status.success(),
        "Cannot resolve Herdr caller process ancestry"
    );
    let text = String::from_utf8(output.stdout)?;
    let parents: HashMap<u32, u32> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect();
    let mut result = Vec::new();
    let mut pid = std::process::id();
    while pid > 1 && !result.contains(&pid) {
        result.push(pid);
        let Some(parent) = parents.get(&pid) else {
            break;
        };
        pid = *parent;
    }
    Ok(result)
}
