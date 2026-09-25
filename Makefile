.PHONY: check check-rust check-app check-docs format format-docs e2e

check: check-rust check-app check-docs

check-rust:
	cargo fmt --all -- --check
	cargo test --workspace --all-targets --all-features
	cargo build --workspace --all-features
	cargo clippy --workspace --all-targets --all-features -- -D warnings

check-app:
	pnpm -C app build

check-docs:
	dprint check

format: format-docs
	cargo fmt --all

format-docs:
	dprint fmt

e2e:
	pnpm -C app e2e
