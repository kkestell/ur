"""Live Google provider smoke test."""

from __future__ import annotations

from smoke_test.harness import SmokeHarness

PARIS_WEATHER_PROMPT = "What is the weather in Paris, and should I wear a coat?"
GOOGLE_CASES: tuple[tuple[str, str, str, str], ...] = (
    ("flash-low", "google/gemini-3-flash-preview", "low", "1024"),
    ("flash-high", "google/gemini-3-flash-preview", "high", "2048"),
    ("pro-medium", "google/gemini-3.1-pro-preview", "medium", "1536"),
    ("pro-high", "google/gemini-3.1-pro-preview", "high", "3072"),
    ("flash-lite-minimal", "google/gemini-3.1-flash-lite-preview", "minimal", "768"),
)


def run_case(
    h: SmokeHarness,
    case_name: str,
    model_ref: str,
    thinking_level: str,
    max_output_tokens: str,
) -> None:
    model_id = model_ref.split("/", 1)[1]

    h.section(f"{case_name} ({model_ref})")

    h.run("role", "set", "default", model_ref)
    h.run(
        "extension", "config", "llm-google", "set",
        f"{model_id}.thinking_level", thinking_level,
    )
    h.run(
        "extension", "config", "llm-google", "set",
        f"{model_id}.max_output_tokens", max_output_tokens,
    )
    h.run_with_retries("run", PARIS_WEATHER_PROMPT)


def run(h: SmokeHarness) -> None:
    if not h.getenv("GOOGLE_API_KEY"):
        print("Skipping Google provider smoke test: GOOGLE_API_KEY is not set.")
        return

    h.run("extension", "enable", "test-extension")

    try:
        for case in GOOGLE_CASES:
            run_case(h, *case)
    finally:
        h.run("extension", "disable", "test-extension")
