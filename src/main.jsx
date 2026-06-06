import React from "react";
import ReactDOM from "react-dom/client";
// Self-hosted fonts: Mganga may run offline, so no CDN fonts (see mganga-brand).
import "@fontsource/bricolage-grotesque/700.css";
import "@fontsource/reddit-sans/400.css";
import "@fontsource/reddit-sans/500.css";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
