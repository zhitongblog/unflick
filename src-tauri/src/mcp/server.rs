use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::core::daemon;

use super::tools;

/// Run the MCP server over stdio (JSON-RPC 2.0).
/// Commands are routed to the daemon process.
pub fn run_mcp_server() -> i32 {
    // Ensure daemon is running
    if !daemon::is_daemon_running() {
        let exe = std::env::current_exe().unwrap();
        let _ = std::process::Command::new(exe)
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if daemon::is_daemon_running() {
                break;
            }
        }

        if !daemon::is_daemon_running() {
            eprintln!("failed to start daemon");
            return 1;
        }
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err_response = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("parse error: {}", e)
                    }
                });
                let _ = writeln!(stdout, "{}", err_response);
                let _ = stdout.flush();
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request["method"].as_str().unwrap_or("");

        let response = match method {
            "initialize" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "unflick",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                })
            }
            "notifications/initialized" => {
                continue;
            }
            "tools/list" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": tools::tool_definitions()
                    }
                })
            }
            "tools/call" => {
                let tool_name = request["params"]["name"].as_str().unwrap_or("");
                let arguments = request["params"].get("arguments").cloned().unwrap_or(json!({}));
                let result = tools::handle_tool_via_daemon(tool_name, &arguments);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })
            }
            _ => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("method not found: {}", method)
                    }
                })
            }
        };

        let _ = writeln!(stdout, "{}", response);
        let _ = stdout.flush();
    }

    0
}
