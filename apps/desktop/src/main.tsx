import React from "react";
import ReactDOM from "react-dom/client";
import "@mimic/ui/tokens.css";
import "@mimic/ui/ui.css";
import "./styles/global.css";
import { App } from "./app/App";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
