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

// ── Charakteranlage: POST /characters (C-1 Stufe 1 ohne Gegenstände; C-5
//    ergänzt die skills-Zeile nach Beschluss 5a, drei Statements je Regel 8b) ─
// Regelstelle: docs/Charaktererstellung_und_Charakterdarstellung.md,
// Abschnitt „Festlegungen zur Charakteranlage (Entscheidungen a–j)".
// Session-/Realm-/Auth-Ablehnungen sind wörtlich dieselben Fälle wie bei der
// GET-Route (characters_for); die übrigen Gründe folgen der dortigen
// Tabelle. Der Endpunkt teilt sich den Port mit GET /characters; welcher
// Betreiber ihn wie erreichbar macht (HEALTH_BIND_HOST), entscheidet das
// Deployment, nicht dieser Pfad.

/// Reservierte Funktionsbezeichnungen (Groß-/Kleinschreibung gleichgültig);
/// Abschnitt „Vorname" derselben Regelstelle.
const RESERVED_FIRST_NAMES: [&str; 3] = ["gm", "admin", "support"];

/// Prüft den Vornamen und liefert die zu speichernde, getrimmte Form:
/// 3–10 Zeichen, Buchstaben inklusive deutscher Umlaute sowie Bindestrich;
/// keine Ziffern, keine sonstigen Sonderzeichen; leer nach Trim abgelehnt;
/// reservierte Bezeichnungen abgelehnt. Die Groß-/Kleinschreibung bleibt
/// erhalten — die Namenseindeutigkeit trägt die Kollation (Abschnitt
/// „Voraussetzung der Kollation"), nicht dieser Validator.
fn validate_first_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let count = trimmed.chars().count();
    if count < 3 || count > 10 {
        return None;
    }
    if !trimmed.chars().all(|c| {
        c.is_ascii_alphabetic() || matches!(c, 'ä' | 'ö' | 'ü' | 'Ä' | 'Ö' | 'Ü' | 'ß' | '-')
    }) {
        return None;
    }
    if RESERVED_FIRST_NAMES.contains(&trimmed.to_lowercase().as_str()) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Rasse wählbar? Nur der exakte kanonische, kleingeschriebene Wert aus
/// `SELECTABLE_RACES`; Groß-/Kleinschreibvarianten sind keine kanonischen
/// Werte und damit nicht wählbar (Abschnitt „Spielbare Rassen bei der
/// Erstellung").
fn race_is_selectable(race: &str, selectable: &[String]) -> bool {
    selectable.iter().any(|r| r == race)
}

/// Die sieben Startwerte aus der beim Realm-Start geladenen RAM-Kopie —
/// keine DB-Lesung der Matrix während der Anlage. `dexterity` (Spalte
/// `race_definitions.dexterity`) liegt als `Attributes.dexterity` vor;
/// `db::write_attributes` schreibt genau das in `characters.agility`.
fn start_attributes_from(
    matrix: &[crate::world::RaceStartAttributes],
    race: &str,
) -> Option<crate::attributes::Attributes> {
    matrix
        .iter()
        .find(|row| row.race == race)
        .map(|row| crate::attributes::Attributes {
            strength: row.strength,
            constitution: row.constitution,
            dexterity: row.dexterity,
            intelligence: row.intelligence,
            wisdom: row.wisdom,
            luck: row.luck,
            endurance: row.endurance,
        })
}

/// Fachliche Antwort auf den Anlagen-Ausgang: Erfolg 201 mit
/// `character_id` (kanonische positive DB-ID als String, analog der
/// GET-Liste) und Name; Indexkonflikt/Vorprüfungstreffer identisch als
/// 409 `name taken` (nie 500); DB-Ausfall 503 `database unavailable`.
fn create_db_response(
    result: Result<u64, crate::db::CreateCharacterError>,
    name: &str,
) -> (u16, serde_json::Value) {
    match result {
        Ok(id) => (
            201,
            serde_json::json!({"character_id": id.to_string(), "name": name}),
        ),
        Err(crate::db::CreateCharacterError::NameTaken) => {
            (409, serde_json::json!({"error": "name taken"}))
        }
        Err(crate::db::CreateCharacterError::Unavailable) => {
            (503, serde_json::json!({"error": "database unavailable"}))
        }
    }
}

