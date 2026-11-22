//! Reach a Rex server in WSL from Windows. Windows cannot open a Unix socket in a
//! WSL distribution, so each connection runs a small bridge there through `wsl.exe`,
//! whose input and output carry the socket's bytes. Rex does not run on Windows
//! itself: home is the server Rex in WSL follows.
#![cfg_attr(not(windows), allow(dead_code))]
use std::sync::RwLock;
#[cfg(windows)]
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    future::Future,
    io::{self, Read},
    os::windows::io::{AsRawHandle, RawHandle},
    path::PathBuf,
    pin::Pin,
    process::Stdio,
    sync::{Mutex, OnceLock},
    task::{Context, Poll, ready},
    time::Duration,
};

#[cfg(windows)]
use anyhow::{Context as _, Result, ensure};
#[cfg(windows)]
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    process::{Child, ChildStdin, ChildStdout, Command},
    task::JoinHandle,
};

use crate::wire;
#[cfg(windows)]
use crate::{ssh, wire::Target};

/// The bridge, run by `perl -e` with the socket's path after `--`, so that no path
/// is taken for one of perl's switches. It has no backslash, which
/// `wsl.exe` would collapse, and no double quote, which Windows would escape. It
/// half-closes the socket when its input ends, and ends with the socket.
pub const BRIDGE: &str = "use IO::Socket::UNIX;use IO::Select;my $s=IO::Socket::UNIX->new(Peer=>$ARGV[0]) or die join(q(: ),q(rexy bridge),$ARGV[0],$!);binmode STDIN;binmode STDOUT;binmode $s;my $sel=IO::Select->new(*STDIN,$s);sub put{my($h,$b)=@_;while(length $b){my $n=syswrite($h,$b);defined $n or exit 1;substr($b,0,$n,q())}}while($sel->count){for my $h($sel->can_read){my $b;my $n=sysread($h,$b,65536);if(fileno($h)==fileno($s)){$n or exit 0;put(*STDOUT,$b)}else{if($n){put($s,$b)}else{$sel->remove($h);shutdown($s,1)}}}}";

/// What a distribution's name is printed after, on a line of its own. WSL may print
/// warnings of its own before anything that is run prints.
const NAMED: &str = "rexy-name:";
/// How home is asked for in a distribution: its name, then what its `rex` follows.
/// Rex's installer puts `rex` in `~/.local/bin`, which a bare `sh` has not been told of.
const HOSTS: &str = "echo rexy-name:$WSL_DISTRO_NAME; PATH=$HOME/.local/bin:$PATH rex hosts --json --autostart=false";
/// How a distribution is asked its name.
const NAME: &str = "echo rexy-name:$WSL_DISTRO_NAME";
/// How a distribution's server is asked where it listens. Neither this nor the
/// above starts a server that is not running.
const STATUS: &str = "PATH=$HOME/.local/bin:$PATH rex server status --json --autostart=false";
/// How long a distribution may take to answer, starting first if it must.
#[cfg(windows)]
const PATIENCE: Duration = Duration::from_secs(20);
/// How long the default distribution may take to say where home is, while the
/// window waits to open: it may have to start first.
const BOOTING: std::time::Duration = std::time::Duration::from_secs(10);
/// How long one already running may take to say, or WSL to say which are running.
const RUNNING: std::time::Duration = std::time::Duration::from_secs(3);
/// What a server's answer to the bridge's first bytes begins with.
const ANSWER: &[u8] = b"HTTP/";
/// What each of WSL's own warnings begins with. They come before anything else.
const WARNING: &[u8] = b"wsl:";
/// How much of what `wsl.exe` says is kept: enough for its reason.
const KEPT: usize = 2048;

/// The distribution home is in, where a socket written as a path is.
static HOME: RwLock<Option<String>> = RwLock::new(None);
/// The distribution whose Rex said where home is, wherever home is.
static FOLLOWS: RwLock<Option<String>> = RwLock::new(None);

/// The distribution home is in, if home is in WSL.
pub fn home() -> Option<String> {
    HOME.read().ok()?.clone()
}

/// Take `endpoint` for home, and its distribution, if it names one, for where the
/// sockets other endpoints name are. Only on Windows is a socket elsewhere.
pub fn adopt(endpoint: &str) {
    if cfg!(windows)
        && let Ok(mut home) = HOME.write()
    {
        *home = distro(endpoint).map(str::to_owned);
    }
}

