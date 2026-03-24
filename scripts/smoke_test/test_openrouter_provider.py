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
        h.section("OpenRouter tool-calling flow")

        h.run("role", "set", "default", "openrouter/qwen/qwen3.5-9b")
        h.run("role", "get", "default")
        h.run("extension", "config", "llm-openrouter", "list")
        h.run_with_retries("run", PARIS_WEATHER_PROMPT)
    finally:
        h.run("extension", "enable", "llm-google")
        h.run("extension", "disable", "test-extension")
