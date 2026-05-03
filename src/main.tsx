import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// macOS: punch a hole through html/body so the NSView with mpv video,
// inserted *below* the WKWebView in `bring_up_video_pipeline`, is
// visible. The white-flash fix in index.html sets html+body to opaque
// `#030712` (with `!important`) — necessary on Windows where the
// WebView2 default white background flashes before CSS loads. On
// macOS that same opacity hides the video entirely. The native
// NSWindow's `backgroundColor: "#030712"` already paints dark before
// React mounts, so dropping body opacity here is safe and uses Cocoa's
// own backdrop instead of HTML for the dark fill.
if (/Mac/i.test(navigator.userAgent)) {
  const clear = (el: HTMLElement) => {
    el.style.setProperty("background", "transparent", "important");
    el.style.setProperty("background-color", "transparent", "important");
  };
  clear(document.documentElement);
  clear(document.body);
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
