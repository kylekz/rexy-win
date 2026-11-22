//! Rex WebSocket transport and bounded, pipelined control requests.
// Protocol interoperability informed by rexrs and rex-rune (MIT); see THIRD_PARTY.md.
use crate::{
    model::{Asked, Call, Choice, Question},
    program::{self, Program},
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt, Stream, StreamExt, stream};
use percent_encoding::percent_decode_str;
use serde_json::{Value, json};
use std::{collections::HashMap, fmt, net::Ipv6Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{Notify, mpsc, oneshot},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        self, Message,
        client::IntoClientRequest,
        protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode},
    },
};
use url::Url;

pub const DATA_PROTOCOL: &str = "rex.data.terminal.v1";
pub const ENDED: &str = "Rex control connection ended";
const CONTROL_PROTOCOL: &str = "rex.control.v1";
/// The revisions of the control API this client speaks. It needs what came with the
/// first, and was written for none newer than the last, which a newer server still
/// speaks to it. A server asks an action's questions only of a client that speaks
/// revision 17.
const REVISIONS: std::ops::RangeInclusive<u64> = 13..=18;
/// How a server begins its reason for closing on a client it shares no revision with.
const REFUSAL: &str = "no common API revision";
const MAX_IN_FLIGHT: usize = 64;
/// How long one address may take to answer, and to agree to a WebSocket.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a server in WSL may take, which may have to wait for WSL to start.
const WSL_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long one write may take, or one answer in a handshake.
const STEP_TIMEOUT: Duration = Duration::from_secs(8);
/// How often the control connection is looked over. A server not heard from between
/// two looks is pinged, and one still not heard from by the next is taken for gone.
const PING_EVERY: Duration = Duration::from_secs(10);
/// How many looks a request may wait for its answer: 40 s at the least. The server
/// itself gives a key's Lua handler 5 s and an action from its configuration 30 s.
const REQUEST_TICKS: u64 = 5;
/// How long a goodbye may take to leave. Quitting never waits longer on a dead link.
pub const CLOSE_TIMEOUT: Duration = Duration::from_millis(250);

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub type Socket = WebSocketStream<tokio_tungstenite::MaybeTlsStream<Box<dyn Io>>>;

/// Why a connection is over, where the server or the link has said. It rides in the
/// error, for whoever must decide what to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The server closed the control connection, with a WebSocket status and its reason.
    Closed { code: u16, reason: String },
    /// The server and this client have no API revision in common. Trying again will
    /// not help; the words say what each speaks, and which of them to update.
    Incompatible(String),
    /// The server closed a terminal's stream, likewise.
    StreamClosed { code: u16, reason: String },
    /// The terminal itself has closed. There is nothing left to reconnect to.
    BlockClosed,
    /// The server refused a stream's handshake, with the reason byte of its REJECT.
    Rejected(u8),
    /// The link went away with no close frame.
    Dropped,
    /// Nothing comes from the server, not even an answer to a ping.
    Silent,
}

impl Ended {
    /// What a close frame says, on the control connection or on a terminal's `stream`.
    fn closed(frame: Option<CloseFrame>, stream: bool) -> Self {
        // A close with no status is 1005, by the WebSocket standard.
        let (code, reason) = frame
            .map_or((1005, String::new()), |frame| (frame.code.into(), frame.reason.to_string()));
        match (stream, code) {
            (true, 4000) => Self::BlockClosed,
            (true, _) => Self::StreamClosed { code, reason },
            (false, 1008) if reason.starts_with(REFUSAL) => {
                let why = reason[REFUSAL.len()..].trim_start_matches([':', ' ']);
                Self::Incompatible(why.into())
            }
            (false, _) => Self::Closed { code, reason },
        }
    }
}

/// A close this client has no words for, with its status and reason as they came.
fn unnamed(f: &mut fmt::Formatter<'_>, what: &str, code: u16, reason: &str) -> fmt::Result {
    match reason {
        "" => write!(f, "{what} (status {code})"),
        reason => write!(f, "{what} (status {code}: {reason})"),
    }
}

