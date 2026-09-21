import React from "react";
import ReactDOM from "react-dom/client";
// Bundled, not fetched. The app promises that nothing leaves the computer, and
// a webfont request to a CDN would be the one thing on screen that did.
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-serif/400.css";
import "@fontsource/ibm-plex-serif/400-italic.css";
import "@fontsource/ibm-plex-serif/500.css";
import "@mimic/ui/tokens.css";
import "@mimic/ui/ui.css";
import "./styles/global.css";
import { App } from "./app/App";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
