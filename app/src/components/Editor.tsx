import type { AgentCapabilities, AvailableCommand, ContentBlock } from "@agentclientprotocol/sdk";
import { type DragEvent, type KeyboardEvent, useState } from "react";
import { request } from "../ipc";
import type { Status } from "../ipc/bindings/Status";
import { matchingCommands, slashQuery } from "../slash";
import type { ThreadState } from "../transcript/blocks";
import { type ConfigValue, ConfigPicker } from "./ConfigPicker";
import { UsageIndicator } from "./UsageIndicator";

/** An image file dropped on the editor, sent with the next prompt. */
export type ImageAttachment = { name: string; mimeType: string; data: string };

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
  const [message, setMessage] = useState<string>();
  // The command list's highlighted row, and whether Escape or an insertion
  // closed the list until the text changes.
  const [highlight, setHighlight] = useState(0);
  const [listClosed, setListClosed] = useState(false);
  const running = status?.type === "working" || status?.type === "needs_permission";
  const commands = thread?.commands ?? [];
  const query = slashQuery(text);
  const matches = query === undefined || listClosed ? [] : matchingCommands(commands, query);

  const changeText = (next: string) => {
    setText(next);
    setListClosed(false);
    setHighlight(0);
  };

  const send = () => {
    if (text === "" && attachments.length === 0) {
      return;
    }
    const sent = { text, attachments };
    const content: ContentBlock[] = [];
    if (text !== "") {
      content.push({ type: "text", text });
    }
    for (const image of attachments) {
      content.push({ type: "image", mimeType: image.mimeType, data: image.data });
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
      readImage(file)
        .then((image) => setAttachments((current) => [...current, image]))
        .catch((error) => setMessage(`${file.name}: ${error}`));
    }
  };

  const row = Math.min(highlight, matches.length - 1);
  return (
    <div className="editor" onDragOver={(event) => event.preventDefault()} onDrop={onDrop}>
      {matches.length > 0 && (
        <div className="popover command-list">
          {matches.map((command, index) => (
            <div
              key={command.name}
              className={"command" + (index === row ? " highlighted" : "")}
              onMouseEnter={() => setHighlight(index)}
              onMouseDown={(event) => {
                // Keeps the textarea focused.
                event.preventDefault();
                insert(command);
              }}
            >
              <span className="buffer">/{command.name}</span>
              <span className="dim">{command.description}</span>
            </div>
          ))}
        </div>
      )}
      {attachments.length > 0 && (
        <div className="attachments">
          {attachments.map((image, index) => (
            <div key={index} className="chip">
              <ImageGlyph />
              <span className="label">{image.name}</span>
              <button
                className="remove"
                title="Remove"
                onClick={() => setAttachments((current) => current.filter((_, i) => i !== index))}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}
      <textarea
        placeholder={commands.length > 0 ? "Message agent — / for commands" : "Message agent"}
        value={text}
        onChange={(event) => changeText(event.target.value)}
        onKeyDown={onKeyDown}
      />
      {message !== undefined && <div className="editor-message">{message}</div>}
      <div className="editor-actions">
        {thread?.usage != null && <UsageIndicator usage={thread.usage} />}
        {(thread?.configOptions ?? []).map((option) => (
          <ConfigPicker
            key={option.id}
            option={option}
            onChange={(value) => setConfigOption(option.id, value)}
          />
        ))}
        {running ? <button onClick={stop}>Stop</button> : <button onClick={send}>Send</button>}
      </div>
    </div>
  );
}

/** Reads the file as base64 image content. */
function readImage(file: File): Promise<ImageAttachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = reader.result as string;
      resolve({ name: file.name, mimeType: file.type, data: url.slice(url.indexOf(",") + 1) });
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

function ImageGlyph() {
  return (
    <svg className="icon" width="14" height="14" viewBox="0 0 14 14" fill="none">
      <rect x="1.5" y="2.5" width="11" height="9" rx="1.5" stroke="currentColor" />
      <circle cx="5" cy="5.5" r="1" fill="currentColor" />
      <path d="M2 10.5 5.5 7l2.5 2.5L10 8l2.5 2.5" stroke="currentColor" />
    </svg>
  );
}
