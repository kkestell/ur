#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
UR="$ROOT/target/debug/ur"

# Load .env from project root
if [ -f "$ROOT/.env" ]; then
    set -a
    # shellcheck disable=SC1091
    . "$ROOT/.env"
    set +a
fi

# ── Build ────────────────────────────────────────────────────────────
echo "Building host..."
cargo build --manifest-path "$ROOT/Cargo.toml" 2>&1

echo "Building extensions..."
for dir in \
    extensions/system/session-jsonl \
    extensions/system/compaction-llm \
    extensions/system/llm-google \
    extensions/workspace/test-extension \
    extensions/workspace/llm-test
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
mkdir -p "$UR_ROOT/extensions/system/llm-google"
mkdir -p "$WORKSPACE/.ur/extensions/test-extension"
mkdir -p "$WORKSPACE/.ur/extensions/llm-test"

# Copy WASM artifacts into tier directories (no TOML needed).
copy_wasm() {
    local dir="$1" wasm="$2"
    cp "$wasm" "$dir/"
}

copy_wasm "$UR_ROOT/extensions/system/session-jsonl" \
    "$ROOT/extensions/system/session-jsonl/target/wasm32-wasip2/release/session_jsonl.wasm"

copy_wasm "$UR_ROOT/extensions/system/compaction-llm" \
    "$ROOT/extensions/system/compaction-llm/target/wasm32-wasip2/release/compaction_llm.wasm"

copy_wasm "$UR_ROOT/extensions/system/llm-google" \
    "$ROOT/extensions/system/llm-google/target/wasm32-wasip2/release/llm_google.wasm"

copy_wasm "$WORKSPACE/.ur/extensions/test-extension" \
    "$ROOT/extensions/workspace/test-extension/target/wasm32-wasip2/release/test_extension.wasm"

copy_wasm "$WORKSPACE/.ur/extensions/llm-test" \
    "$ROOT/extensions/workspace/llm-test/target/wasm32-wasip2/release/llm_test.wasm"

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

run extensions inspect llm-google

# Cannot disable only llm-provider
fail extensions disable llm-google

# Cannot disable only compaction-provider
fail extensions disable compaction-llm

# Cannot disable only session-provider
fail extensions disable session-jsonl

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

# Get default resolves to google's default model
run model get default

# Get unknown role falls back to default
run model get fast

# Show available settings for the resolved default model
run model config default

# Set default role to google provider (validated against list-models)
run model set default google/gemini-3-flash-preview

# Verify it persisted
run model get default

# Set a fast role to gemini-3-pro-preview
run model set fast google/gemini-3-pro-preview

# List shows both roles
run model list

# Show settings for each role
run model config default
run model config fast

# Reject invalid model references
fail model set default fake/nonexistent
fail model set default invalid-no-slash
fail model set default google/nonexistent-model

# ── Provider setting tests ───────────────────────────────────────────
echo ""
echo "═══ Provider setting tests ═══"

# Set an integer setting
run model setting default temperature 150

# Set another integer setting
run model setting fast max_output_tokens 4096

# Reject unknown setting key
fail model setting default nonexistent_key 42

# Reject out-of-range integer
fail model setting default temperature 999

# Reject wrong type (string for integer)
fail model setting default temperature abc

# Verify config file has provider settings
echo ""
echo "Config file contents:"
cat "$UR_ROOT/config.toml"

# ── Deterministic agent turn test ─────────────────────────────────────
echo ""
echo "═══ Agent turn test ═══"

# Enable test-extension (tool provider) and llm-test (deterministic LLM)
run extensions enable test-extension
run extensions enable llm-test

# Set default role to the deterministic test LLM
run model set default test/echo

# Run a full agent turn
OUTPUT="$(UR_ROOT="$UR_ROOT" "$UR" -w "$WORKSPACE" run 2>&1)"
echo "$OUTPUT"

# Verify the full turn loop fired
for expected in \
    "[turn] loading session" \
    "[turn] session loaded" \
    "[turn] adding user message" \
    "[turn] calling LLM streaming" \
    "[turn] LLM returned tool call" \
    "[turn] dispatching tool" \
    "[turn] tool result" \
    "[turn] LLM returned message" \
    "[turn] appending" \
    "[turn] compacting"
do
    if ! echo "$OUTPUT" | grep -qF "$expected"; then
        echo "FAIL: missing expected output: $expected"
        exit 1
    fi
done
echo "Agent turn test passed."

# Disable llm-test and reset model so it doesn't interfere with real API test
run extensions disable llm-test
run model set default google/gemini-3-flash-preview

# ── Real API integration test ─────────────────────────────────────────
echo ""
echo "═══ Integration test ═══"

# Run a single agent turn with real API
OUTPUT="$(UR_ROOT="$UR_ROOT" GOOGLE_API_KEY="$GOOGLE_API_KEY" "$UR" -w "$WORKSPACE" run 2>&1)" || true
echo "$OUTPUT"

for expected in \
    "[turn] loading session" \
    "[turn] session loaded" \
    "[turn] adding user message" \
    "[turn] resolving role" \
    "[turn] calling LLM streaming"
do
    if ! echo "$OUTPUT" | grep -qF "$expected"; then
        echo "FAIL: missing expected output: $expected"
        exit 1
    fi
done
echo "Integration test passed."

echo ""
echo "All smoke tests passed."
