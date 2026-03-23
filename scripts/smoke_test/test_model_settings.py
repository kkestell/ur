"""Extension setting smoke tests."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("role", "set", "default", "google/gemini-3-flash-preview")
    h.run("role", "set", "fast", "google/gemini-3.1-pro-preview")
    h.run("role", "set", "lite", "google/gemini-3.1-flash-lite-preview")

    # Set settings via extension config set with dotted keys
    h.run(
        "extension", "config", "llm-google", "set",
        "gemini-3-flash-preview.thinking_level", "minimal",
    )
    h.run(
        "extension", "config", "llm-google", "set",
        "gemini-3.1-pro-preview.thinking_level", "low",
    )
    h.run(
        "extension", "config", "llm-google", "set",
        "gemini-3.1-flash-lite-preview.thinking_level", "minimal",
    )
    h.run(
        "extension", "config", "llm-google", "set",
        "gemini-3.1-pro-preview.max_output_tokens", "4096",
    )
    h.run(
        "extension", "config", "llm-google", "set",
        "gemini-3-flash-preview.max_output_tokens", "2048",
    )

    # List settings
    config_output = h.run("extension", "config", "llm-google", "list")
    assert "thinking_level" in config_output.stdout
    assert "max_output_tokens" in config_output.stdout

    # Error cases
    h.run_err(
        "extension", "config", "llm-google", "set",
        "nonexistent_key", "42",
    )
    h.run_err(
        "extension", "config", "llm-google", "set",
        "gemini-3-flash-preview.thinking_level", "ultra",
    )
    h.run_err(
        "extension", "config", "llm-google", "set",
        "gemini-3.1-pro-preview.thinking_level", "minimal",
    )
    h.run_err(
        "extension", "config", "llm-google", "set",
        "gemini-3-flash-preview.max_output_tokens", "0",
    )

    # Readonly rejection
    h.run_err(
        "extension", "config", "llm-google", "set",
        "gemini-3-flash-preview.context_window_in", "500000",
    )

    # Verify config.toml contains dotted keys
    config_text = h.config_path.read_text(encoding="utf-8")
    assert 'thinking_level' in config_text
    assert "max_output_tokens" in config_text
    h.cat(h.config_path)
