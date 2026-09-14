//! Kernel process lifetimes. Public addresses, labels and commands are not identity.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start: String,
}

impl ProcessIdentity {
    pub fn read(pid: u32) -> Result<Self> {
        ensure!(pid > 1, "Invalid Herdr process PID");
        Ok(Self {
            pid,
            start: process_start(pid)?,
        })
    }

    pub fn is_live(&self) -> bool {
        Self::read(self.pid).is_ok_and(|live| live == *self)
    }
}

#[cfg(target_os = "macos")]
fn process_start(pid: u32) -> Result<String> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    // SAFETY: the output pointer and length describe the initialized local object.
    ensure!(
        unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        } == size,
        "Cannot verify Herdr process lifetime"
    );
    ensure!(
        info.pbi_uid == unsafe { libc::geteuid() } && info.pbi_status != 5,
        "Herdr process is foreign or exited"
    );
    Ok(format!(
        "{}-{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    ))
}

#[cfg(target_os = "linux")]
fn process_start(pid: u32) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    let path = format!("/proc/{pid}");
    ensure!(
        std::fs::metadata(&path)?.uid() == unsafe { libc::geteuid() },
        "Herdr process belongs to another user"
    );
    let stat = std::fs::read_to_string(format!("{path}/stat"))?;
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .context("Invalid process stat")?
        .1
        .split_whitespace()
        .collect();
    ensure!(
        fields
            .first()
            .is_some_and(|state| !matches!(*state, "Z" | "X")),
        "Herdr process exited"
    );
    let start = fields.get(19).context("Missing process start time")?;
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    Ok(format!("{start}-{}", boot.trim()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_start(_: u32) -> Result<String> {
    anyhow::bail!("Herdr process verification requires macOS or Linux")
}

/// Read the native launcher environment from its live process, not from a
/// descendant that can carry stale/mixed caller variables.
pub fn launch_environment(
    process: &ProcessIdentity,
) -> Result<std::collections::HashMap<String, String>> {
    ensure!(process.is_live(), "Herdr launcher exited");
    let bytes = process_environment(process.pid)?;
    let environment = bytes
        .split(|b| *b == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (key, value) = entry.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect();
    ensure!(process.is_live(), "Herdr launcher lifetime changed");
    Ok(environment)
}

#[cfg(target_os = "macos")]
fn process_environment(pid: u32) -> Result<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut limit: libc::c_int = 0;
    let mut limit_size = std::mem::size_of_val(&limit);
    let mut limit_mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    ensure!(
        unsafe {
            libc::sysctl(
                limit_mib.as_mut_ptr(),
                2,
                (&mut limit as *mut libc::c_int).cast(),
                &mut limit_size,
                std::ptr::null_mut(),
                0,
            )
        } == 0
            && limit > 0
            && limit <= 2 * 1024 * 1024,
        "Invalid native argument buffer limit"
    );
    let mut bytes = vec![0u8; limit as usize];
    let mut length = bytes.len();
    // SAFETY: mib and the output buffer remain allocated for this bounded call.
    ensure!(
        unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                bytes.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        } == 0,
        "Cannot read native Herdr launcher environment: {}",
        std::io::Error::last_os_error()
    );
    bytes.truncate(length);
    ensure!(bytes.len() >= 4, "Invalid launcher process arguments");
    let argc = i32::from_ne_bytes(bytes[..4].try_into()?);
    ensure!(argc > 0, "Invalid launcher argument count");
    let mut offset = 4;
    offset += bytes[offset..]
        .iter()
        .position(|b| *b == 0)
        .context("Missing launcher executable")?;
    while bytes.get(offset) == Some(&0) {
        offset += 1;
    }
    for _ in 0..argc {
        offset += bytes
            .get(offset..)
            .context("Missing launcher argument")?
            .iter()
            .position(|b| *b == 0)
            .context("Unterminated launcher argument")?
            + 1;
    }
    Ok(bytes
        .get(offset..)
        .context("Missing launcher environment")?
        .to_vec())
}

#[cfg(target_os = "linux")]
fn process_environment(pid: u32) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(format!("/proc/{pid}/environ"))?
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_environment(_: u32) -> Result<Vec<u8>> {
    anyhow::bail!("Herdr launcher identity requires macOS or Linux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_identity_rejects_pid_reuse_and_exit() {
        let current = ProcessIdentity::read(std::process::id()).unwrap();
        assert!(current.is_live());
        let reused = ProcessIdentity {
            start: "another lifetime".into(),
            ..current.clone()
        };
        assert!(!reused.is_live());
        assert!(ProcessIdentity::read(0).is_err());
    }
}
