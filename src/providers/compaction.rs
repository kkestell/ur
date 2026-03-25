//! Message compaction provider.
//!
//! Currently a stub that returns messages unchanged. Will later use
//! an LLM provider for actual summarization-based compaction.

use anyhow::Result;

use super::CompactionProvider;
use crate::types::Message;

/// Stub compaction provider that passes messages through unchanged.
#[derive(Debug)]
pub struct StubCompactionProvider;

impl CompactionProvider for StubCompactionProvider {
    fn compact(&self, messages: &[Message]) -> Result<Vec<Message>> {
        Ok(messages.to_vec())
    }
}
