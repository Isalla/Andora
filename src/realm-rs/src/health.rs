// health — HTTP-Server (PORT_HTTP): GET /health, /status, /players sowie
// sessiongeprüfte eigene Charakterauswahl /characters (id + name).
// Minimal handgerollt (kein Web-Framework nötig): eine TCP-Verbindung,
// eine Request-Zeile, JSON-Antwort. Entspricht dem Übergangsstand
// (health.ts), ergänzt um realm_id und Version.
use std::collections::HashMap;
use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::auth_api::AuthApi;
use crate::config::Config;
use crate::db;
use crate::mail;
use crate::spool::PersistRuntime;
use crate::spool::PersistStatus;
use crate::world::Shared;
use sqlx::Pool;

#[derive(Clone)]
struct CharacterListContext {
    auth: crate::auth_api::AuthApi,
    pool: sqlx::Pool<sqlx::MySql>,
}

// Account binding comes exclusively from Auth, never from a query parameter.
async fn characters_for(
    target: &str,
    realm_id: u32,
    context: Option<&CharacterListContext>,
) -> (u16, serde_json::Value) {
    let error = |status, reason| (status, serde_json::json!({"error": reason}));
    let Ok(url) = reqwest::Url::parse(&format!("http://realm{target}")) else {
        return error(400, "invalid request");
    };
    let params: Vec<_> = url.query_pairs().collect();
    let sessions: Vec<_> = params.iter().filter(|(k, _)| k == "session_id").collect();
    let realms: Vec<_> = params.iter().filter(|(k, _)| k == "realm_id").collect();
    if realms.len() != 1 || realms[0].1.parse::<u32>().ok() != Some(realm_id) {
        return error(400, "wrong realm");
    }
    if sessions.len() != 1 || sessions[0].1.is_empty() {
        return error(401, "invalid session");
    }
    let Some(context) = context.filter(|c| c.auth.enabled()) else {
        return error(503, "auth unavailable");
    };
    let session = match context.auth.validate_session(&sessions[0].1).await {
        Ok(session) => session,
        Err(_) => return error(503, "auth unavailable"),
    };
    let Some(account_id) = session.account_id.filter(|id| session.valid && *id > 0) else {
        return error(401, "invalid session");
    };
    match crate::db::list_characters(&context.pool, account_id).await {
        Ok(characters) => (200, serde_json::json!({"characters": characters})),
        Err(_) => error(503, "characters unavailable"),
    }
}

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
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        503 => "Service Unavailable",
        _ => "Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
}

