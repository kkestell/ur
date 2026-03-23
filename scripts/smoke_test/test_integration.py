"""Real API integration smoke test."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness

PARIS_WEATHER_PROMPT = "What is the weather in Paris, and should I wear a coat?"


def run(h: SmokeHarness) -> None:
    if not h.getenv("GOOGLE_API_KEY"):
        print("Skipping integration test: GOOGLE_API_KEY is not set.")
        return

    h.run("extensions", "enable", "test-extension")
    h.run("model", "set", "default", "google/gemini-3-flash-preview")

    try:
        result = h.run(
            "run",
            env={"UR_RUN_USER_MESSAGE": PARIS_WEATHER_PROMPT},
        )
        assert "LLM returned tool call: get_weather(" in result.stdout
        assert "tool result:" in result.stdout
        assert "calling LLM streaming (3 messages, includes tool result)" in result.stdout
        assert "Paris" in result.stdout
        assert "coat" in result.stdout.lower()
    finally:
        h.run("extensions", "disable", "test-extension")