impl fmt::Display for Ended {
    /// The server's codes and reasons, as its API reference lists them, in plain words.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed { code, reason } => match (*code, reason.as_str()) {
                (1000 | 1005, _) => f.write_str("The server closed the connection"),
                (1001, "keepalive timeout") => {
                    f.write_str("The server stopped hearing from this client")
                }
                (1002 | 1003 | 1007, _) => {
                    f.write_str("The server could not read what this client sent")
                }
                (1008, "event backlog exceeded") => {
                    f.write_str("This client fell too far behind the server's news")
                }
                (1009, "event exceeds message limit") => {
                    f.write_str("The server had news too large to send")
                }
                (1009, _) => f.write_str("A request was larger than the server accepts"),
                _ => unnamed(f, "The server closed the connection", *code, reason),
            },
            Self::Incompatible(why) => {
                write!(f, "This client and the server share no API revision: {why}")
            }
            Self::StreamClosed { code, reason } => match (*code, reason.as_str()) {
                (1000 | 1005, _) => f.write_str("The terminal's stream ended"),
                (1001, "keepalive timeout") => {
                    f.write_str("The server stopped hearing from this terminal's stream")
                }
                (1001, "data provider canceled") => {
                    f.write_str("The server stopped the terminal's stream")
                }
                (1008, "data provider unavailable") => {
                    f.write_str("The terminal has no stream of this kind")
                }
                (1011, "data provider failed") => f.write_str("The terminal failed on the server"),
                (1013, "data reconnect required") => {
                    f.write_str("The server wants the terminal's stream started afresh")
                }
                (_, "data handshake rejected") => {
                    f.write_str("The server refused the terminal's stream")
                }
                _ => unnamed(f, "The server closed the terminal's stream", *code, reason),
            },
            Self::BlockClosed => f.write_str("The terminal has closed"),
            Self::Rejected(reason) => match reason {
                1 => f.write_str("The server no longer knows the terminal stream's ticket"),
                2 => f.write_str("This client already has a stream of this terminal"),
                3 => f.write_str("The terminal's stream is gone; the terminal may have closed"),
                4 => f.write_str("The server has no room for another stream just now"),
                5 => f.write_str("The server did not get the terminal stream's hello in time"),
                6 => f.write_str("The server failed to open the terminal's stream"),
                7 => f.write_str("The ticket was for another kind of stream"),
                _ => write!(f, "The server refused the terminal's stream (reason {reason})"),
            },
            Self::Dropped => f.write_str("The connection to the server was lost"),
            Self::Silent => f.write_str("The server stopped answering"),
        }
    }
}

impl std::error::Error for Ended {}

/// Why a request came to nothing on a connection that went on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
    /// The server would not do it: the code of its error, and its words for it.
    Refused { code: String, message: String },
    /// It was larger than the server takes, which is so many bytes, and was not sent.
    TooLarge(usize),
    /// The server did not answer in time what was asked by this method.
    Unanswered(String),
}

impl Failed {
    /// Whether the server refused it with this code of its own.
    pub fn is(&self, refusal: &str) -> bool {
        matches!(self, Self::Refused { code, .. } if code == refusal)
    }
}

impl fmt::Display for Failed {
    /// The server's words alone, where it has any: its codes are for programs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { code, message } if message.is_empty() => {
                write!(f, "The server refused ({code})")
            }
            Self::Refused { message, .. } => f.write_str(message),
            Self::TooLarge(most) => write!(f, "Request exceeds server limit ({most} bytes)"),
            Self::Unanswered(method) => write!(f, "The server took too long to answer {method}"),
        }
    }
}

impl std::error::Error for Failed {}

/// A WebSocket error as the end it stands for, where it stands for one.
fn lost(error: tungstenite::Error) -> anyhow::Error {
    use tungstenite::{Error, error::ProtocolError::ResetWithoutClosingHandshake};
    match error {
        Error::ConnectionClosed
        | Error::AlreadyClosed
        | Error::Protocol(ResetWithoutClosingHandshake) => Ended::Dropped.into(),
        // The system's own word for it is worth keeping.
        Error::Io(error) => anyhow::Error::new(error).context(Ended::Dropped),
        error => error.into(),
    }
}

/// One address a WebSocket can be opened to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A Unix socket on this machine. Its path is kept as it is written, the
    /// way the server's machine reads it, whatever this one would make of it.
    Unix(String),
    /// A `ws://` or `wss://` address, with nothing after its port.
    Net(Url),
    /// A Unix socket in a WSL distribution, reached from Windows through a bridge
    /// there. With no socket named, the distribution's own `rex` is asked for it.
    Wsl { distro: String, socket: Option<String> },
}

/// The path of a Unix socket an endpoint names, as the server's machine reads it.
pub fn unix_path(endpoint: &str) -> Option<String> {
    // Rex writes a socket as a URL, its path percent-encoded.
    if let Some(path) = endpoint.strip_prefix("unix://") {
        let decoded = percent_decode_str(path).decode_utf8();
        return Some(decoded.as_deref().unwrap_or(path).into());
    }
    let path = endpoint.strip_prefix("unix:");
    path.or_else(|| endpoint.starts_with('/').then_some(endpoint)).map(str::to_owned)
}