/// Interne Mail-Endpunkte (Go-API-Forward, service-authentisiert).
/// Der Realm vertraut dem Go-API, das die Spielersession bereits geprüft hat;
/// die Service-Signatur ist die Trust-Grenze. Charaktereigentum wird
/// gegen die Realm-DB geprüft.
async fn handle_internal_mail(
    cfg: &Config,
    pool: &sqlx::Pool<sqlx::MySql>,
    spool: &crate::spool::Spool,
    shared: Shared,
    method: &str,
    path: &str,
    body: &[u8],
    headers: &HashMap<String, String>,
) -> Result<(u16, serde_json::Value), String> {
    let error = |status, reason| (status, serde_json::json!({"ok": false, "error": reason}));

    // Service-Auth: X-Andora-Service / X-Andora-Timestamp / X-Andora-Signature
    // Payload: METHOD\nPATH\n\nTS\nSHA256(BODY) (gleiche Konvention wie
    // auth_api.rs::sign / src/api/auth.go::signPayload).
    let service_id = headers.get("X-Andora-Service").cloned().unwrap_or_default();
    if service_id != cfg.mail_inbound.service_id || service_id.is_empty() {
        return Ok(error(401, "unknown service"));
    }
    let ts_str = headers.get("X-Andora-Timestamp").cloned().unwrap_or_default();
    let Ok(ts) = ts_str.parse::<i64>() else {
        return Ok(error(401, "missing timestamp"));
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if (now - ts).abs() > 300 {
        return Ok(error(401, "stale timestamp"));
    }
    let sig = headers.get("X-Andora-Signature").cloned().unwrap_or_default();
    let body_hash = hex::encode(Sha256::digest(body));
    let payload = format!("{method}\n{path}\n\n{ts}\n{body_hash}");
    let mut mac = Hmac::<Sha256>::new_from_slice(cfg.mail_inbound.secret.as_bytes())
        .expect("hmac key");
    mac.update(payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    if !hmac_verify(&sig, &expected) {
        return Ok(error(401, "bad signature"));
    }

    // Route auflösen
    let (action, _mail_id_opt) = if path == "/internal/mail/list" {
        ("list", None as Option<&str>)
    } else if path == "/internal/mail/read" {
        ("read", None as Option<&str>)
    } else if path == "/internal/mail/claim" {
        ("claim", None as Option<&str>)
    } else {
        return Ok(error(404, "not found"));
    };

    // Body parsen (minimal: character_id, mail_id)
    let body_val: Result<serde_json::Value, _> = serde_json::from_slice(body);
    let Ok(body_val) = body_val else {
        return Ok(error(400, "invalid json body"));
    };
    let character_id = body_val
        .get("character_id")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as u64;
    let mail_id = body_val
        .get("mail_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if character_id == 0 {
        return Ok(error(400, "invalid_character"));
    }

    // Serverseitig Character-Account-Binding (docs §2.1): der geprüften
    // Session-Account (signierten Forward) MUST owningly match der
    // character_id. Applies identically to list/read/claim — a clientseitig
    // character_id is never trusted alone; cross-account access is denied.
    let owner: Option<(i64, i64)> = sqlx::query_as(
        "SELECT id, account_id FROM characters WHERE id = ?",
    )
    .bind(character_id as i64)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("character load: {e}"))?;

    let Some((_char_id, owner_account_id)) = owner else {
        return Ok(error(404, "character not found"));
    };

    let body_account_id = body_val
        .get("account_id")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i64;
    if !crate::mail::mail_owner_verdict(body_account_id, owner_account_id) {
        return Ok(error(403, "not_owner"));
    }

    match action {
        "list" => match mail::list_mails_for_character(pool, character_id).await {
            Ok(mails) => Ok((200, serde_json::json!({"ok": true, "mails": mails}))),
            Err(e) => Ok((503, serde_json::json!({"ok": false, "error": e}))),
        },
        "read" => {
            if mail_id.is_empty() {
                return Ok(error(400, "missing mail_id"));
            }
            match mail::read_mail(pool, character_id, &mail_id).await {
                Ok(Some(m)) => Ok((200, serde_json::json!({"ok": true, "mail": m}))),
                Ok(None) => Ok((404, serde_json::json!({"ok": false, "error": "not_found"}))),
                Err(e) => Ok((503, serde_json::json!({"ok": false, "error": e}))),
            }
        }
        "claim" => {
            if mail_id.is_empty() {
                return Ok(error(400, "missing mail_id"));
            }

            // Mail must exist for THIS character and not yet be claimed (row
            // present). Lesestatus (unread/read) is orthogonal — a read Mail
            // stays claimable. The authoritative consumption is in the Drain.
            let mail_exists: Option<(String,)> = sqlx::query_as(
                "SELECT mail_id FROM character_mail WHERE mail_id = ? AND character_id = ?"
            )
            .bind(&mail_id)
            .bind(character_id as i64)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("mail load: {e}"))?;

            if mail_exists.is_none() {
                return Ok(error(404, "mail not found or already claimed"));
            }

            // Öffentliche Claim via Runtime-Commit-Service: Gate → aktuelle
            // Validierung → unveränderliche Vorbereitung → bestätigte Spool-
            // Dauerhaftigkeit → RAM-Handoff. Der DB-Application (Snapshot +
            // Escrow-Verbrauch + Receipt + Revision) bleibt in dem Drain
            // (`Spool::drain_mail_claim` → `apply_mail_claim_to_db`).
            let request = crate::mail::MailClaimRequest {
                commit_id: crate::mail::mail_claim_id(character_id, &mail_id),
                character_id,
                mail_id: mail_id.clone(),
            };
            match crate::mail::commit_mail_claim(spool, &shared, request, pool).await {
                Ok(()) => Ok((200, serde_json::json!({"ok": true}))),
                Err(e) => Ok((503, serde_json::json!({"ok": false, "error": e}))),
            }
        }
        _ => Ok(error(404, "not found")),
    }
}

