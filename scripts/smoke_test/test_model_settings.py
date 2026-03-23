"""Provider setting smoke tests."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("model", "set", "default", "google/gemini-3-flash-preview")
    h.run("model", "set", "fast", "google/gemini-3.1-pro-preview")
    h.run("model", "set", "lite", "google/gemini-3.1-flash-lite-preview")

    h.run("model", "setting", "default", "thinking_level", "minimal")
    h.run("model", "setting", "fast", "thinking_level", "low")
    h.run("model", "setting", "lite", "thinking_level", "minimal")
    h.run("model", "setting", "fast", "max_output_tokens", "4096")
    h.run("model", "setting", "default", "max_output_tokens", "2048")
    config_output = h.run("model", "config", "default")
    assert "thinking_level" in config_output.stdout
    assert "max_output_tokens" in config_output.stdout
    assert "temperature" not in config_output.stdout

    h.run_err("model", "setting", "default", "nonexistent_key", "42")
    h.run_err("model", "setting", "default", "temperature", "100")
    h.run_err("model", "setting", "default", "thinking_level", "ultra")
    h.run_err("model", "setting", "fast", "thinking_level", "minimal")
    h.run_err("model", "setting", "default", "max_output_tokens", "0")

    config_text = h.config_path.read_text(encoding="utf-8")
    assert 'thinking_level = "minimal"' in config_text
    assert 'thinking_level = "low"' in config_text
    assert "max_output_tokens = 2048" in config_text
    assert "max_output_tokens = 4096" in config_text
    h.cat(h.config_path)
