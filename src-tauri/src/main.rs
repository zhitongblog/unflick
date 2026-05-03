// Hide console window in release GUI mode on Windows
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use clap::Parser;

use unflick_lib::cli::{run_cli, Cli};
use unflick_lib::mcp::run_mcp_server;

/// Env var the GUI reads on startup to auto-play a file. Set when Windows
/// invokes us via "Open with..." / a file association — Explorer passes the
/// chosen file as argv[1], which clap would otherwise reject as an unknown
/// subcommand.
const PENDING_FILE_ENV: &str = "UNFLICK_OPEN_FILE";

fn main() {
    // Linux: force GTK to the X11 backend so our LinuxVideoSurface
    // (which uses XCreateSimpleWindow + GLX) gets a Xlib window
    // handle from Tauri instead of Wayland. Tauri's webkit2gtk-4.1
    // stack also runs fine through Xwayland on Wayland sessions —
    // GNOME Wayland, WSLg, etc. Native Wayland support lands later;
    // for v0.8.2 we standardise on X11 to keep the surface code
    // single-path. User can opt-out by exporting GDK_BACKEND first.
    #[cfg(target_os = "linux")]
    unsafe {
        // Use libc::setenv directly because some early-init paths in
        // gdk / webkit might not see Rust's env::set_var if it
        // updates the in-process environ pointer asymmetrically with
        // glibc's cached one. setenv()'s third arg = 1 means
        // "overwrite" — but we still respect a user-provided value.
        if std::env::var_os("GDK_BACKEND").is_none() {
            let key = b"GDK_BACKEND\0".as_ptr() as *const std::os::raw::c_char;
            let val = b"x11\0".as_ptr() as *const std::os::raw::c_char;
            extern "C" {
                fn setenv(
                    name: *const std::os::raw::c_char,
                    value: *const std::os::raw::c_char,
                    overwrite: std::os::raw::c_int,
                ) -> std::os::raw::c_int;
            }
            setenv(key, val, 1);
        }

        // mpv refuses to initialise when LC_NUMERIC is anything other
        // than "C" — its config parser has historically been bitten by
        // locales that use "," as a decimal separator (de_DE, fr_FR,
        // zh_CN.UTF-8 with certain glibc builds…). On Linux this fails
        // silently with mpv_create returning NULL plus the warning
        // "Non-C locale detected. This is not supported." spammed by
        // libmpv. Pin LC_NUMERIC before any mpv call so the user's
        // chosen locale stays for everything else (currency, dates,
        // collation) — only the numeric part is forced to C.
        let lc_key = b"LC_NUMERIC\0".as_ptr() as *const std::os::raw::c_char;
        let lc_val = b"C\0".as_ptr() as *const std::os::raw::c_char;
        let lc_all_key = b"LC_ALL\0".as_ptr() as *const std::os::raw::c_char;
        extern "C" {
            fn setenv(
                name: *const std::os::raw::c_char,
                value: *const std::os::raw::c_char,
                overwrite: std::os::raw::c_int,
            ) -> std::os::raw::c_int;
            fn setlocale(
                category: std::os::raw::c_int,
                locale: *const std::os::raw::c_char,
            ) -> *mut std::os::raw::c_char;
        }
        // LC_ALL takes precedence over LC_NUMERIC, so make sure it's
        // not pinning some other value first.
        let _ = std::env::var_os("LC_ALL").map(|_| {
            setenv(lc_all_key, b"\0".as_ptr() as *const _, 1);
        });
        setenv(lc_key, lc_val, 1);
        // Apply to the live C runtime too — env vars only take effect
        // for processes that read them again, but we already loaded
        // libc before this point, so call setlocale directly.
        const LC_NUMERIC: std::os::raw::c_int = 1;
        setlocale(LC_NUMERIC, lc_val);
    }

    // Special case before clap: a single positional arg that points at a
    // real file means "Explorer asked us to open this." Bypass CLI parsing
    // and route to the GUI with the path stashed for the frontend.
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() == 2 {
        let arg = &raw_args[1];
        if !arg.starts_with('-') && std::path::Path::new(arg).is_file() {
            std::env::set_var(PENDING_FILE_ENV, arg);
            unflick_lib::run();
            return;
        }
    }

    let cli = Cli::parse();

    // Mode 1: MCP server (--mcp flag)
    if cli.mcp {
        // Re-attach console for CLI/MCP modes
        #[cfg(target_os = "windows")]
        unsafe { winapi_attach_console(); }
        std::process::exit(run_mcp_server());
    }

    // Mode 2: CLI (subcommand provided)
    if cli.command.is_some() {
        #[cfg(target_os = "windows")]
        unsafe { winapi_attach_console(); }
        std::process::exit(run_cli(cli));
    }

    // Mode 3: GUI (no subcommand, no flags).
    // Try to attach to a parent console when there is one — lets us see
    // eprintln!() output (render-thread errors, mpv init failures, etc.)
    // when the user launches from cmd / bash for diagnostics. Returns 0
    // when there's no console to attach to (normal Explorer launch),
    // which is harmless.
    #[cfg(target_os = "windows")]
    unsafe { winapi_attach_console(); }
    init_file_log();
    unflick_lib::run();
}

/// Set up a best-effort log file at `%TEMP%/unflick.log` that captures
/// eprintln output via a redirected stderr. Useful when there's no parent
/// console (Explorer / file-association launches). Failures are silent —
/// running without logs is fine.
fn init_file_log() {
    let path = std::env::temp_dir().join("unflick.log");
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        // Line a header so each run is visible in the rolling log.
        let _ = std::io::Write::write_all(
            &mut std::io::stderr(),
            format!(
                "\n=== unflick {} starting at {} ===\n",
                env!("CARGO_PKG_VERSION"),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_else(|_| "?".into()),
            )
            .as_bytes(),
        );
        // Best-effort stderr redirect on Windows. We re-open stderr to point
        // at the log file so all the eprintln in render_loop / lib.rs land
        // there. If we attached to a parent console above, those eprintlns
        // will *also* show in the console because of how Windows file
        // descriptors work — that's fine.
        #[cfg(target_os = "windows")]
        unsafe {
            use std::os::windows::io::AsRawHandle;
            #[link(name = "kernel32")]
            extern "system" {
                fn SetStdHandle(std_handle: u32, handle: *mut std::ffi::c_void) -> i32;
            }
            const STD_ERROR_HANDLE: u32 = 0xFFFFFFF4; // -12 as u32
            let h = f.as_raw_handle() as *mut std::ffi::c_void;
            SetStdHandle(STD_ERROR_HANDLE, h);
            // Leak the file handle so it stays open for the program lifetime.
            std::mem::forget(f);
        }
    }
}

/// Re-attach to the parent console so CLI output is visible
#[cfg(target_os = "windows")]
unsafe fn winapi_attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
    AttachConsole(ATTACH_PARENT_PROCESS);
}
