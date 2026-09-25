import { useEffect, useSyncExternalStore } from "react";
import { type SessionEvent, onSession, request } from "../ipc";
import type { ThreadState } from "../transcript/blocks";
import { reduce } from "../transcript/reduce";

// Every session's thread state stays, so reselecting a session does not
// refetch.
const threads = new Map<string, ThreadState>();
const listeners = new Set<() => void>();

// The sessions `subscribe` was sent for. The wire protocol has no
// `unsubscribe`, and the Link replays these after a reconnect.
const subscribed = new Set<string>();

export function apply(event: SessionEvent): void {
  const next = reduce(threads.get(event.session) ?? { blocks: [] }, event);
  if (next === undefined) {
    threads.delete(event.session);
    // A session that comes back, when its workspace is added again, is
    // subscribed afresh.
    subscribed.delete(event.session);
  } else {
    threads.set(event.session, next);
  }
  for (const listener of listeners) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useThread(id: string): ThreadState | undefined {
  return useSyncExternalStore(subscribe, () => threads.get(id));
}

/** Subscribes to the session the first time it is used, and reads its thread. */
export function useSession(id: string): ThreadState | undefined {
  useEffect(() => {
    if (!subscribed.has(id)) {
      subscribed.add(id);
      request({ type: "subscribe", session: id }).catch(console.error);
    }
  }, [id]);
  return useThread(id);
}

// Installed at module load, before any component subscribes.
onSession(apply).catch(console.error);