/// The addresses an endpoint stands for, in the order to try them. A socket or a
/// URL is one address. A bare host name is tried as Rex tries it: HTTPS on port 443,
/// HTTPS on 7297, then HTTP on 7297. A bare host and port is tried with TLS, then without.
pub fn targets(endpoint: &str) -> Result<Vec<Target>> {
    targets_from(endpoint, crate::wsl::home().as_deref())
}

/// The addresses an endpoint stands for, where a socket is in the WSL
/// distribution `wsl` if one is home: Windows has no sockets of its own to name.
pub fn targets_from(endpoint: &str, wsl: Option<&str>) -> Result<Vec<Target>> {
    if let Some(path) = unix_path(endpoint) {
        return Ok(vec![match wsl {
            Some(distro) => {
                // It is passed to a program in WSL, which might take another for a switch.
                ensure!(path.starts_with('/'), "A socket in WSL is a path from /, not {path}");
                Target::Wsl { distro: distro.into(), socket: Some(path) }
            }
            None => Target::Unix(path),
        }]);
    }
    if let Some(rest) = endpoint.strip_prefix("wsl://") {
        let (distro, socket) = rest.find('/').map_or((rest, ""), |slash| rest.split_at(slash));
        ensure!(!distro.is_empty(), "Name a WSL distribution after wsl://, as in wsl://Ubuntu");
        ensure!(crate::wsl::named(distro), "WSL has no distribution called {distro}");
        let socket = (socket.len() > 1).then(|| socket.to_owned());
        return Ok(vec![Target::Wsl { distro: distro.into(), socket }]);
    }
    if endpoint.contains("://") {
        let mut url = Url::parse(endpoint)?;
        let scheme = match url.scheme() {
            "http" | "ws" => "ws",
            "https" | "wss" => "wss",
            _ => bail!(
                "Use a Unix socket path, a host name, or an ssh://, wsl://, ws:// or wss:// endpoint"
            ),
        };
        url.set_scheme(scheme).map_err(|_| anyhow!("Invalid endpoint scheme"))?;
        url.set_path("");
        url.set_query(None);
        url.set_fragment(None);
        return Ok(vec![Target::Net(url)]);
    }
    // An IPv6 address may come without its brackets, and a URL cannot hold it so.
    let bare = match endpoint.parse::<Ipv6Addr>() {
        Ok(address) => format!("[{address}]"),
        Err(_) => endpoint.to_owned(),
    };
    // Under a scheme with no port of its own, so that any port found was written.
    let named = Url::parse(&format!("rex://{bare}")).context("Invalid endpoint")?;
    let host = named.host_str().filter(|host| !host.is_empty());
    let host = host.context("Missing endpoint host")?;
    let at = |scheme: &str, port: u16| {
        let url = Url::parse(&format!("{scheme}://{host}:{port}")).context("Invalid endpoint")?;
        Ok(Target::Net(url))
    };
    match named.port() {
        Some(port) => [at("wss", port), at("ws", port)].into_iter().collect(),
        None => [at("wss", 443), at("wss", 7297), at("ws", 7297)].into_iter().collect(),
    }
}

/// The host and port to connect to for a network address. An IPv6 address is written
/// in brackets, and looked up without them.
fn host_port(url: &Url) -> Result<(&str, u16)> {
    let host = url.host_str().context("Missing endpoint host")?;
    let port = url.port_or_known_default();
    let port = port.context("Missing endpoint port")?;
    Ok((host.trim_start_matches('[').trim_end_matches(']'), port))
}

/// How long an address may take to answer, and to agree to a WebSocket.
fn connect_timeout(target: &Target) -> Duration {
    match target {
        Target::Wsl { .. } => WSL_CONNECT_TIMEOUT,
        _ => CONNECT_TIMEOUT,
    }
}

