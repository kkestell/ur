# herd-mini — 80/20 herdr

The insight: most of herdr's complexity is from living in *other people's terminals*.
PTY emulation, screen-scraping 22 TUI agents to guess `blocked/working`,
per-agent resume flags, copy mode, prefix keys, remote screen streaming, live handoff.

If panes are native ACP clients instead, you delete all of that and get better state for free.

Keep herdr's job — **many agents, many repos, leave them running, see what needs you, steer from anywhere** — but replace "server owns PTYs" with "server owns ACP sessions" and replace "TUI in your terminal" with "native Iced GUI."

Stack: **Rust + Iced**. Agent panes are a **custom ACP client**. No special support for TUI coding agents (although in theory you could run one in a dumb PTY pane).

## Architecture: tiny daemon + Iced client

```
[ ACP servers ] <--stdio JSON-RPC--> [ herd-mini daemon ] <--TCP/socket--> [ Iced GUI / CLI ]
  claude-acp, codex-acp,            owns sessions,           detachable, multiple
  gemini-acp, custom...             appends to event log     clients ok
```

* **Daemon:** headless Rust. Owns one ACP child process per agent, holds the session, buffers the event log. GUI disconnects? Daemon keeps going. This preserves herdr's #1 superpower with ~10% of the code — no PTYs to keep alive, just child processes + JSON logs.
* **Iced client:** purely a viewer. `pane_grid` of agent cards, workspace sidebar, detail view. No terminal emulation. Disconnect = just drop the TCP subscription.
* **Generic PTY pane (dumb):** one escape hatch pane type for `npm run dev / tail logs / htop` — and yes, you *could* run a TUI agent in there, but it gets zero agent-awareness. No detection, no status, just scrollback.

## Why ACP deletes complexity

ACP (Agent Client Protocol, JSON-RPC over stdio) already gives you what herdr works so hard to infer:

| herdr does hard | ACP gives you free |
|---|---|
| screen manifest to guess `blocked/working/done` | `request_permission`, `tool_call` start/stop, `agent_message_chunk`, idle |
| 22 integrations with different resume flags | `session/new`, `session/load`, `session/cancel` — uniform |
| `pane read / wait-for-output` via scrollback parsing | `session/update` event stream, filterable |
| `agent.prompt` by injecting keys into a TUI | `session/prompt` with structured parts |

So a "pane" isn't a terminal. It's:

```rust
struct AgentPane {
    workspace_id: WorkspaceId,
    backend: String, // e.g. "claude-acp"
    session_id: String,
    status: Working | WaitingPermission { tool: String } | Done | Error,
    transcript: Vec<Update>, // markdown + tool calls + diffs, rendered natively in Iced
}
```

Permission requests become native buttons [Allow Once / Always / Deny] instead of "go click in that cursed TUI."

## What to keep vs. explicitly drop

**Keep (the 80% utility):**

1. Workspace = repo/task container, owns agents. Sidebar rolls up to "who needs you."
2. Detach/reattach + snapshot restore. Trivial now: event log + `session/load` on daemon restart. No `session-history.json` secrets debate, no live handoff.
3. Combined agent list + `prompt / read / wait`. Minimal API: `list, prompt, cancel, approve/deny, read(transcript), wait(status==blocked|done)`.
4. One remote story: daemon per machine, Iced client multiplexes TCP connections. No screen streaming — just JSON updates. Way cheaper.

**Drop (the 80% complexity):**

* No VT/xterm, no alternate screen, no mouse-capture wars, no copy mode, no `ctrl+b` prefix system. Iced text selection + normal shortcuts just work.
* No per-agent screen parsers, no `integration install claude/codex/...`, no lifecycle-vs-session-identity matrix.
* No tab sizing negotiation, no multi-client same-tab pane sizing, no 64-pane handoff batching.
* No plugin system v1. Replace with: single `on-event exec <cmd>` webhook. Add marketplace later if ever.

## Iced sketch

* `Subscription`: one per daemon connection, streams `AgentUpdate`.
* `View`: left = workspaces with dot (`- working / blocked / done`), center = `pane_grid`, right = focused transcript with markdown + tool diff + permission card.
* All state is dumb derived state from the event log — no terminal renderer to maintain.

You lose: "works in any terminal over SSH" and "runs the TUI agents you already love exactly as-is." You keep: "run 10 agents, walk away, come back, unblock the two that need you, from anywhere."

That's the 80/20.
