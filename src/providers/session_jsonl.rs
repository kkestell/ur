//! JSONL-based session persistence provider.
//!
//! Each session is stored as a `.jsonl` file in the sessions directory,
//! with one JSON object per line representing a `SessionEvent`.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::debug;

use super::SessionProvider;
use crate::types::{SessionEvent, SessionInfo};

/// JSONL file-based session provider.
#[derive(Debug)]
pub struct JsonlSessionProvider {
    sessions_dir: PathBuf,
}

impl JsonlSessionProvider {
    pub fn new(sessions_dir: impl Into<PathBuf>) -> Self {
        Self {
            sessions_dir: sessions_dir.into(),
        }
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{session_id}.jsonl"))
    }
}

impl SessionProvider for JsonlSessionProvider {
    fn load_session(&self, session_id: &str) -> Result<Vec<SessionEvent>> {
        let path = self.session_path(session_id);
        match std::fs::File::open(&path) {
            Ok(file) => {
                let reader = std::io::BufReader::new(file);
                let mut events = Vec::new();
                for (i, line) in reader.lines().enumerate() {
                    let line = line
                        .with_context(|| format!("reading line {} of {}", i + 1, path.display()))?;
                    if line.trim().is_empty() {
                        continue;
                    }
                    let event: SessionEvent = serde_json::from_str(&line)
                        .with_context(|| format!("parsing line {} of {}", i + 1, path.display()))?;
                    events.push(event);
                }
                debug!(
                    session_id,
                    events = events.len(),
                    "loaded session from JSONL"
                );
                Ok(events)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(session_id, "no existing session file");
                Ok(Vec::new())
            }
            Err(e) => Err(e).with_context(|| format!("opening {}", path.display())),
        }
    }

    fn append_session(&self, session_id: &str, event: &SessionEvent) -> Result<()> {
        let path = self.session_path(session_id);
        std::fs::create_dir_all(&self.sessions_dir)
            .with_context(|| format!("creating {}", self.sessions_dir.display()))?;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {} for append", path.display()))?;

        let json = serde_json::to_string(event)?;
        writeln!(file, "{json}").with_context(|| format!("writing to {}", path.display()))?;
        Ok(())
    }

    fn replace_session(&self, session_id: &str, events: &[SessionEvent]) -> Result<()> {
        let path = self.session_path(session_id);
        std::fs::create_dir_all(&self.sessions_dir)
            .with_context(|| format!("creating {}", self.sessions_dir.display()))?;

        let mut file = std::fs::File::create(&path)
            .with_context(|| format!("creating {} for replace", path.display()))?;

        for event in events {
            let json = serde_json::to_string(event)?;
            writeln!(file, "{json}").with_context(|| format!("writing to {}", path.display()))?;
        }
        debug!(
            session_id,
            events = events.len(),
            "replaced session in JSONL"
        );
        Ok(())
    }

    fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let dir = &self.sessions_dir;
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                let id = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let metadata = std::fs::metadata(&path)?;
                let created_at = metadata
                    .created()
                    .or_else(|_| metadata.modified())
                    .map(|t| humantime::format_rfc3339_seconds(t).to_string())
                    .unwrap_or_default();
                let message_count = count_lines(&path).unwrap_or(0);
                sessions.push(SessionInfo {
                    id,
                    created_at,
                    message_count,
                });
            }
        }
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(sessions)
    }
}

fn count_lines(path: &Path) -> Result<u32> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    #[expect(clippy::cast_possible_truncation, reason = "sessions won't exceed u32")]
    let count = reader.lines().count() as u32;
    Ok(count)
}
