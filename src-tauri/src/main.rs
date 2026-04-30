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

    // Mode 3: GUI (no subcommand, no flags)
    unflick_lib::run();
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
