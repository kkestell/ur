# Testing

Run `cargo test --workspace` for the Rust test suite and
`cargo build
--workspace` for a debug build.

Test ACP behavior against a test agent built with the SDK's `Agent.builder()` in
the test process, without a model provider or Ox. Use Ox for live end-to-end
checks, including each milestone's check in `eng/todo.md`.

## Test discipline

The test suite is curated code. Each test owns one durable, observable
guarantee, and the suite changes as the guarantees change.

- Give each guarantee one owning test, named for that guarantee. A test that
  checks unrelated guarantees is split so each failure names what broke.
- A change adds a test for each new guarantee and for each reproduced
  regression, at the closest stable boundary. It rewrites or deletes the tests
  for guarantees it changes or removes, in the same change.
- Test a guarantee once. Tests at different layers repeat an assertion only when
  those layers have distinct failure modes.
- Use table-driven cases for one behavior over varied inputs. Each case states
  its input and expected result, and a failure message identifies the case.
- Keep setup proportional to the guarantee. Shared fixtures and helpers hold
  setup that several tests need, so each test body reads as its guarantee.
- Test observable behavior. Internals change freely while the guarantees they
  serve hold.
- Before finishing any change that touches tests, inspect the complete test diff
  and report which guarantees gained, lost, or moved their owning tests.
