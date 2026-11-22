//! Files sent to a pane's machine. Each is reserved with the server and its
//! bytes go over a data connection of their own, and once all of them have come
//! the terminal types the paths they have there, as a paste.
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, future::try_join_all};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use crate::wire::{self, Ended, Rpc, Target};

const PROTOCOL: &str = "rex.data.bytes.v1";
const RESERVE: &str = "com.superlogical.terminal.upload_begin";
/// How much of a file goes in one message.
const PART: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub enum File {
    /// One on this machine.
    Path(PathBuf),
    /// What was pasted, under the name it is to have.
    Pasted(&'static str, Arc<[u8]>),
}

/// A file's name, how long it is, and what reads it.
type Opened = (String, u64, Box<dyn Read + Send>);

impl File {
    /// Where it is on this machine, if it is a file here.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Pasted(..) => None,
        }
    }

    fn open(&self) -> Result<Opened> {
        match self {
            Self::Path(path) => {
                let name = path.file_name().unwrap_or(path.as_os_str());
                let name = name.to_string_lossy().into_owned();
                // Windows will not open a directory as a file, and says only that.
                #[cfg(windows)]
                ensure!(!path.is_dir(), "{name} is not a file, and only files are sent");
                let file = std::fs::File::open(path);
                let file = file.with_context(|| format!("{name} could not be read"))?;
                let about = file.metadata()?;
                ensure!(about.is_file(), "{name} is not a file, and only files are sent");
                Ok((name, about.len(), Box::new(file)))
            }
            Self::Pasted(name, bytes) => {
                Ok(((*name).to_owned(), bytes.len() as u64, Box::new(Cursor::new(bytes.clone()))))
            }
        }
    }
}

/// A path as a shell takes it for one word: as it is where it is plain, and
/// otherwise between quotes.
pub fn quoted(path: &str) -> String {
    let plain = |c: char| c.is_alphanumeric() || "_@%+=:,./-".contains(c);
    match !path.is_empty() && path.chars().all(plain) {
        true => path.to_owned(),
        false => format!("'{}'", path.replace('\'', r"'\''")),
    }
}

/// A new random UUID, written as Rex wants it: in lowercase, with hyphens.
fn id() -> Result<String> {
    Ok(uuid::Uuid::new_v4().hyphenated().to_string())
}

/// Send files to a pane's machine, where its terminal types their paths in
/// this order once every one has come. Nothing is typed if any does not.
pub async fn send(
    rpc: &Rpc,
    target: &Target,
    session: &str,
    pane: &str,
    files: &[File],
) -> Result<()> {
    // Before any is reserved, each is known to be one that can be sent.
    let opened = files.iter().map(File::open).collect::<Result<Vec<_>>>()?;
    let (batch, count) = (id()?, opened.len());
    let mut sends = Vec::new();
    for (index, (name, size, bytes)) in opened.into_iter().enumerate() {
        let file = format!("upload:{}", id()?);
        let args = json!({"ref": file, "name": name, "size": size});
        let mut request = json!({"session_id": session, "block_id": pane, "args": args});
        // A file on its own is no batch.
        if count > 1 {
            request["args"]["batch"] = json!(batch);
            request["args"]["count"] = json!(count);
            request["args"]["index"] = json!(index);
        }
        let reserved = rpc.call(RESERVE, request).await?;
        let ticket = reserved["ticket"].as_str().context("Missing upload ticket")?.to_owned();
        sends.push(stream(target, ticket, name, size, bytes));
    }
    // Together, so that no ticket runs out while another file is on its way.
    try_join_all(sends).await.map(drop)
}

/// One file's bytes, all of them and then an empty message. The server closes
/// the connection in the ordinary way once the batch's paths are typed.
async fn stream(
    target: &Target,
    ticket: String,
    name: String,
    size: u64,
    mut bytes: Box<dyn Read + Send>,
) -> Result<()> {
    let mut socket = wire::socket(target, "/api/data", PROTOCOL).await?;
    wire::handshake(&mut socket, &ticket).await?;
    let (mut part, mut left) = (vec![0; PART], size);
    while left > 0 {
        let most = PART.min(usize::try_from(left).unwrap_or(PART));
        let read = bytes.read(&mut part[..most])?;
        ensure!(read > 0, "{name} changed while it was being sent");
        socket.send(Message::Binary(part[..read].to_vec().into())).await?;
        left -= read as u64;
    }
    socket.send(Message::Binary(Vec::new().into())).await?;
    loop {
        let Err(error) = wire::next_binary(&mut socket).await else {
            continue;
        };
        return match error.downcast_ref() {
            Some(Ended::StreamClosed { code: 1000, .. }) => Ok(()),
            Some(Ended::StreamClosed { code, reason }) => bail!("{name}: {reason} ({code})"),
            _ => Err(error),
        };
    }
}

#[cfg(test)]
#[path = "../tests/unit/upload.rs"]
mod tests;
