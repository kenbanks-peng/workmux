//! Controlled process tests, not live Herdr terminal evidence.
//!
//! Each launcher has its own process group. No Herdr server is contacted.
//! A test-owned executable with the selected shell's name gates startup, then
//! runs the real shell without user configuration. This controls the startup
//! boundary without making assertions about private launch files.
use super::pane_launch::{Handshake, Launch};
use crate::multiplexer::{PaneHandshake, agent::shell_quote};
use anyhow::{Context, Result, bail, ensure};
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(12);

struct Process(Option<Child>);
impl Process {
    fn start(script: &str, home: &std::path::Path) -> Result<Self> {
        let output = std::fs::File::create(home.join("output"))?;
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", script])
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", home)
            .env("ZDOTDIR", home)
            .env("XDG_CONFIG_HOME", home)
            .env("TERM", "dumb")
            .current_dir(home)
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output);
        // Detach from the agent's controlling terminal as well as its group.
        // Only async-signal-safe work is permitted between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        Ok(Self(Some(command.spawn()?)))
    }

    fn wait(&mut self) -> Result<ExitStatus> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(status) = self.0.as_mut().unwrap().try_wait()? {
                self.0 = None;
                return Ok(status);
            }
            ensure!(
                Instant::now() < deadline,
                "test-owned launcher did not exit"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            // The unreaped child reserves this PID. Only its test-owned group
            // can be signalled. Never use an inherited terminal/process group.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
            let _ = child.wait();
        }
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    shell: PathBuf,
    entered: PathBuf,
    proceed: PathBuf,
    marker: PathBuf,
    nu: bool,
}
impl Fixture {
    fn new(name: &str, fail: bool) -> Result<Option<Self>> {
        let real = match which::which(name) {
            Ok(path) => path,
            Err(_) if !fail => {
                eprintln!("NOT RUN: {name} runtime is unavailable");
                return Ok(None);
            }
            Err(_) => PathBuf::from(name),
        };
        let directory = tempfile::Builder::new()
            .prefix("workmux shell launch 'quoted' ")
            .tempdir()?;
        let shell = directory.path().join(name);
        let entered = directory.path().join("entered");
        let proceed = directory.path().join("proceed");
        let marker = directory.path().join("command-ran");
        let flags = match name {
            "bash" => "--noprofile --norc",
            "zsh" => "-d -f",
            "fish" => "--no-config",
            "nu" => "--no-config-file",
            _ => "",
        };
        let startup = if fail {
            "exit 37".to_string()
        } else {
            format!(
                "while [ ! -f {} ]; do sleep 0.02; done\nexec {} {flags} \"$@\"",
                shell_quote(&proceed.to_string_lossy()),
                shell_quote(&real.to_string_lossy())
            )
        };
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\n/usr/bin/touch {}\n{startup}\n",
                shell_quote(&entered.to_string_lossy())
            ),
        )?;
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700))?;
        Ok(Some(Self {
            directory,
            shell,
            entered,
            proceed,
            marker,
            nu: name == "nu",
        }))
    }

    fn launch(&self) -> Result<(Arc<Launch>, Process)> {
        let launch = Launch::new(self.shell.to_str().unwrap(), self.directory.path())?;
        let process = Process::start(&launch.script(), self.directory.path())?;
        let deadline = Instant::now() + DEADLINE;
        while !self.entered.exists() {
            ensure!(Instant::now() < deadline, "startup fixture did not run");
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok((launch, process))
    }

    fn command(&self) -> String {
        if self.nu {
            format!(
                "^/usr/bin/touch {}; exit 0",
                serde_json::to_string(&self.marker.to_string_lossy()).unwrap()
            )
        } else {
            format!(
                "/usr/bin/touch {}; exit 0",
                shell_quote(&self.marker.to_string_lossy())
            )
        }
    }

    fn output(&self) -> String {
        std::fs::read_to_string(self.directory.path().join("output")).unwrap_or_default()
    }

    fn allow_startup(&self) -> Result<()> {
        std::fs::write(&self.proceed, "")?;
        Ok(())
    }
}

