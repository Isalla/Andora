// handlers — Spielnachrichten (Port von src/realm handlers/*).
// HELLO: Handoff- UND Session-Prüfung (Zielkette Login → Realm; fail-closed,
// sonst wäre die Elternkontrolle über Session-Löschung umgehbar).
// MOVE: Speed-Cap. CHAT: Eltern-Gate + AOFB-Broadcast. HEARTBEAT: SYNC-ACK.
use std::sync::Arc;
use std::time::Instant;

use sqlx::{MySql, Pool};
use tokio::sync::mpsc;

use crate::auth_api::AuthApi;
use crate::combat::CombatState;
use crate::combat::ability::AbilityRegistry;
use crate::config::{CombatCfg, Config, NpcCfg};
use crate::db;
use crate::npc::aggro_trigger;
use crate::parental::{self, SharedParental};
use crate::protocol::{s2c, Frame};
use crate::world::{apply_move, ensure_visible, truncate_chat, Player, Shared};
use crate::attributes;

pub struct Ctx {
    pub cfg: Arc<Config>,
    pub db: Pool<MySql>,
    pub auth: AuthApi,
    pub shared: Shared,
    pub parental: SharedParental,
    pub registry: AbilityRegistry,
}

fn get_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
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
    let account_id = verify_entry(&ctx.auth, ctx.cfg.realm_id, &handoff, &session_id).await?;

    let c = db::load_character(&ctx.db, &char_id).await.map_err(|e| {
        log::error!("HELLO load character: {e}");
        "character unavailable".to_string()
    })?;
    let weapon_skill = db::load_weapon_skill(&ctx.db, &c.id, &ctx.cfg.combat.weapon_skill_id).await;
    let learned_abilities: std::collections::HashSet<String> =
        db::load_character_abilities(&ctx.db, &c.id).await.unwrap_or_default().into_iter().collect();
    let me = Player {
        id: c.id.clone(),
        name: c.name.clone(),
        x: c.x,
        y: c.y,
        face: 0.0,
        ping_ms: 0,
        zone_id: 0,
        hp: c.hp,
        max_hp: c.hp,
        mana: c.mana,
        max_mana: c.mana_max,
        lang,
        account_id,
        session_id: session_id.clone(),
        entities: Default::default(),
        last_activity: Instant::now(),
        tx: tx.clone(),
        char_class: c.char_class.clone(),
        class: c.class,
        faction_transition: c.faction_transition,
        level: c.level,
        armor: c.armor,
        weapon_skill,
        combat: None,
        effects: Vec::new(),
        cooldowns: std::collections::BTreeMap::new(),
        active_cast: None,
        learned_abilities,
        attributes: attributes::Attributes {
            strength: c.strength,
            constitution: c.constitution,
            dexterity: c.dexterity,
            intelligence: c.intelligence,
            wisdom: c.wisdom,
            luck: c.luck,
            endurance: c.endurance,
        },
        max_hp_base: c.hp,
        max_mana_base: c.mana_max,
        sitting: false,
        hp_regen_bonus: 0.0,
        mana_regen_bonus: 0.0,
        hp_regen_carry: 0.0,
        mana_regen_carry: 0.0,
    };
    let mut me = me;
    attributes::recompute_max_resources(&mut me);
    {
        let mut world = ctx.shared.lock().await;
        world.players.insert(me.id.clone(), me);
        world.by_conn.insert(conn_id, c.id.clone());
    }

    // Elternkontrolle: BLOCKED am Login -> Einstieg verweigert.
    if let Err(reason) = parental::attach(&ctx.parental, tx, &c.id, account_id, &session_id).await {
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
            .filter(|q| q.id != c.id && (q.x - mx).hypot(q.y - my) <= ctx.cfg.aofb_radius)
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
        Some(a) if a.len() >= 2 => (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)),
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

    let moving = (dx.abs() > f64::EPSILON || dy.abs() > f64::EPSILON)
        && !crate::combat::effects::is_rooted(
            &world.players.get(&pid).map(|p| &p.effects).unwrap_or(&Vec::new()),
        );

    if let Some(me) = world.players.get_mut(&pid) {
        if !moving {
            return;
        }
        let (x, y) = apply_move(me.x, me.y, dx, dy, tick_ms);
        me.x = x;
        me.y = y;
        me.last_activity = Instant::now();
    }

    // Cast-Unterbrechung durch Bewegung (Ability-System.md §3;
    // Kampfsystem.md §9: Zauber werden durch Bewegung unterbrochen).
    let had_cast = world.players.get(&pid).map(|p| p.active_cast.is_some()).unwrap_or(false);
    if moving && had_cast {
        if let Some(event) = crate::combat::ability::interrupt_cast(&mut world, &pid) {
            crate::combat::ability::broadcast_combat_event(&world, 0.0, 0.0, 0.0, &event);
        }
    }
}