/// Take `distro` for the one whose Rex said where home is: `ssh` runs there when
/// home is not in WSL itself.
pub fn follow(distro: &str) {
    if let Ok(mut follows) = FOLLOWS.write() {
        *follows = Some(distro.to_owned());
    }
}

/// The distribution whose Rex said where home is, if one did.
fn follows() -> Option<String> {
    FOLLOWS.read().ok()?.clone()
}

/// The distribution a `wsl://` endpoint names.
pub fn distro(endpoint: &str) -> Option<&str> {
    let rest = endpoint.strip_prefix("wsl://")?;
    rest.split('/').next().filter(|distro| !distro.is_empty())
}

/// Whether a distribution could be called this. Whatever else `wsl.exe` prints
/// where a name should be is why it has none. No name begins as a switch does.
pub(crate) fn named(name: &str) -> bool {
    let fitting = |c: char| c.is_ascii_alphanumeric() || "._-".contains(c);
    !name.is_empty() && !name.starts_with('-') && name.chars().all(fitting)
}

/// Home, as `distro`'s `rex hosts --json` printed it: that distribution's own server
/// is reached through WSL, and any other as it is written.
pub fn home_in(distro: &str, hosts: &str) -> Option<String> {
    let hosts: serde_json::Value = serde_json::from_str(hosts).ok()?;
    let home = &hosts["home"];
    let endpoint = home["endpoint"].as_str().filter(|endpoint| !endpoint.is_empty())?;
    let path = wire::unix_path(endpoint).filter(|path| path.starts_with('/'));
    match path {
        Some(path) if home["this_machine"] == true => Some(format!("wsl://{distro}{path}")),
        _ => Some(endpoint.to_owned()),
    }
}

/// A distribution's name, from the line `NAME` printed it on, and what came after
/// that line. Lines before it are WSL's.
fn name_then(said: &str) -> Option<(&str, &str)> {
    let mut rest = said;
    loop {
        let (line, after) = rest.split_once('\n').unwrap_or((rest, ""));
        if let Some(name) = line.strip_prefix(NAMED) {
            return Some(name.trim()).filter(|name| named(name)).map(|name| (name, after));
        }
        if after.is_empty() {
            return None;
        }
        rest = after;
    }
}

/// A distribution's name, as `NAME` printed it.
pub fn name_in(said: &str) -> Option<&str> {
    name_then(said).map(|(name, _)| name)
}

/// What a distribution answered to `HOSTS`: its name, and home if its `rex` said.
pub fn answered(said: &str) -> (Option<&str>, Option<String>) {
    match name_then(said) {
        Some((name, hosts)) => (Some(name), home_in(name, hosts)),
        None => (None, None),
    }
}

/// How much of what was printed is WSL's own warnings, each a whole line, which
/// come before anything else.
fn warnings(said: &[u8]) -> usize {
    let mut start = 0;
    while said[start..].starts_with(WARNING) {
        match said[start..].iter().position(|&byte| byte == b'\n') {
            Some(end) => start += end + 1,
            None => break,
        }
    }
    start
}

/// What was printed, less the warnings WSL put before it.
fn unwarned(said: &[u8]) -> &[u8] {
    &said[warnings(said)..]
}

/// How much of the bridge's first bytes are WSL's warnings, and whether what follows
/// them is a server's answer, once that is known.
fn answer_in(heard: &[u8]) -> (usize, Option<bool>) {
    let warned = warnings(heard);
    let rest = &heard[warned..];
    // A warning may not have come whole yet.
    if !rest.is_empty() && WARNING.starts_with(&rest[..rest.len().min(WARNING.len())]) {
        return (warned, None);
    }
    let seen = rest.len().min(ANSWER.len());
    let answer = if rest[..seen] != ANSWER[..seen] {
        Some(false)
    } else {
        (seen == ANSWER.len()).then_some(true)
    };
    (warned, answer)
}

/// What the bridge to `socket` is run with in WSL. The socket must be a path from
/// the root, and perl is told where its own switches end, so that no socket is
/// taken for one.
pub fn bridge_args(socket: &str) -> anyhow::Result<[&str; 5]> {
    anyhow::ensure!(socket.starts_with('/'), "A socket in WSL is a path from /, not {socket}");
    Ok(["perl", "-e", BRIDGE, "--", socket])
}

