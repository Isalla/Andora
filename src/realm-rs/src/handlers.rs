// handlers — Spielnachrichten (Port von src/realm handlers/*).
// HELLO: Handoff- UND Session-Prüfung (Zielkette Login → Realm; fail-closed,
// sonst wäre die Elternkontrolle über Session-Löschung umgehbar).
// MOVE: Speed-Cap. CHAT: Eltern-Gate + AOFB-Broadcast. HEARTBEAT: SYNC-ACK.
use std::sync::Arc;
use std::time::Instant;

use sqlx::{MySql, Pool};
use tokio::sync::mpsc;

use crate::auth_api::AuthApi;
use crate::config::Config;
use crate::db;
use crate::parental::{self, SharedParental};
use crate::protocol::{s2c, Frame};
use crate::world::{apply_move, ensure_visible, truncate_chat, Player, Shared};

pub struct Ctx {
    pub cfg: Arc<Config>,
    pub db: Pool<MySql>,
    pub auth: AuthApi,
    pub shared: Shared,
    pub parental: SharedParental,
}

fn get_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

/// Einstiegsprüfung (fail-closed), sobald die Auth-API konfiguriert ist:
/// 1) gültiger, realm-gebundener Einmal-Handoff (wird verbraucht),
/// 2) gültige Session (Auth-API), deren account_id IDENTISCH dem
///    Handoff-Account ist.
/// Ohne beides verweigert der Realm den Einstieg — sonst könnte ein
/// Spieler seine Session löschen/lösen und die Elternkontrolle
/// (Playtime, Puffer, Force-Logout) umgehen.
/// Gibt das Account-ID zurück; bei Auth-API-Fehlern, ungültigem
/// Handoff, fehlender/ungültiger Session oder Account-Discrepanz Err.
/// Ohne konfigurierte Auth-API (Entwicklung/Test) ist das ein No-Op
/// (account_id 0, wie bisher).
pub async fn verify_entry(
    auth: &AuthApi,
    realm_id: u32,
    handoff: &str,
    session_id: &str,
) -> Result<u32, String> {
    if !auth.enabled() {
        return Ok(0);
    }
    if handoff.is_empty() {
        return Err("handoff required".into());
    }
    let h = auth.validate_handoff(handoff).await.map_err(|e| {
        log::error!("HELLO handoff validate: {e}");
        "handoff_unavailable".to_string()
    })?;
    let Some(h_account) = h.account_id.filter(|a| *a != 0) else {
        return Err("handoff_invalid".to_string());
    };
    if !h.valid || h.realm_id != Some(realm_id) {
        return Err("handoff_invalid".to_string());
    }
    if session_id.is_empty() {
        return Err("session required".into());
    }
    let s = auth.validate_session(session_id).await.map_err(|e| {
        log::error!("HELLO session validate: {e}");
        "session_unavailable".to_string()
    })?;
    if !s.valid || s.account_id != Some(h_account) {
        return Err("session_invalid".to_string());
    }
    Ok(h_account)
}

/// HELLO: Einstieg mit Handoff-Token (Zielkette), Charakter laden,
/// registrieren, Elternkontrolle anhängen, WELCOME + Nachbar-Spawns.
/// Gibt bei Ablehnung Err(reason) zurück (Verbindung schließen).
pub async fn handle_hello(
    ctx: &Ctx,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &serde_json::Value,
) -> Result<(), String> {
    let char_id = get_str(data, "char_id");
    if char_id.is_empty() {
        return Err("missing char_id".into());
    }
    let lang = {
        let l = get_str(data, "lang");
        if l.is_empty() {
            "de".to_string()
        } else {
            l
        }
    };
    let session_id = get_str(data, "session_id");
    let handoff = get_str(data, "handoff_token");

    // Eintritt nur mit gültigem, realm-gebundenem Handoff UND gültiger,
    // account-identischer Session, sobald die Auth-API konfiguriert ist
    // (fail-closed; ohne wäre die Elternkontrolle umgehbar). Der Handoff
    // wird dabei verbraucht (einmalig).
    let account_id =
        verify_entry(&ctx.auth, ctx.cfg.realm_id, &handoff, &session_id).await?;

    let c = db::load_character(&ctx.db, &char_id).await.map_err(|e| {
        log::error!("HELLO load character: {e}");
        "character unavailable".to_string()
    })?;
    let me = Player {
        id: c.id.clone(),
        name: c.name.clone(),
        x: c.x,
        y: c.y,
        face: 0.0,
        ping_ms: 0,
        zone_id: 0,
        hp: 100,
        max_hp: 100,
        lang,
        account_id,
        session_id: session_id.clone(),
        entities: Default::default(),
        last_activity: Instant::now(),
        tx: tx.clone(),
    };
    {
        let mut world = ctx.shared.lock().await;
        world.players.insert(me.id.clone(), me);
        world.by_conn.insert(conn_id, c.id.clone());
    }

    // Elternkontrolle: BLOCKED am Login -> Einstieg verweigert.
    if let Err(reason) =
        parental::attach(&ctx.parental, tx, &c.id, account_id, &session_id).await
    {
        let mut world = ctx.shared.lock().await;
        world.players.remove(&c.id);
        world.by_conn.remove(&conn_id);
        parental::detach(&ctx.parental, &c.id).await;
        return Err(reason);
    }

    tx.send(
        Frame::new(
            seq,
            s2c::WELCOME,
            serde_json::json!({"you": {"id": c.id, "name": c.name, "x": c.x, "y": c.y}}),
        )
        .encode(),
    )
    .map_err(|_| "send failed".to_string())?;

    // Gebiets-Kollegen: Spieler spawnen (SPAWN + STATE) und umgekehrt.
    {
        let mut world = ctx.shared.lock().await;
        let (mx, my) = {
            let me = world.players.get(&c.id).ok_or("gone".to_string())?;
            (me.x, me.y)
        };
        let others: Vec<String> = world
            .players
            .values()
            .filter(|q| {
                q.id != c.id && (q.x - mx).hypot(q.y - my) <= ctx.cfg.aofb_radius
            })
            .map(|q| q.id.clone())
            .collect();
        for oid in others {
            let other = world.players.remove(&oid).unwrap();
            let mut me = world.players.remove(&c.id).unwrap();
            ensure_visible(&mut me, &other);
            me.entities.insert(oid.clone());
            world.players.insert(oid, other);
            world.players.insert(c.id.clone(), me);
        }
    }
    Ok(())
}

