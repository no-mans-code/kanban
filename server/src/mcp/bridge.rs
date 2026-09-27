//! `kanban-server mcp`: the stdio transport. Reads one JSON-RPC message per
//! line from stdin, forwards it to a running board's `POST /mcp`, and writes
//! each reply as one line to stdout. Keeping the board as the only process
//! that touches the database means changes made by agents appear live in
//! every open browser. Nothing but protocol messages may go to stdout.

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

pub fn run<R: BufRead, W: Write>(
    input: R,
    mut output: W,
    target: &Target,
    user: Option<&str>,
) -> std::io::Result<()> {
    if let Some(u) = user
        && u.chars().any(|c| c.is_control())
    {
        return Err(std::io::Error::other("--as must be a plain username"));
    }
    for line in input.lines() {
        let line = line?;
        let message = line.trim();
        if message.is_empty() {
            continue;
        }
        let reply = match post(target, message, user) {
            Ok(reply) => reply,
            Err(problem) => unreachable_reply(message, target, &problem),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// Requests (not notifications) must still get an answer when the board is down.
fn unreachable_reply(message: &str, target: &Target, problem: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(message).ok()?;
    let id = parsed.get("id")?.clone();
    parsed.get("method")?;
    let text = format!(
        "Kanban board not reachable at {} ({problem}). Start it first: run-windows.cmd, ./run-unix.sh, \
         ./run-macos.sh or `docker compose up -d`.",
        target.url()
    );
    Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": text } }).to_string())
}

fn post(target: &Target, body: &str, user: Option<&str>) -> Result<Option<String>, String> {
    let addr = (target.host.trim_matches(['[', ']']), target.port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("host did not resolve")?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(300))).map_err(|e| e.to_string())?;
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        target.host,
        target.port,
        body.len()
    );
    if let Some(u) = user {
        request.push_str(&format!("{}: {u}\r\n", super::USER_HEADER));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("malformed HTTP response")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let mut payload = raw[split + 4..].to_vec();
    if head.contains("transfer-encoding: chunked") {
        payload = dechunk(&payload)?;
    }
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("no HTTP status")?;
    match status {
        202 => Ok(None),
        200 => {
            // Re-encode compactly: one message per line, as stdio requires.
            let value: Value = serde_json::from_slice(&payload).map_err(|e| e.to_string())?;
            Ok(Some(value.to_string()))
        }
        other => Err(format!("HTTP {other}: {}", String::from_utf8_lossy(&payload).trim())),
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
        run(input.as_bytes(), &mut out, &target, None).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1, "the notification gets no reply");
        assert!(text.contains("not reachable"));
    }
}
