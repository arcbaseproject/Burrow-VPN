import { createRoot } from "react-dom/client";
import "@fontsource-variable/archivo/standard.css";
import App from "./App";
import "./index.css";

const render = () => createRoot(document.getElementById("root")!).render(<App />);

// Outside Tauri (plain `npm run dev`), fake the backend so the UI can be previewed.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) import("./devMock").then(render);
else render();