/// Whether what is run in `distro` with `args` reaches it as it is written: the
/// distribution has a name, and no argument has a backslash, which `wsl.exe` would
/// collapse, or a double quote, which Windows would escape. What would not is refused,
/// not changed.
pub fn passable<S: AsRef<str>>(distro: &str, args: &[S]) -> anyhow::Result<()> {
    anyhow::ensure!(named(distro), "WSL has no distribution called {distro}");
    let unfit = args.iter().map(AsRef::as_ref).find(|arg| arg.contains(['\\', '"']));
    if let Some(arg) = unfit {
        anyhow::bail!("WSL cannot be given {arg}: it has a backslash or a double quote");
    }
    Ok(())
}

/// The distributions to look for home in, in order: the default, then those running.
/// Docker Desktop's own are no place for it.
pub fn distros(default: Option<&str>, running: &str) -> Vec<String> {
    let mut chosen: Vec<String> = Vec::new();
    for name in default.into_iter().chain(running.lines()) {
        let name = name.trim_matches(|c: char| c.is_whitespace() || c == '\0' || c == '\u{feff}');
        let known = chosen.iter().any(|known| known.eq_ignore_ascii_case(name));
        if named(name) && !known && !name.starts_with("docker-desktop") {
            chosen.push(name.into());
        }
    }
    chosen
}

/// Makes the directory for forwards' sockets, which only this user can reach, and
/// says where it is and which sockets are in it. It is `rexy` in the runtime
/// directory, or where there is none, a place of this user's own in `/tmp`, which
/// is everyone's: one another made there first is refused. Like the bridge, it has
/// no backslash and no double quote.
pub const PLACE: &str = "my $r=$ENV{XDG_RUNTIME_DIR};$r=~s{/+$}{} if defined $r;my $d=(defined $r && $r=~m{^/.})?qq($r/rexy):qq(/tmp/rexy-$>);mkdir $d,0700;my @s=lstat $d;(-d _ && $s[4]==$> && !($s[2]&63)) or die qq(rexy: $d is not a directory of this user's own$/);opendir my $h,$d or die qq(rexy: $d: $!$/);print qq(rexy-dir:$d$/);print qq(rexy-socket:$_$/) for grep {m{^ssh-}} readdir $h";

/// The directory for forwards' sockets, and the names of those in it, as `PLACE`
/// printed them. A directory that could not be passed back to WSL is none.
pub fn placed(said: &str) -> Option<(String, Vec<String>)> {
    let lines = || said.lines().map(|line| line.trim_end_matches('\r'));
    let directory = lines().find_map(|line| line.strip_prefix("rexy-dir:"))?;
    let fitting = directory.starts_with('/') && !directory.contains(['\\', '"']);
    let sockets = lines().filter_map(|line| line.strip_prefix("rexy-socket:"));
    fitting.then(|| (directory.to_owned(), sockets.map(str::to_owned).collect()))
}

/// Where a distribution mounts the drives, from what `wslpath -u C:/` printed, which
/// is its last line.
pub fn mount_root(said: &str) -> Option<String> {
    let drive = said.lines().map(str::trim).rfind(|line| !line.is_empty())?;
    let root = drive.strip_suffix("c/").or_else(|| drive.strip_suffix('c'))?;
    root.ends_with('/').then(|| root.to_owned())
}

