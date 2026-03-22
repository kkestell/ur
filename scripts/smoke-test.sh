#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
UR="$ROOT/target/debug/ur"

# ── Build ────────────────────────────────────────────────────────────
echo "Building host..."
cargo build --manifest-path "$ROOT/Cargo.toml" 2>&1

echo "Building extensions..."
for dir in \
    extensions/system/session-jsonl \
    extensions/system/compaction-llm \
    extensions/system/llm-openai \
    extensions/user/llm-anthropic \
    extensions/workspace/test-extension
do
    cargo build --manifest-path "$ROOT/$dir/Cargo.toml" \
        --target wasm32-wasip2 --release 2>&1
done

# ── Set up temp directory ────────────────────────────────────────────
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

UR_ROOT="$TMPDIR/ur-root"
WORKSPACE="$TMPDIR/workspace"

mkdir -p "$UR_ROOT/extensions/system/session-jsonl"
mkdir -p "$UR_ROOT/extensions/system/compaction-llm"
mkdir -p "$UR_ROOT/extensions/system/llm-openai"
mkdir -p "$UR_ROOT/extensions/user/llm-anthropic"
mkdir -p "$WORKSPACE/.ur/extensions/test-extension"

# Copy WASM artifacts and generate extension.toml sidecar files
write_toml() {
    local dir="$1" id="$2" name="$3" wasm="$4" slot="${5:-}"
    cp "$wasm" "$dir/"
    local wasm_name
    wasm_name="$(basename "$wasm")"
    {
        echo "[extension]"
        echo "id = \"$id\""
        echo "name = \"$name\""
        [ -n "$slot" ] && echo "slot = \"$slot\""
        echo "wasm = \"$wasm_name\""
    } > "$dir/extension.toml"
}

write_toml "$UR_ROOT/extensions/system/session-jsonl" \
    "session-jsonl" "Session JSONL" \
    "$ROOT/extensions/system/session-jsonl/target/wasm32-wasip2/release/session_jsonl.wasm" \
    "session-provider"

write_toml "$UR_ROOT/extensions/system/compaction-llm" \
    "compaction-llm" "Compaction LLM" \
    "$ROOT/extensions/system/compaction-llm/target/wasm32-wasip2/release/compaction_llm.wasm" \
    "compaction-provider"

write_toml "$UR_ROOT/extensions/system/llm-openai" \
    "llm-openai" "LLM OpenAI" \
    "$ROOT/extensions/system/llm-openai/target/wasm32-wasip2/release/llm_openai.wasm" \
    "llm-provider"

write_toml "$UR_ROOT/extensions/user/llm-anthropic" \
    "llm-anthropic" "LLM Anthropic" \
    "$ROOT/extensions/user/llm-anthropic/target/wasm32-wasip2/release/llm_anthropic.wasm" \
    "llm-provider"

write_toml "$WORKSPACE/.ur/extensions/test-extension" \
    "test-extension" "Test Extension" \
    "$ROOT/extensions/workspace/test-extension/target/wasm32-wasip2/release/test_extension.wasm"

run() {
    echo ""
    echo "$ ur $*"
    UR_ROOT="$UR_ROOT" "$UR" -w "$WORKSPACE" "$@"
}

fail() {
    echo ""
    echo "$ ur $* (expect error)"
    if UR_ROOT="$UR_ROOT" "$UR" -w "$WORKSPACE" "$@" 2>&1; then
        echo "FAIL: expected error but command succeeded"
        exit 1
    fi
}

# ── Smoke tests ──────────────────────────────────────────────────────
echo ""
echo "═══ Smoke tests ═══"

run extensions list

run extensions inspect session-jsonl

# Enable second llm-provider (allowed — at-least-1)
run extensions enable llm-anthropic

# Disable first llm-provider (anthropic still covers it)
run extensions disable llm-openai

# Verify list reflects changes
run extensions list

# Cannot disable last llm-provider
fail extensions disable llm-anthropic

# Cannot disable only compaction-provider
fail extensions disable compaction-llm

# Cannot disable only session-provider
fail extensions disable session-jsonl

# Re-enable openai
run extensions enable llm-openai

# Enable no-slot workspace extension (always allowed)
run extensions enable test-extension

# Disable test-extension (no slot — always allowed)
run extensions disable test-extension

# Final state
run extensions list

# ── Model role tests ─────────────────────────────────────────────────
echo ""
echo "═══ Model role tests ═══"

# List shows default model with no config file
run model list

# Get default resolves to first provider's default model
run model get default

# Get unknown role falls back to default
run model get fast

# Show available settings for the resolved default model
run model config default

# Set default role to anthropic provider (validated against list-models)
run model set default anthropic/claude-sonnet-4-6

# Verify it persisted
run model get default

# Set a fast role to openai
run model set fast openai/gpt-5.4

# List shows both roles
run model list

# Show settings for each role
run model config default
run model config fast

# Reject invalid model references
fail model set default fake/nonexistent
fail model set default invalid-no-slash
fail model set default anthropic/nonexistent-model

# ── Provider setting tests ───────────────────────────────────────────
echo ""
echo "═══ Provider setting tests ═══"

# Set an integer setting
run model setting default thinking_budget 8000

# Set an enum setting
run model setting fast reasoning_effort high

# Reject unknown setting key
fail model setting default nonexistent_key 42

# Reject out-of-range integer
fail model setting default thinking_budget 999999

# Reject invalid enum value
fail model setting fast reasoning_effort turbo

# Reject wrong type (string for integer)
fail model setting default thinking_budget abc

# Verify config file has provider settings
echo ""
echo "Config file contents:"
cat "$UR_ROOT/config.toml"

echo ""
echo "All smoke tests passed."