/// MOVE: Position serverseitig validiert (Speed-Cap, keine Teleports).
pub async fn handle_move(shared: &Shared, conn_id: u64, data: &serde_json::Value, tick_ms: u64) {
    let (dx, dy) = match data.get("dir").and_then(|v| v.as_array()) {
        Some(a) if a.len() >= 2 => (
            a[0].as_f64().unwrap_or(0.0),
            a[1].as_f64().unwrap_or(0.0),
        ),
        _ => (
            data.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
            data.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
        ),
    };
    let mut world = shared.lock().await;
    let pid = match world.by_conn.get(&conn_id) {
        Some(pid) => pid.clone(),
        None => return,
    };
    if let Some(me) = world.players.get_mut(&pid) {
        let (x, y) = apply_move(me.x, me.y, dx, dy, tick_ms);
        me.x = x;
        me.y = y;
        me.last_activity = Instant::now();
    }
}

/// CHAT: Eltern-Gate, 240-Zeichen-Cap, AOFB-Broadcast (+ Echo an selbst).
pub async fn handle_chat(
    parental: &SharedParental,
    shared: &Shared,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &serde_json::Value,
    aofb_radius: f64,
) {
    let pid = {
        let world = shared.lock().await;
        match world.by_conn.get(&conn_id) {
            Some(pid) => pid.clone(),
            None => return,
        }
    };
    if !parental::chat_allowed(parental, &pid).await {
        let _ = tx.send(
            Frame::new(seq, s2c::PARENTAL_RESULT, serde_json::json!({"ok": false, "reason": "chat_locked"}))
                .encode(),
        );
        return;
    }
    let text = data.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let text = truncate_chat(text);
    if text.is_empty() {
        return;
    }
    let channel = data.get("channel").and_then(|v| v.as_str()).unwrap_or("local");
    let world = shared.lock().await;
    let Some(me) = world.players.get(&pid) else {
        return;
    };
    let payload = Frame::new(
        0,
        s2c::CHAT,
        serde_json::json!({"from": me.name, "channel": channel, "text": text}),
    )
    .encode();
    for o in world.players.values() {
        if (o.x - me.x).hypot(o.y - me.y) > aofb_radius {
            continue;
        }
        let _ = o.tx.send(payload.clone());
    }
    let _ = tx.send(payload);
}

/// HEARTBEAT → SYNC-ACK (plus Ping-/Aktivitäts-Update).
pub async fn handle_heartbeat(
    shared: &Shared,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &serde_json::Value,
) {
    {
        let mut world = shared.lock().await;
        if let Some(pid) = world.by_conn.get(&conn_id).cloned() {
            if let Some(me) = world.players.get_mut(&pid) {
                me.last_activity = Instant::now();
                if let Some(ping) = data.get("ping_ms").and_then(|v| v.as_f64()) {
                    if ping >= 0.0 && ping < 10000.0 {
                        me.ping_ms = ping.round() as u32;
                    }
                }
            }
        }
    }
    let _ = tx.send(
        Frame::new(seq, s2c::SYNC, serde_json::json!({"ack_seq": seq})).encode(),
    );
}

#[cfg(test)]
mod tests {
    // Lokal-HTTP-Stub für die signierten Auth-API-Aufrufe: canned
    // JSON-Responses für /handoff/validate und /session/validate,
    // konfigurierbar pro Test (Signature wird im Test nicht geprüft).

    use super::*;
    use crate::auth_api::AuthApi;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct StubState {
        handoff_resp: String,
        handoff_status: u16,
        session_resp: String,
        session_status: u16,
    }

