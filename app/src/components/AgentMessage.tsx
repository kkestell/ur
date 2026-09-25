import { memo } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

/**
 * An agent message rendered as Markdown, with a copy button under it that
 * copies its Markdown source. Memoized: the transcript reducer keeps the
 * blocks it does not touch, so earlier messages are not parsed again on every
 * chunk of the current one.
 */
export const AgentMessage = memo(function AgentMessage({ text }: { text: string }) {
  const copy = () => {
    navigator.clipboard.writeText(text).catch(console.error);
  };
  return (
    <div className="block agent">
      <Markdown remarkPlugins={[remarkGfm]}>{text}</Markdown>
      <button className="copy" title="Copy" onClick={copy}>
        ⧉
      </button>
    </div>
  );
});