/// ATTACK (Combat V1 + V2): Auto-Grundangriff starten oder beenden.
/// Payload: {target_id} = starten, {stop: true} = beenden.
/// Realm-autoritativ validiert: Ziel existiert, ist nicht selbst, lebt
/// und liegt in Waffen-Reichweite. Ziel kann ein Spieler (V1) oder ein
/// NPC/Monster (V2) sein. NPCs werden nur angegriffen, wenn sie
/// `attackable` sind und nicht in Evade/Return (Combat V2, §18/§20).
/// Gültige Starts bewaffnen den Angriff; der erste Schlag folgt sofort
/// (Duration als abgelaufen gesetzt), alle weiteren im Duration-Takt
/// (siehe combat::combat_tick). Ein gültiger Angriff auf einen NPC löst
/// dessen (defensives) Aggro aus (§21 Aggro-Formen).
pub async fn handle_attack(
    shared: &Shared,
    conn_id: u64,
    data: &serde_json::Value,
    cfg: &CombatCfg,
    npc_cfg: &NpcCfg,
) {
    let mut world = shared.lock().await;
    let pid = match world.by_conn.get(&conn_id) {
        Some(pid) => pid.clone(),
        None => return,
    };
    // Stop: bewaffneten Angriff beenden (immer erlaubt).
    if data.get("stop").and_then(|v| v.as_bool()).unwrap_or(false) {
        if let Some(me) = world.players.get_mut(&pid) {
            me.combat = None;
        }
        return;
    }
    let target_id = data.get("target_id").and_then(|v| v.as_str()).unwrap_or("");
    if target_id.is_empty() || target_id == pid {
        return;
    }
    // Ist das Ziel ein NPC? (Target-Namespace: "npc_<spawn_id>").
    let is_npc = world.npcs.contains_key(target_id);
    // Validierung (immutable): Ziel existiert, nicht tot, in Reichweite,
    // Angreifer lebt. NPCs zusätzlich: attackable + nicht in Evade/Return.
    let valid = {
        let me = match world.players.get(&pid) {
            Some(me) => me,
            None => return,
        };
        if me.hp <= 0 {
            false
        } else if is_npc {
            world.npcs.get(target_id).is_some_and(|n| {
                n.status == crate::npc::NpcStatus::Alive
                    && n.effective_attackable()
                    && (me.x - n.x).hypot(me.y - n.y) <= cfg.weapon_range
            })
        } else {
            world
                .players
                .get(target_id)
                .is_some_and(|t| t.hp > 0 && (me.x - t.x).hypot(me.y - t.y) <= cfg.weapon_range)
        }
    };
    if !valid {
        return; // kein Kampfzustand, kein Schaden.
    }
    if is_npc {
        // Defensives/soziales Aggro auslösen (§21) — NPC verteidigt sich
        // bzw. die feste Gruppe/Fraktion steigt ein.
        aggro_trigger(&mut world, target_id, &pid, npc_cfg);
    }
    let now = Instant::now();
    if let Some(me) = world.players.get_mut(&pid) {
        me.combat = Some(CombatState {
            target_id: target_id.to_string(),
            // Erster Schlag sofort beim nächsten Tick; danach Duration-Takt.
            last_attack: now
                .checked_sub(std::time::Duration::from_millis(cfg.weapon_duration_ms))
                .unwrap_or(now),
        });
    }
}

