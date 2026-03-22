"""Model role smoke tests."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("model", "list")
    h.run("model", "get", "default")
    h.run("model", "get", "fast")
    h.run("model", "config", "default")

    h.run("model", "set", "default", "google/gemini-3-flash-preview")
    h.run("model", "get", "default")

    h.run("model", "set", "fast", "google/gemini-3-pro-preview")
    h.run("model", "list")
    h.run("model", "config", "default")
    h.run("model", "config", "fast")

    h.run("model", "set", "default", "google/gemini-3-pro-preview")
    h.run("model", "config", "default")
    h.run("model", "set", "default", "google/gemini-3-flash-preview")

    h.run_err("model", "set", "default", "fake/nonexistent")
    h.run_err("model", "set", "default", "invalid-no-slash")
    h.run_err("model", "set", "default", "google/nonexistent-model")
