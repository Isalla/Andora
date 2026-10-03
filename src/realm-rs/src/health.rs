// health — HTTP-Status-Server (PORT_HTTP): GET /health, /status, /players.
// Minimal handgerollt (kein Web-Framework nötig): eine TCP-Verbindung,
// eine Request-Zeile, JSON-Antwort. Entspricht dem Übergangsstand
// (health.ts), ergänzt um realm_id und Version.
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::config::Config;
use crate::spool::PersistRuntime;
use crate::spool::PersistStatus;
use crate::world::Shared;

/// Die beiden **rein lesend** erhobenen Beobachtungswerte der `/status`
///-Antwort (`P-30` Quarantäne, `P-23` Persistenzstatus). Sie werden im
/// Handler ausschließlich gelesen und hier nur weitergereicht.
struct StatusObservation {
    quarantine_unattributed: Option<usize>,
    persistence: PersistStatus,
}

fn body_for(
    path: &str,
    cfg: &Config,
    players_json: &serde_json::Value,
    player_count: usize,
    uptime_s: u64,
    tick: (f64, f64, u64),
    observed: StatusObservation,
) -> (u16, serde_json::Value) {
    let StatusObservation {
        quarantine_unattributed,
        persistence,
    } = observed;
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
                // P-23: beobachtbarer Persistenzstatus. Reine Beobachtung:
                // steuert nichts und ist keine Spielfreigabe. Werte:
                // `recovering` | `ready` | `degraded` (PersistStatus::as_str).
                "persistence_status": persistence.as_str(),
                "tick": {
                    "interval_ms": cfg.tick_ms,
                    "last_ms": (tick.0 * 100.0).round() / 100.0,
                    "avg_ms": (tick.1 * 100.0).round() / 100.0,
                    "count": tick.2,
                },
                // P-30: Zahl der nicht zuordenbaren Quarantänedateien.
                // `null` = Bestand nicht ermittelbar (nicht "keine Fälle").
                "quarantine_unattributed": quarantine_unattributed,
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

pub async fn serve(
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
) -> Result<(), String> {
    let addrs = crate::config::bind_addrs(&cfg.health_bind_host, cfg.health_port)?;
    let mut listeners = Vec::with_capacity(addrs.len());
    for addr in &addrs {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("health listen {addr}: {e}"))?;
        log::info!("health on {addr} (/health, /status, /players)");
        listeners.push(listener);
    }
    serve_bound(listeners, cfg, shared, persist).await
}

/// Kern des Servers: je bereits gebundener Listener der vorhandene
/// `accept_loop`; endet einer, werden die übrigen Tasks abgebrochen. Produktion
/// und Test nutzen denselben Kern — es gibt keine zweite Serverimplementierung.
async fn serve_bound(
    listeners: Vec<tokio::net::TcpListener>,
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
) -> Result<(), String> {
    let mut tasks: Vec<tokio::task::JoinHandle<Result<(), String>>> =
        Vec::with_capacity(listeners.len());
    for listener in listeners {
        let cfg = cfg.clone();
        let shared = shared.clone();
        let persist = persist.clone();
        tasks.push(tokio::spawn(async move {
            accept_loop(listener, cfg, shared, persist).await
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

/// `P-23`: Startstelle des Monitoring. Der Startpfad ruft **diese** Funktion
/// **vor** der Startup-Recovery auf; der Integrationstest nutzt dieselbe
/// Funktion (mit eigenem, durch echte Bindung ermitteltem Listener), damit die
/// Startreihenfolge durch denselben Produktionspfad abgesichert ist.
///
/// Handle, Fehlerbehandlung und der Shutdown-`abort()` bleiben beim Aufrufer —
/// so entsteht keine verwaiste Monitoring-Task.
pub fn spawn_monitor(
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = serve(cfg, shared, persist).await {
            log::error!("health: {e}");
        }
    })
}

/// Wie `spawn_monitor`, aber auf einem **bereits gebundenen** Listener. Nur für
/// den Test: der Listener entsteht dort durch eine echte Bindung auf Port 0,
/// also ohne Portraten und ohne Probe-Bindung mit anschließendem Freigeben.
/// Produktionscode ruft ausschließlich `spawn_monitor`/`serve` auf.
#[cfg(test)]
pub(crate) fn spawn_monitor_on(
    listener: tokio::net::TcpListener,
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = serve_bound(vec![listener], cfg, shared, persist).await {
            log::error!("health: {e}");
        }
    })
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
) -> Result<(), String> {
    loop {
        let (mut sock, _) = listener
            .accept()
            .await
            .map_err(|e| format!("health accept: {e}"))?;
        let cfg = cfg.clone();
        let shared = shared.clone();
        let persist = persist.clone();
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
            // P-30: ein reiner Lese-Scan je /status, kein Cache, keine
            // Mutation, keine Archivierung, keine Logausgabe, keine Sperre.
            let quarantine_unattributed = persist.unattributed_quarantine_count();
            // P-23: Persistenzstatus rein lesend beobachten. Der Zugriff
            // kopiert den Status unter dem kurzen Status-Mutex und **setzt**
            // nichts: kein Health-Request kann einen Zustandswechsel oder eine
            // Spielfreigabe auslösen.
            let persistence = persist.status();
            let (status, body) = body_for(
                path,
                &cfg,
                &serde_json::Value::Array(players_json),
                player_count,
                uptime_s,
                tick,
                StatusObservation {
                    quarantine_unattributed,
                    persistence,
                },
            );
            let _ = sock.write_all(response(status, &body).as_bytes()).await;
        });
    }
}

