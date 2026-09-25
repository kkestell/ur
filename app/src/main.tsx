import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// No StrictMode: its doubled effects would attach the terminal twice.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