async fn create_character_for(
    target: &str,
    realm_id: u32,
    cfg: &Config,
    context: Option<&CharacterListContext>,
    shared: &Shared,
    body: &[u8],
) -> (u16, serde_json::Value) {
    let error = |status, reason| (status, serde_json::json!({"error": reason}));
    // Vor jeder Schreiboperation: gültige Session dieses Realms. Die
    // Prüfung ist wörtlich die der GET-Route — account_id ausschließlich
    // aus der Sessionprüfung, eine clientgelieferte account_id oder
    // character_id erreicht diese Stelle gar nicht erst und hat keine
    // Wirkung.
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
    // Body {"name", "race"}; unbekannte Zusatzfelder (auch account_id,
    // character_id) werden verworfen, nie gelesen.
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return error(400, "invalid request");
    };
    let name_raw = value.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let Some(name) = validate_first_name(name_raw) else {
        return error(400, "invalid name");
    };
    let race = value.get("race").and_then(|v| v.as_str()).unwrap_or("");
    if !race_is_selectable(race, &cfg.races.selectable) {
        return error(400, "race not selectable");
    }
    // Startwerte aus der RAM-Kopie des Starts. Die World-Sperre wird
    // innerhalb dieses Blocks erworben und VOR jedem DB-Zugriff wieder
    // freigegeben: keine DB-I/O unter World-Sperre.
    let attrs = {
        let world = shared.lock().await;
        let attrs = start_attributes_from(&world.race_start_attributes, race);
        drop(world);
        attrs
    };
    // Der fail-closed-Loader garantiert je wählbarer Rasse genau eine
    // Zeile; fehlt sie trotz alledem, sind die Startdaten nicht
    // verfügbar — es wurde nichts geschrieben.
    let Some(attrs) = attrs else {
        return error(503, "database unavailable");
    };
    let result = crate::db::create_character(
        &context.pool,
        account_id,
        &name,
        race,
        &attrs,
        &cfg.combat.weapon_skill_id,
    )
    .await;
    create_db_response(result, &name)
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

/// Audit H-3: welche der aufgelösten Bind-Adressen sind **Wildcard**-
/// Bindungen (`0.0.0.0` bzw. `[::]`)? Reine Entscheidungsfunktion, damit
/// die Schwelle der Startwarnung ohne echte Bindung testbar ist. Loopback-,
/// Einzel- und private Adressen ergeben eine leere Liste.
fn wildcard_binds(addrs: &[std::net::SocketAddr]) -> Vec<std::net::SocketAddr> {
    addrs
        .iter()
        .copied()
        .filter(|a| a.ip().is_unspecified())
        .collect()
}