fn startup_failure(shell: &str) -> Result<()> {
    // All script branches can be tested even when the real runtime is absent.
    let fixture = Fixture::new(shell, true)?.unwrap();
    let (launch, mut process) = fixture.launch()?;
    let result = Box::new(Handshake::new(launch.clone())).wait();
    let error = result.expect_err("failed startup must not report readiness");
    ensure!(
        error.to_string().contains("cancelled") || error.to_string().contains("timed out"),
        "unexpected readiness error: {error}"
    );
    assert_eq!(process.wait()?.code(), Some(37));
    assert!(launch.deliver(&fixture.command()).is_err());
    assert!(!fixture.marker.exists());
    Ok(())
}

fn delayed_readiness(shell: &str) -> Result<()> {
    let Some(fixture) = Fixture::new(shell, false)? else {
        return Ok(());
    };
    let (launch, mut process) = fixture.launch()?;
    let (tx, rx) = mpsc::channel();
    let handshake = Handshake::new(launch.clone());
    let waiter = std::thread::spawn(move || tx.send(Box::new(handshake).wait()));
    assert!(
        matches!(
            rx.recv_timeout(Duration::from_millis(250)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "readiness was reported before startup was released"
    );
    assert!(!fixture.marker.exists());
    fixture.allow_startup()?;
    rx.recv_timeout(DEADLINE)
        .context("no readiness after startup release")?
        .with_context(|| fixture.output())?;
    waiter.join().unwrap()?;
    launch.deliver(&fixture.command())?;
    assert!(
        process.wait()?.success(),
        "{shell}: delivered command failed: {}",
        fixture.output()
    );
    assert!(fixture.marker.exists(), "{shell}: command was not executed");
    Ok(())
}

fn cancellation(shell: &str) -> Result<()> {
    let Some(fixture) = Fixture::new(shell, false)? else {
        return Ok(());
    };
    // Dropping a handshake during startup must stop the shell, not leave it
    // waiting for a command that can no longer be delivered.
    let (launch, mut process) = fixture.launch()?;
    drop(Handshake::new(launch.clone()));
    assert!(!process.wait()?.success());
    assert!(launch.deliver(&fixture.command()).is_err());
    assert!(!fixture.marker.exists());

    // Explicit cancellation must also unblock an active readiness wait.
    std::fs::remove_file(&fixture.entered)?;
    let (launch, mut process) = fixture.launch()?;
    let waiting = launch.clone();
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || tx.send(Box::new(Handshake::new(waiting)).wait()));
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(250)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    launch.cancel();
    let error = rx
        .recv_timeout(DEADLINE)?
        .expect_err("cancelled wait succeeded");
    waiter.join().unwrap()?;
    assert!(error.to_string().contains("cancelled"));
    assert!(!process.wait()?.success());
    assert!(launch.deliver(&fixture.command()).is_err());

    // Once initialized, cancellation must still prevent command execution.
    std::fs::remove_file(&fixture.entered)?;
    fixture.allow_startup()?;
    let (launch, mut process) = fixture.launch()?;
    Box::new(Handshake::new(launch.clone()))
        .wait()
        .with_context(|| fixture.output())?;
    launch.cancel();
    if launch.deliver(&fixture.command()).is_ok() {
        bail!("{shell}: cancelled launch accepted a command");
    }
    assert!(!process.wait()?.success());
    assert!(!fixture.marker.exists());
    Ok(())
}

macro_rules! shell_cases {
    ($($shell:ident),+ $(,)?) => {$(
        mod $shell {
            use super::*;
            #[test]
            fn startup_failure_rejects_readiness_and_delivery() -> Result<()> {
                startup_failure(stringify!($shell))
            }
            #[test]
            fn delayed_startup_waits_then_delivers() -> Result<()> {
                delayed_readiness(stringify!($shell))
            }
            #[test]
            fn cancellation_before_during_and_after_readiness() -> Result<()> {
                cancellation(stringify!($shell))
            }
        }
    )+};
}
shell_cases!(sh, bash, zsh, dash, ksh, fish, nu);

mod ash {
    use super::*;

    #[test]
    fn startup_failure_rejects_readiness_and_delivery() -> Result<()> {
        startup_failure("ash")
    }

    #[test]
    #[ignore = "requires an ash runtime; run explicitly on a host with ash"]
    fn delayed_startup_waits_then_delivers() -> Result<()> {
        which::which("ash")?;
        delayed_readiness("ash")
    }

    #[test]
    #[ignore = "requires an ash runtime; run explicitly on a host with ash"]
    fn cancellation_before_during_and_after_readiness() -> Result<()> {
        which::which("ash")?;
        cancellation("ash")
    }
}
