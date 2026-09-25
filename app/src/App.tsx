import { useState } from "react";
import { TerminalPane } from "./components/TerminalPane";

export default function App() {
  const [error, setError] = useState<string>();
  if (error !== undefined) {
    return <pre className="error">{error}</pre>;
  }
  return <TerminalPane onError={setError} />;
}