/// `P-23`: Minimale Testkonfiguration. `pub(crate)`, weil der
/// Integrationstest in `spool.rs` (Recovery geparkt im DB-Schritt) dieselbe
/// Konfiguration für den echten HTTP-Request benötigt — eine zweite, gleiche
/// Fixture wäre nur Duplikat. Reine Testhilfe, kein Produktionspfad.
#[cfg(test)]
pub(crate) fn test_config() -> Config {
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
        persist: crate::config::PersistCfg {
            player_persist_interval_ms: 900_000,
            drain_interval_ms: 500,
            persistence_dir: String::from("/tmp/realm-rs-persist-health"),
        },
        group: crate::group::GroupCfg::default(),
        inventory: Default::default(),
        loot: Default::default(),
        progression: crate::progression::ProgressionCfg::default(),
        security: crate::config::SecurityCfg::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        test_config()
    }

    fn observed(
        quarantine_unattributed: Option<usize>,
        persistence: PersistStatus,
    ) -> StatusObservation {
        StatusObservation {
            quarantine_unattributed,
            persistence,
        }
    }

    #[test]
    fn health_shapes() {
        let c = cfg();
        let empty = serde_json::json!([]);
        let (s, b) = body_for(
            "/health",
            &c,
            &empty,
            0,
            42,
            (0.0, 0.0, 0),
            observed(None, PersistStatus::Recovering),
        );
        assert_eq!(s, 200);
        assert_eq!(b["ok"], true);
        assert_eq!(b["realm_id"], 1);
        let (s, b) = body_for(
            "/status",
            &c,
            &empty,
            3,
            42,
            (1.234, 1.1, 7),
            observed(Some(0), PersistStatus::Ready),
        );
        assert_eq!(s, 200);
        assert_eq!(b["players"], 3);
        assert_eq!(b["tick"]["count"], 7);
        let (s, _) = body_for(
            "/nope",
            &c,
            &empty,
            0,
            0,
            (0.0, 0.0, 0),
            observed(None, PersistStatus::Ready),
        );
        assert_eq!(s, 404);
    }

    /// `P-23`: Alle drei Persistenzstatuswerte erscheinen im **bestehenden**
    /// `/status`-Feld `persistence_status`; die übrigen Felder bleiben erhalten.
    #[test]
    fn status_reports_every_persistence_status_value() {
        let c = cfg();
        let empty = serde_json::json!([]);
        for (status, expected) in [
            (PersistStatus::Recovering, "recovering"),
            (PersistStatus::Ready, "ready"),
            (PersistStatus::Degraded, "degraded"),
        ] {
            let (s, b) = body_for(
                "/status",
                &c,
                &empty,
                0,
                1,
                (0.0, 0.0, 0),
                observed(None, status),
            );
            assert_eq!(s, 200);
            assert_eq!(
                b["persistence_status"], expected,
                "Statuswert muss beobachtbar sein"
            );
            // Bestehende Felder bleiben unverändert erhalten.
            assert_eq!(b["ok"], true);
            assert_eq!(b["server_up"], true);
            assert_eq!(b["realm_id"], 1);
            assert_eq!(b["version"], env!("CARGO_PKG_VERSION"));
            assert_eq!(b["players"], 0);
            assert!(b["quarantine_unattributed"].is_null());
        }
    }

    /// `P-23`: `/health` bleibt Liveness und erhält **kein** Statusfeld — die
    /// Beobachtung des Persistenzstatus bleibt ausschließlich in `/status`.
    #[test]
    fn health_stays_liveness_without_status_field() {
        let c = cfg();
        let empty = serde_json::json!([]);
        let (s, b) = body_for(
            "/health",
            &c,
            &empty,
            0,
            7,
            (0.0, 0.0, 0),
            observed(None, PersistStatus::Recovering),
        );
        assert_eq!(s, 200);
        assert_eq!(b["ok"], true);
        assert_eq!(b["uptime_s"], 7);
        assert!(
            b.get("persistence_status").is_none(),
            "/health bleibt Liveness ohne Readiness-Semantik"
        );
        // Auch /players wird nicht umgestaltet.
        let (s, b) = body_for(
            "/players",
            &c,
            &empty,
            0,
            7,
            (0.0, 0.0, 0),
            observed(None, PersistStatus::Recovering),
        );
        assert_eq!(s, 200);
        assert!(b.get("persistence_status").is_none());
    }
}