/// Extrahiert Request-Header aus dem rohen Buffer (bis \r\n\r\n) als Map.
/// Klein-/Grossschreibung wird nicht normalisiert; Aufrufer vergleichen
// case-insensitive falls nötig.
fn headers_from_buf(buf: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(he) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return out;
    };
    let head = String::from_utf8_lossy(&buf[..he]);
    for line in head.lines().skip(1) {
        // erste Zeile ist die Request-Zeile, überspringen
        if let Some((k, v)) = line.split_once(':') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

fn hmac_verify(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        result |= x ^ y;
    }
    result == 0
}

async fn serve_with_characters(
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
    characters: Option<CharacterListContext>,
) -> Result<(), String> {
    let addrs = crate::config::bind_addrs(&cfg.health_bind_host, cfg.health_port)?;
    let mut listeners = Vec::with_capacity(addrs.len());
    for addr in &addrs {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("health listen {addr}: {e}"))?;
        log::info!("health on {addr} (/health, /status, /players, /characters)");
        listeners.push(listener);
    }
    serve_bound(listeners, cfg, shared, persist, characters).await
}

/// Kern des Servers: je bereits gebundener Listener der vorhandene
/// `accept_loop`; endet einer, werden die übrigen Tasks abgebrochen. Produktion
/// und Test nutzen denselben Kern — es gibt keine zweite Serverimplementierung.
async fn serve_bound(
    listeners: Vec<tokio::net::TcpListener>,
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
    characters: Option<CharacterListContext>,
) -> Result<(), String> {
    let mut tasks: Vec<tokio::task::JoinHandle<Result<(), String>>> =
        Vec::with_capacity(listeners.len());
    for listener in listeners {
        let cfg = cfg.clone();
        let shared = shared.clone();
        let persist = persist.clone();
        let characters = characters.clone();
        tasks.push(tokio::spawn(async move {
            accept_loop(listener, cfg, shared, persist, characters).await
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
    auth: crate::auth_api::AuthApi,
    pool: sqlx::Pool<sqlx::MySql>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = serve_with_characters(
            cfg,
            shared,
            persist,
            Some(CharacterListContext { auth, pool }),
        )
        .await
        {
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
        if let Err(e) = serve_bound(vec![listener], cfg, shared, persist, None).await {
            log::error!("health: {e}");
        }
    })
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
    characters: Option<CharacterListContext>,
) -> Result<(), String> {
    loop {
        let (mut sock, _) = listener
            .accept()
            .await
            .map_err(|e| format!("health accept: {e}"))?;
        let cfg = cfg.clone();
        let shared = shared.clone();
        let persist = persist.clone();
        let characters = characters.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            // TCP can split the request line. Bound size and time; never log it.
            let read = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut n = 0;
                loop {
                    let count = sock.read(&mut buf[n..]).await.ok()?;
                    if count == 0 {
                        return None;
                    }
                    n += count;
                    if buf[..n].windows(2).any(|w| w == b"\r\n") {
                        return Some(n);
                    }
                    if n == buf.len() {
                        return None;
                    }
                }
            })
            .await;
            let Ok(Some(n)) = read else {
                return;
            };
            // Lesen bis Header-Ende (\r\n\r\n), dann optional Body nach
            // Content-Length. Bound Größe und Zeit.
            let mut total = n;
            let header_end = buf[..n]
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| p + 4);
            let mut content_len: usize = 0;
            if let Some(he) = header_end {
                let head = String::from_utf8_lossy(&buf[..he]);
                for line in head.lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            content_len = v.trim().parse::<usize>().unwrap_or(0);
                        }
                    }
                }
            }
            while total < header_end.unwrap_or(0) + content_len {
                let remaining = buf.len() - total;
                if remaining == 0 {
                    break; // Buffer zu klein für Body -> Body leer lassen
                }
                let count = match sock.read(&mut buf[total..]).await {
                    Ok(c) => c,
                    Err(_) => break,
                };
                if count == 0 {
                    break;
                }
                total += count;
            }
            let head_end = header_end.unwrap_or(n.min(4));
            let head = String::from_utf8_lossy(&buf[..head_end]);
            let mut parts = head.split_whitespace();
            let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
            let body = &buf[head_end..total];

            // Interne Mail-Endpunkte (POST, service-authentisiert).
            // Der characters-Kontext liefert den DB-Pool; ohne ihn können
            // Mail-Endpunkte nicht arbeiten (503).
            if method == "POST" && path.starts_with("/internal/mail/") {
                let pool = characters.as_ref().map(|c| &c.pool);
                let Some(pool) = pool else {
                    let _ = sock.write_all(response(503, &serde_json::json!({"ok":false,"error":"mail_db_unavailable"})).as_bytes()).await;
                    return;
                };
                let (status, body_val) = match handle_internal_mail(
                    &cfg,
                    pool,
                    persist.spool(),
                    shared.clone(),
                    method,
                    path,
                    body,
                    &headers_from_buf(&buf[..head_end]),
                ).await {
                    Ok(v) => v,
                    Err(e) => (500, serde_json::json!({"ok": false, "error": e})),
                };
                let _ = sock.write_all(response(status, &body_val).as_bytes()).await;
                return;
            }

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
            if path.split('?').next() == Some("/characters") {
                let (status, body) = characters_for(path, cfg.realm_id, characters.as_ref()).await;
                let _ = sock.write_all(response(status, &body).as_bytes()).await;
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
        coordinator: crate::config::CoordinatorConfig {
            url: String::new(),
            service_id: String::new(),
            secret: String::new(),
        },
        mail_inbound: crate::config::MailInboundConfig {
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
        content_root: String::new(),
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

    #[tokio::test]
    async fn character_list_rejects_wrong_realm_missing_session_and_disabled_auth() {
        assert_eq!(
            characters_for("/characters?realm_id=2&session_id=x", 1, None)
                .await
                .0,
            400
        );
        assert_eq!(
            characters_for("/characters?realm_id=1", 1, None).await.0,
            401
        );
        assert_eq!(
            characters_for("/characters?realm_id=1&session_id=x&session_id=y", 1, None)
                .await
                .0,
            401
        );
        assert_eq!(
            characters_for("/characters?realm_id=1&session_id=x&account_id=7", 1, None)
                .await
                .0,
            503
        );
    }

    #[tokio::test]
    async fn invalid_session_is_rejected_before_any_database_access() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fake_auth = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            let body = r#"{"valid":false,"account_id":7}"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let context = CharacterListContext {
            auth: crate::auth_api::AuthApi::new(&crate::config::AuthApiConfig {
                url: format!("http://{addr}"),
                service_id: "test".into(),
                secret: "test".into(),
            })
            .unwrap(),
            // Lazy unreachable pool: an attempted DB access would fail, not return 401.
            pool: sqlx::mysql::MySqlPoolOptions::new()
                .connect_lazy("mysql://test:test@127.0.0.1:1/test")
                .unwrap(),
        };
        let (code, body) = characters_for(
            "/characters?realm_id=1&session_id=invalid&account_id=7",
            1,
            Some(&context),
        )
        .await;
        assert_eq!(code, 401);
        assert_eq!(body, serde_json::json!({"error": "invalid session"}));
        fake_auth.await.unwrap();
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
