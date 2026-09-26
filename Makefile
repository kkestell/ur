.PHONY: check check-rust check-app check-docs format format-docs e2e run

check: check-rust check-app check-docs

check-rust:
	cargo fmt --all -- --check
	cargo test --workspace --all-targets --all-features
	cargo build --workspace --all-features
	cargo clippy --workspace --all-targets --all-features -- -D warnings

check-app:
	pnpm -C app build
	pnpm -C app test

check-docs:
	dprint check

format: format-docs
	cargo fmt --all

format-docs:
	dprint fmt

e2e:
	pnpm -C app e2e

# Builds the daemon, starts it on the default socket, and runs the GUI with
# `pnpm tauri dev`. Closing the window or pressing Ctrl-C stops the daemon.
# Writes ~/.config/ur/config.json with the development config when missing.
run:
	scripts/run