    async fn start_stub(handoff_resp: &str, session_resp: &str) -> AuthApi {
        start_stub_status(handoff_resp, 200, session_resp, 200).await
    }

    async fn start_stub_status(
        handoff_resp: &str,
        handoff_status: u16,
        session_resp: &str,
        session_status: u16,
    ) -> AuthApi {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let state = StubState {
            handoff_resp: handoff_resp.to_string(),
            session_resp: session_resp.to_string(),
            session_status,
            handoff_status,
        };
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let (r, mut w) = tokio::io::split(sock);
                let h_resp = state.handoff_resp.clone();
                let h_status = state.handoff_status;
                let s_resp = state.session_resp.clone();
                let s_status = state.session_status;
                tokio::spawn(async move {
                    let mut br = tokio::io::BufReader::new(r);
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let n = br.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if String::from_utf8_lossy(&buf).contains("\r\n\r\n")
                            || buf.len() > 8192
                        {
                            break;
                        }
                    }
                    let reqhead = String::from_utf8_lossy(&buf);
                    let path = reqhead
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("");
                    let (resp, status) = if path == "/handoff/validate" {
                        (h_resp, h_status)
                    } else if path == "/session/validate" {
                        (s_resp, s_status)
                    } else {
                        (r#"{"error":"unknown path"}"#.to_string(), 200)
                    };
                    let raw = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        resp.len(),
                        resp
                    );
                    let _ = w.write_all(raw.as_bytes()).await;
                });
            }
        });
        api(&format!("http://{addr}"))
    }

    fn api(url: &str) -> AuthApi {
        AuthApi::new(&crate::config::AuthApiConfig {
            url: url.to_string(),
            service_id: "realm-de1-service".into(),
            secret: "s3cret".into(),
        })
        .expect("authapi client")
    }

    const HANDOFF_OK: &str = r#"{"valid":true,"account_id":7,"realm_id":42}"#;
    const HANDOFF_DEAD: &str = r#"{"valid":false,"account_id":null,"realm_id":null}"#;
    const HANDOFF_RELM: &str = r#"{"valid":true,"account_id":7,"realm_id":9}"#;
    const SESSION_OK: &str = r#"{"valid":true,"account_id":7,"expires_at":"2030-01-01T00:00:00Z"}"#;
    const SESSION_DEAD: &str = r#"{"valid":false,"account_id":null,"expires_at":null}"#;
    const SESSION_OTHER: &str = r#"{"valid":true,"account_id":8,"expires_at":"2030-01-01T00:00:00Z"}"#;

    #[tokio::test]
    async fn entry_requires_handoff_and_matching_session() {
        let auth = start_stub(HANDOFF_OK, SESSION_OK).await;
        assert_eq!(verify_entry(&auth, 42, "h", "s").await, Ok(7));
    }

    #[tokio::test]
    async fn entry_fails_without_handoff_or_session() {
        let auth = start_stub(HANDOFF_OK, SESSION_OK).await;
        assert_eq!(
            verify_entry(&auth, 42, "", "s").await,
            Err("handoff required".into())
        );
        assert_eq!(
            verify_entry(&auth, 42, "h", "").await,
            Err("session required".into())
        );
    }

    #[tokio::test]
    async fn entry_fails_on_dead_tokens_or_mismatched_account() {
        let a = start_stub(HANDOFF_DEAD, SESSION_OK).await;
        assert_eq!(
            verify_entry(&a, 42, "h", "s").await,
            Err("handoff_invalid".into())
        );
        let a = start_stub(HANDOFF_RELM, SESSION_OK).await;
        assert_eq!(
            verify_entry(&a, 42, "h", "s").await,
            Err("handoff_invalid".into())
        );
        let a = start_stub(HANDOFF_OK, SESSION_DEAD).await;
        assert_eq!(
            verify_entry(&a, 42, "h", "s").await,
            Err("session_invalid".into())
        );
        let a = start_stub(HANDOFF_OK, SESSION_OTHER).await;
        // Session gültig, gehört aber zu einem anderen Account.
        assert_eq!(
            verify_entry(&a, 42, "h", "s").await,
            Err("session_invalid".into())
        );
    }

    #[tokio::test]
    async fn entry_fails_closed_on_authapi_transport_errors() {
        // Kein Listener: Connect-Refusal auf 127.0.0.1 (reservierter Port
        // 1 ist garantiert unverbindbar).
        let auth = api("http://127.0.0.1:1");
        assert_eq!(
            verify_entry(&auth, 42, "h", "s").await,
            Err("handoff_unavailable".into())
        );
        // Auth-API erreichbar, aber Session-Call mit HTTP-Fehler
        // (z. B. 503): Einstieg bleibt verwehrt.
        let auth =
            start_stub_status(HANDOFF_OK, 200, SESSION_DEAD, 503).await;
        assert_eq!(
            verify_entry(&auth, 42, "h", "s").await,
            Err("session_unavailable".into())
        );
    }

    #[test]
    fn entry_noop_without_authapi() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let auth = api("");
            assert_eq!(verify_entry(&auth, 42, "h", "s").await, Ok(0));
        });
    }
}
