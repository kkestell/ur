.DEFAULT_GOAL := build

CARGO ?= cargo

HOST_MANIFEST := Cargo.toml

# install builds release by default; set DEBUG=1 for a debug install.
ifdef DEBUG
  HOST_BINARY_DIR := target/debug
  INSTALL_CARGO_FLAGS :=
else
  HOST_BINARY_DIR := target/release
  INSTALL_CARGO_FLAGS := --release
endif

HOST_BINARY := $(HOST_BINARY_DIR)/ur
HOST_BINARY_TUI := $(HOST_BINARY_DIR)/ur-tui

BINDIR ?= $(HOME)/.local/bin

.PHONY: \
	build \
	check \
	test \
	clippy \
	fmt \
	format \
	fmt-check \
	verify \
	install \
	uninstall

build:
	$(CARGO) build --manifest-path $(HOST_MANIFEST)

check:
	$(CARGO) check --manifest-path $(HOST_MANIFEST) --all-targets

test:
	$(CARGO) test --manifest-path $(HOST_MANIFEST)

clippy:
	$(CARGO) clippy --manifest-path $(HOST_MANIFEST) --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --manifest-path $(HOST_MANIFEST) --all

format: fmt

fmt-check:
	$(CARGO) fmt --manifest-path $(HOST_MANIFEST) --all --check

verify: fmt-check check test clippy

install:
	$(CARGO) build --manifest-path $(HOST_MANIFEST) $(INSTALL_CARGO_FLAGS)
	@mkdir -p "$(BINDIR)"
	cp "$(HOST_BINARY)" "$(BINDIR)/ur"
	cp "$(HOST_BINARY_TUI)" "$(BINDIR)/ur-tui"

uninstall:
	rm -f "$(BINDIR)/ur" "$(BINDIR)/ur-tui"
