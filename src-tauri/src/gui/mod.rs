pub mod commands;
pub mod state;
pub mod window;
// ─── Track A part 2 — GUI self-test bridge ────────────────────────────
/// The webview side of `core::dev`: running a program in the window and
/// getting the value back out.
pub mod dev;
/// The one part of the bridge that is per-platform: a picture of the
/// interface layer.
pub mod dev_capture;
