.DEFAULT_GOAL := build

CARGO ?= cargo
WASM_TARGET ?= wasm32-wasip2

HOST_MANIFEST := Cargo.toml

# install builds release by default; set DEBUG=1 for a debug install.
ifdef DEBUG
  HOST_BINARY := target/debug/ur
  INSTALL_CARGO_FLAGS :=
else
  HOST_BINARY := target/release/ur
  INSTALL_CARGO_FLAGS := --release
endif

BINDIR ?= $(HOME)/.local/bin
UR_ROOT ?= $(HOME)/.ur
SYSTEM_EXTENSION_INSTALL_DIR := $(UR_ROOT)/extensions/system

# Built-in extensions ship from the system tier.
BUILTIN_EXTENSION_MANIFESTS := \
	extensions/system/session-jsonl/Cargo.toml \
	extensions/system/compaction-llm/Cargo.toml \
	extensions/system/llm-google/Cargo.toml \
	extensions/system/llm-openrouter/Cargo.toml

BUILTIN_EXTENSION_DIRS := $(patsubst %/Cargo.toml,%,$(BUILTIN_EXTENSION_MANIFESTS))

# Workspace test extensions built for integration tests.
TEST_EXTENSION_MANIFESTS := \
	extensions/workspace/test-extension/Cargo.toml \
	extensions/workspace/llm-test/Cargo.toml

# Repo-local checks also cover the workspace test extensions.
REPO_EXTENSION_MANIFESTS := \
	$(BUILTIN_EXTENSION_MANIFESTS) \
	$(TEST_EXTENSION_MANIFESTS)

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
	install \
	uninstall

build: build-ur build-extensions

build-ur:
	$(CARGO) build --manifest-path $(HOST_MANIFEST)

build-extensions:
	@for manifest in $(BUILTIN_EXTENSION_MANIFESTS) $(TEST_EXTENSION_MANIFESTS); do \
		echo "==> cargo build --manifest-path $$manifest --target $(WASM_TARGET) --release"; \
		$(CARGO) build --manifest-path "$$manifest" --target $(WASM_TARGET) --release; \
	done

check:
	$(CARGO) check --manifest-path $(HOST_MANIFEST) --all-targets
	@for manifest in $(REPO_EXTENSION_MANIFESTS); do \
		echo "==> cargo check --manifest-path $$manifest --target $(WASM_TARGET)"; \
		$(CARGO) check --manifest-path "$$manifest" --target $(WASM_TARGET); \
	done

test: build-extensions
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

install: build-extensions
	$(CARGO) build --manifest-path $(HOST_MANIFEST) $(INSTALL_CARGO_FLAGS)
	@mkdir -p "$(BINDIR)"
	cp "$(HOST_BINARY)" "$(BINDIR)/ur"
	@for ext_dir in $(BUILTIN_EXTENSION_DIRS); do \
		ext_name=$$(basename "$$ext_dir"); \
		dest="$(SYSTEM_EXTENSION_INSTALL_DIR)/$$ext_name"; \
		rm -rf "$$dest"; \
		mkdir -p "$$dest"; \
		find "$$ext_dir/target/$(WASM_TARGET)/release" -name '*.wasm' -exec cp {} "$$dest/" \; ; \
	done

uninstall:
	rm -f "$(BINDIR)/ur"
	rm -rf "$(UR_ROOT)"
