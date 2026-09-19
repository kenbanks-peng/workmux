//! File-based delivery to an initialized shell. No launch command is typed into
//! a terminal. A temporary POSIX guard cancels its own terminal job before delivery.
use super::super::{PaneHandshake, agent::shell_quote};
use anyhow::{Result, bail, ensure};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(8);

pub struct Launch {
    directory: tempfile::TempDir,
    shell: String,
    owner_directory: std::path::PathBuf,
    clear: Mutex<bool>,
}
impl Launch {
    pub fn new(shell: &str, owner_directory: &Path) -> Result<Arc<Self>> {
        ensure!(
            super::util::is_posix_shell(shell)
                || matches!(
                    Path::new(shell).file_name().and_then(|s| s.to_str()),
                    Some("fish" | "nu")
                ),
            "Unsupported Herdr launch shell: {shell}"
        );
        Ok(Arc::new(Self {
            directory: tempfile::Builder::new()
                .prefix("workmux-herdr-launch-")
                .tempdir_in(owner_directory)?,
            shell: shell.into(),
            owner_directory: owner_directory.into(),
            clear: Mutex::new(false),
        }))
    }
    fn path(&self, name: &str) -> String {
        shell_quote(&self.directory.path().join(name).to_string_lossy())
    }
    fn nu_path(&self, name: &str) -> String {
        serde_json::to_string(&self.directory.path().join(name).to_string_lossy()).unwrap()
    }
    fn is_nu(&self) -> bool {
        Path::new(&self.shell)
            .file_name()
            .is_some_and(|name| name == "nu")
    }
    pub fn clear(&self) -> Result<()> {
        ensure!(
            !self.directory.path().join("command").exists(),
            "Herdr launch was already delivered"
        );
        *self.clear.lock().unwrap() = true;
        Ok(())
    }
    pub fn cancel(&self) {
        let _ = std::fs::write(self.directory.path().join("cancel"), "");
    }
    /// Set identity in the controlled shell, not by prefixing `env` to a
    /// command: configured commands can contain shell builtins or compound lists.
    pub fn deliver_to_pane(&self, command: &str, instance: &str, pane: &str) -> Result<()> {
        self.deliver(&self.command_with_identity(command, instance, pane)?)
    }
    fn command_with_identity(&self, command: &str, instance: &str, pane: &str) -> Result<String> {
        use super::super::{
            STATUS_TARGET_BACKEND_ENV, STATUS_TARGET_INSTANCE_ENV, STATUS_TARGET_PANE_ENV,
        };
        let mut script = String::new();
        for (name, value) in [
            (STATUS_TARGET_BACKEND_ENV, "herdr"),
            (STATUS_TARGET_INSTANCE_ENV, instance),
            (STATUS_TARGET_PANE_ENV, pane),
        ] {
            if self.is_nu() {
                script.push_str(&format!(
                    "$env.{name} = {}\n",
                    serde_json::to_string(value)?
                ));
            } else if Path::new(&self.shell)
                .file_name()
                .is_some_and(|s| s == "fish")
            {
                script.push_str(&format!("set -gx {name} {}\n", shell_quote(value)));
            } else {
                script.push_str(&format!("export {name}={}\n", shell_quote(value)));
            }
        }
        script.push_str(command);
        Ok(script)
    }
    pub fn deliver(&self, command: &str) -> Result<()> {
        ensure!(
            !self.directory.path().join("cancel").exists(),
            "Herdr launch was cancelled"
        );
        // Source opens/parses this file before acknowledging it. Once acknowledged,
        // removing the directory cannot truncate the command or its arguments.
        let mut script = if self.is_nu() {
            format!("^/usr/bin/touch {}\n", self.nu_path("received"))
        } else {
            format!("/usr/bin/touch {}\n", self.path("received"))
        };
        // Do not let a fast command exit (and close its PTY) before the guard
        // has stopped. The final acknowledgement also prevents directory removal
        // from racing this source file's wait for the guard.
        let release = format!(
            "while [ ! -f {} ]; do sleep 0.01; done; /usr/bin/touch {}",
            self.path("guard-done"),
            self.path("released")
        );
        if self.is_nu() {
            script.push_str(&format!(
                "^/bin/sh -c {}\n",
                serde_json::to_string(&release)?
            ));
        } else {
            script.push_str(&format!("/bin/sh -c {}\n", shell_quote(&release)));
        }
        if *self.clear.lock().unwrap() {
            if self.is_nu() {
                script.push_str("^/usr/bin/printf '\\033[2J\\033[3J\\033[H'\n");
            } else {
                script.push_str("/usr/bin/printf '\\033[2J\\033[3J\\033[H'\n");
            }
        }
        script.push_str(command);
        script.push('\n');
        let temporary = self.directory.path().join("command.tmp");
        std::fs::write(&temporary, script)?;
        std::fs::rename(temporary, self.directory.path().join("command"))?;
        let result = self.wait_file("released");
        if result.is_err() {
            self.cancel();
        }
        result
    }
    fn wait_command(&self) -> String {
        format!(
            "while [ ! -f {} ]; do [ -d {} ] && [ ! -f {} ] || exit 1; sleep 0.02; done",
            self.path("command"),
            self.path(""),
            self.path("cancel")
        )
    }
    /// The guard runs only until delivery. PPID confirms the launcher is still
    /// its parent. Signal group 0, which contains the guard itself, rather than
    /// a numeric PID that could be reused during cleanup.
    fn guarded(&self, body: &str) -> String {
        let guard = format!(
            "i=0; while [ ! -f {received} ]; do \
             parent=$( /bin/ps -o ppid= -p $$ | /usr/bin/tr -d ' ' ); \
             if ! kill -0 {owner} 2>/dev/null; then /bin/rm -rf {owner_directory}; [ \"$parent\" != \"$1\" ] || kill -KILL 0; exit; fi; \
             [ \"$parent\" = \"$1\" ] || {{ /bin/rm -rf {directory}; exit; }}; \
             if [ ! -d {directory} ] || [ -f {cancel} ] || {{ [ ! -f {ready} ] && [ $i -ge 160 ]; }}; then \
             /bin/rm -rf {directory}; kill -KILL 0; exit; fi; i=$((i+1)); sleep 0.05; done; /usr/bin/touch {guard_done}",
            owner_directory = shell_quote(&self.owner_directory.to_string_lossy()),
            received = self.path("received"),
            guard_done = self.path("guard-done"),
            directory = self.path(""),
            cancel = self.path("cancel"),
            owner = std::process::id(),
            ready = self.path("ready")
        );
        format!(
            "/bin/sh -c {} sh \"$$\" >/dev/null 2>&1 & {body}",
            shell_quote(&guard)
        )
    }
    pub fn script(&self) -> String {
        let wait = shell_quote(&self.wait_command());
        let shell = shell_quote(&self.shell);
        let body = if self.is_nu() {
            // Nu source paths are parsed before execution. After the first login
            // initialization signals readiness, start Nu with the now-present
            // source file. --execute keeps that initialized shell usable afterward.
            let source = format!("source {}", self.nu_path("command"));
            let body = format!(
                "^/usr/bin/touch {}; ^/bin/sh -c {}; if $env.LAST_EXIT_CODE != 0 {{ exit 1 }}; exec {} --login --interactive --execute {}",
                self.nu_path("ready"),
                serde_json::to_string(&self.wait_command()).unwrap(),
                serde_json::to_string(&self.shell).unwrap(),
                serde_json::to_string(&source).unwrap()
            );
            format!(
                "exec {shell} --login --interactive --commands {}",
                shell_quote(&body)
            )
        } else if Path::new(&self.shell)
            .file_name()
            .is_some_and(|name| name == "fish")
        {
            let body = format!(
                "/usr/bin/touch {}; /bin/sh -c {wait}; or exit 1; source {}; exec {shell} -il",
                self.path("ready"),
                self.path("command")
            );
            format!("exec {shell} -ilc {}", shell_quote(&body))
        } else {
            let body = format!(
                "/usr/bin/touch {}; /bin/sh -c {wait} || exit 1; . {}; exec {shell} -il",
                self.path("ready"),
                self.path("command")
            );
            format!("exec {shell} -ilc {}", shell_quote(&body))
        };
        self.guarded(&body)
    }
    pub fn initial_shell(&self) -> String {
        self.guarded(&format!(
            "/usr/bin/touch {}; exec {} -il",
            self.path("ready"),
            shell_quote(&self.shell)
        ))
    }
    pub fn release(&self) -> Result<()> {
        if self.directory.path().exists() {
            std::fs::write(self.directory.path().join("received"), "")?;
            self.wait_file("guard-done")?;
        }
        Ok(())
    }
    /// A short-lived gate prevents commands such as `workmux run true` from
    /// exiting before their terminal can be moved into the destination tab.
    pub fn move_gate(&self) -> String {
        self.guarded(&format!(
            "{}; . {}",
            self.wait_command(),
            self.path("command")
        ))
    }
    fn wait_file(&self, name: &str) -> Result<()> {
        let start = Instant::now();
        while !self.directory.path().join(name).is_file() {
            ensure!(
                self.directory.path().is_dir() && !self.directory.path().join("cancel").exists(),
                "Herdr controlled shell launch was cancelled"
            );
            if start.elapsed() >= TIMEOUT {
                bail!("Herdr controlled shell launch timed out after 8s");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
}

pub struct Handshake {
    pub launch: Arc<Launch>,
    ready: bool,
}
impl Handshake {
    pub fn new(launch: Arc<Launch>) -> Self {
        Self {
            launch,
            ready: false,
        }
    }
}
impl PaneHandshake for Handshake {
    fn wrapper_command(&self, _shell: &str) -> String {
        self.launch.script()
    }
    fn script_content(&self, _shell: &str) -> String {
        self.launch.script()
    }
    fn wait(mut self: Box<Self>) -> Result<()> {
        self.launch.wait_file("ready")?;
        self.ready = true;
        Ok(())
    }
}
impl Drop for Handshake {
    fn drop(&mut self) {
        if !self.ready {
            self.launch.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_identity_preserves_shell_commands_and_quotes() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let instance = "/tmp/socket 'with spaces' $HOME;雪";
        let pane = "boot~terminal's-id";
        for shell in ["/bin/sh", "/bin/bash", "/bin/zsh", "fish", "nu"] {
            if std::process::Command::new(shell)
                .arg("--version")
                .output()
                .is_err()
            {
                continue;
            }
            let launch = Launch::new(shell, directory.path())?;
            let command = if shell == "nu" {
                "cd /; print $env.WORKMUX_STATUS_BACKEND; print $env.WORKMUX_STATUS_INSTANCE; print $env.WORKMUX_STATUS_PANE_ID; pwd"
            } else {
                "cd /; printf '%s\\n' \"$WORKMUX_STATUS_BACKEND\" \"$WORKMUX_STATUS_INSTANCE\" \"$WORKMUX_STATUS_PANE_ID\"; pwd"
            };
            let script = launch.command_with_identity(command, instance, pane)?;
            let output = std::process::Command::new(shell)
                .args(["-c", &script])
                .output()?;
            assert!(
                output.status.success(),
                "{shell}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8(output.stdout)?,
                format!("herdr\n{instance}\n{pane}\n/\n"),
                "{shell}"
            );
        }
        Ok(())
    }
}
