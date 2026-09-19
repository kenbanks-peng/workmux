//! Pinned JSON API transport. Every connection is checked before mutation.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
pub struct Snapshot {
    pub focused_tab_id: Option<String>,
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    pub label: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    pub label: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pane {
    pub pane_id: String,
    pub terminal_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub focused: bool,
    pub cwd: String,
    pub foreground_cwd: Option<String>,
    pub title: Option<String>,
    pub label: Option<String>,
}
const TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE: usize = 8 * 1024 * 1024;

pub struct Client {
    pub endpoint: String,
    // Herdr serves one request per connection. Cache only a protocol-verified
    // peer lifetime; check every new connection and never retry mutations.
    boot: Mutex<Option<String>>,
}
impl Client {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            boot: Mutex::new(None),
        }
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value> {
        let mut boot = self
            .boot
            .lock()
            .map_err(|_| anyhow::anyhow!("Herdr identity lock poisoned"))?;
        let deadline = Instant::now() + TIMEOUT;
        if boot.is_none() {
            let mut stream = connect(&self.endpoint, deadline)?;
            let identity = self.connection_identity(&stream)?;
            let result = exchange(&mut stream, "session.snapshot", json!({}), deadline)?;
            validate_snapshot(&result)?;
            *boot = Some(identity);
            if method == "session.snapshot" {
                return Ok(result);
            }
        }
        let mut stream = connect(&self.endpoint, deadline)?;
        ensure!(
            boot.as_deref() == Some(self.connection_identity(&stream)?.as_str()),
            "Herdr server lifetime changed; refusing to reuse live targets"
        );
        let result = exchange(&mut stream, method, params, deadline)?;
        if method == "session.snapshot" {
            validate_snapshot(&result)?;
        }
        Ok(result)
    }

    fn connection_identity(&self, stream: &UnixStream) -> Result<String> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(&self.endpoint)?;
        Ok(format!(
            "{}-{}-{}",
            metadata.dev(),
            metadata.ino(),
            peer_lifetime(stream)?
        ))
    }

    pub fn server_pid(&self) -> Result<u32> {
        // connection_identity is dev-inode-pid followed by the peer start time.
        self.boot()?
            .split('-')
            .nth(2)
            .context("Missing Herdr server PID")?
            .parse()
            .context("Invalid Herdr server PID")
    }

    pub fn boot(&self) -> Result<String> {
        if let Some(boot) = self
            .boot
            .lock()
            .map_err(|_| anyhow::anyhow!("Herdr identity lock poisoned"))?
            .clone()
        {
            return Ok(boot);
        }
        self.snapshot()?;
        self.boot
            .lock()
            .map_err(|_| anyhow::anyhow!("Herdr identity lock poisoned"))?
            .clone()
            .context("Herdr peer identity was not recorded")
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        validate_snapshot(&self.request("session.snapshot", json!({}))?)
    }
}

fn validate_snapshot(result: &Value) -> Result<Snapshot> {
    let snapshot = &result["snapshot"];
    let version = snapshot["version"]
        .as_str()
        .context("Missing Herdr server version")?;
    let protocol = snapshot["protocol"]
        .as_u64()
        .context("Missing Herdr server protocol")?;
    let version_parts = version
        .split('.')
        .map(|part| {
            part.bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| part.parse::<u64>().ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>();
    ensure!(
        version_parts.is_some_and(|parts| parts.len() == 3 && parts.as_slice() >= &[0, 9, 0])
            && protocol == 22,
        "Unsupported Herdr server: version {version}, protocol {protocol}; expected >= 0.9.0 protocol 22"
    );
    serde_json::from_value(snapshot.clone()).context("Malformed Herdr snapshot")
}

fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .context("Herdr request timed out")
}

fn connect(endpoint: &str, deadline: Instant) -> Result<UnixStream> {
    let path = Path::new(endpoint);
    ensure!(
        path.is_absolute(),
        "Set HERDR_SOCKET_PATH to an absolute Herdr API socket path"
    );
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: zero is a valid initial sockaddr_un; all copied bytes are bounded.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    ensure!(
        bytes.len() < address.sun_path.len() && !bytes.contains(&0),
        "Invalid Herdr socket path"
    );
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (dest, src) in address.sun_path.iter_mut().zip(bytes) {
        *dest = *src as libc::c_char;
    }
    // SAFETY: socket has no pointer arguments; ownership passes immediately to UnixStream.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    ensure!(
        fd >= 0,
        "Create Herdr socket: {}",
        std::io::Error::last_os_error()
    );
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // Do not leak the transport into child processes.
    ensure!(
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
        "Set Herdr socket close-on-exec: {}",
        std::io::Error::last_os_error()
    );
    stream.set_nonblocking(true)?;
    // SAFETY: address is initialized and the supplied size matches its allocation.
    let result = unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        ensure!(
            error.raw_os_error() == Some(libc::EINPROGRESS),
            "Connect to Herdr at {endpoint}: {error}"
        );
        wait_ready(fd, libc::POLLOUT, deadline)?;
        if let Some(error) = stream.take_error()? {
            return Err(error).context("Connect to Herdr");
        }
    }
    Ok(stream)
}

