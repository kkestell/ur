//! Shared plumbing for ur providers that speak the OpenAI Chat Completions
//! dialect (OpenAI, DeepSeek, OpenRouter).
//!
//! This crate factors out the parts of those providers that are identical or
//! differ only in narrow, well-isolated ways: API-key and user validation
//! ([`keys`]), request-body encode helpers ([`request`]), SSE line framing and
//! completion folding ([`sse`]), and the HTTP retry/stream state machine
//! ([`executor`]). Each provider supplies the genuine deltas — headers, status
//! tables, reasoning encoding, and its own `Delta` / usage wire shapes — via
//! the [`executor::Dialect`] trait and the per-provider `decode_chunk`.

pub mod executor;
pub mod keys;
pub mod request;
pub mod sse;
