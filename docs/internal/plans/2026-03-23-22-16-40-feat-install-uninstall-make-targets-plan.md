---
title: "feat: Add install and uninstall make targets"
type: feat
date: 2026-03-23
---

# feat: Add install and uninstall make targets

## Overview

Add `make install` and `make uninstall` targets so the project can install the
`ur` binary into `~/.local/bin` and install bundled system extensions into
`~/.ur/extensions/system`. At the same time, remove the hard-coded
`target/wasm32-wasip2/release` lookup from extension discovery so the Rust code
does not know about Cargo build output paths. `uninstall` should remove
`~/.local/bin/ur` and the entire `~/.ur` tree exactly as requested.

## Research Summary

### Internal references

- `CLAUDE.md` says project workflows should go through `Makefile` targets.
- `Makefile` already defines the host build (`build-ur`) and bundled extension
  builds (`build-extensions`) using `BUILTIN_EXTENSION_MANIFESTS`.
- `docs/extensions/overview.md` documents the system extension tier as
  `$UR_ROOT/extensions/system/`, with `UR_ROOT` defaulting to `~/.ur`.
- `src/discovery.rs` scans `ur_root.join("extensions/system")`, so install
  output must land there to participate in normal discovery.
- `src/discovery.rs` currently special-cases both a top-level `.wasm` and a
  Cargo-specific `target/wasm32-wasip2/release/*.wasm` path. That coupling
  should be removed.
- `scripts/smoke_test/harness.py` already copies built artifacts into extension
  directories, which gives a natural place to align install behavior once
  discovery becomes path-agnostic within each extension directory.

### External research decision

The codebase already has strong local guidance for this change, and the request
is repo-local Makefile behavior. Proceeding without external research.

## Problem Statement

The repository can build the host binary and bundled extensions, but there is
no standard install workflow. That leaves local setup to manual `cp` commands,
which is easy to get wrong and does not encode the project's expected install
layout in one place.

Separately, extension discovery currently bakes Cargo's
`target/wasm32-wasip2/release` layout into Rust code. That is the wrong layer
for build-system knowledge and makes extension loading depend on a specific
build output convention instead of simply locating `.wasm` files within an
extension directory.

## Proposed Solution

### Makefile additions

Add install-related variables near the existing build variables:

- `BINDIR ?= $(HOME)/.local/bin`
- `UR_ROOT ?= $(HOME)/.ur`
- `SYSTEM_EXTENSION_INSTALL_DIR := $(UR_ROOT)/extensions/system`
- `HOST_BINARY := target/debug/ur`
- `BUILTIN_EXTENSION_DIRS := $(patsubst %/Cargo.toml,%,$(BUILTIN_EXTENSION_MANIFESTS))`

Update `.PHONY` to include `install` and `uninstall`.

### Discovery cleanup

Update extension discovery so each extension directory is treated as a package
root and the loader searches for `.wasm` files generically within that tree
instead of checking Cargo-specific locations. The important rule is:

- allowed: `.wasm` at the extension root
- allowed: `.wasm` in subdirectories under that extension
- not allowed: Rust code that knows about `target/wasm32-wasip2/release`

To keep behavior deterministic, discovery should either pick the single `.wasm`
found under an extension directory or return a clear error if multiple candidate
WASM files exist for one extension.

### `install` target behavior

`install` should depend on `build` so the host binary and bundled extension
artifacts exist before copying.

Implementation shape:

1. Create `$(BINDIR)` and `$(SYSTEM_EXTENSION_INSTALL_DIR)` if they do not
   exist.
2. Copy `$(HOST_BINARY)` to `$(BINDIR)/ur`.
3. Loop over `$(BUILTIN_EXTENSION_DIRS)` and copy each bundled extension
   into `$(SYSTEM_EXTENSION_INSTALL_DIR)` in a clean installed layout that
   contains the extension's `.wasm` artifact and any needed companion files,
   without depending on Cargo's `target/` directory structure at runtime.

To prevent stale files from older installs, refresh each destination extension
directory before copying the current one.

### `uninstall` target behavior

`uninstall` should be tolerant of missing paths and should:

- remove `$(BINDIR)/ur`
- remove `$(UR_ROOT)`

This is intentionally broad because the request explicitly says to delete
`~/.ur`, not only the bundled system extension subtree.

## Acceptance Criteria

- `make install` creates `~/.local/bin/ur`
- `make install` creates `~/.ur/extensions/system/<extension>/...` for each
  bundled extension directory
- Installed extensions remain discoverable through the existing
  `$UR_ROOT/extensions/system` scan path
- Discovery accepts `.wasm` files located anywhere inside an extension
  directory, including subdirectories
- Discovery no longer hard-codes `target/wasm32-wasip2/release` or other
  Cargo-specific paths
- `make uninstall` removes `~/.local/bin/ur`
- `make uninstall` removes `~/.ur`
- Re-running either target is safe when destination paths already exist or are
  already absent

## Implementation Steps

### Step 1: Remove Cargo-specific discovery logic

Update `src/discovery.rs` so extension scanning recursively locates `.wasm`
files beneath each extension directory rather than probing a hard-coded
`target/wasm32-wasip2/release` path. Define deterministic behavior for zero,
one, or multiple `.wasm` matches.

### Step 2: Derive install paths from existing build metadata

Update `Makefile` to define reusable path variables for the host binary, install
directories, and bundled extension directories derived from
`BUILTIN_EXTENSION_MANIFESTS`.

### Step 3: Add `install`

Add a new `install: build` target that:

- creates destination directories
- copies `target/debug/ur` to `~/.local/bin/ur`
- refreshes and copies each bundled extension into
  `~/.ur/extensions/system/<id>` in a runtime-friendly layout that does not
  rely on the source tree's `target/` structure

### Step 4: Add `uninstall`

Add an `uninstall` target that removes `~/.local/bin/ur` and `~/.ur` with
no-error semantics when they are missing.

### Step 5: Align docs and test helpers

Update any docs and helpers that still describe Cargo-specific runtime layout:

- `docs/extensions/overview.md`
- `scripts/smoke_test/harness.py`

The runtime contract should describe extension directories in terms of contained
WASM artifacts, not Cargo output folders.

### Step 6: Validate locally

Run:

- `make install`
- `test -x ~/.local/bin/ur`
- `find ~/.ur/extensions/system -maxdepth 2 -type d | sort`
- `make uninstall`
- `test ! -e ~/.local/bin/ur`
- `test ! -e ~/.ur`

## Risks / Notes

- `install` will copy the debug host binary (`target/debug/ur`) because that is
  what the current `build-ur` target produces. A release-install flow would be a
  separate enhancement.
- Recursive discovery needs a clear rule for multiple `.wasm` files under one
  extension directory. Failing loudly is safer than guessing.
- `uninstall` removes the full `~/.ur` tree, which also deletes user-tier
  extensions, workspace manifests, and any other ur state. That is consistent
  with the explicit request and should be preserved in implementation.

## Files Modified

- `Makefile`
- `src/discovery.rs`
- `scripts/smoke_test/harness.py`
- `docs/extensions/overview.md`
