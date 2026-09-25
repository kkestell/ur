import type { ToolCallContent } from "@agentclientprotocol/sdk";

/**
 * A tool call's text content and diffs as preformatted text. Terminal content
 * and non-text content are skipped: ur advertises no terminal capability, and
 * images are not shown here yet.
 */
export function ToolCallContentView({ content }: { content: ToolCallContent[] }) {
  return content.map((item, index) => {
    switch (item.type) {
      case "content":
        return item.content.type === "text" ? (
          <pre key={index} className="tool-call-content">
            {item.content.text}
          </pre>
        ) : null;
      case "diff":
        return (
          <pre key={index} className="tool-call-content">
            {item.path}
            {"\n"}
            {item.newText}
          </pre>
        );
      case "terminal":
        return null;
    }
  });
}
