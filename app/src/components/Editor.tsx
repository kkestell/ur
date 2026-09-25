import { type KeyboardEvent, useState } from "react";
import { request } from "../ipc";
import type { Status } from "../ipc/bindings/Status";

export function Editor({ session, status }: { session: string; status: Status | undefined }) {
  const [text, setText] = useState("");
  const [message, setMessage] = useState<string>();
  const running = status?.type === "working" || status?.type === "needs_permission";

  const send = () => {
    if (text === "") {
      return;
    }
    setMessage(undefined);
    setText("");
    request({ type: "prompt", session, content: [{ type: "text", text }] })
      .then((response) => {
        if (response.type === "busy") {
          setMessage("The session is busy.");
        } else if (response.type === "error") {
          setMessage(response.message);
        }
      })
      .catch((error) => setMessage(String(error)));
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

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
    }
  };

  return (
    <div className="editor">
      <textarea
        placeholder="Message agent"
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={onKeyDown}
      />
      {message !== undefined && <div className="editor-message">{message}</div>}
      <div className="editor-actions">
        {running ? <button onClick={stop}>Stop</button> : <button onClick={send}>Send</button>}
      </div>
    </div>
  );
}
