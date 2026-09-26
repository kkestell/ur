# Agent message links

## Scope and coverage

Reviewed commit `59f8773`, the Agent message links plan and work log, the opener plugin's injected
script and permissions in `tauri-plugin-opener` 2.5.5, and the rest of the webview for other places
that render links. Used correctness, security, dependencies, comments, and documentation lenses,
with the Rust Cargo lens for the manifests and lockfile.

## Findings

No findings.

`AgentMessage` is the only place in the webview that renders links, and GFM autolinks go through the
same `a` component. A ⌘-click or ⌥-click, which the plugin's script leaves alone, asks the webview
for a new window, and ur sets no new-window handler, so the app stays in place. The new transitive
crates under `zbus` build only for Linux.

## Checks run

- `git show 59f8773` — read the diff and the lockfile changes.
- `cargo tree -p ur-app -i zbus --target aarch64-apple-darwin` — nothing on macOS.
- Searched `app/src` for `href`, `<a`, and `Markdown` — only `AgentMessage.tsx`.
- Relied on the work log's `make check`, `make e2e`, and WebDriver check of the same tree; the
  review changed no code.

## Verdict

The change fixes OX-0007 as planned. Nothing to act on.
