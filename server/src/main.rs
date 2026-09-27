use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use kanban_server::auth::new_setup_code;
use kanban_server::{AppState, demo, mcp, open_db, router};
use tracing_subscriber::EnvFilter;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// `kanban-server healthcheck`: exit 0 if the local server answers
/// /api/health. Used by the Docker HEALTHCHECK, so the image needs no curl.
fn healthcheck(port: u16) -> ! {
    let ok = (|| -> std::io::Result<bool> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response.starts_with("HTTP/1.0 200") || response.starts_with("HTTP/1.1 200"))
    })()
    .unwrap_or(false);
    std::process::exit(if ok { 0 } else { 1 })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port: u16 = env_or("KANBAN_PORT", "8610").parse()?;
    if args.iter().any(|a| a == "healthcheck") {
        healthcheck(port);
    }
    if args.first().is_some_and(|a| a == "mcp") {
        // stdio MCP transport. Runs before logging is set up: stdout carries
        // only protocol messages.
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        let url = value("--url")
            .or_else(|| std::env::var("KANBAN_MCP_URL").ok())
            .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
        let token = value("--token").or_else(|| std::env::var("KANBAN_MCP_TOKEN").ok()).ok_or_else(|| {
            anyhow::anyhow!(
                "no API token given. Pass --token kbn_... or set KANBAN_MCP_TOKEN. \
                 Create one for your account from the board's Settings > Tokens page."
            )
        })?;
        let target = mcp::bridge::Target::parse(&url).map_err(anyhow::Error::msg)?;
        let stdin = std::io::stdin().lock();
        mcp::bridge::run(stdin, std::io::stdout().lock(), &target, &token)?;
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "kanban_server=info,tower_http=warn".into()),
        )
        .init();

    let seed_demo = args.iter().any(|a| a == "--demo") || env_or("KANBAN_DEMO", "0") == "1";
    let db_path = PathBuf::from(env_or("KANBAN_DB", "data/kanban.db"));
    let ip: IpAddr = env_or("KANBAN_BIND", "127.0.0.1").parse()?;
    if !ip.is_loopback() {
        tracing::warn!(
            "binding to {ip}: make sure KANBAN_ALLOWED_HOSTS is set, and that you understand every account's password/token reaches this address"
        );
    }

    let db = open_db(&db_path).await?;
    let mut state = AppState::new(db);
    state.cookie_secure = env_or("KANBAN_COOKIE_SECURE", "0") == "1";
    let app = router(state.clone());
    demo::bootstrap(&app).await?;
    if seed_demo {
        demo::seed(&app).await?;
    }

    if kanban_server::auth::setup_required(&state.db).await {
        let code = new_setup_code();
        *state.setup_code.lock().expect("setup code lock") = Some(code.clone());
        tracing::warn!("no administrator yet — open the board and enter this one-time setup code: {code}");
    }

    let addr = SocketAddr::new(ip, port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("kanban board on http://{addr} (database: {})", db_path.display());
    // No graceful shutdown: open SSE streams never finish on their own, so it
    // would hang Ctrl+C. SQLite in WAL mode is safe to stop at any point.
    axum::serve(listener, app).await?;
    Ok(())
}
