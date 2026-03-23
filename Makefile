.DEFAULT_GOAL := build

CARGO ?= cargo
WASM_TARGET ?= wasm32-wasip2

HOST_MANIFEST := Cargo.toml

# Built-in extensions ship from the system tier.
BUILTIN_EXTENSION_MANIFESTS := \
	extensions/system/session-jsonl/Cargo.toml \
	extensions/system/compaction-llm/Cargo.toml \
	extensions/system/llm-google/Cargo.toml \
	extensions/system/llm-openrouter/Cargo.toml

# Repo-local checks also cover the smoke-test workspace extension.
REPO_EXTENSION_MANIFESTS := \
	$(BUILTIN_EXTENSION_MANIFESTS) \
	extensions/workspace/test-extension/Cargo.toml

.PHONY: \
	build \
	build-ur \
	build-extensions \
	check \
	test \
	clippy \
	fmt \
	format \
	fmt-check \
	verify \
	smoke-test

build: build-ur build-extensions

build-ur:
	$(CARGO) build --manifest-path $(HOST_MANIFEST)

build-extensions:
	@for manifest in $(BUILTIN_EXTENSION_MANIFESTS); do \
		echo "==> cargo build --manifest-path $$manifest --target $(WASM_TARGET) --release"; \
		$(CARGO) build --manifest-path "$$manifest" --target $(WASM_TARGET) --release; \
	done

check:
	$(CARGO) check --manifest-path $(HOST_MANIFEST) --all-targets
	@for manifest in $(REPO_EXTENSION_MANIFESTS); do \
		echo "==> cargo check --manifest-path $$manifest --target $(WASM_TARGET)"; \
		$(CARGO) check --manifest-path "$$manifest" --target $(WASM_TARGET); \
	done

test:
	$(CARGO) test --manifest-path $(HOST_MANIFEST)

clippy:
	$(CARGO) clippy --manifest-path $(HOST_MANIFEST) --all-targets -- -D warnings
	@for manifest in $(REPO_EXTENSION_MANIFESTS); do \
		echo "==> cargo clippy --manifest-path $$manifest --target $(WASM_TARGET) -- -D warnings"; \
		$(CARGO) clippy --manifest-path "$$manifest" --target $(WASM_TARGET) -- -D warnings; \
	done

fmt:
	$(CARGO) fmt --manifest-path $(HOST_MANIFEST) --all
	@for manifest in $(REPO_EXTENSION_MANIFESTS); do \
		echo "==> cargo fmt --manifest-path $$manifest --all"; \
		$(CARGO) fmt --manifest-path "$$manifest" --all; \
	done

format: fmt

fmt-check:
	$(CARGO) fmt --manifest-path $(HOST_MANIFEST) --all --check
	@for manifest in $(REPO_EXTENSION_MANIFESTS); do \
		echo "==> cargo fmt --manifest-path $$manifest --all --check"; \
		$(CARGO) fmt --manifest-path "$$manifest" --all --check; \
	done

verify: fmt-check check test clippy

smoke-test:
	python3 scripts/smoke-test.py
