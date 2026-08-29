import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Overlay from "./features/overlay/Overlay";
import Picker from "./features/picker/Picker";
import "./design/tokens.css";
import "./design/app.css";

/**
 * All Kestrel windows load this one bundle and choose a root component from
 * the query string. Tauri creates windows by URL, so this is cheaper than a
 * multi-page build and keeps a single design-token stylesheet.
 */
const params = new URLSearchParams(window.location.search);
const view = params.get("view") ?? "main";
const number = (key: string, fallback = 0) => {
  const value = Number(params.get(key));
  return Number.isFinite(value) ? value : fallback;
};

function root() {
  switch (view) {
    case "overlay":
      // The overlay window must not paint a background of its own, or the
      // transparent NSWindow / WebView2 surface below it is hidden.
      document.documentElement.classList.add("is-overlay");
      return (
        <Overlay
          origin={{ x: number("x"), y: number("y") }}
          size={{ width: number("w", window.innerWidth), height: number("h", window.innerHeight) }}
        />
      );
    case "picker":
      return <Picker initialTab={params.get("tab") === "displays" ? "displays" : "windows"} />;
    default:
      return <App />;
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{root()}</React.StrictMode>,
);
