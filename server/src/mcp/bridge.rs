//! `kanban-server mcp`: the stdio transport. Reads one JSON-RPC message per
//! line from stdin, forwards it to a running board's `POST /mcp` with the
//! given API token, and writes each reply as one line to stdout. Keeping the
//! board as the only process that touches the database means changes made
//! by agents appear live in every open browser. Nothing but protocol
//! messages may go to stdout.

use std::io::{BufRead, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{Value, json};

pub struct Target {
    pub host: String,
    pub port: u16,
}

impl Target {
    /// Accepts `http://host:port` (any path is ignored; `/mcp` is used).
    pub fn parse(url: &str) -> Result<Target, String> {
        let rest =
            url.strip_prefix("http://").ok_or("only http:// URLs are supported (the board runs locally)")?;
        let authority = rest.split('/').next().unwrap_or(rest);
        // An IPv6 host is bracketed ("[::1]:8610"), so only a colon after the
        // closing bracket starts the port.
        let (host, port) = match authority.find(']') {
            Some(end) => (&authority[..=end], authority[end + 1..].strip_prefix(':')),
            None => match authority.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (authority, None),
            },
        };
        let port = match port {
            Some(p) => p.parse().map_err(|_| format!("bad port in {url}"))?,
            None => 80,
        };
        let host = host.to_string();
        if host.is_empty() {
            return Err(format!("no host in {url}"));
        }
        Ok(Target { host, port })
    }

    fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// What went wrong trying to reach `/mcp`, so the JSON-RPC error we hand
/// back can say something more useful than "not reachable" when the board
/// answered but rejected the token.
enum Failure {
    /// Never got an HTTP response at all (board not running, wrong port, ...).
    Unreachable(String),
    /// The board answered with a non-2xx status. `message` is the board's
    /// own error message when it sent one (e.g. an expired token).
    Rejected { status: u16, message: Option<String> },
}

pub fn run<R: BufRead, W: Write>(
    input: R,
    mut output: W,
    target: &Target,
    token: &str,
) -> std::io::Result<()> {
    if token.trim().is_empty() || token.chars().any(|c| c.is_control()) {
        return Err(std::io::Error::other("a Kanban API token is required (--token, or KANBAN_MCP_TOKEN)"));
    }
    for line in input.lines() {
        let line = line?;
        let message = line.trim();
        if message.is_empty() {
            continue;
        }
        let reply = match post(target, message, token) {
            Ok(reply) => reply,
            Err(failure) => failure_reply(message, target, failure),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// Requests (not notifications) must still get an answer when something
/// goes wrong, so the calling agent can see why and, for a rejected token,
/// fix it rather than retry forever.
fn failure_reply(message: &str, target: &Target, failure: Failure) -> Option<String> {
    let parsed: Value = serde_json::from_str(message).ok()?;
    let id = parsed.get("id")?.clone();
    parsed.get("method")?;
    let text = match failure {
        Failure::Unreachable(problem) => format!(
            "Kanban board not reachable at {} ({problem}). Start it first: run-windows.cmd, ./run-unix.sh, \
             ./run-macos.sh or `docker compose up -d`.",
            target.url()
        ),
        Failure::Rejected { status: 401, message } => {
            format!(
                "The board rejected this API token ({}). Create a new one from Settings > Tokens.",
                message.unwrap_or_else(|| "unauthorized".into())
            )
        }
        Failure::Rejected { status: 403, message } => {
            format!(
                "This API token doesn't have access to do that ({}).",
                message.unwrap_or_else(|| "forbidden".into())
            )
        }
        Failure::Rejected { status, message } => {
            format!(
                "The board answered HTTP {status}{}.",
                message.map(|m| format!(": {m}")).unwrap_or_default()
            )
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": text } }).to_string())
}

fn post(target: &Target, body: &str, token: &str) -> Result<Option<String>, Failure> {
    let unreachable = |e: std::io::Error| Failure::Unreachable(e.to_string());
    let addr = (target.host.trim_matches(['[', ']']), target.port)
        .to_socket_addrs()
        .map_err(unreachable)?
        .next()
        .ok_or_else(|| Failure::Unreachable("host did not resolve".into()))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(unreachable)?;
    stream.set_read_timeout(Some(Duration::from_secs(300))).map_err(unreachable)?;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nAuthorization: Bearer {token}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        target.host,
        target.port,
        body.len()
    );
    stream.write_all(request.as_bytes()).map_err(unreachable)?;
    stream.write_all(body.as_bytes()).map_err(unreachable)?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(unreachable)?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| Failure::Unreachable("malformed HTTP response".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let mut payload = raw[split + 4..].to_vec();
    if head.contains("transfer-encoding: chunked") {
        payload = dechunk(&payload).map_err(Failure::Unreachable)?;
    }
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Failure::Unreachable("no HTTP status".into()))?;
    match status {
        202 => Ok(None),
        200 => {
            // Re-encode compactly: one message per line, as stdio requires.
            let value: Value =
                serde_json::from_slice(&payload).map_err(|e| Failure::Unreachable(e.to_string()))?;
            Ok(Some(value.to_string()))
        }
        other => {
            let message: Option<String> = serde_json::from_slice::<Value>(&payload)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from));
            Err(Failure::Rejected { status: other, message })
        }
    }
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").ok_or("bad chunk")?;
        let size_text = String::from_utf8_lossy(&data[..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "bad chunk size")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(data.get(..size).ok_or("truncated chunk")?);
        data = data.get(size + 2..).ok_or("truncated chunk")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls() {
        let t = Target::parse("http://127.0.0.1:8610").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("127.0.0.1", 8610));
        let t = Target::parse("http://localhost:9000/mcp").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("localhost", 9000));
        let t = Target::parse("http://[::1]:8610").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("[::1]", 8610));
        assert!(Target::parse("https://x").is_err());
    }

    #[test]
    fn dechunks() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
    }

    #[test]
    fn answers_requests_when_the_board_is_down() {
        // Port 9 (discard) on loopback: nothing listens there.
        let target = Target { host: "127.0.0.1".into(), port: 9 };
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";
        let mut out = Vec::new();
        run(input.as_bytes(), &mut out, &target, "kbn_test").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1, "the notification gets no reply");
        assert!(text.contains("not reachable"));
    }

    #[test]
    fn refuses_to_run_without_a_token() {
        let target = Target { host: "127.0.0.1".into(), port: 9 };
        assert!(run(std::io::empty(), Vec::new(), &target, "").is_err());
    }
}
