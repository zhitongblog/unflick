use clap::Parser;

use unflick_lib::cli::{run_cli, Cli};
use unflick_lib::mcp::run_mcp_server;

fn main() {
    let cli = Cli::parse();

    // Mode 1: MCP server (--mcp flag)
    if cli.mcp {
        std::process::exit(run_mcp_server());
    }

    // Mode 2: CLI (subcommand provided)
    if cli.command.is_some() {
        std::process::exit(run_cli(cli));
    }

    // Mode 3: GUI (no subcommand, no flags)
    unflick_lib::run();
}
