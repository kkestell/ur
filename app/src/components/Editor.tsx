import type {
  AgentCapabilities,
  AvailableCommand,
  ContentBlock,
  SessionConfigOption,
} from "@agentclientprotocol/sdk";
import { Ellipsis } from "lucide-react";
import {
  type DragEvent,
  type KeyboardEvent,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { request } from "../ipc";
import type { Status } from "../ipc/bindings/Status";
import { matchingCommands, slashQuery } from "../slash";
import type { ThreadState } from "../transcript/blocks";
import { type ConfigValue, ConfigPicker } from "./ConfigPicker";
import { Popover, useClickOutside } from "./Popover";
import { UsageIndicator } from "./UsageIndicator";

/** The space between config pickers, Tailwind's `gap-1`. */
const PICKER_GAP = 4;

/** An image file dropped on the editor, sent with the next prompt once read. */
export type ImageAttachment = { id: number; name: string; mimeType: string; data: string | null };

export function Editor({
  session,
  status,
  thread,
  capabilities,
}: {
  session: string;
  status: Status | undefined;
  thread: ThreadState | undefined;
  capabilities: AgentCapabilities | null;
}) {
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<ImageAttachment[]>([]);
  const nextAttachmentId = useRef(0);
  const [message, setMessage] = useState<string>();
  // The command list's highlighted row, and whether Escape or an insertion
  // closed the list until the text changes.
  const [highlight, setHighlight] = useState(0);
  const [listClosed, setListClosed] = useState(false);
  const running = status?.type === "working" || status?.type === "needs_permission";
  const commands = thread?.commands ?? [];
  const options = thread?.configOptions ?? [];
  // How many config pickers fit in the bottom row; the rest are in the More
  // menu. `natural` holds a hidden copy of every picker and the More button
  // at their natural widths.
  const [inline, setInline] = useState(options.length);
  const optionsRow = useRef<HTMLDivElement>(null);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const natural = useRef<HTMLDivElement>(null);
  const query = slashQuery(text);
  const matches = query === undefined || listClosed ? [] : matchingCommands(commands, query);

  const changeText = (next: string) => {
    setText(next);
    setListClosed(false);
    setHighlight(0);
  };

  const send = () => {
    if ((text === "" && attachments.length === 0) || attachments.some((image) => image.data === null)) {
      return;
    }
    const sent = { text, attachments };
    const content: ContentBlock[] = [];
    if (text !== "") {
      content.push({ type: "text", text });
    }
    for (const image of attachments) {
      content.push({ type: "image", mimeType: image.mimeType, data: image.data! });
    }
    setMessage(undefined);
    setText("");
    setAttachments([]);
    // A rejected prompt comes back to the editor, unless something new was
    // typed or attached meanwhile.
    const restore = () => {
      setText((current) => (current === "" ? sent.text : current));
      setAttachments((current) => (current.length === 0 ? sent.attachments : current));
    };
    request({ type: "prompt", session, content })
      .then((response) => {
        if (response.type === "busy") {
          setMessage("The session is busy.");
          restore();
        } else if (response.type === "error") {
          setMessage(response.message);
          restore();
        }
      })
      .catch((error) => {
        setMessage(String(error));
        restore();
      });
  };

  const stop = () => {
    setMessage(undefined);
    request({ type: "cancel", session })
      .then((response) => {
        if (response.type === "error") {
          setMessage(response.message);
        }
      })
      .catch((error) => setMessage(String(error)));
  };

  const setConfigOption = (config_id: string, value: ConfigValue) => {
    setMessage(undefined);
    request({ type: "set_config_option", session, config_id, value })
      .then((response) => {
        if (response.type === "error") {
          setMessage(response.message);
        }
      })
      .catch((error) => setMessage(String(error)));
  };

  const insert = (command: AvailableCommand) => {
    setText(`/${command.name}`);
    setListClosed(true);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (matches.length > 0) {
      const row = Math.min(highlight, matches.length - 1);
      switch (event.key) {
        case "ArrowUp":
          event.preventDefault();
          setHighlight((row + matches.length - 1) % matches.length);
          return;
        case "ArrowDown":
          event.preventDefault();
          setHighlight((row + 1) % matches.length);
          return;
        case "Enter":
        case "Tab":
          event.preventDefault();
          insert(matches[row]);
          return;
        case "Escape":
          event.preventDefault();
          setListClosed(true);
          return;
      }
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
    }
  };

  const onDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const files = [...event.dataTransfer.files];
    if (files.length === 0) {
      return;
    }
    if (capabilities?.promptCapabilities?.image !== true) {
      setMessage("The server does not accept images.");
      return;
    }
    setMessage(undefined);
    for (const file of files) {
      if (!file.type.startsWith("image/")) {
        setMessage(`${file.name} is not an image.`);
        continue;
      }
      const id = nextAttachmentId.current++;
      setAttachments((current) => [...current, { id, name: file.name, mimeType: file.type, data: null }]);
      readImage(file)
        .then((data) => setAttachments((current) => current.map((image) => image.id === id ? { ...image, data } : image)))
        .catch((error) => {
          setAttachments((current) => current.filter((image) => image.id !== id));
          setMessage(`${file.name}: ${error}`);
        });
    }
  };

  useLayoutEffect(() => {
    const available = optionsRow.current!;
    const copies = natural.current!;
    const fit = () => {
      const widths = [...copies.children].map((copy) => copy.getBoundingClientRect().width);
      const more = widths.pop()!;
      setInline(fittingCount(widths, more, available.getBoundingClientRect().width));
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(available);
    observer.observe(copies);
    return () => observer.disconnect();
  }, [thread?.configOptions]);

  const row = Math.min(highlight, matches.length - 1);
  return (
    <div className="editor relative flex shrink-0 flex-col border-t border-divider bg-editor px-4 py-3" onDragOver={(event) => event.preventDefault()} onDrop={onDrop}>
      {matches.length > 0 && (
        <Popover
          anchor={textarea}
          align="start"
          className="command-list max-h-60 min-w-80 max-w-[min(480px,calc(100vw-16px))] overflow-y-auto p-1"
        >
          {matches.map((command, index) => (
            <div
              key={command.name}
              className={"command flex cursor-default gap-4 rounded px-3 py-1.5" + (index === row ? " highlighted bg-control" : "")}
              onMouseEnter={() => setHighlight(index)}
              onMouseDown={(event) => {
                // Keeps the textarea focused.
                event.preventDefault();
                insert(command);
              }}
            >
              <span className="buffer min-w-24 shrink-0 font-mono">/{command.name}</span>
              <span className="dim min-w-0 truncate text-fg-dim">{command.description}</span>
            </div>
          ))}
        </Popover>
      )}
      {attachments.length > 0 && (
        <div className="attachments mb-2 flex flex-wrap gap-2">
          {attachments.map((image) => (
            <div key={image.id} className="chip flex max-w-60 items-center gap-1.5 rounded bg-raised py-1 pr-1 pl-2">
              <ImageGlyph />
              <span className="label min-w-0 truncate">{image.name}</span>
              {image.data === null && <span className="loading text-fg-dim">Loading…</span>}
              <button
                className="remove flex size-5 shrink-0 items-center justify-center rounded text-fg-dim hover:bg-control-hover hover:text-fg"
                title="Remove"
                onClick={() => setAttachments((current) => current.filter((attachment) => attachment.id !== image.id))}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}
      <textarea
        ref={textarea}
        className="min-h-20 w-full resize-none bg-transparent font-mono text-ui text-fg outline-none placeholder:text-fg-dim"
        placeholder={commands.length > 0 ? "Message agent — / for commands" : "Message agent"}
        value={text}
        onChange={(event) => changeText(event.target.value)}
        onKeyDown={onKeyDown}
      />
      {message !== undefined && <div className="editor-message py-1 text-danger">{message}</div>}
      <div className="editor-actions relative flex min-w-0 items-center gap-2">
        {thread?.usage != null && <UsageIndicator usage={thread.usage} />}
        <div ref={optionsRow} className="editor-options flex min-w-0 flex-1 items-center justify-end gap-1">
          {options.slice(0, inline).map((option) => (
            <div key={option.id} className="shrink-0">
              <ConfigPicker option={option} onChange={(value) => setConfigOption(option.id, value)} />
            </div>
          ))}
          {inline < options.length && (
            <MoreOptions options={options.slice(inline)} onChange={setConfigOption} />
          )}
        </div>
        <div className="invisible absolute size-0 overflow-hidden" aria-hidden inert>
          <div ref={natural} className="flex w-max">
            {options.map((option) => (
              <div key={option.id} className="shrink-0">
                <ConfigPicker option={option} onChange={() => {}} />
              </div>
            ))}
            <MoreOptions options={[]} onChange={() => {}} />
          </div>
        </div>
        {running ? (
          <button className="shrink-0 whitespace-nowrap rounded border border-control-edge bg-control px-3 py-1 hover:bg-control-hover" onClick={stop}>Stop</button>
        ) : (
          <button className="shrink-0 whitespace-nowrap rounded border border-control-edge bg-control px-3 py-1 hover:bg-control-hover disabled:opacity-50" disabled={attachments.some((image) => image.data === null)} onClick={send}>Send</button>
        )}
      </div>
    </div>
  );
}

/**
 * How many pickers, of `widths`, fit in order in `available` pixels, leaving
 * room for the More button, `more` pixels wide, when some do not.
 */
function fittingCount(widths: number[], more: number, available: number): number {
  const width = (count: number) =>
    widths.slice(0, count).reduce((sum, picker) => sum + picker + PICKER_GAP, 0);
  if (width(widths.length) - PICKER_GAP <= available) {
    return widths.length;
  }
  let count = 0;
  while (count < widths.length && width(count + 1) + more <= available) {
    count++;
  }
  return count;
}

/** The More button and its menu of the config pickers that do not fit. */
function MoreOptions({
  options,
  onChange,
}: {
  options: SessionConfigOption[];
  onChange: (config_id: string, value: ConfigValue) => void;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const insideProps = useClickOutside(open, () => setOpen(false));
  return (
    <div {...insideProps} className="more-options shrink-0">
      <button
        ref={button}
        className="flex size-7 items-center justify-center rounded text-fg-muted hover:bg-control hover:text-fg"
        title="More settings"
        onClick={() => setOpen(!open)}
      >
        <Ellipsis size={16} strokeWidth={1.75} />
      </button>
      {open && (
        <Popover
          anchor={button}
          align="end"
          className="more-options-list flex min-w-60 max-w-[calc(100vw-16px)] flex-col p-1"
        >
          {options.map((option) => (
            <div key={option.id} className="flex items-center justify-between gap-4 pl-2">
              <span className="dim min-w-0 truncate text-fg-dim">{option.name}</span>
              <ConfigPicker option={option} onChange={(value) => onChange(option.id, value)} />
            </div>
          ))}
        </Popover>
      )}
    </div>
  );
}

/** Reads the file as base64 image content. */
function readImage(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = reader.result as string;
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

function ImageGlyph() {
  return (
    <svg className="icon shrink-0 text-fg-muted" width="14" height="14" viewBox="0 0 14 14" fill="none">
      <rect x="1.5" y="2.5" width="11" height="9" rx="1.5" stroke="currentColor" />
      <circle cx="5" cy="5.5" r="1" fill="currentColor" />
      <path d="M2 10.5 5.5 7l2.5 2.5L10 8l2.5 2.5" stroke="currentColor" />
    </svg>
  );
}
