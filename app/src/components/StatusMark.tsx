import type { SessionSummary } from "../ipc/bindings/SessionSummary";

/** The mark for a session's status: nothing when idle. */
export function StatusMark({ status }: { status: SessionSummary["status"] }) {
  switch (status.type) {
    case "idle":
      return null;
    case "working":
      return <span className="status spinner" />;
    case "needs_permission":
      return <span className="status dot" />;
    case "failed":
      return <span className="status failed">!</span>;
  }
}
