"""Provider setting smoke tests."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("model", "setting", "default", "temperature", "150")
    h.run("model", "setting", "fast", "max_output_tokens", "4096")
    h.run("model", "setting", "default", "max_output_tokens", "2048")
    h.run("model", "setting", "default", "temperature", "0")
    h.run("model", "setting", "default", "temperature", "200")
    h.run("model", "config", "default")

    h.run_err("model", "setting", "default", "nonexistent_key", "42")
    h.run_err("model", "setting", "default", "temperature", "999")
    h.run_err("model", "setting", "default", "temperature", "abc")

    h.cat(h.config_path)