/// A file on Windows as `distro` names it, where it can read it: on a drive, which
/// it mounts under `mounts`, or in the distribution itself. Another distribution's
/// files, and a share's, it cannot.
pub fn path_in(path: &str, distro: &str, mounts: &str) -> Option<String> {
    // A path may come in the long form, which only Windows reads.
    let path = match path.strip_prefix(r"\\?\") {
        Some(long) => long.strip_prefix(r"UNC\").map_or(long.into(), |share| format!(r"\\{share}")),
        None => path.to_owned(),
    };
    let path = path.replace('\\', "/");
    if let Some(share) = path.strip_prefix("//") {
        let (server, rest) = share.split_once('/')?;
        let wsl = ["wsl.localhost", "wsl$"].iter().any(|wsl| server.eq_ignore_ascii_case(wsl));
        let (name, inside) = rest.split_once('/').unwrap_or((rest, ""));
        return (wsl && name.eq_ignore_ascii_case(distro)).then(|| format!("/{inside}"));
    }
    let mut chars = path.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    let inside = chars.as_str().strip_prefix(":/")?;
    Some(format!("{mounts}{}/{inside}", drive.to_ascii_lowercase()))
}

/// What `wsl.exe` or the bridge last said, other than a code for programs.
fn reason(said: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(said);
    let mut lines = text.lines().rev().map(str::trim);
    let line = lines.find(|line| !line.is_empty() && !line.starts_with("Error code:"))?;
    // Perl says where it died, which is no help here.
    Some(line.trim_end_matches(" at -e line 1.").to_owned())
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

#[cfg(windows)]
fn program() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map_or_else(|| r"C:\Windows".into(), PathBuf::from);
    root.join(r"System32\wsl.exe")
}

/// Every `wsl.exe` started here is in one job, which Windows ends with this process
/// however that ends. This process is not in it: what it opens, as a browser for a
/// link, outlives it.
#[cfg(windows)]
pub(crate) fn hold(child: &Child) {
    if let Some(handle) = child.raw_handle() {
        join(handle);
    }
}

#[cfg(windows)]
fn join(child: RawHandle) {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
    };
    // Kept as a number: a handle is a pointer, which may not be shared.
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| {
        // SAFETY: The job is made with no name and default security, and the limits
        // given are a zeroed structure of the size given, with one flag set.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return 0;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            );
            if set == 0 {
                CloseHandle(job);
                return 0;
            }
            job as usize
        }
    });
    if job != 0 {
        // SAFETY: Both handles are open: the job's for as long as the process, and
        // the child's while its owner holds it. One that is not taken in is still
        // killed when it is dropped.
        unsafe { AssignProcessToJobObject(job as HANDLE, child as HANDLE) };
    }
}

/// `wsl.exe` as every run here uses it: printing UTF-8, with no console window,
/// and killed when it is dropped.
#[cfg(windows)]
fn wsl() -> Command {
    let mut command = Command::new(program());
    command.env("WSL_UTF8", "1").creation_flags(CREATE_NO_WINDOW).kill_on_drop(true);
    command.stdin(Stdio::null());
    command
}

/// `wsl.exe` set to run a program in a distribution, with these arguments. What
/// would not reach it as it is written is refused, as `passable` says.
#[cfg(windows)]
pub(crate) fn command<I: IntoIterator<Item = S>, S: AsRef<str>>(
    distro: &str,
    args: I,
) -> Result<Command> {
    let args: Vec<S> = args.into_iter().collect();
    passable(distro, &args)?;
    let mut command = wsl();
    command.args(["-d", distro, "--exec"]);
    command.args(args.iter().map(|arg| OsStr::new(arg.as_ref())));
    Ok(command)
}

/// Run a command in a distribution, and say what it printed.
#[cfg(windows)]
pub(crate) async fn ask(distro: &str, args: &[&str]) -> Result<String> {
    run(command(distro, args)?).await
}