/// ABILITY (Combat V3): Fähigkeit auslösen.
/// Payload: {ability_id, target_id?, x?, y?}
/// Realm-autoritativ: Cast-Management, Mana, Cooldown, Effekte.
pub async fn handle_ability(
    ctx: &Ctx,
    conn_id: u64,
    data: &serde_json::Value,
) {
    let pid = {
        let world = ctx.shared.lock().await;
        match world.by_conn.get(&conn_id) {
            Some(pid) => pid.clone(),
            None => return,
        }
    };

    let ability_id = get_str(data, "ability_id");
    if ability_id.is_empty() {
        return;
    }
    let target_id = {
        let t = get_str(data, "target_id");
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    };
    let ground_x = data.get("x").and_then(|v| v.as_f64());
    let ground_y = data.get("y").and_then(|v| v.as_f64());

    let mut world = ctx.shared.lock().await;
    let now = std::time::Instant::now();
    let wall_now = std::time::SystemTime::now();

    // Sende-Resultat an den Casting-Spieler zurück.
    let events = crate::combat::ability::start_ability(
        &mut world,
        &ctx.registry,
        &pid,
        &ability_id,
        target_id.as_deref(),
        ground_x,
        ground_y,
        now,
        wall_now,
    );

    let (caster_x, caster_y) = world
        .players
        .get(&pid)
        .map(|p| (p.x, p.y))
        .unwrap_or((0.0, 0.0));
    for event in &events {
        crate::combat::ability::broadcast_combat_event(
            &world, caster_x, caster_y, ctx.cfg.aofb_radius, event,
        );
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
            Frame::new(
                seq,
                s2c::PARENTAL_RESULT,
                serde_json::json!({"ok": false, "reason": "chat_locked"}),
            )
            .encode(),
        );
        return;
    }
    let text = data.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let text = truncate_chat(text);
    if text.is_empty() {
        return;
    }
    let channel = data
        .get("channel")
        .and_then(|v| v.as_str())
        .unwrap_or("local");
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
    let _ = tx.send(Frame::new(seq, s2c::SYNC, serde_json::json!({"ack_seq": seq})).encode());
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
                        if String::from_utf8_lossy(&buf).contains("\r\n\r\n") || buf.len() > 8192 {
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
    const SESSION_OTHER: &str =
        r#"{"valid":true,"account_id":8,"expires_at":"2030-01-01T00:00:00Z"}"#;

    fn npc_cfg() -> NpcCfg {
        NpcCfg {
            social_aggro_radius: 15.0,
            no_link_ms: 5000,
            return_speed: 5.0,
            persist_interval_ms: 30000,
        }
    }

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
        let auth = start_stub_status(HANDOFF_OK, 200, SESSION_DEAD, 503).await;
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

    #[tokio::test]
    async fn attack_start_stop_and_validation() {
        let shared = crate::world::new_shared();
        let cfg = CombatCfg {
            weapon_skill_id: "schwerter".into(),
            weapon_damage: 10,
            weapon_duration_ms: 2000,
            weapon_range: 2.0,
            hit_miss_permille: 100,
            hit_dodge_permille: 100,
            hit_parry_permille: 50,
            hit_block_permille: 100,
            hit_crit_permille: 100,
            hit_crit_mult_percent: 150,
            hit_block_reduce_percent: 50,
            armor_pct_per_point: 2,
            armor_cap_tank: 50,
            armor_cap_mage: 20,
            armor_cap_default: 30,
            skill_hit_bonus_permille: 5,
        };
        // Welt mit zwei Spielern aufbauen (a bei 0,0; b bei 1,0 → in Reichweite).
        {
            let mut w = shared.lock().await;
            let (ta, _) = mpsc::unbounded_channel();
            let (tb, _) = mpsc::unbounded_channel();
            w.players.insert(
                "a".into(),
                crate::world::Player {
                    id: "a".into(),
                    name: "a".into(),
                    x: 0.0,
                    y: 0.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 100,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 0,
                    session_id: String::new(),
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: ta,
                    char_class: "Adventurer".into(),
                    class: crate::class::ClassStatus::Adventurer,
                    faction_transition: false,
                    level: 1,
                    armor: 0,
                    weapon_skill: 1,
                    combat: None,
                    mana: 50,
                    max_mana: 50,
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
                    hp_regen_bonus: 0.0,
                    mana_regen_bonus: 0.0,
                    hp_regen_carry: 0.0,
                    mana_regen_carry: 0.0,
                },
            );
            w.players.insert(
                "b".into(),
                crate::world::Player {
                    id: "b".into(),
                    name: "b".into(),
                    x: 1.0,
                    y: 0.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 100,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 0,
                    session_id: String::new(),
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: tb,
                    char_class: "Mage".into(),
                    class: crate::class::ClassStatus::Mage,
                    faction_transition: false,
                    level: 1,
                    armor: 0,
                    weapon_skill: 1,
                    combat: None,
                    mana: 50,
                    max_mana: 50,
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
                    hp_regen_bonus: 0.0,
                    mana_regen_bonus: 0.0,
                    hp_regen_carry: 0.0,
                    mana_regen_carry: 0.0,
                },
            );
            w.by_conn.insert(7, "a".into());
        }

        // Ungültig: Ziel existiert nicht → kein Kampfzustand.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "ghost"}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        assert!(shared.lock().await.players["a"].combat.is_none());

        // Ungültig: sich selbst anvisieren → kein Kampfzustand.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "a"}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        assert!(shared.lock().await.players["a"].combat.is_none());

        // Gültig: b in Reichweite → bewaffnet.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "b"}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        {
            let w = shared.lock().await;
            let c = w.players["a"].combat.as_ref().expect("bewaffnet");
            assert_eq!(c.target_id, "b");
        }

        // Stop → Kampf beendet.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"stop": true}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        assert!(shared.lock().await.players["a"].combat.is_none());
    }

    #[tokio::test]
    async fn attack_out_of_range_is_rejected() {
        let shared = crate::world::new_shared();
        let cfg = crate::config::combat_config(&Default::default()); // Reichweite 2 m
        {
            let mut w = shared.lock().await;
            let (ta, _) = mpsc::unbounded_channel();
            let (tb, _) = mpsc::unbounded_channel();
            w.players.insert(
                "a".into(),
                crate::world::Player {
                    id: "a".into(),
                    name: "a".into(),
                    x: 0.0,
                    y: 0.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 100,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 0,
                    session_id: String::new(),
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: ta,
                    char_class: "Adventurer".into(),
                    class: crate::class::ClassStatus::Adventurer,
                    faction_transition: false,
                    level: 1,
                    armor: 0,
                    weapon_skill: 1,
                    combat: None,
                    mana: 50,
                    max_mana: 50,
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
                    hp_regen_bonus: 0.0,
                    mana_regen_bonus: 0.0,
                    hp_regen_carry: 0.0,
                    mana_regen_carry: 0.0,
                },
            );
            w.players.insert(
                "b".into(),
                crate::world::Player {
                    id: "b".into(),
                    name: "b".into(),
                    x: 50.0,
                    y: 0.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 100,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 0,
                    session_id: String::new(),
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: tb,
                    char_class: "Mage".into(),
                    class: crate::class::ClassStatus::Mage,
                    faction_transition: false,
                    level: 1,
                    armor: 0,
                    weapon_skill: 1,
                    combat: None,
                    mana: 50,
                    max_mana: 50,
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
                    hp_regen_bonus: 0.0,
                    mana_regen_bonus: 0.0,
                    hp_regen_carry: 0.0,
                    mana_regen_carry: 0.0,
                },
            );
            w.by_conn.insert(7, "a".into());
        }
        // b steht 50 m entfernt → kein gültiger Start.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "b"}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        assert!(shared.lock().await.players["a"].combat.is_none());
    }
}
