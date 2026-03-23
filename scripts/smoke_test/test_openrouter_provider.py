"""Live OpenRouter provider smoke test."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness

PARIS_WEATHER_PROMPT = "What is the weather in Paris, and should I wear a coat?"


def run(h: SmokeHarness) -> None:
    if not h.getenv("OPENROUTER_API_KEY"):
        print("Skipping OpenRouter provider smoke test: OPENROUTER_API_KEY is not set.")
        return

    h.run("extensions", "enable", "test-extension")
    h.run("extensions", "disable", "llm-google")

    try:
        # Verify default model resolves to openrouter.
        get_result = h.run("model", "get", "default")
        assert "openrouter/" in get_result.stdout, (
            f"expected openrouter default model, got: {get_result.stdout}"
        )

        # Show dynamic settings surface.
        h.run("model", "config", "default")

        # Run the tool-calling flow.
        result = h.run(
            "run",
            env={"UR_RUN_USER_MESSAGE": PARIS_WEATHER_PROMPT},
        )
        assert 'resolving role "default"' in result.stdout
        assert "openrouter/" in result.stdout
        assert "LLM returned tool call: get_weather(" in result.stdout
        assert "tool result:" in result.stdout
        assert "calling LLM streaming" in result.stdout
        assert "includes tool result" in result.stdout
        stdout_lower = result.stdout.lower()
        assert "paris" in stdout_lower
        assert "coat" in stdout_lower
    finally:
        h.run("extensions", "enable", "llm-google")
        h.run("extensions", "disable", "test-extension")
