"""Model role smoke tests."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    result = h.run("model", "list")
    assert "google/gemini-3-flash-preview" in result.stdout

    h.run("model", "get", "default")
    h.run("model", "get", "fast")

    default_config = h.run("model", "config", "default")
    assert "thinking_level" in default_config.stdout
    assert "max_output_tokens" in default_config.stdout
    assert "temperature" not in default_config.stdout

    h.run("model", "set", "default", "google/gemini-3-flash-preview")
    h.run("model", "get", "default")

    h.run("model", "set", "fast", "google/gemini-3.1-pro-preview")
    h.run("model", "set", "lite", "google/gemini-3.1-flash-lite-preview")

    result = h.run("model", "list")
    assert "fast        google/gemini-3.1-pro-preview" in result.stdout
    assert "lite        google/gemini-3.1-flash-lite-preview" in result.stdout

    default_config = h.run("model", "config", "default")
    assert "[minimal, low, medium, high]" in default_config.stdout
    assert "(default: high)" in default_config.stdout

    fast_config = h.run("model", "config", "fast")
    assert "[low, medium, high]" in fast_config.stdout
    assert "(default: high)" in fast_config.stdout

    lite_config = h.run("model", "config", "lite")
    assert "[minimal, low, medium, high]" in lite_config.stdout
    assert "(default: minimal)" in lite_config.stdout

    h.run("model", "set", "default", "google/gemini-3.1-pro-preview")
    h.run("model", "config", "default")
    h.run("model", "set", "default", "google/gemini-3-flash-preview")

    h.run_err("model", "set", "default", "fake/nonexistent")
    h.run_err("model", "set", "default", "invalid-no-slash")
    h.run_err("model", "set", "default", "google/nonexistent-model")
