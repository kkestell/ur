import { memo } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

// The opener plugin opens a link with target="_blank" in its default
// application, the default browser for web pages. A link without it would
// replace the app with the linked page.
const components: Components = {
  a: ({ href, title, children }) => (
    <a href={href} title={title} target="_blank">
      {children}
    </a>
  ),
};

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
    <div className="block agent mb-3 leading-relaxed last:mb-0">
      <div className="prose prose-sm prose-invert max-w-none prose-a:text-link prose-p:my-2 prose-pre:my-2 prose-pre:bg-raised prose-ul:my-2 prose-ol:my-2 prose-code:before:content-none prose-code:after:content-none">
        <Markdown remarkPlugins={[remarkGfm]} components={components}>{text}</Markdown>
      </div>
      <button className="copy ml-auto flex size-6 items-center justify-center rounded text-fg-dim hover:bg-control hover:text-fg" title="Copy" onClick={copy}>
        ⧉
      </button>
    </div>
  );
});
