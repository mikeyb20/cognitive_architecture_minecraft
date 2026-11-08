//! Replay cache (spec §23): answers stored by prompt hash, so a run can be
//! replayed exactly and without spending. One JSON line per answer.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{LlmError, Prompt, Provider, Reply};

/// Record calls the provider on a miss and stores the answer; Replay
/// never calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    Record,
    Replay,
}

#[derive(Serialize, Deserialize)]
struct Line {
    key: String,
    reply: Reply,
}

/// FNV-1a over the provider id and every part of the prompt. Stable across
/// builds and platforms, unlike `std`'s hasher.
pub fn prompt_key(provider: &str, p: &Prompt) -> u64 {
    let max = p.max_tokens.to_string();
    let parts = [provider, &p.system, &p.user, &max];
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        // A separator byte that never appears in UTF-8 text.
        for b in part.bytes().chain([0xff]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

pub struct Cached<P> {
    inner: P,
    mode: CacheMode,
    file: Option<PathBuf>,
    entries: BTreeMap<u64, Reply>,
}

impl<P: Provider> Cached<P> {
    /// A cache held in memory only.
    pub fn in_memory(inner: P, mode: CacheMode) -> Self {
        Self {
            inner,
            mode,
            file: None,
            entries: BTreeMap::new(),
        }
    }

    /// A cache kept in `file`, loading what is already there. Lines that
    /// do not parse (a run killed mid-write) are skipped.
    pub fn open(inner: P, mode: CacheMode, file: PathBuf) -> std::io::Result<Self> {
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let entries = text
            .lines()
            .filter_map(|l| serde_json::from_str::<Line>(l).ok())
            .filter_map(|l| Some((u64::from_str_radix(&l.key, 16).ok()?, l.reply)))
            .collect();
        Ok(Self {
            inner,
            mode,
            file: Some(file),
            entries,
        })
    }

    /// Answers held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn store(&mut self, key: u64, reply: &Reply) -> Result<(), LlmError> {
        self.entries.insert(key, reply.clone());
        let Some(path) = &self.file else {
            return Ok(());
        };
        let line = Line {
            key: format!("{key:016x}"),
            reply: reply.clone(),
        };
        let json = serde_json::to_string(&line).map_err(|e| LlmError::Parse(e.to_string()))?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| writeln!(f, "{json}"))
            .map_err(|e| LlmError::Transport(e.to_string()))
    }
}

impl<P: Provider> Provider for Cached<P> {
    fn id(&self) -> String {
        self.inner.id()
    }

    fn complete(&mut self, prompt: &Prompt) -> Result<Reply, LlmError> {
        let key = prompt_key(&self.inner.id(), prompt);
        if let Some(r) = self.entries.get(&key) {
            return Ok(Reply {
                cached: true,
                ..r.clone()
            });
        }
        if self.mode == CacheMode::Replay {
            return Err(LlmError::NotCached);
        }
        let reply = self.inner.complete(prompt)?;
        self.store(key, &reply)?;
        Ok(reply)
    }
}
