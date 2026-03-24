"""Deterministic agent turn smoke test."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("extension", "enable", "test-extension")
    h.run("extension", "enable", "llm-test")
    h.run("role", "set", "default", "test/echo")

    try:
        h.run("-v", "run")
    finally:
        h.run("extension", "disable", "llm-test")
        h.run("extension", "disable", "test-extension")
        h.run("role", "set", "default", "google/gemini-3-flash-preview")
