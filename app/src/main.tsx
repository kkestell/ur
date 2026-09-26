import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// A file dropped outside the editor would otherwise navigate the webview to
// it.
for (const type of ["dragover", "drop"]) {
  window.addEventListener(type, (event) => event.preventDefault());
}

// No StrictMode: its doubled effects would attach the terminal twice.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
