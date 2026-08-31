use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::McpServer;

pub fn tool_primer(servers: &[McpServer]) -> Option<String> {
    let live: Vec<&McpServer> = servers.iter().filter(|s| s.enabled && !s.command.is_empty()).collect();
    if live.is_empty() {
        return None;
    }
    let mut lines = vec![
        "Connected MCP servers (tools are listed when reachable):".to_string(),
    ];
    for server in live {
        match list_tools(server) {
            Ok(tools) if !tools.is_empty() => {
                lines.push(format!("- {} → {}", server.name, tools.join(", ")));
            }
            Ok(_) => lines.push(format!("- {} (no tools advertised)", server.name)),
            Err(err) => lines.push(format!("- {} unreachable ({err})", server.name)),
        }
    }
    Some(lines.join("\n"))
}

fn list_tools(server: &McpServer) -> Result<Vec<String>, String> {
    if !is_safe_mcp_command(&server.command) {
        return Err("MCP command is not allowed".into());
    }
    if server.args.iter().any(|a| a.len() > 200) {
        return Err("MCP args too long".into());
    }
    let mut child = Command::new(&server.command)
        .args(&server.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;

    let init = rpc(1, "initialize", json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "flint", "version": "0.1.0"}
    }));
    let list = rpc(2, "tools/list", json!({}));

    {
        let stdin = child.stdin.as_mut().ok_or("no stdin")?;
        write_msg(stdin, &init)?;
        write_msg(stdin, &list)?;
    }

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut tools = Vec::new();
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(4) {
        let Some(body) = read_msg(&mut reader)? else {
            break;
        };
        if let Ok(value) = serde_json::from_str::<Value>(&body) {
            if value.get("id") == Some(&json!(2)) {
                if let Some(list) = value.pointer("/result/tools").and_then(Value::as_array) {
                    for tool in list {
                        if let Some(name) = tool.get("name").and_then(Value::as_str) {
                            tools.push(name.to_string());
                        }
                    }
                }
                break;
            }
        }
    }
    let _ = child.kill();
    Ok(tools)
}

fn is_safe_mcp_command(command: &str) -> bool {
    matches!(command, "npx")
}

fn rpc(id: u32, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

fn write_msg<W: Write>(w: &mut W, value: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len()).map_err(|e| e.to_string())?;
    w.write_all(&body).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

fn read_msg<R: BufRead>(reader: &mut R) -> Result<Option<String>, String> {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        headers.push_str(&line);
    }
    let len = headers
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|v| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    if len == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
    String::from_utf8(buf).map(Some).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::rpc;
    use serde_json::json;

    #[test]
    fn rpc_envelope() {
        let msg = rpc(1, "initialize", json!({}));
        assert_eq!(msg["method"], "initialize");
        assert_eq!(msg["id"], 1);
    }

    #[test]
    fn mcp_command_allowlist() {
        assert!(super::is_safe_mcp_command("npx"));
        assert!(!super::is_safe_mcp_command("npx;rm"));
        assert!(!super::is_safe_mcp_command("/usr/bin/npx"));
        assert!(!super::is_safe_mcp_command("bash"));
    }
}
