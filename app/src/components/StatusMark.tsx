import type { SessionSummary } from "../ipc/bindings/SessionSummary";

/** The mark for a session's status: nothing when idle. */
export function StatusMark({ status }: { status: SessionSummary["status"] }) {
  switch (status.type) {
    case "idle":
      return null;
    case "working":
      return <span className="status spinner size-2 shrink-0 animate-spin rounded-full border-[1.5px] border-fg-muted border-t-transparent" />;
    case "needs_permission":
      return <span className="status dot size-2 shrink-0 rounded-full bg-warning" />;
    case "failed":
      return <span className="status failed shrink-0 font-bold text-danger">!</span>;
  }
}
