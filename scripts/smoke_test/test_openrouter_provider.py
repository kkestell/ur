"""Live OpenRouter provider smoke test."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness

PARIS_WEATHER_PROMPT = "What is the weather in Paris, and should I wear a coat?"


def run(h: SmokeHarness) -> None:
    if not h.getenv("OPENROUTER_API_KEY"):
        print("Skipping OpenRouter provider smoke test: OPENROUTER_API_KEY is not set.")
        return

    h.run("extension", "enable", "test-extension")
    h.run("extension", "disable", "llm-google")

    try:
        # Set a specific model for deterministic testing.
        h.run("role", "set", "default", "openrouter/qwen/qwen3.5-9b")

        # Verify it resolves correctly.
        get_result = h.run("role", "get", "default")
        assert "openrouter/qwen/qwen3.5-9b" in get_result.stdout, (
            f"expected openrouter/qwen/qwen3.5-9b, got: {get_result.stdout}"
        )

        # Show dynamic settings surface.
        h.run("extension", "config", "llm-openrouter", "list")

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
        h.run("extension", "enable", "llm-google")
        h.run("extension", "disable", "test-extension")