/// Open a WebSocket to one address.
pub async fn socket(target: &Target, path: &str, protocol: &str) -> Result<Socket> {
    tokio::time::timeout(connect_timeout(target), async {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (io, mut url): (Box<dyn Io>, Url) = match target {
            #[cfg(unix)]
            Target::Unix(socket) => {
                let unix = tokio::net::UnixStream::connect(socket).await;
                let unix = unix.context("Connect to Rex Unix socket")?;
                (Box::new(unix), Url::parse("ws://localhost")?)
            }
            #[cfg(not(unix))]
            Target::Unix(_) => {
                bail!("Unix sockets are not reachable from Windows; use wsl://…")
            }
            #[cfg(windows)]
            Target::Wsl { distro, socket } => {
                let socket = socket.as_deref().context("No socket was found in WSL")?;
                let bridge = crate::wsl::Bridge::open(distro, socket)?;
                (Box::new(bridge), Url::parse("ws://localhost")?)
            }
            #[cfg(not(windows))]
            Target::Wsl { .. } => bail!("wsl:// endpoints are reached only from Windows"),
            Target::Net(url) => {
                let tcp = tokio::net::TcpStream::connect(host_port(url)?).await?;
                tcp.set_nodelay(true)?;
                (Box::new(tcp), url.clone())
            }
        };
        url.set_path(path);
        let mut request = url.as_str().into_client_request()?;
        request.headers_mut().insert("Sec-WebSocket-Protocol", protocol.parse()?);
        let config = WebSocketConfig::default()
            .max_message_size(Some(64 * 1024 * 1024))
            .max_frame_size(Some(16 * 1024 * 1024));
        let opened =
            tokio_tungstenite::client_async_tls_with_config(request, io, Some(config), None).await;
        // What a bridge to WSL fails with is its own words, which need nothing more said.
        #[cfg(windows)]
        let opened = opened.map_err(|error| match error {
            tungstenite::Error::Io(error) if matches!(target, Target::Wsl { .. }) => error.into(),
            error => anyhow::Error::from(error),
        });
        let (socket, response) = opened?;
        ensure!(
            response.headers().get("Sec-WebSocket-Protocol").and_then(|h| h.to_str().ok())
                == Some(protocol),
            "Rex did not negotiate {protocol}"
        );
        Ok(socket)
    })
    .await
    .context("Rex connection timed out")?
}

/// Open a WebSocket at the first of an endpoint's addresses that answers, and say
/// which that was. The one that answered `last` time, if it is among them, is tried
/// first: each of the others may take its full time to say nothing.
async fn reach(
    endpoint: &str,
    last: Option<&Target>,
    path: &str,
    protocol: &str,
) -> Result<(Socket, Target)> {
    let mut targets = targets(endpoint)?;
    targets.sort_by_key(|target| Some(target) != last);
    let several = targets.len() > 1;
    let mut failure = anyhow!("This endpoint has no address");
    for target in targets {
        // A socket in WSL is asked for once, here: the terminals' streams go to
        // the one found.
        #[cfg(windows)]
        let target = match crate::wsl::resolved(target).await {
            Ok(target) => target,
            Err(error) => {
                failure = error;
                continue;
            }
        };
        match socket(&target, path, protocol).await {
            Ok(socket) => return Ok((socket, target)),
            Err(error) => failure = error,
        }
    }
    // The last address's failure alone would not say that others were tried.
    Err(if several {
        failure.context(format!("No Rex server answered at {endpoint}"))
    } else {
        failure
    })
}

/// The next message of any kind. A close and a lost link are errors that say which,
/// in the words for the control connection or for a terminal's `stream`.
async fn message(socket: &mut Socket, stream: bool) -> Result<Message> {
    match socket.next().await {
        Some(Ok(Message::Close(frame))) => Err(Ended::closed(frame, stream).into()),
        Some(Ok(message)) => {
            // Reading a ping only queues its answer. This sends it.
            if message.is_ping() {
                socket.flush().await.map_err(lost)?;
            }
            Ok(message)
        }
        Some(Err(error)) => Err(lost(error)),
        None => Err(Ended::Dropped.into()),
    }
}

async fn text(socket: &mut Socket) -> Result<Value> {
    loop {
        if let Message::Text(text) = message(socket, false).await? {
            return Ok(serde_json::from_str(&text)?);
        }
    }
}

pub async fn next_binary(socket: &mut Socket) -> Result<Vec<u8>> {
    loop {
        if let Message::Binary(bytes) = message(socket, true).await? {
            return Ok(bytes.to_vec());
        }
    }
}

async fn send_with_deadline(
    socket: &mut Socket,
    message: Message,
    deadline: Duration,
) -> Result<()> {
    tokio::time::timeout(deadline, socket.send(message))
        .await
        .context("Rex WebSocket write timed out; request was not retried")?
        .map_err(lost)
}

