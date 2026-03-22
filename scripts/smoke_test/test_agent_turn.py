"""Deterministic agent turn smoke test."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("extensions", "enable", "test-extension")
    h.run("extensions", "enable", "llm-test")
    h.run("model", "set", "default", "test/echo")

    h.run("run")

    h.run("extensions", "disable", "llm-test")
    h.run("extensions", "disable", "test-extension")
    h.run("model", "set", "default", "google/gemini-3-flash-preview")
