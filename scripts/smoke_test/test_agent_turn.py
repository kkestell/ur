"""Deterministic agent turn smoke test."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("extensions", "enable", "test-extension")
    h.run("extensions", "enable", "llm-test")
    h.run("model", "set", "default", "test/echo")

    try:
        result = h.run("run")
        assert "LLM returned tool call: get_weather(" in result.stdout
        assert "tool result:" in result.stdout
        assert "calling LLM streaming (3 messages, includes tool result)" in result.stdout
        assert "Tool result received:" in result.stdout
    finally:
        h.run("extensions", "disable", "llm-test")
        h.run("extensions", "disable", "test-extension")
        h.run("model", "set", "default", "google/gemini-3-flash-preview")
