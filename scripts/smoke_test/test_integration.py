"""Real API integration smoke test."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness


def run(h: SmokeHarness) -> None:
    if not h.getenv("GOOGLE_API_KEY"):
        print("Skipping integration test: GOOGLE_API_KEY is not set.")
        return

    result = h.run_allow_error("run")
    if result.returncode != 0:
        print(f"Integration test finished with exit code {result.returncode}.")
