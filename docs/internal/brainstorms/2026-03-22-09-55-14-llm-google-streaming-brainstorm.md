# Brainstorm: Real LLM Integration via Google Gemini

**Date:** 2026-03-22
**Status:** Complete

## What We're Building

Replace the dummy/stub LLM extension with a real Google Gemini integration that makes actual API calls. This is the first real LLM provider in the system, proving out the full extension architecture end-to-end.

**Scope:** Completion + tool use + streaming, all through the WASM extension system.

**Models:** gemini-3-pro-preview, gemini-3-flash-preview (Flash as default).

## Why This Approach

### WASI HTTP in Extensions
Extensions make their own HTTP calls via `wasi:http/outgoing-handler`. This keeps the host thin and the extension self-contained — each provider owns its full HTTP lifecycle, request formatting, response parsing, and error handling. No host-side API client code needed.

### API Key via init(config)
The host reads `.env` / environment variables and passes `GOOGLE_API_KEY` to the extension through the existing `init(config)` WIT interface. Keeps the WASM sandbox tight — no filesystem or env var access needed.

### Resource-Based Streaming
Add a WIT resource type `completion-stream` to the `llm-provider` interface with a `next()` method that returns `option<completion-chunk>`. The extension initiates the HTTP request and returns a stream handle. The host pulls chunks by calling `next()` until `None`.

**Why resource-based over alternatives:**
- Pull-based: host controls consumption pace
- Idiomatic WASM component model pattern
- Clean SRP: extension owns HTTP + parsing, host owns display
- Scales to all future providers without interface changes

### System Extension
Lives in `extensions/system/llm-google/`, enabled by default. Google is a first-class built-in provider.

## Key Decisions

1. **WASI HTTP in extensions** — extensions own their HTTP calls, not the host
2. **API key via init config** — host reads .env, passes through existing WIT init interface
3. **Resource-based streaming** — WIT resource with `next()` pull method
4. **Models: gemini-3-pro-preview + gemini-3-flash-preview** — Flash as default
5. **Full scope** — completion + tool use + streaming from day one
6. **System tier** — enabled by default

## Open Questions

- **completion-chunk type:** What fields does the chunk record need? At minimum: delta text, tool call deltas, finish reason. Needs WIT type design.
- **WASI HTTP setup:** wasmtime-wasi-http crate integration — how much host-side plumbing is needed to enable outgoing requests?
- **Error handling:** How should HTTP errors, rate limits, and malformed responses surface through the stream?
- **Existing stubs:** Should OpenAI/Anthropic stubs be updated to the streaming interface too, or left as-is for now?
- **Smoke test:** How to test the real API call in CI without a real key? Mock server? Skip?
- **Google API format:** Need to verify the exact Gemini 3.1 REST API shape for streaming + tool use.