pub async fn handshake(socket: &mut Socket, ticket: &str) -> Result<u64> {
    let ticket = URL_SAFE_NO_PAD.decode(ticket)?;
    ensure!(ticket.len() == 16, "Invalid data ticket length");
    let mut hello = vec![1];
    hello.extend(ticket);
    send_with_deadline(socket, Message::Binary(hello.into()), STEP_TIMEOUT).await?;
    let welcome = tokio::time::timeout(STEP_TIMEOUT, next_binary(socket)).await;
    let welcome = welcome.context("The server did not answer the terminal stream's hello")??;
    // A REJECT, with its reason.
    if let [0x82, reason, ..] = welcome[..] {
        return Err(Ended::Rejected(reason).into());
    }
    ensure!(
        welcome.len() == 32 && welcome[0] == 0x81,
        "Rex answered the terminal stream's hello with something else"
    );
    let nonce = u64::from_le_bytes(welcome[24..32].try_into()?);
    ensure!(nonce != 0, "Invalid terminal nonce");
    Ok(nonce)
}

/// What comes of a request on a connection that goes on.
pub type Answer = Result<Value, Failed>;
type Reply = oneshot::Sender<Answer>;
pub type Response = oneshot::Receiver<Answer>;

struct Request {
    method: String,
    payload: Value,
    reply: Reply,
}

#[derive(Clone)]
pub struct Rpc {
    requests: mpsc::Sender<Request>,
}

impl Rpc {
    /// Queue requests in wire order. Both this queue and the outstanding response map are bounded.
    pub async fn enqueue(&self, method: &str, payload: Value) -> Result<Response> {
        let (reply, rx) = oneshot::channel();
        self.requests
            .send(Request { method: method.into(), payload, reply })
            .await
            .map_err(|_| anyhow!(ENDED))?;
        Ok(rx)
    }

    pub async fn call(&self, method: &str, payload: Value) -> Result<Value> {
        let rx = self.enqueue(method, payload).await?;
        Ok(rx.await.context(ENDED)??)
    }
}

pub struct Control {
    pub rpc: Rpc,
    pub client_id: String,
    pub reconnect_token: Option<String>,
    /// The server knew the token it was sent, and this is the client it was given
    /// to, still in the session it had joined and owning what it owned.
    pub resumed: bool,
    /// The one address that answered, of those the endpoint stood for. Terminal
    /// streams go straight to it.
    pub target: Target,
    /// Unbounded, so that reading replies never waits on whoever reads these. The
    /// server ends a connection that falls far behind.
    pub events: mpsc::UnboundedReceiver<Event>,
    /// Ends with the connection, and with an `Ended` in its error if it is known why.
    pub task: tokio::task::JoinHandle<Result<()>>,
    closing: Arc<Notify>,
}

