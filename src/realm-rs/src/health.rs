// health — HTTP-Status-Server (PORT_HTTP): GET /health, /status, /players.
// Minimal handgerollt (kein Web-Framework nötig): eine TCP-Verbindung,
// eine Request-Zeile, JSON-Antwort. Entspricht dem Übergangsstand
// (health.ts), ergänzt um realm_id und Version.
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::config::Config;
use crate::world::Shared;

fn body_for(
    path: &str,
    cfg: &Config,
    players_json: &serde_json::Value,
    player_count: usize,
    uptime_s: u64,
    tick: (f64, f64, u64),
) -> (u16, serde_json::Value) {
    match path {
        "/health" => (
            200,
            serde_json::json!({"ok": true, "realm_id": cfg.realm_id, "uptime_s": uptime_s}),
        ),
        "/status" => (
            200,
            serde_json::json!({
                "ok": true,
                "server_up": true,
                "realm_id": cfg.realm_id,
                "version": env!("CARGO_PKG_VERSION"),
                "uptime_s": uptime_s,
                "players": player_count,
                "tick": {
                    "interval_ms": cfg.tick_ms,
                    "last_ms": (tick.0 * 100.0).round() / 100.0,
                    "avg_ms": (tick.1 * 100.0).round() / 100.0,
                    "count": tick.2,
                },
            }),
        ),
        "/players" => (200, serde_json::json!({"players": players_json})),
        _ => (404, serde_json::json!({"ok": false, "error": "not found"})),
    }
}

fn response(status: u16, body: &serde_json::Value) -> String {
    let text = serde_json::to_string(body).unwrap_or_else(|_| "{}".into());
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
}

pub async fn serve(cfg: Arc<Config>, shared: Shared) -> Result<(), String> {
    let addrs = crate::config::bind_addrs(&cfg.health_bind_host, cfg.health_port)?;
    let mut listeners = Vec::with_capacity(addrs.len());
    for addr in &addrs {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("health listen {addr}: {e}"))?;
        log::info!("health on {addr} (/health, /status, /players)");
        listeners.push(listener);
    }
    let mut tasks: Vec<tokio::task::JoinHandle<Result<(), String>>> =
        Vec::with_capacity(listeners.len());
    for listener in listeners {
        let cfg = cfg.clone();
        let shared = shared.clone();
        tasks.push(tokio::spawn(async move {
            accept_loop(listener, cfg, shared).await
        }));
    }
    let (res, _, rest) = futures_util::future::select_all(tasks).await;
    for t in rest {
        t.abort();
    }
    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("health task: {e}")),
    }
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    cfg: Arc<Config>,
    shared: Shared,
) -> Result<(), String> {
    loop {
        let (mut sock, _) = listener
            .accept()
            .await
            .map_err(|e| format!("health accept: {e}"))?;
        let cfg = cfg.clone();
        let shared = shared.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let Ok(n) = sock.read(&mut buf).await else {
                return;
            };
            let head = String::from_utf8_lossy(&buf[..n]);
            let mut parts = head.split_whitespace();
            let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
            if method != "GET" {
                let _ = sock
                    .write_all(
                        response(
                            405,
                            &serde_json::json!({"ok":false,"error":"method not allowed"}),
                        )
                        .as_bytes(),
                    )
                    .await;
                return;
            }
            let world = shared.lock().await;
            let player_count = world.players.len();
            let uptime_s = world.started.elapsed().as_secs();
            let tick = (world.tick.last_ms, world.tick.avg_ms, world.tick.count);
            let players_json: Vec<serde_json::Value> = world
                .players
                .values()
                .map(|p| {
                    serde_json::json!({
                        "id": p.id, "name": p.name, "zone_id": p.zone_id,
                        "ping_ms": p.ping_ms,
                        "last_activity": p.last_activity.elapsed().as_secs(),
                    })
                })
                .collect();
            drop(world);
            let (status, body) = body_for(
                path,
                &cfg,
                &serde_json::Value::Array(players_json),
                player_count,
                uptime_s,
                tick,
            );
            let _ = sock.write_all(response(status, &body).as_bytes()).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            realm_id: 1,
            ws_port: 3001,
            health_port: 3002,
            ws_bind_host: String::new(),
            health_bind_host: String::new(),
            tick_ms: 100,
            aofb_radius: 20.0,
            render_cap: 64,
            ollama_url: String::new(),
            auth_api: crate::config::AuthApiConfig {
                url: String::new(),
                service_id: String::new(),
                secret: String::new(),
            },
            realm_db: crate::config::DbConfig {
                host: String::new(),
                port: 3306,
                user: String::new(),
                password: String::new(),
                database: String::new(),
            },
            migrations_dir: String::new(),
            allow_destructive: false,
            combat: crate::config::combat_config(&Default::default()),
            npc: crate::config::npc_config(&Default::default()),
            group: crate::group::GroupCfg::default(),
        }
    }

    #[test]
    fn health_shapes() {
        let c = cfg();
        let empty = serde_json::json!([]);
        let (s, b) = body_for("/health", &c, &empty, 0, 42, (0.0, 0.0, 0));
        assert_eq!(s, 200);
        assert_eq!(b["ok"], true);
        assert_eq!(b["realm_id"], 1);
        let (s, b) = body_for("/status", &c, &empty, 3, 42, (1.234, 1.1, 7));
        assert_eq!(s, 200);
        assert_eq!(b["players"], 3);
        assert_eq!(b["tick"]["count"], 7);
        let (s, _) = body_for("/nope", &c, &empty, 0, 0, (0.0, 0.0, 0));
        assert_eq!(s, 404);
    }
}
