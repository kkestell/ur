.PHONY: check check-rust check-docs format format-docs e2e run release

check: check-rust check-docs

check-rust:
	cargo fmt --all -- --check
	cargo test --workspace --all-targets --all-features
	cargo build --workspace --all-features
	cargo clippy --workspace --all-targets --all-features -- -D warnings

check-docs:
	dprint check

format: format-docs
	cargo fmt --all

format-docs:
	dprint fmt

e2e:
	cargo build --workspace
	cargo test -p ur --test tui -- --ignored

run:
	scripts/run

release:
	cargo build -p ur --release