/// Wait for a control connection's worker to end, which it does with the
/// connection, and say why that is over: with an `Ended` if it is known.
pub async fn ended(task: &mut tokio::task::JoinHandle<Result<()>>) -> anyhow::Error {
    match task.await.context("Rex control worker stopped") {
        Ok(Ok(())) => anyhow!(ENDED),
        Ok(Err(error)) | Err(error) => error,
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Control {
    /// Connect to an endpoint, at the address that answered `last` time if it is one
    /// of its own, and as the client a `token` was given to if the server knows it.
    pub async fn connect(
        endpoint: &str,
        token: Option<&str>,
        last: Option<&Target>,
    ) -> Result<Self> {
        let (mut socket, target) = reach(endpoint, last, "/api/control", CONTROL_PROTOCOL).await?;
        // This machine's name tells this client from the same on another.
        #[cfg(unix)]
        let machine = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
        #[cfg(not(unix))]
        let machine = std::env::var("COMPUTERNAME").unwrap_or_default();
        let mut hello = json!({
            "type": "hello",
            "api_revision": REVISIONS.end(),
            "api_min_revision": REVISIONS.start(),
            "client": {
                "kind": "app",
                "platform": "linux",
                "name": "Rexy",
                "version": env!("CARGO_PKG_VERSION"),
                "label": match machine.trim() {
                    "" => "Rexy",
                    machine => machine,
                },
            },
        });
        if let Some(token) = token {
            hello["reconnect_token"] = json!(token);
        }
        send_with_deadline(&mut socket, Message::Text(hello.to_string().into()), STEP_TIMEOUT)
            .await?;
        // A server that cannot serve this client closes instead, and says why.
        let welcome = tokio::time::timeout(STEP_TIMEOUT, text(&mut socket)).await;
        let welcome = welcome.context("The server did not answer this client's hello")??;
        ensure!(
            welcome["type"] == "welcome" && welcome["protocol"] == CONTROL_PROTOCOL,
            "Invalid Rex control welcome"
        );
        // A server from before revisions were agreed on welcomes whoever comes. It
        // is told of in the words of one that refuses.
        let revision = |field: &str| welcome[field].as_u64().unwrap_or(0);
        if !REVISIONS.contains(&revision("api_revision")) {
            let why = format!(
                "server speaks {} to {}, client needs {} to {}; update the server",
                revision("api_min_revision"),
                revision("api_server_revision"),
                REVISIONS.start(),
                REVISIONS.end(),
            );
            return Err(Ended::Incompatible(why).into());
        }
        let client_id =
            welcome["client"]["id"].as_str().context("Missing Rex client identity")?.to_owned();
        let reconnect_token = welcome["client"]["reconnect_token"].as_str().map(str::to_owned);
        let max_message = welcome["limits"]["max_message_bytes"].as_u64().unwrap_or(65536) as usize;
        let (requests, receiver) = mpsc::channel(MAX_IN_FLIGHT);
        let (sender, events) = mpsc::unbounded_channel();
        let closing = Arc::new(Notify::new());
        let mut clock =
            tokio::time::interval_at(tokio::time::Instant::now() + PING_EVERY, PING_EVERY);
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let ticks = stream::poll_fn(move |cx| clock.poll_tick(cx).map(|_| Some(())));
        let task = tokio::spawn(control_loop(
            socket,
            receiver,
            sender,
            max_message,
            closing.clone(),
            ticks,
        ));
        Ok(Self {
            rpc: Rpc { requests },
            client_id,
            reconnect_token,
            resumed: welcome["client"]["resumed"] == true,
            target,
            events,
            task,
            closing,
        })
    }

    /// Say goodbye, so that the server lets this client go at once and does not keep
    /// its place for a minute. Waits a moment for the word to leave, and never long:
    /// the link may be dead.
    pub async fn close(&mut self) {
        self.closing.notify_one();
        // A task that has ended may have been asked for its result already.
        if !self.task.is_finished() {
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, &mut self.task).await;
        }
    }
}

/// What the server says has changed, of the things this client keeps up with.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// There is a new session, last of them all, with this name.
    Created {
        session: String,
        name: String,
    },
    Destroyed {
        session: String,
    },
    /// A session has moved to just before another, or to the end.
    Moved {
        session: String,
        before: Option<String>,
    },
    /// What a session's view shows is at a new revision. Its name, its tabs, their
    /// panes and who owns each are all in the view. The server sends news of each of
    /// those as well, and one of these with it, which is the one to go by: the view
    /// that comes of it is whole, and its revision says whether it is already held.
    Viewed {
        session: String,
        revision: u64,
    },
    /// The host list has changed, to the one with this hash.
    Hosts {
        hash: String,
    },
    /// News of one terminal.
    Block {
        block: String,
        news: News,
    },
    /// The configuration that keymaps come of has been read again, and has this hash.
    Keymap {
        hash: String,
    },
    /// This client is asked to do one of its own actions.
    Asked(Asked),
    /// An action of the server's configuration, which this client began, has
    /// a call for it to make on another server.
    Call(Call),
    /// An action of the server's has a question for the user.
    Question(Question),
    /// A question is over without an answer from here: another client gave one,
    /// or nobody did in time.
    Settled {
        call: String,
    },
}

#[derive(Debug, PartialEq)]
pub enum News {
    /// Its title.
    Title(String),
    /// What is in its foreground.
    Program(Running),
    /// What its programs say they are doing: each record of it that has changed, by
    /// its name and whole, or `None` for one that is no more.
    Status(Vec<(String, Option<Value>)>),
    /// How far along a program in it says it is, in hundredths, or that it no
    /// longer says.
    Progress(Option<u8>),
    /// A program in it has copied text: all of it, or if it is long the `id` to
    /// ask for it by.
    Copied { text: Option<String>, id: u64 },
    /// A program in it has rung the bell.
    Bell,
    /// One has a notice for the desktop. Either part of it may be empty.
    Notice { title: String, body: String },
}

/// The process in a terminal's foreground, as far as the server says.
#[derive(Debug, Default, PartialEq)]
pub struct Running {
    pub pid: Option<u64>,
    /// Its icon, if any of its names has one.
    pub program: Option<Program>,
    /// What it goes by, and the directory it is in.
    pub name: String,
    pub folder: Option<String>,
}

/// A process by its names. The most telling come first: the script it runs, what
/// it was started as, and its executable, which may be a file named for its
/// version. The first is what it goes by, without the directories that lead to it.
pub fn running(process: &Value) -> Running {
    let named = ["script", "argv0", "name"].map(|key| process[key].as_str());
    let named = named.into_iter().flatten();
    Running {
        pid: process["pid"].as_u64(),
        program: Program::named(named.clone()),
        name: named.clone().next().map(program::base).unwrap_or_default().into(),
        folder: process["cwd"].as_str().filter(|at| !at.is_empty()).map(str::to_owned),
    }
}

