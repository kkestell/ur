import { spawn } from "node:child_process"
import { fileURLToPath } from "node:url"

const hook = fileURLToPath(new URL("attention", import.meta.url))

function run(state) {
  return new Promise((resolve) => {
    const child = spawn(hook, ["opencode", state], { stdio: ["pipe", "ignore", "ignore"] })
    child.on("error", resolve)
    child.on("close", resolve)
    child.stdin.end("{}")
  })
}

export const UrAttention = async ({ client }) => {
  // Subagent sessions have a parent. Only their questions and permission requests mark the pane.
  const subagents = new Map()
  const requests = new Set()
  // A session can report busy or idle more than once per turn.
  const busy = new Set()
  // An error or interrupt is published before the session goes idle.
  const endings = new Map()
  let queue = Promise.resolve()

  async function subagent(id) {
    if (!subagents.has(id)) {
      const session = await client.session.get({ path: { id } }).catch(() => undefined)
      subagents.set(id, Boolean(session?.data?.parentID))
    }
    return subagents.get(id)
  }

  async function handle({ type, properties }) {
    switch (type) {
      case "permission.asked":
      case "question.asked":
        requests.add(properties.id)
        return run("permission")
      case "permission.replied":
      case "question.replied":
      case "question.rejected":
        if (requests.delete(properties.requestID) && requests.size === 0) await run("clear")
        return
      case "session.created":
      case "session.updated":
        subagents.set(properties.info.id, Boolean(properties.info.parentID))
        if (type === "session.created" && !properties.info.parentID) await run("clear")
        return
    }
    const id = properties?.sessionID
    if (!id || (await subagent(id))) return
    if (type === "session.error") {
      endings.set(id, properties.error?.name === "MessageAbortedError" ? "clear" : "error")
    } else if (type === "session.status" && properties.status.type === "busy") {
      endings.delete(id)
      if (!busy.has(id)) {
        busy.add(id)
        if (requests.size === 0) await run("clear")
      }
    } else if (type === "session.status" && properties.status.type === "idle" && busy.delete(id)) {
      const ending = endings.get(id) ?? "finished"
      endings.delete(id)
      requests.clear()
      await run(ending)
    }
  }

  return {
    event: async ({ event }) => {
      queue = queue.then(() => handle(event)).catch(() => {})
    },
  }
}