fn wait_ready(fd: libc::c_int, events: libc::c_short, deadline: Instant) -> Result<()> {
    loop {
        let mut pollfd = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let timeout = remaining(deadline)?.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: pollfd is live for this call. HUP is handled by the following I/O.
        let ready = unsafe { libc::poll(&mut pollfd, 1, timeout.max(1)) };
        if ready < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        ensure!(ready > 0, "Herdr request timed out or socket poll failed");
        return Ok(());
    }
}

fn bounded_io(
    stream: &mut UnixStream,
    events: libc::c_short,
    deadline: Instant,
    mut operation: impl FnMut(&mut UnixStream) -> std::io::Result<usize>,
) -> Result<usize> {
    loop {
        remaining(deadline)?;
        match operation(stream) {
            Ok(n) => return Ok(n),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_ready(stream.as_raw_fd(), events, deadline)?
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn exchange(
    stream: &mut UnixStream,
    method: &str,
    params: Value,
    deadline: Instant,
) -> Result<Value> {
    let mut request =
        serde_json::to_vec(&json!({"id":"workmux", "method":method, "params":params}))?;
    request.push(b'\n');
    let mut rest = request.as_slice();
    while !rest.is_empty() {
        let n = bounded_io(stream, libc::POLLOUT, deadline, |stream| stream.write(rest))
            .with_context(|| format!("Write Herdr {method}"))?;
        ensure!(n > 0, "Herdr socket closed during request");
        rest = &rest[n..];
    }
    let mut response = Vec::new();
    loop {
        let mut chunk = [0; 8192];
        let n = bounded_io(stream, libc::POLLIN, deadline, |stream| {
            stream.read(&mut chunk)
        })
        .with_context(|| format!("Read Herdr {method}"))?;
        ensure!(n > 0, "Incomplete Herdr response");
        response.extend_from_slice(&chunk[..n]);
        ensure!(response.len() <= MAX_RESPONSE, "Oversized Herdr response");
        if chunk[..n].contains(&b'\n') {
            break;
        }
    }
    let response: Value = serde_json::from_slice(&response).context("Malformed Herdr response")?;
    ensure!(response["id"] == "workmux", "Mismatched Herdr response ID");
    if let Some(error) = response.get("error") {
        bail!("Herdr {method}: {error}");
    }
    response
        .get("result")
        .filter(|v| v.is_object())
        .cloned()
        .context("Herdr response has no object result")
}

/// Kernel peer credentials and process start time, not a user label or PID alone.
/// Each new request checks this identity before it sends any bytes.
#[cfg(target_os = "macos")]
fn peer_lifetime(stream: &UnixStream) -> Result<String> {
    let mut pid: libc::pid_t = 0;
    let mut size = std::mem::size_of_val(&pid) as libc::socklen_t;
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let info_size = std::mem::size_of_val(&info) as i32;
    // SAFETY: all pointers refer to initialized, correctly sized local objects.
    unsafe {
        ensure!(
            libc::getsockopt(
                stream.as_raw_fd(),
                0,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut size
            ) == 0,
            "Cannot verify Herdr peer PID: {}",
            std::io::Error::last_os_error()
        );
        ensure!(
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                info_size
            ) == info_size,
            "Cannot verify Herdr peer lifetime"
        );
        ensure!(
            info.pbi_uid == libc::geteuid(),
            "Herdr server belongs to another user"
        );
    }
    Ok(format!(
        "{pid}-{}-{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    ))
}
#[cfg(target_os = "linux")]
fn peer_lifetime(stream: &UnixStream) -> Result<String> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&cred) as libc::socklen_t;
    unsafe {
        ensure!(
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut size
            ) == 0,
            "Cannot verify Herdr peer credentials"
        );
        ensure!(
            cred.uid == libc::geteuid(),
            "Herdr server belongs to another user"
        );
    }
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", cred.pid))?;
    let start = stat
        .rsplit_once(')')
        .context("Invalid process stat")?
        .1
        .split_whitespace()
        .nth(19)
        .context("Missing process start time")?;
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    Ok(format!("{}-{start}-{}", cred.pid, boot.trim()))
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn peer_lifetime(_: &UnixStream) -> Result<String> {
    bail!("Herdr peer lifetime verification requires macOS or Linux")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;
    use std::thread;

    const SNAPSHOT: &str = "{\"id\":\"workmux\",\"result\":{\"snapshot\":{\"version\":\"0.9.0\",\"protocol\":22,\"workspaces\":[],\"tabs\":[],\"panes\":[]}}}\n";

    #[test]
    fn accepts_minimum_and_newer_versions_with_supported_protocol() {
        for (version, accepted) in [
            ("0.8.99", false),
            ("0.9.0", true),
            ("0.9.1", true),
            ("0.10.0", true),
            ("1.0.0", true),
            ("invalid", false),
            ("0.9", false),
            ("0.9.0-rc.1", false),
        ] {
            for protocol in [21, 22, 23] {
                let result = json!({"snapshot": {
                    "version":version, "protocol":protocol,
                    "workspaces":[], "tabs":[], "panes":[]
                }});
                assert_eq!(
                    validate_snapshot(&result).is_ok(),
                    accepted && protocol == 22,
                    "version {version}, protocol {protocol}"
                );
            }
        }
    }

    fn serve(path: &Path, responses: Vec<String>) -> thread::JoinHandle<Vec<String>> {
        let listener = UnixListener::bind(path).unwrap();
        thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| {
                    let (mut stream, _) = listener.accept().unwrap();
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut request = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut request)
                        .unwrap();
                    let _ = stream.write_all(response.as_bytes());
                    request
                })
                .collect()
        })
    }

    #[test]
    fn two_endpoints_in_the_same_peer_process_have_distinct_identity() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.sock");
        let second = directory.path().join("second.sock");
        let one = serve(&first, vec![SNAPSHOT.into()]);
        let two = serve(&second, vec![SNAPSHOT.into()]);
        let first = Client::new(first.to_string_lossy().into_owned());
        let second = Client::new(second.to_string_lossy().into_owned());
        assert_ne!(first.boot().unwrap(), second.boot().unwrap());
        assert_eq!(first.server_pid().unwrap(), second.server_pid().unwrap());
        one.join().unwrap();
        two.join().unwrap();
    }

    #[test]
    fn arbitrary_request_checks_protocol_before_mutation_and_caches_only_success() {
        for rejected in [
            SNAPSHOT.replace("0.9.0", "0.8.0"),
            SNAPSHOT.replace(":22", ":21"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("api.sock");
            let server = serve(&path, vec![rejected.clone(), rejected]);
            let client = Client::new(path.to_str().unwrap().into());
            for _ in 0..2 {
                assert!(
                    client
                        .request("workspace.create", json!({"label":"must-not-exist"}))
                        .unwrap_err()
                        .to_string()
                        .contains("Unsupported Herdr server")
                );
                assert!(client.boot.lock().unwrap().is_none());
            }
            for request in server.join().unwrap() {
                assert_eq!(
                    serde_json::from_str::<Value>(&request).unwrap()["method"],
                    "session.snapshot"
                );
            }
        }
    }

    #[test]
    fn each_request_uses_a_verified_new_connection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api.sock");
        let server = serve(
            &path,
            vec![
                SNAPSHOT.into(),
                "{\"id\":\"workmux\",\"result\":{}}\n".into(),
                "{\"id\":\"workmux\",\"result\":{}}\n".into(),
            ],
        );
        let client = Client::new(path.to_str().unwrap().into());
        client.request("one", json!({})).unwrap();
        client.request("two", json!({})).unwrap();
        let methods: Vec<_> = server
            .join()
            .unwrap()
            .iter()
            .map(|r| {
                serde_json::from_str::<Value>(r).unwrap()["method"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(methods, ["session.snapshot", "one", "two"]);
    }

    #[test]
    fn lifetime_mismatch_sends_no_request() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api.sock");
        let server = serve(&path, vec![SNAPSHOT.into(), String::new()]);
        let client = Client::new(path.to_str().unwrap().into());
        client.snapshot().unwrap();
        *client.boot.lock().unwrap() = Some("replacement-server".into());
        assert!(
            client
                .request("workspace.close", json!({}))
                .unwrap_err()
                .to_string()
                .contains("server lifetime changed")
        );
        assert!(server.join().unwrap()[1].is_empty());
    }

    #[test]
    fn malformed_and_error_responses_are_rejected_without_retry() {
        for (response, expected) in [
            ("not json\n".into(), "Malformed Herdr response"),
            (
                "{\"id\":\"other\",\"result\":{}}\n".into(),
                "Mismatched Herdr response ID",
            ),
            ("{\"id\":\"workmux\"}\n".into(), "no object result"),
            (
                "{\"id\":\"workmux\",\"result\":null}\n".into(),
                "no object result",
            ),
            (
                "{\"id\":\"workmux\",\"error\":{\"code\":\"not_found\"}}\n".into(),
                "not_found",
            ),
            ("{}".into(), "Incomplete Herdr response"),
            ("x".repeat(MAX_RESPONSE + 1), "Oversized Herdr response"),
            (
                "{\"id\":\"workmux\",\"result\":{\"snapshot\":{}}}\n".into(),
                "Missing Herdr server version",
            ),
            (
                SNAPSHOT.replace("\"panes\":[]", "\"panes\":null"),
                "Malformed Herdr snapshot",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("api.sock");
            let server = serve(&path, vec![response]);
            let client = Client::new(path.to_str().unwrap().into());
            let error = client.request("workspace.create", json!({})).unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
            assert!(client.boot.lock().unwrap().is_none());
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }

    #[test]
    fn mutation_error_is_reported_without_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api.sock");
        let server = serve(
            &path,
            vec![
                SNAPSHOT.into(),
                "{\"id\":\"workmux\",\"error\":{\"code\":\"workspace_create_failed\"}}\n".into(),
            ],
        );
        let client = Client::new(path.to_str().unwrap().into());
        let error = client
            .request("workspace.create", json!({"label":"test"}))
            .unwrap_err();
        assert!(error.to_string().contains("workspace_create_failed"));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            serde_json::from_str::<Value>(&requests[1]).unwrap()["method"],
            "workspace.create"
        );
    }

    #[test]
    fn stalled_request_write_has_a_deadline() {
        let (mut client, _server) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        let start = Instant::now();
        let error = exchange(
            &mut client,
            "test",
            json!({"text":"x".repeat(MAX_RESPONSE)}),
            start + Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn response_deadline_is_total_not_per_chunk() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        let writer = thread::spawn(move || {
            let mut request = String::new();
            BufReader::new(server.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            for _ in 0..10 {
                thread::sleep(Duration::from_millis(20));
                if server.write_all(b" ").is_err() {
                    break;
                }
            }
        });
        let start = Instant::now();
        assert!(
            exchange(
                &mut client,
                "test",
                json!({}),
                start + Duration::from_millis(60)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_millis(180));
        drop(client);
        writer.join().unwrap();
    }

    #[test]
    fn endpoint_must_be_explicit_absolute_and_available() {
        for endpoint in ["", "relative.sock", "/does-not-exist/workmux-herdr.sock"] {
            assert!(Client::new(endpoint.into()).snapshot().is_err());
        }
    }
}
