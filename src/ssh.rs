//! Reach a Rex server through `ssh`, the way the Rex CLI does: ask the machine where
//! its server listens, then forward that Unix socket to a local one. Only `sshd` and
//! `rex` are needed on the other side.
//!
//! Windows has no Unix sockets of its own, so there `ssh` runs in the WSL
//! distribution home is in, and forwards to a socket there, which is reached
//! through WSL as any other.
#![cfg_attr(not(unix), allow(dead_code))]
#[cfg(unix)]
use std::{
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::Instant,
};
use std::{process::Stdio, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
#[cfg(unix)]
use tokio::process::{Child, ChildStdin};
#[cfg(windows)]
use tokio::process::{Child, ChildStdin};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

use crate::wire;

/// How long the other machine may take to say where its server is, and again to
/// open the forward.
const PATIENCE: Duration = Duration::from_secs(20);
/// How much of what `ssh` says is kept: enough for its last line.
const KEPT: usize = 2048;

/// A running forward. Dropping it ends the forward.
#[cfg(unix)]
pub struct Tunnel {
    child: Child,
    /// The remote `cat` ends with its input, so the forward cannot outlive this process.
    _stdin: Option<ChildStdin>,
    local: PathBuf,
}

/// `ssh` as every run here uses it: never prompting, noticing a dead link, and clear
/// of a shared connection in the user's configuration, which would keep a forward
/// after its `ssh` has ended.
const OPTIONS: [&str; 6] = [
    "BatchMode=yes",
    "ConnectTimeout=10",
    "ServerAliveInterval=15",
    "ServerAliveCountMax=3",
    "ControlMaster=no",
    "ControlPath=none",
];

/// `ssh`, with `OPTIONS`.
fn ssh() -> Command {
    let mut command = Command::new("ssh");
    for option in OPTIONS {
        command.args(["-o", option]);
    }
    command.arg("-T").kill_on_drop(true);
    command
}

/// The last thing `ssh` said, which is its reason for giving up.
fn reason(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().rev().find(|line| !line.trim().is_empty());
    line.unwrap_or("ssh gave no reason").trim().to_owned()
}

/// Split `ssh://[user@]host[:port][/socket]` into what `ssh` takes as a destination
/// and the server's socket on that machine, if the endpoint names it.
fn parse(endpoint: &str) -> (&str, Option<&str>) {
    let scheme = "ssh://".len();
    match endpoint.get(scheme..).and_then(|host| host.find('/')) {
        Some(slash) => {
            let (destination, socket) = endpoint.split_at(scheme + slash);
            (destination, Some(socket))
        }
        None => (endpoint, None),
    }
}

/// Rex's installer puts a personal `rex` in `~/.local/bin`, which the shell that
/// `ssh` runs a command in often has not been told about.
const STATUS: &str = r#"sh -c 'PATH="$HOME/.local/bin:$PATH" rex server status --json'"#;

/// The server's socket on its own machine, from the status that machine's `rex` printed.
pub(crate) fn remote_socket(status: &Value) -> Option<String> {
    // Every `rex` names the endpoint it found the server at. Only the newer ones list
    // all it listens on, which is the place to look when that endpoint is no socket.
    let named = status["endpoint"].as_str().and_then(wire::unix_path);
    let named = named.filter(|path| path.starts_with('/'));
    named.or_else(|| {
        let listeners = status["listeners"].as_array().into_iter().flatten();
        listeners
            .filter(|listener| listener["transport"] == "unix" && listener["primary"] == true)
            .find_map(|listener| listener["addresses"][0].as_str().map(str::to_owned))
    })
}

/// Ask the machine where its Rex server listens.
async fn server_socket(destination: &str) -> Result<String> {
    let mut status = ssh();
    status.args(["--", destination, STATUS]);
    asked(status).await
}

/// Where the machine's Rex server listens, as `status`, an `ssh` set to ask it, says.
async fn asked(mut status: Command) -> Result<String> {
    status.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = status.spawn().context("Run ssh")?;
    #[cfg(windows)]
    crate::wsl::hold(&child);
    // `ssh` gives up on a machine that does not answer, but not on a login or a
    // command that hangs. Given up on here, it is killed.
    let output = tokio::time::timeout(PATIENCE, child.wait_with_output()).await;
    let output = output
        .context("That machine did not say in time where its Rex server is")?
        .context("Run ssh")?;
    ensure!(output.status.success(), "{}", reason(&output.stderr));
    let status: Value = serde_json::from_slice(&output.stdout).context("Read server status")?;
    remote_socket(&status).context("That machine's Rex server has no Unix socket")
}

/// Read what `ssh` says for as long as it runs, so that it never waits on a full
/// pipe, and keep the last of it.
pub(crate) async fn drain(mut pipe: impl AsyncRead + Unpin) -> Vec<u8> {
    let (mut said, mut chunk) = (Vec::new(), [0; 1024]);
    while let Ok(read @ 1..) = pipe.read(&mut chunk).await {
        said.extend_from_slice(&chunk[..read]);
        let old = said.len().saturating_sub(KEPT);
        said.drain(..old);
    }
    said
}

/// Make a directory that only `user` can reach, or be sure that the one already
/// there is such. In a place others can write to, one of them may have made it first.
#[cfg(unix)]
fn private(directory: &Path, user: u32) -> Result<()> {
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(directory)?;
    // Asked of the name itself: a link to a directory is not one.
    let found = std::fs::symlink_metadata(directory)?;
    ensure!(
        found.is_dir() && found.uid() == user,
        "{} is not a directory of this user's own",
        directory.display()
    );
    Ok(())
}

/// A fresh path for the local end of a forward, in a directory only this user can reach.
#[cfg(unix)]
fn local_socket() -> Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    // SAFETY: geteuid has no arguments and cannot fail.
    let user = unsafe { libc::geteuid() };
    // Where there is no runtime directory, the place is everyone's, and each user
    // needs a name there of their own.
    let directory = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) => PathBuf::from(runtime).join("rexy"),
        None => std::env::temp_dir().join(format!("rexy-{user}")),
    };
    private(&directory, user)?;
    // A process that was killed leaves its sockets behind. Clear those out.
    for entry in std::fs::read_dir(&directory)?.flatten() {
        let name = entry.file_name();
        let owner = name.to_str().and_then(|name| name.split('-').nth(1));
        if owner.is_some_and(|pid| !Path::new("/proc").join(pid).exists()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let (pid, next) = (std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    Ok(directory.join(format!("ssh-{pid}-{next}.sock")))
}

#[cfg(unix)]
impl Tunnel {
    pub async fn open(endpoint: &str) -> Result<Self> {
        let (destination, socket) = parse(endpoint);
        let remote = match socket {
            Some(socket) => socket.to_owned(),
            None => server_socket(destination).await?,
        };
        let local = local_socket()?;
        let mut child = ssh()
            .args(["-o", "ExitOnForwardFailure=yes"])
            .args(["-o", "StreamLocalBindUnlink=yes"])
            .args(["-o", "StreamLocalBindMask=0177"])
            .arg("-L")
            .arg(format!("{}:{remote}", local.display()))
            .args(["--", destination, "cat"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("Run ssh")?;
        let stderr = child.stderr.take().context("Read ssh")?;
        // Left running once the forward is open, to end when `ssh` does.
        let mut said = tokio::spawn(drain(stderr));
        let mut tunnel = Self { _stdin: child.stdin.take(), child, local };
        let started = Instant::now();
        while tokio::net::UnixStream::connect(&tunnel.local).await.is_err() {
            if tunnel.child.try_wait()?.is_some() {
                // Its last words may still be on their way. Something it started
                // can hold the pipe open, so they are not waited on for long.
                let said = tokio::time::timeout(Duration::from_secs(1), &mut said).await;
                let said = said.ok().and_then(Result::ok).unwrap_or_default();
                bail!("{}", reason(&said));
            }
            ensure!(started.elapsed() < PATIENCE, "ssh did not open the forward in time");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(tunnel)
    }

    /// The local end, as an endpoint to connect to: a bare path, which is taken as
    /// it is written.
    pub fn endpoint(&self) -> String {
        self.local.display().to_string()
    }
}

#[cfg(unix)]
impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.local);
    }
}

/// `STATUS` as it is passed through `wsl.exe`, which collapses a backslash, and
/// after Windows, which escapes a double quote. Neither is in it.
#[cfg(any(windows, test))]
const STATUS_WSL: &str = "sh -c 'PATH=$HOME/.local/bin:$PATH rex server status --json'";

/// Waits in WSL until the forward's socket at the path it is given answers, trying
/// every quarter second. One process waits, rather than one for each try.
#[cfg(any(windows, test))]
const READY: &str = "use IO::Socket::UNIX;select(undef,undef,undef,0.25) until IO::Socket::UNIX->new(Peer=>$ARGV[0])";

/// `READY` as it is run in WSL, for the socket at `local`, which comes after `--`
/// so that perl takes no path for a switch of its own.
#[cfg(any(windows, test))]
fn ready_in_wsl(local: &str) -> [&str; 5] {
    ["perl", "-e", READY, "--", local]
}

/// Whether a forward's socket, by its name, was left behind by a process that is no
/// longer running. Only a name as `socket_name` makes it is anyone's.
#[cfg(any(windows, test))]
fn left_behind(name: &str, running: impl Fn(u32) -> bool) -> bool {
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let Some(rest) = name.strip_prefix("ssh-").and_then(|rest| rest.strip_suffix(".sock")) else {
        return false;
    };
    match rest.split_once('-') {
        Some((pid, next)) if digits(pid) && digits(next) => {
            pid.parse().is_ok_and(|pid| pid != std::process::id() && !running(pid))
        }
        _ => false,
    }
}

/// What `ssh` is run with in WSL: itself, `OPTIONS`, then `rest`. What has a
/// backslash or a double quote would not reach it as it is, and is refused.
#[cfg(any(windows, test))]
fn in_wsl(rest: &[&str]) -> Result<Vec<String>> {
    let mut args = vec!["ssh".to_owned()];
    for option in OPTIONS {
        args.extend(["-o".to_owned(), option.to_owned()]);
    }
    args.push("-T".to_owned());
    args.extend(rest.iter().map(|arg| (*arg).to_owned()));
    if let Some(arg) = args.iter().find(|arg| arg.contains(['\\', '"'])) {
        bail!("WSL cannot be given {arg} for ssh: it has a backslash or a double quote");
    }
    Ok(args)
}

/// `ssh` in WSL, to ask `destination` where its Rex server is.
#[cfg(any(windows, test))]
fn status_in_wsl(destination: &str) -> Result<Vec<String>> {
    in_wsl(&["--", destination, STATUS_WSL])
}

/// `ssh` in WSL, to forward `local` there to `remote` on `destination`. Each path
/// is part of one argument, whatever spaces it has.
#[cfg(any(windows, test))]
fn forward_in_wsl(local: &str, remote: &str, destination: &str) -> Result<Vec<String>> {
    let forward = format!("{local}:{remote}");
    in_wsl(&[
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "StreamLocalBindUnlink=yes",
        "-o",
        "StreamLocalBindMask=0177",
        "-L",
        &forward,
        "--",
        destination,
        "cat",
    ])
}

/// The name of a forward's socket: this process's, and its count of forwards.
#[cfg(any(windows, test))]
fn socket_name(pid: u32, next: u32) -> String {
    format!("ssh-{pid}-{next}.sock")
}

/// A socket in a WSL distribution, as an endpoint to connect to.
#[cfg(any(windows, test))]
fn wsl_endpoint(distro: &str, path: &str) -> String {
    format!("wsl://{distro}{path}")
}

/// The distribution `ssh` runs in: home's, or else WSL's default. Windows has no
/// other place for the forward's socket.
#[cfg(any(windows, test))]
fn ssh_distro(distro: Option<String>) -> Result<String> {
    distro.context(
        "ssh:// needs a distribution in WSL: there ssh forwards the server's socket for Windows",
    )
}

/// A running forward, by `ssh` in WSL to a socket there. Dropping it ends the forward.
#[cfg(windows)]
pub struct Tunnel {
    child: Child,
    /// The remote `cat` ends with its input, so the forward cannot outlive this process.
    _stdin: Option<ChildStdin>,
    distro: String,
    /// The forward's socket, as the distribution names it.
    local: String,
}

#[cfg(windows)]
impl Tunnel {
    pub async fn open(endpoint: &str) -> Result<Self> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let distro = ssh_distro(crate::wsl::home_or_default().await)?;
        let (destination, socket) = parse(endpoint);
        let remote = match socket {
            Some(socket) => socket.to_owned(),
            None => asked(crate::wsl::command(&distro, status_in_wsl(destination)?)?).await?,
        };
        let (directory, sockets) = crate::wsl::place(&distro).await?;
        // A process that was killed leaves its sockets behind. Clear those out.
        let left = sockets.iter().filter(|name| left_behind(name, crate::wsl::running));
        let left: Vec<String> = left.map(|name| format!("{directory}/{name}")).collect();
        if !left.is_empty() {
            let mut rm = vec!["rm", "-f", "--"];
            rm.extend(left.iter().map(String::as_str));
            let _ = crate::wsl::ask(&distro, &rm).await;
        }
        let next = NEXT.fetch_add(1, Ordering::Relaxed);
        let local = format!("{directory}/{}", socket_name(std::process::id(), next));
        let forward = forward_in_wsl(&local, &remote, destination)?;
        let mut command = crate::wsl::command(&distro, forward)?;
        command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = command.spawn().context("Run ssh in WSL")?;
        crate::wsl::hold(&child);
        let stderr = child.stderr.take().context("Read ssh")?;
        // Left running once the forward is open, to end when `ssh` does.
        let mut said = tokio::spawn(drain(stderr));
        let mut tunnel = Self { _stdin: child.stdin.take(), child, distro, local };
        let mut ready = crate::wsl::command(&tunnel.distro, ready_in_wsl(&tunnel.local))?;
        let ready = ready.stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        let mut ready = ready.context("Run wsl.exe")?;
        crate::wsl::hold(&ready);
        let waited = tokio::time::timeout(PATIENCE, async {
            tokio::select! {
                ready = ready.wait() => Some(ready),
                _ = tunnel.child.wait() => None,
            }
        });
        match waited.await.context("ssh did not open the forward in time")? {
            Some(ready) => {
                let ready = ready.context("Run wsl.exe")?;
                ensure!(ready.success(), "WSL could not tell whether ssh opened the forward");
                Ok(tunnel)
            }
            None => {
                // Its last words may still be on their way. Something it started
                // can hold the pipe open, so they are not waited on for long.
                let said = tokio::time::timeout(Duration::from_secs(1), &mut said).await;
                let said = said.ok().and_then(Result::ok).unwrap_or_default();
                bail!("{}", reason(&said));
            }
        }
    }

    /// The forward's socket, as an endpoint to connect to: in WSL, through a bridge.
    pub fn endpoint(&self) -> String {
        wsl_endpoint(&self.distro, &self.local)
    }
}

#[cfg(windows)]
impl Drop for Tunnel {
    /// `ssh` leaves its socket behind. The next with that name would replace it,
    /// but there may be no next.
    fn drop(&mut self) {
        crate::wsl::forget(&self.distro, &["rm", "-f", "--", &self.local]);
    }
}

#[cfg(test)]
#[path = "../tests/unit/ssh.rs"]
mod tests;