/// The process a terminal started, if that is a shell. While it is also the one in
/// the foreground it is at its prompt, and closing the terminal ends nothing else.
pub fn shell(child: &Value) -> Option<u64> {
    const SHELLS: [&str; 13] = [
        "sh", "bash", "zsh", "fish", "dash", "ksh", "mksh", "tcsh", "csh", "nu", "pwsh", "elvish",
        "xonsh",
    ];
    let name = child["name"].as_str()?;
    child["pid"].as_u64().filter(|_| SHELLS.contains(&name))
}

fn event(message: &Value) -> Option<Event> {
    let event = &message["payload"];
    let (session, event) = if event["type"] == "session_event" {
        let within = &event["payload"];
        (within["session_id"].as_str(), &within["event"])
    } else {
        (None, event)
    };
    let fields = &event["payload"];
    let text = |value: &Value| value.as_str().map(str::to_owned);
    Some(match event["type"].as_str()? {
        "session_created" => Event::Created {
            session: text(&fields["session"]["session_id"])?,
            name: text(&fields["session"]["label"]).unwrap_or_default(),
        },
        "session_destroyed" => Event::Destroyed { session: text(&fields["session_id"])? },
        "session_moved" => Event::Moved {
            session: text(&fields["session_id"])?,
            before: text(&fields["before_session_id"]),
        },
        "session_view_changed" => {
            Event::Viewed { session: session?.to_owned(), revision: fields["revision"].as_u64()? }
        }
        "hosts_changed" => Event::Hosts { hash: text(&fields["hash"])? },
        "block_event" => Event::Block {
            block: text(&fields["block_id"])?,
            news: news(fields["name"].as_str()?, &fields["payload"])?,
        },
        "keymap_changed" => Event::Keymap { hash: text(&fields["hash"])? },
        "client_perform_requested" => Event::Asked(Asked {
            call: Some(text(&fields["call_id"])?),
            name: text(&fields["action"])?,
            args: fields["args"].clone(),
            session: text(&fields["session_id"]),
            pane: text(&fields["block_id"]),
        }),
        "client_call_requested" => Event::Call(Call {
            call: text(&fields["call_id"])?,
            server: text(&fields["server"])?,
            method: text(&fields["method"])?,
            args: fields["args"].clone(),
        }),
        "client_prompt_requested" => {
            Event::Question(question(text(&fields["call_id"])?, &fields["prompt"]))
        }
        "client_prompt_ended" => Event::Settled { call: text(&fields["call_id"])? },
        _ => return None,
    })
}

/// A question as the server puts it. What it offers is taken as far as it is
/// understood: a choice with no name has its `id` for one.
fn question(call: String, prompt: &Value) -> Question {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let choice = |item: &Value| {
        let id = text(&item["id"]);
        Choice {
            title: item["title"].as_str().unwrap_or(&id).to_owned(),
            under: text(&item["subtitle"]),
            detail: text(&item["detail"]),
            chosen: item["selected"] == true,
            icon: text(&item["icon"]),
            id,
        }
    };
    Question {
        call,
        title: text(&prompt["title"]),
        hint: text(&prompt["placeholder"]),
        value: text(&prompt["value"]),
        confirm: prompt["confirm"].as_str().map(str::to_owned),
        choices: (prompt["kind"] == "pick")
            .then(|| prompt["items"].as_array().into_iter().flatten().map(choice).collect()),
        several: prompt["multiple"] == true,
        other: prompt["allow_other"] == true,
    }
}

/// How far along something is said to be, in hundredths.
pub fn percent(value: &Value) -> Option<u8> {
    value.as_u64().map(|percent| percent.min(100) as u8)
}

/// News of a terminal, by the name its creator gives that kind of news.
fn news(name: &str, fields: &Value) -> Option<News> {
    let text = |value: &Value| value.as_str().map(str::to_owned);
    Some(match name {
        "title_changed" => News::Title(text(&fields["title"])?),
        "process_changed" => News::Program(running(&fields["foreground"])),
        "program_status_changed" => {
            News::Status(vec![(text(&fields["id"])?, Some(fields["record"].clone()))])
        }
        "program_status_removed" => {
            let ids = fields["ids"].as_array()?.iter();
            News::Status(ids.filter_map(|id| Some((text(id)?, None))).collect())
        }
        // Work with no end in sight has nothing to count.
        "progress_report" => News::Progress(match fields["state"].as_str()? {
            "set" | "pause" | "error" => percent(&fields["progress"]),
            _ => None,
        }),
        "clipboard_written" => News::Copied {
            text: text(&fields["text"]),
            id: fields["id"].as_u64().unwrap_or_default(),
        },
        "bell" => News::Bell,
        "desktop_notification" => News::Notice {
            title: text(&fields["title"]).unwrap_or_default(),
            body: text(&fields["body"]).unwrap_or_default(),
        },
        _ => return None,
    })
}

