import type { PermissionOption, ToolCallContent } from "@agentclientprotocol/sdk";
import type { PendingPermission } from "../ipc/bindings/PendingPermission";
import { shortcutLabel } from "../keys";

/** One pending permission request: its tool call, its options, and "Awaiting Confirmation." */
export function Permission({
  title,
  request,
  onAnswer,
}: {
  title: string;
  request: PendingPermission;
  onAnswer: (option: PermissionOption) => void;
}) {
  const { toolCall, options } = request.request;
  // A request with two options of the same kind shows the shortcut on the first.
  const shown = new Set<string>();
  return (
    <>
      <div className="block permission">
        <div className="permission-title">{title}</div>
        {Array.isArray(toolCall.content) && toolCall.content.map((content, index) => (
          <ContentView key={index} content={content} />
        ))}
        <div className="permission-options">
          {options.map((option) => {
            const first = !shown.has(option.kind);
            shown.add(option.kind);
            return (
              <button key={option.optionId} className="option" onClick={() => onAnswer(option)}>
                <span className="icon">{icon(option.kind)}</span>
                <span className="label">{option.name}</span>
                {first && <span className="shortcut">{shortcutLabel(option.kind)}</span>}
              </button>
            );
          })}
        </div>
      </div>
      <div className="block awaiting">Awaiting Confirmation.</div>
    </>
  );
}

function icon(kind: PermissionOption["kind"]): string {
  switch (kind) {
    case "allow_once":
    case "allow_always":
      return "✓";
    case "reject_once":
    case "reject_always":
      return "✕";
  }
}

/**
 * Text content and diffs as preformatted text. Terminal content and non-text
 * content are skipped: ur advertises no terminal capability, and images are
 * not shown here yet.
 */
function ContentView({ content }: { content: ToolCallContent }) {
  switch (content.type) {
    case "content":
      return content.content.type === "text" ? (
        <pre className="permission-content">{content.content.text}</pre>
      ) : null;
    case "diff":
      return (
        <pre className="permission-content">
          {content.path}
          {"\n"}
          {content.newText}
        </pre>
      );
    case "terminal":
      return null;
  }
}
