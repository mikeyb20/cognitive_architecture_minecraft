// A one-shot HTTP server on localhost: answers each request with the next
// canned response and hands back what it was sent.
#![allow(dead_code, reason = "each test file uses a different subset")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, channel};
use std::thread;

/// What the client sent: request line, lower-cased headers, body.
pub struct Seen {
    pub line: String,
    pub headers: Vec<(String, String)>,
    pub body: serde_json::Value,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Serve `replies` (status, body) in order; returns the base URL.
pub fn serve(replies: Vec<(u16, String)>) -> (String, Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = channel();
    thread::spawn(move || {
        for (status, body) in replies {
            let (stream, _) = listener.accept().unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut headers = Vec::new();
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap();
                headers.push((k.to_lowercase(), v.trim().to_string()));
            }
            let len: usize = headers
                .iter()
                .find(|(k, _)| k == "content-length")
                .map_or(0, |(_, v)| v.parse().unwrap());
            let mut buf = vec![0; len];
            r.read_exact(&mut buf).unwrap();
            let sent = serde_json::from_slice(&buf).unwrap_or(serde_json::Value::Null);
            let mut w = stream;
            write!(
                w,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            tx.send(Seen {
                line: line.trim_end().into(),
                headers,
                body: sent,
            })
            .unwrap();
        }
    });
    (url, rx)
}

pub fn prompt(user: &str) -> llm::Prompt {
    llm::Prompt {
        system: "You pick one action.".into(),
        user: user.into(),
        max_tokens: 2000,
    }
}
