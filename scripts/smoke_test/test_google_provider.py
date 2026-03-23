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
    # Extract model ID from "google/model-id"
    model_id = model_ref.split("/", 1)[1]

    print()
    print(
        "Running Google smoke case:",
        case_name,
        f"(model={model_ref}, thinking_level={thinking_level}, max_output_tokens={max_output_tokens})",
    )

    set_model = h.run("role", "set", "default", model_ref)
    assert model_ref in set_model.stdout

    set_thinking = h.run(
        "extension", "config", "llm-google", "set",
        f"{model_id}.thinking_level", thinking_level,
    )
    assert f"thinking_level = {thinking_level}" in set_thinking.stdout

    set_max_tokens = h.run(
        "extension", "config", "llm-google", "set",
        f"{model_id}.max_output_tokens", max_output_tokens,
    )
    assert f"max_output_tokens = {max_output_tokens}" in set_max_tokens.stdout

    result = h.run(
        "run",
        env={"UR_RUN_USER_MESSAGE": PARIS_WEATHER_PROMPT},
    )
    assert f'resolving role "default" → {model_ref}' in result.stdout
    assert "LLM returned tool call: get_weather(" in result.stdout
    assert "tool result:" in result.stdout
    assert "calling LLM streaming (3 messages, includes tool result)" in result.stdout
    assert "Paris" in result.stdout
    assert "coat" in result.stdout.lower()


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
