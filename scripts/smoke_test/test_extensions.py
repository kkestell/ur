"""Extension management smoke tests."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    h.run("extensions", "list")
    h.run("extensions", "inspect", "session-jsonl")
    h.run("extensions", "inspect", "llm-google")
    h.run("extensions", "inspect", "compaction-llm")
    h.run_err("extensions", "inspect", "nonexistent")

    # llm-provider is AtLeastOne, so disable one of the two enabled providers.
    h.run("extensions", "disable", "llm-google")
    # Now only llm-openrouter remains — disabling it should fail.
    h.run_err("extensions", "disable", "llm-openrouter")
    # Re-enable google for the rest of the suite.
    h.run("extensions", "enable", "llm-google")

    h.run_err("extensions", "disable", "compaction-llm")
    h.run_err("extensions", "disable", "session-jsonl")
    h.run_err("extensions", "enable", "nonexistent")
    h.run_err("extensions", "disable", "nonexistent")

    h.run("extensions", "inspect", "test-extension")
    h.run("extensions", "enable", "test-extension")
    h.run_err("extensions", "enable", "test-extension")

    h.run("extensions", "enable", "llm-test")
    h.run("extensions", "list")
    h.run("extensions", "disable", "llm-test")

    h.run("extensions", "disable", "test-extension")
    h.run("extensions", "list")