#[cfg(windows)]
async fn run(mut command: Command) -> Result<String> {
    let child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let child = child.context("Run wsl.exe")?;
    hold(&child);
    let output = tokio::time::timeout(PATIENCE, child.wait_with_output()).await;
    let output = output.context("WSL did not answer in time")?.context("Run wsl.exe")?;
    // `wsl.exe` says why it could not run something with what it prints.
    let why = || reason(&output.stderr).or_else(|| reason(&output.stdout));
    ensure!(output.status.success(), "{}", why().unwrap_or_else(|| output.status.to_string()));
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The distribution to reach other machines from: home's, or else the one whose
/// Rex said where home is, or else WSL's default. That is asked only when it is
/// needed, so that WSL is not started for a home elsewhere, and once it has
/// answered, no more.
#[cfg(windows)]
pub(crate) async fn home_or_default() -> Option<String> {
    static DEFAULT: Mutex<Option<String>> = Mutex::new(None);
    if let Some(home) = home().or_else(follows) {
        return Some(home);
    }
    if let Some(known) = DEFAULT.lock().ok().and_then(|known| known.clone()) {
        return Some(known);
    }
    let mut command = wsl();
    command.args(["--exec", "sh", "-c", NAME]);
    let said = run(command).await.ok()?;
    let name = name_in(&said)?.to_owned();
    if let Ok(mut known) = DEFAULT.lock() {
        *known = Some(name.clone());
    }
    Some(name)
}

/// A target in WSL with its socket: the distribution's own `rex` is asked for it
/// where the endpoint did not name it. Any other is as it was.
#[cfg(windows)]
pub async fn resolved(target: Target) -> Result<Target> {
    let Target::Wsl { distro, socket: None } = target else {
        return Ok(target);
    };
    let found = async {
        let status = ask(&distro, &["sh", "-c", STATUS]).await?;
        let status = unwarned(status.as_bytes());
        let status: serde_json::Value =
            serde_json::from_slice(status).context("Read server status")?;
        let socket = ssh::remote_socket(&status).context("The server has no Unix socket")?;
        // It is passed back to WSL for each connection.
        passable(&distro, &bridge_args(&socket)?)?;
        anyhow::Ok(socket)
    };
    let socket =
        found.await.with_context(|| format!("No Rex server was found in WSL's {distro}"))?;
    Ok(Target::Wsl { distro, socket: Some(socket) })
}

/// The directory for forwards' sockets in a distribution, and the sockets in it.
/// It is made each time: WSL empties the runtime directory when it shuts down.
#[cfg(windows)]
pub(crate) async fn place(distro: &str) -> Result<(String, Vec<String>)> {
    let said = ask(distro, &["perl", "-e", PLACE]).await;
    let said = said.with_context(|| format!("Make a place for ssh's sockets in WSL's {distro}"))?;
    placed(&said).with_context(|| format!("WSL's {distro} has no place for ssh's sockets"))
}

/// Whether a process on this machine is running, as far as can be told: one that
/// cannot be asked is taken to be.
#[cfg(windows)]
pub(crate) fn running(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // SAFETY: OpenProcess takes a process id and gives a handle or none, and the
    // handle is closed once its exit code has been read into a local.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            // Windows says so of an id no process has.
            return GetLastError() != ERROR_INVALID_PARAMETER;
        }
        let mut code = 0;
        let read = GetExitCodeProcess(process, &raw mut code);
        CloseHandle(process);
        read == 0 || code == STILL_ACTIVE as u32
    }
}

/// Start a command in a distribution and leave it to finish on its own, as what
/// is dropped must, without waiting.
#[cfg(windows)]
pub(crate) fn forget(distro: &str, args: &[&str]) {
    use std::os::windows::process::CommandExt;
    if passable(distro, args).is_err() {
        return;
    }
    let mut command = std::process::Command::new(program());
    command.env("WSL_UTF8", "1").creation_flags(CREATE_NO_WINDOW);
    command.args(["-d", distro, "--exec"]).args(args);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Ok(child) = command.spawn() {
        join(child.as_raw_handle());
    }
}

/// Where a distribution mounts the drives, asked once of each.
#[cfg(windows)]
pub async fn mounts(distro: &str) -> String {
    static KNOWN: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
    if let Some(known) = KNOWN.lock().ok().and_then(|known| known.get(distro).cloned()) {
        return known;
    }
    let said = ask(distro, &["wslpath", "-u", "C:/"]).await;
    let root = said.ok().and_then(|said| mount_root(&said)).unwrap_or_else(|| "/mnt/".into());
    if let Ok(mut known) = KNOWN.lock() {
        known.insert(distro.into(), root.clone());
    }
    root
}

/// One connection's bridge: what is written goes to the socket, and what is read
/// came from it. Dropping it kills `wsl.exe`, which ends the bridge in WSL too.
#[cfg(windows)]
pub struct Bridge {
    _child: Child,
    output: ChildStdout,
    input: Option<ChildStdin>,
    /// What came before the server's first answer, held until it is known to be
    /// one. `wsl.exe` prints why it cannot run the bridge where the answer would be.
    heard: Vec<u8>,
    /// Whether it was the server's answer, once that is known.
    answered: Option<bool>,
    ended: bool,
    /// What the bridge says of itself, which is why it ended, if it did.
    said: Option<JoinHandle<Vec<u8>>>,
    failed: Option<String>,
}

#[cfg(windows)]
impl Bridge {
    pub fn open(distro: &str, socket: &str) -> Result<Self> {
        Self::spawn(command(distro, bridge_args(socket)?)?)
    }

    fn spawn(mut command: Command) -> Result<Self> {
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        command.kill_on_drop(true);
        let mut child = command.spawn().context("Run wsl.exe")?;
        hold(&child);
        let said = child.stderr.take().map(|stderr| tokio::spawn(ssh::drain(stderr)));
        let (output, input) = (child.stdout.take(), child.stdin.take());
        Ok(Self {
            output: output.context("Read wsl.exe")?,
            input,
            _child: child,
            heard: Vec::new(),
            answered: None,
            ended: false,
            said,
            failed: None,
        })
    }
}