async fn serve_with_characters(
    cfg: Arc<Config>,
    shared: Shared,
    persist: Arc<PersistRuntime>,
    characters: Option<CharacterListContext>,
) -> Result<(), String> {
    // H-3: health-spezifische Auflösung. Ohne gesetztes HEALTH_BIND_HOST
    // wird nur an Loopback gebunden; ausdruecklich gesetzte Werte
    // (0.0.0.0, private IP, Hostname, IPv6) behalten ihre Semantik. Ein
    // Aufloesungsfehler bleibt ein Fehler — keine stille Ersatzadresse.
    let addrs = crate::config::health_bind_addrs(&cfg.health_bind_host, cfg.health_port)?;
    // Eine ausdruecklich konfigurierte Wildcard-Bindung ist erlaubt, aber
    // nicht still: deutliche Startwarnung je betroffener Adresse. Ins Log
    // kommen ausschliesslich die Adresse und die betroffenen Endpunkte —
    // keine Secrets, keine Header, keine Konfigurationswerte.
    for addr in wildcard_binds(&addrs) {
        log::warn!(
            "health: WARNUNG - ausdruecklich an Wildcard-Adresse {addr} gebunden; \
             /health, /status und /players sind ohne Authentifizierung erreichbar \
             (POST /internal/mail/* nur mit gueltiger Service-Signatur). \
             HEALTH_BIND_HOST auf eine interne Adresse beschraenken (Standard: \
             127.0.0.1) oder die Adresse netzwerkseitig (Firewall / interne Zone) absichern."
        );
    }
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

            // Charakteranlage (POST /characters, Stufe 1 ohne Gegenstände):
            // direkt neben GET /characters auf demselben Port. Validierung,
            // Sessionprüfung und Transaktion laufen in create_character_for;
            // ohne characters-Kontext (Auth/Pool) antwortet der Pfad 503.
            if method == "POST" && path.split('?').next() == Some("/characters") {
                let (status, body_val) = create_character_for(
                    path,
                    cfg.realm_id,
                    &cfg,
                    characters.as_ref(),
                    &shared,
                    body,
                )
                .await;
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
        weather: crate::config::WeatherCfg::default(),
        death: crate::config::DeathCfg::default(),
        races: crate::config::RacesCfg::default(),
        security: crate::config::SecurityCfg::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        test_config()
    }

    // Audit H-3: Die Startwarnung darf ausschließlich bei Wildcard-Bindungen
    // (0.0.0.0 / [::]) ausgelöst werden — nie bei Loopback oder Einzeladresse.
    #[test]
    fn wildcard_binds_flags_only_unspecified_addresses() {
        use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
        let all = vec![
            SocketAddr::from((Ipv4Addr::LOCALHOST, 3002)),
            SocketAddr::from((Ipv6Addr::LOCALHOST, 3002)),
            "192.168.10.5:3002".parse::<SocketAddr>().unwrap(),
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3002)),
            SocketAddr::from((Ipv6Addr::UNSPECIFIED, 3002)),
        ];
        let w = wildcard_binds(&all);
        assert_eq!(w.len(), 2, "nur 0.0.0.0 und [::] sind Wildcard-Bindungen");
        assert!(w.iter().all(|a| a.ip().is_unspecified()));
        assert!(
            wildcard_binds(&[SocketAddr::from((Ipv4Addr::LOCALHOST, 3002))]).is_empty(),
            "Loopback darf keine Warnung ausloesen"
        );
        assert!(wildcard_binds(&[]).is_empty());
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

    // ── Charakteranlage POST /characters (C-1, Stufe 1 ohne Gegenstände;
    //    C-5 ergänzt den skill_id-Durchgriff auf die Konfiguration) ──

    /// Einmaliger Fake-Auth-Dienst wie im GET-Test; antwortet `times` Mal
    /// mit derselben Session-JSON. Der Pool bleibt lazy und unerreichbar:
    /// jede erreichte DB-I/O würde fehlschlagen, nie succeed.
    async fn fake_auth_context(
        session_response: &'static str,
        times: usize,
    ) -> (CharacterListContext, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            for _ in 0..times {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                socket.read(&mut request).await.unwrap();
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{session_response}",
                            session_response.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let context = CharacterListContext {
            auth: crate::auth_api::AuthApi::new(&crate::config::AuthApiConfig {
                url: format!("http://{addr}"),
                service_id: "test".into(),
                secret: "test".into(),
            })
            .unwrap(),
            pool: sqlx::mysql::MySqlPoolOptions::new()
                .connect_lazy("mysql://test:test@127.0.0.1:1/test")
                .unwrap(),
        };
        (context, task)
    }

    fn race_row(race: &str, base: i32) -> crate::world::RaceStartAttributes {
        crate::world::RaceStartAttributes {
            race: race.to_string(),
            strength: base,
            dexterity: base + 1,
            intelligence: base + 2,
            constitution: base + 3,
            wisdom: base + 4,
            luck: base + 5,
            endurance: base + 6,
        }
    }

    fn shared_with_matrix() -> Shared {
        let mut world = crate::world::World::new();
        world.race_start_attributes = vec![
            race_row("mensch", 2),
            race_row("elf", 1),
            race_row("andorer", 3),
        ];
        std::sync::Arc::new(tokio::sync::Mutex::new(world))
    }

    #[tokio::test]
    async fn create_character_rejects_wrong_realm_and_bad_session_params() {
        let shared = shared_with_matrix();
        let body = br#"{"name":"Alda","race":"mensch"}"#;
        let (code, v) = create_character_for(
            "/characters?realm_id=2&session_id=x",
            1,
            &test_config(),
            None,
            &shared,
            body,
        )
        .await;
        assert_eq!(
            (code, v),
            (400, serde_json::json!({"error": "wrong realm"}))
        );
        for target in [
            "/characters?realm_id=1",
            "/characters?realm_id=1&session_id=",
            "/characters?realm_id=1&session_id=x&session_id=y",
        ] {
            let (code, v) =
                create_character_for(target, 1, &test_config(), None, &shared, body).await;
            assert_eq!(
                (code, v),
                (401, serde_json::json!({"error": "invalid session"})),
                "{target}"
            );
        }
        // Clientgelieferte account_id bleibt ohne Wirkung; ohne aktivierte
        // Auth ist die Prüfung nicht durchführbar (wie GET: 503).
        assert_eq!(
            create_character_for(
                "/characters?realm_id=1&session_id=x&account_id=7",
                1,
                &test_config(),
                None,
                &shared,
                body,
            )
            .await
            .0,
            503
        );
    }

    #[tokio::test]
    async fn create_character_rejects_invalid_session_before_any_write() {
        let (context, fake_auth) = fake_auth_context(r#"{"valid":false,"account_id":7}"#, 1).await;
        let shared = shared_with_matrix();
        let (code, v) = create_character_for(
            "/characters?realm_id=1&session_id=invalid&account_id=7",
            1,
            &test_config(),
            Some(&context),
            &shared,
            br#"{"name":"Alda","race":"mensch"}"#,
        )
        .await;
        // Ungültige Sitzung: 401, bevor irgendein Schreibpfad berührt wird —
        // der unerreichbare Pool würde andernals als 503 antworten.
        assert_eq!(
            (code, v),
            (401, serde_json::json!({"error": "invalid session"}))
        );
        fake_auth.await.unwrap();
    }

    #[test]
    fn first_name_rules_hold_before_any_write() {
        // Grenze 3 und 10 Zeichen (Unicode-Zähler, keine Bytes)
        assert_eq!(validate_first_name("Ala").as_deref(), Some("Ala"));
        assert_eq!(
            validate_first_name("Albertinus").as_deref(),
            Some("Albertinus")
        );
        assert_eq!(validate_first_name("Al"), None);
        assert_eq!(validate_first_name("AlbertinusX"), None);
        // Trim; leer nach Trim abgelehnt
        assert_eq!(validate_first_name("  Alda  ").as_deref(), Some("Alda"));
        assert_eq!(validate_first_name("   "), None);
        assert_eq!(validate_first_name(""), None);
        // Buchstaben inklusive deutscher Umlaute sowie Bindestrich
        assert_eq!(
            validate_first_name("Jörg-Alda").as_deref(),
            Some("Jörg-Alda")
        );
        assert_eq!(validate_first_name("Grüße").as_deref(), Some("Grüße"));
        // Ziffern, innere Leerzeichen, sonstige Sonderzeichen abgelehnt
        assert_eq!(validate_first_name("Alraune9"), None);
        assert_eq!(validate_first_name("Al da"), None);
        assert_eq!(validate_first_name("Alda_"), None);
        assert_eq!(validate_first_name("Alda!"), None);
        // Reservierte Funktionsbezeichnungen, Groß-/Kleinschreibung gleichgültig
        for reserved in [
            "gm", "GM", "Gm", "admin", "Admin", "ADMIN", "support", "Support",
        ] {
            assert_eq!(
                validate_first_name(reserved),
                None,
                "{reserved} ist reserviert"
            );
        }
        // Normale Groß-/Kleinschreibung bleibt erhalten — die Eindeutigkeit
        // trägt die Kollation, nicht der Validator.
        assert_eq!(validate_first_name("Alda").as_deref(), Some("Alda"));
        assert_eq!(validate_first_name("alda").as_deref(), Some("alda"));
    }

    #[test]
    fn only_canonical_selectable_races_pass() {
        let selectable: Vec<String> = crate::config::DEFAULT_SELECTABLE_RACES
            .iter()
            .map(|r| r.to_string())
            .collect();
        for r in ["mensch", "elf", "andorer"] {
            assert!(race_is_selectable(r, &selectable), "{r} ist wählbar");
        }
        // Deaktivierte Expansion-Rassen und Nicht-Kanonisches (auch
        // Groß-/Kleinschreibvarianten) sind nicht wählbar.
        for r in [
            "luzilla",
            "mandalonier",
            "Mensch",
            "MENSCH",
            "mensch ",
            "",
            "zwerg",
        ] {
            assert!(
                !race_is_selectable(r, &selectable),
                "{r:?} ist nicht wählbar"
            );
        }
    }

    #[tokio::test]
    async fn create_character_rejects_deactivated_race_and_bad_name_after_session() {
        let (context, fake_auth) = fake_auth_context(r#"{"valid":true,"account_id":7}"#, 4).await;
        let shared = shared_with_matrix();
        for (body, reason) in [
            (r#"{"name":"Alda","race":"luzilla"}"#, "race not selectable"),
            (r#"{"name":"Alda","race":"Mensch"}"#, "race not selectable"),
            (r#"{"name":"Admin","race":"mensch"}"#, "invalid name"),
            (r#"{"name":"Alraune9","race":"mensch"}"#, "invalid name"),
        ] {
            let (code, v) = create_character_for(
                "/characters?realm_id=1&session_id=s",
                1,
                &test_config(),
                Some(&context),
                &shared,
                body.as_bytes(),
            )
            .await;
            assert_eq!(
                (code, v),
                (400, serde_json::json!({"error": reason})),
                "{body}"
            );
        }
        fake_auth.await.unwrap();
    }

    #[test]
    fn start_attributes_are_mapped_per_race_from_the_ram_copy() {
        let matrix = vec![
            race_row("mensch", 2),
            race_row("elf", 1),
            race_row("andorer", 3),
        ];
        // Je wählbarer Rasse die eigene Zeile; dexterity (race_definitions)
        // bleibt Attributes.dexterity und gelangt über db::write_attributes
        // in characters.agility.
        let a = start_attributes_from(&matrix, "elf").expect("Zeile je wählbarer Rasse");
        assert_eq!(
            a,
            crate::attributes::Attributes {
                strength: 1,
                constitution: 4,
                dexterity: 2,
                intelligence: 3,
                wisdom: 5,
                luck: 6,
                endurance: 7,
            }
        );
        assert_eq!(
            start_attributes_from(&matrix, "mensch").map(|x| x.strength),
            Some(2)
        );
        assert_eq!(
            start_attributes_from(&matrix, "andorer").map(|x| x.strength),
            Some(3)
        );
        assert!(start_attributes_from(&matrix, "luzilla").is_none());
    }

    #[test]
    fn create_outcomes_answer_exactly_per_the_decided_table() {
        // Erfolg 201 mit character_id (kanonische ID als String, analog der
        // GET-Liste) und Name; Vorprüfungstreffer und Indexkonflikt werden
        // von db::create_character beide als NameTaken gemeldet und sind hier
        // fachlich identisch: 409 name taken — nie 500.
        assert_eq!(
            create_db_response(Ok(42), "Alda"),
            (
                201,
                serde_json::json!({"character_id": "42", "name": "Alda"})
            )
        );
        assert_eq!(
            create_db_response(Err(crate::db::CreateCharacterError::NameTaken), "Alda"),
            (409, serde_json::json!({"error": "name taken"}))
        );
        assert_eq!(
            create_db_response(Err(crate::db::CreateCharacterError::Unavailable), "Alda"),
            (503, serde_json::json!({"error": "database unavailable"}))
        );
    }

    #[tokio::test]
    async fn create_character_database_failure_answers_service_unavailable() {
        let (context, fake_auth) = fake_auth_context(r#"{"valid":true,"account_id":7}"#, 1).await;
        let shared = shared_with_matrix();
        // Unerreichbarer Pool: die Vorprüfung schlägt fehl, bevor eine
        // Transaktion beginnt — nichts geschrieben, stabil 503.
        let (code, v) = create_character_for(
            "/characters?realm_id=1&session_id=s",
            1,
            &test_config(),
            Some(&context),
            &shared,
            br#"{"name":"Alda","race":"mensch"}"#,
        )
        .await;
        assert_eq!(
            (code, v),
            (503, serde_json::json!({"error": "database unavailable"}))
        );
        fake_auth.await.unwrap();
    }

    #[test]
    fn create_character_releases_world_lock_before_database_access() {
        // Strukturbeleg (Reihenfolgebeleg-Muster): Im Anlagehandler muss die
        // World-Sperre gelöst sein, BEVOR der DB-Anlagepfad aufgerufen wird —
        // keine DB-I/O unter World-Sperre.
        let src = include_str!("health.rs");
        let start = src
            .find("async fn create_character_for(")
            .expect("Anlagehandler vorhanden");
        let end = src[start..]
            .find("\n}\n")
            .map(|i| start + i)
            .expect("Funktionsende");
        let body = &src[start..end];
        let lock = body
            .find("shared.lock().await")
            .expect("RAM-Lesung unter World-Sperre");
        let release = body
            .find("drop(world)")
            .expect("Sperre wird vor der Anlage gelöst");
        let db_call = body
            .find("db::create_character(")
            .expect("DB-Anlage nach der Sperre");
        assert!(
            lock < release && release < db_call,
            "Reihenfolge: Sperre holen -> freigeben -> DB-Anlage"
        );
    }

    #[test]
    fn create_character_for_passes_the_configured_weapon_skill_id() {
        // Beschluss 5a: Der skill_id der skills-Startzeile kommt aus der
        // Konfiguration (CombatCfg.weapon_skill_id, Umgebungsschlüssel
        // COMBAT_WEAPON_SKILL_ID) — der Handler reicht ihn unverändert an
        // db::create_character durch. Kein fest codierter Skillname im Pfad.
        let src = include_str!("health.rs");
        let start = src
            .find("async fn create_character_for(")
            .expect("Anlagehandler vorhanden");
        let end = src[start..]
            .find("\n}\n")
            .map(|i| start + i)
            .expect("Funktionsende");
        let body = &src[start..end];
        let db_call = body
            .find("db::create_character(")
            .expect("DB-Anlage im Handler");
        let window = &body[db_call..];
        assert!(
            window.contains("cfg.combat.weapon_skill_id"),
            "der konfigurierte weapon_skill_id wird an db::create_character übergeben"
        );
        assert!(
            !window.contains("schwerter"),
            "kein fest codierter Startwert — die Konfiguration entscheidet"
        );
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