fn response_result(message: &Value) -> Answer {
    let error = &message["error"];
    if error.is_object() {
        let text = |key: &str| error[key].as_str().unwrap_or_default().to_owned();
        return Err(Failed::Refused { code: text("code"), message: text("message") });
    }
    Ok(message.get("payload").cloned().unwrap_or_else(|| json!({})))
}

/// A request on its way, and the look it was sent before.
struct Pending {
    sent: u64,
    method: String,
    reply: Reply,
}

/// Tell the server this client is leaving, so that it does not hold the client's place.
async fn goodbye(socket: &mut Socket) -> Result<()> {
    let frame = CloseFrame { code: CloseCode::Normal, reason: "".into() };
    if socket.close(Some(frame)).await.is_ok() {
        // Written is not yet sent: a tunnel that ends with this client may still be
        // holding it. The server's own close, or the link's end, says that it came.
        while let Some(Ok(message)) = socket.next().await {
            if message.is_close() {
                break;
            }
        }
    }
    Ok(())
}

/// Carry requests and their answers until the connection ends. Each of `ticks` is a
/// look over the link and over the requests still waiting.
async fn control_loop(
    mut socket: Socket,
    mut outgoing: mpsc::Receiver<Request>,
    events: mpsc::UnboundedSender<Event>,
    max_message: usize,
    closing: Arc<Notify>,
    mut ticks: impl Stream<Item = ()> + Unpin,
) -> Result<()> {
    let mut pending: HashMap<String, Pending> = HashMap::new();
    let (mut serial, mut tick) = (0_u64, 0_u64);
    // Whether the server has been heard since the last look, and pinged since last heard.
    let (mut heard, mut pinged) = (false, false);
    loop {
        tokio::select! {
            // In this order, so that a look never finds silent a server whose words
            // were only waiting to be read.
            biased;
            _ = closing.notified() => return goodbye(&mut socket).await,
            request = outgoing.recv(), if pending.len() < MAX_IN_FLIGHT => {
                // `Control` holds a way to ask for as long as it lasts, and stops
                // this task as it goes.
                let Some(Request { method, payload, reply }) = request else {
                    return Ok(());
                };
                serial += 1;
                let id = serial.to_string();
                let text =
                    json!({"type": "request", "id": id, "method": method, "payload": payload})
                        .to_string();
                // Sent, it would have the server close the connection.
                if text.len() > max_message {
                    let _ = reply.send(Err(Failed::TooLarge(max_message)));
                    continue;
                }
                send_with_deadline(&mut socket, Message::Text(text.into()), STEP_TIMEOUT)
                    .await?;
                pending.insert(id, Pending { sent: tick, method, reply });
            }
            incoming = message(&mut socket, false) => {
                heard = true;
                let Message::Text(text) = incoming? else { continue };
                let message: Value = serde_json::from_str(&text)?;
                if message["type"] == "response" {
                    let reply = message["id"].as_str().and_then(|id| pending.remove(id));
                    if let Some(Pending { reply, .. }) = reply {
                        let _ = reply.send(response_result(&message));
                    }
                } else if message["type"] == "event"
                    && let Some(event) = event(&message)
                {
                    let _ = events.send(event);
                }
            }
            Some(()) = ticks.next() => {
                tick += 1;
                // A request the server does not answer fails alone, and is not asked
                // again here. The link is judged by its pings. An answer that comes
                // after all is to a number no longer known, and is passed over. An
                // action is never late: the server ends one that runs too long, and
                // one that asks the user something is answered when the user is done.
                let late = |request: &Pending| {
                    tick - request.sent >= REQUEST_TICKS && request.method != "client.action"
                };
                for (_, late) in pending.extract_if(|_, request| late(request)) {
                    let _ = late.reply.send(Err(Failed::Unanswered(late.method)));
                }
                if std::mem::take(&mut heard) {
                    pinged = false;
                } else if pinged {
                    return Err(Ended::Silent.into());
                } else {
                    let ping = Message::Ping(Default::default());
                    send_with_deadline(&mut socket, ping, STEP_TIMEOUT).await?;
                    pinged = true;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/wire.rs"]
mod tests;
