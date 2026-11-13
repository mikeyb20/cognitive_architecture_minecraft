//! The runner's end of the bot link (spec §22): one JSON line out, one JSON
//! line back. Works over any reader and writer, so tests use a local
//! socket and the CLI uses TCP to the bot.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use host_api::link::{LinkReply, LinkRequest};

/// Why a request got no usable reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// Reading or writing failed.
    Io(String),
    /// The bot closed the connection.
    Closed,
    /// The reply was not a `LinkReply`.
    Bad(String),
    /// The bot said it could not serve the request.
    Failed(String),
    /// A valid reply of the wrong kind for the request.
    Unexpected(String),
}

pub struct Link {
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
}

impl Link {
    pub fn new(reader: impl BufRead + Send + 'static, writer: impl Write + Send + 'static) -> Self {
        Self {
            reader: Box::new(reader),
            writer: Box::new(writer),
        }
    }

    /// Connect to the bot at `addr` ("host:port").
    pub fn tcp(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self::new(BufReader::new(stream.try_clone()?), stream))
    }

    /// Send one request and read its reply. A `Failed` reply is an error.
    pub fn call(&mut self, req: &LinkRequest) -> Result<LinkReply, LinkError> {
        let io = |e: std::io::Error| LinkError::Io(e.to_string());
        let line = serde_json::to_string(req).map_err(|e| LinkError::Bad(e.to_string()))?;
        writeln!(self.writer, "{line}").map_err(io)?;
        self.writer.flush().map_err(io)?;
        let mut back = String::new();
        if self.reader.read_line(&mut back).map_err(io)? == 0 {
            return Err(LinkError::Closed);
        }
        match serde_json::from_str(&back) {
            Ok(LinkReply::Failed { message }) => Err(LinkError::Failed(message)),
            Ok(r) => Ok(r),
            Err(e) => Err(LinkError::Bad(e.to_string())),
        }
    }
}