#[cfg(windows)]
impl AsyncRead for Bridge {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        while this.answered != Some(true) {
            if let Some(failed) = &this.failed {
                return Poll::Ready(Err(io::Error::other(failed.clone())));
            }
            if this.ended {
                // It ended before the server answered: why is in what it said.
                let said = match &mut this.said {
                    Some(said) => ready!(Pin::new(said).poll(cx)).unwrap_or_default(),
                    None => Vec::new(),
                };
                this.said = None;
                let why = reason(unwarned(&this.heard)).or_else(|| reason(&said));
                let why = why.or_else(|| reason(&this.heard));
                let why = why.unwrap_or_else(|| "WSL ended the bridge to the server".into());
                this.failed = Some(why);
                continue;
            }
            let mut chunk = [0; 1024];
            let mut read = ReadBuf::new(&mut chunk);
            ready!(Pin::new(&mut this.output).poll_read(cx, &mut read))?;
            match read.filled() {
                [] => this.ended = true,
                got => this.heard.extend_from_slice(got),
            }
            if this.answered.is_none() {
                let (warned, answered) = answer_in(&this.heard);
                // WSL's warnings are not the server's to read.
                if answered == Some(true) {
                    this.heard.drain(..warned);
                }
                this.answered = answered;
            }
            if this.answered == Some(false) {
                let old = this.heard.len().saturating_sub(KEPT);
                this.heard.drain(..old);
            }
        }
        if this.heard.is_empty() {
            return Pin::new(&mut this.output).poll_read(cx, buf);
        }
        let given = this.heard.len().min(buf.remaining());
        buf.put_slice(&this.heard[..given]);
        this.heard.drain(..given);
        Poll::Ready(Ok(()))
    }
}

#[cfg(windows)]
impl AsyncWrite for Bridge {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.input {
            Some(input) => Pin::new(input).poll_write(cx, buf),
            None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.input {
            Some(input) => Pin::new(input).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    /// The bridge's input is closed, and it half-closes the socket.
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush(cx))?;
        self.input = None;
        Poll::Ready(Ok(()))
    }
}

/// Run `wsl.exe` and say what it printed, waiting no longer than `patience`.
#[cfg(windows)]
fn said(args: &[&str], patience: Duration) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(program());
    command.env("WSL_UTF8", "1").creation_flags(CREATE_NO_WINDOW).args(args);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    join(child.as_raw_handle());
    let mut stdout = child.stdout.take()?;
    let (sender, printed) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        let _ = sender.send(out);
    });
    let out = printed.recv_timeout(patience);
    // It has ended unless it took too long.
    let _ = child.kill();
    let _ = child.wait();
    Some(String::from_utf8_lossy(&out.ok()?).into_owned())
}

/// Home, as Rex in WSL has it, and the distribution whose Rex said so. The default
/// distribution is asked first, which starts it if it is stopped, as anything asked
/// of WSL does, and it has `BOOTING` to answer. Then any other already running is
/// asked, each for no longer than `RUNNING`: none that is stopped is started. Where
/// none answers, the error is the default distribution, if there is one.
#[cfg(windows)]
pub fn find_home() -> Result<(String, String), Option<String>> {
    search(|args, patience| said(args, patience).unwrap_or_default())
}

/// Home and its distribution, as `find_home` looks for them, where `said` runs
/// `wsl.exe` with these arguments, waiting no longer than it is told.
pub fn search(
    mut said: impl FnMut(&[&str], std::time::Duration) -> String,
) -> Result<(String, String), Option<String>> {
    let first = said(&["--exec", "sh", "-c", HOSTS], BOOTING);
    let (default, home) = answered(&first);
    if let (Some(default), Some(home)) = (default, home) {
        return Ok((default.to_owned(), home));
    }
    let running = said(&["-l", "--running", "-q"], RUNNING);
    let asked =
        |distro: &String| default.is_some_and(|default| distro.eq_ignore_ascii_case(default));
    for distro in distros(default, &running).iter().filter(|distro| !asked(distro)) {
        let said = said(&["-d", distro, "--exec", "sh", "-c", HOSTS], RUNNING);
        if let (Some(name), Some(home)) = answered(&said) {
            return Ok((name.to_owned(), home));
        }
    }
    Err(default.map(str::to_owned))
}

#[cfg(test)]
#[path = "../tests/unit/wsl.rs"]
mod tests;
