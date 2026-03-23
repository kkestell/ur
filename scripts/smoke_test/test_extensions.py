"""Extension management smoke tests."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("extension", "list")
    h.run("extension", "inspect", "session-jsonl")
    h.run("extension", "inspect", "llm-google")
    h.run("extension", "inspect", "compaction-llm")
    h.run_err("extension", "inspect", "nonexistent")

    # llm-provider is AtLeastOne, so disable one of the two enabled providers.
    h.run("extension", "disable", "llm-google")
    # Now only llm-openrouter remains — disabling it should fail.
    h.run_err("extension", "disable", "llm-openrouter")
    # Re-enable google for the rest of the suite.
    h.run("extension", "enable", "llm-google")

    h.run_err("extension", "disable", "compaction-llm")
    h.run_err("extension", "disable", "session-jsonl")
    h.run_err("extension", "enable", "nonexistent")
    h.run_err("extension", "disable", "nonexistent")

    h.run("extension", "inspect", "test-extension")
    h.run("extension", "enable", "test-extension")
    h.run_err("extension", "enable", "test-extension")

    h.run("extension", "enable", "llm-test")
    h.run("extension", "list")
    h.run("extension", "disable", "llm-test")

    h.run("extension", "disable", "test-extension")
    h.run("extension", "list")
