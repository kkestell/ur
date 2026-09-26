# Agent message links

## Goal

Fix OX-0007 in `docs/agents/issues.csv`: clicking a link in an agent message navigates the webview
to the linked page, and the app is gone until it restarts. When this is done, clicking an http,
https, mailto, or tel link in an agent message opens it in its default application, the default
browser for web pages, and the app stays as it was. Clicking any other link does nothing. Links show
in a blue that reads on the dark background instead of the webview's default link blue.

## Related code

- `app/src/components/AgentMessage.tsx` — renders the agent message with
  `<Markdown remarkPlugins={[remarkGfm]}>`, which turns each Markdown link into a plain `<a href>`
  that the webview follows on click.
- `app/src-tauri/src/main.rs` — the Tauri builder. The `webdriver` feature's plugin is registered on
  it the same way the opener plugin will be.
- `app/src-tauri/capabilities/default.json` — the main window's permissions, `core:default` only.
- `Cargo.toml` and `app/src-tauri/Cargo.toml` — the workspace dependency versions and the core's
  dependencies.
- `app/src/styles.css` — the `.block.agent` rules for Markdown elements. No rule styles links.
- `~/.cargo/registry/src/*/tauri-plugin-opener-2.5.5/src/lib.rs` and `src/init-iife.js` — `init()`
  injects a script into the webview. On a left click on an `<a>` with `target="_blank"` whose URL is
  http, https, mailto, or tel, unless ⌘ or ⌥ is held, the script cancels the click and calls the
  plugin's `open_url` command, which opens the URL in its default application. `permissions/`
  defines `allow-open-url`, which allows the command, and `allow-default-urls`, the scope that
  admits those four URL schemes. `open_url` refuses a URL no scope admits.
- `~/.cargo/registry/src/*/wry-0.55.1/src/wkwebview/class/wry_web_view_ui_delegate.rs` — on macOS,
  the request for a new window that the webview makes for a `target="_blank"` link the script leaves
  alone opens nothing unless the webview has a new-window handler. ur sets none.

## Decisions

### The opener plugin opens the links

`tauri-plugin-opener` 2.5.5 joins the core, registered with `tauri_plugin_opener::init()`. Its
script handles only links with `target="_blank"`, so `AgentMessage` renders every link with it,
through react-markdown's `components` option; react-markdown 9 removed `linkTarget`. The `a`
component keeps the link's `href`, `title`, and children. No webview code calls the plugin, so its
JavaScript package, `@tauri-apps/plugin-opener`, is not added.

Clicking a link then does one of two things:

- An http, https, mailto, or tel link opens in its default application.
- Any other link does nothing, since the script leaves it alone and the webview opens no new window.
  This covers relative paths such as `src/main.rs`, and GFM footnote references, which no longer
  scroll to their footnote.

### Only the permissions links need

The capability gains `opener:allow-open-url` and `opener:allow-default-urls`. `opener:default` would
also allow revealing files in Finder, which ur does not do.

### Link color

`.block.agent a` gets `color: #74ade8`, the blue in the wireframes. The webview's default link blue,
`#0000ee`, is hard to read on the `#1e1e1e` background.

## Naming

- Agent message, webview, core — as defined in `docs/agents/glossary.md`.
- Opener plugin — `tauri-plugin-opener`, Tauri's plugin that opens URLs in their default
  application. The comment in `AgentMessage.tsx`, `AGENTS.md`, and `architecture.md` use this name.

## Test plan

- End-to-end test in `app/e2e/thread.test.ts`,
  `clicking a link in an agent message leaves the app in place`: the fake server's reply repeats the
  prompt `see [main](src/main.rs)`; after the link is clicked, the webview's URL is unchanged and
  the sidebar is still there. A relative link opens no browser, so the test runs on every run
  without opening one; a link without `target="_blank"` fails it by replacing the app. `Gui.url()`
  reads the webview's URL.
- Ad-hoc check against the fake server, whose reply echoes the prompt, through the WebDriver build
  in `target/e2e`: prompt `see [the example](https://example.com/) and [main](src/main.rs)`. Both
  links show in `#74ade8`. Clicking the first opens `https://example.com/` in the default browser,
  and the webview stays at `tauri://localhost` with the sidebar and thread in place. Clicking the
  second does nothing, and the webview stays at `tauri://localhost`.

## Implementation plan

1. `Cargo.toml`: `tauri-plugin-opener = "2.5.5"` in `[workspace.dependencies]`.
   `app/src-tauri/Cargo.toml`: `tauri-plugin-opener.workspace = true` in `[dependencies]`.
2. `app/src-tauri/src/main.rs`: `.plugin(tauri_plugin_opener::init())` on
   `tauri::Builder::default()`.
3. `app/src-tauri/capabilities/default.json`: add `opener:allow-open-url` and
   `opener:allow-default-urls` to `permissions`.
4. `app/src/components/AgentMessage.tsx`: a module-level `components: Components` whose `a` renders
   `<a href={href} title={title} target="_blank">{children}</a>`, passed to `Markdown`. Its comment
   says the opener plugin opens `target="_blank"` links in the default browser, and that a link
   without it would replace the app.
5. `app/src/styles.css`: `.block.agent a { color: #74ade8; }` with the other `.block.agent` rules.
6. `app/e2e/harness.ts`: `Gui.url()`. `app/e2e/thread.test.ts`: the test in the Test plan.
7. The end-to-end suite and the ad-hoc check.

## Documentation updates

- `AGENTS.md`: `app/src-tauri/src/main.rs` registers the opener plugin; `AgentMessage.tsx` renders
  links that open in the default browser; map `app/src-tauri/capabilities/default.json` as the main
  window's permissions: Tauri's core defaults and the opener plugin's `open_url` for http, https,
  mailto, and tel URLs.
- `docs/agents/architecture.md`: under GUI architecture, add the opener plugin, which opens links in
  agent messages in the default browser, to the native pieces that come from Tauri; in the project
  layout, list the opener plugin in `main.rs`.
- `docs/agents/todo.md`: check off OX-0007 under Thread rendering.
- `docs/agents/issues.csv`: set OX-0007's status to `fixed`.
- `docs/agents/testing.md`: the link guarantee.
