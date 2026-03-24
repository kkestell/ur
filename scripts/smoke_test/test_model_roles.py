"""Model role smoke tests."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.section("list and get roles")
    result = h.run("role", "list")
    assert "google/gemini-3-flash-preview" in result.stdout

    h.run("role", "get", "default")
    h.run("role", "get", "fast")

    h.section("extension config list")
    config_list = h.run("extension", "config", "llm-google", "list")
    assert "thinking_level" in config_list.stdout
    assert "max_output_tokens" in config_list.stdout
    assert "context_window_in" in config_list.stdout
    assert "(readonly)" in config_list.stdout

    h.section("set and verify roles")
    h.run("role", "set", "default", "google/gemini-3-flash-preview")
    h.run("role", "get", "default")

    h.run("role", "set", "fast", "google/gemini-3.1-pro-preview")
    h.run("role", "set", "lite", "google/gemini-3.1-flash-lite-preview")

    result = h.run("role", "list")
    assert "fast        google/gemini-3.1-pro-preview" in result.stdout
    assert "lite        google/gemini-3.1-flash-lite-preview" in result.stdout

    h.run("role", "set", "default", "google/gemini-3.1-pro-preview")
    h.run("role", "set", "default", "google/gemini-3-flash-preview")

    h.section("expected errors: invalid role targets")
    h.run_err("role", "set", "default", "fake/nonexistent")
    h.run_err("role", "set", "default", "invalid-no-slash")
    h.run_err("role", "set", "default", "google/nonexistent-model")

    h.section("extension config get (readonly metadata)")
    result = h.run(
        "extension", "config", "llm-google", "get",
        "gemini-3-flash-preview.context_window_in",
    )
    assert "1048576" in result.stdout

    result = h.run(
        "extension", "config", "llm-google", "get",
        "gemini-3-flash-preview.knowledge_cutoff",
    )
    assert "2025-01" in result.stdout

    result = h.run(
        "extension", "config", "llm-google", "get",
        "gemini-3-flash-preview.context_window_out",
    )
    assert "65536" in result.stdout

    result = h.run(
        "extension", "config", "llm-google", "get",
        "gemini-3-flash-preview.cost_in",
    )
    assert "500" in result.stdout

    h.run_err(
        "extension", "config", "llm-google", "get", "nonexistent",
    )
