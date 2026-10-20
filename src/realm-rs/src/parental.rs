// parental — Serverseitige Elternkontrolle im Realm (Port von
// src/realm parental.ts). Der Client entscheidet NIE: alle Werte kommen
// von der Auth-API. Ablauf: HELLO -> /parental/status; BLOCKED am Login
// verweigert den Einstieg (fail-closed); 10-s-Poll pro beaufsichtigtem
// Spieler; force_logout trennt (PARENTAL_BLOCKED + Kick). Sitzungs-
// Freischaltungen (Chat/Voice nach PIN) verfallen am Disconnect.
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use crate::auth_api::{AuthApi, ParentalStatus};
use crate::protocol::{s2c, Frame};
use crate::world::Shared;

/// Realm-seitiger Elternkontroll-State eines Spielers (Sitzungsspeicher).
#[derive(Debug, Clone)]
pub struct ParentalPlayerState {
    pub account_id: u32,
    pub enabled: bool,
    pub remaining_seconds: i64,
    pub blocked: bool,
    pub buffer_until: Option<String>,
    pub force_logout: bool,
    pub chat_allowed: bool,
    pub voice_allowed: bool,
    pub warning: bool,
    pub extended_used_today: bool,
    /// Temporäre Sitzungs-Freischaltungen nach PIN (verfallen am Disconnect).
    pub temp_chat: bool,
    pub temp_voice: bool,
}

impl ParentalPlayerState {
    fn fresh(account_id: u32) -> Self {
        Self {
            account_id,
            enabled: false,
            remaining_seconds: 0,
            blocked: false,
            buffer_until: None,
            force_logout: false,
            chat_allowed: true,
            voice_allowed: true,
            warning: false,
            extended_used_today: false,
            temp_chat: false,
            temp_voice: false,
        }
    }

    fn apply_api(&mut self, s: &ParentalStatus) {
        self.enabled = s.enabled;
        self.remaining_seconds = s.remaining_seconds;
        self.blocked = s.blocked;
        self.buffer_until = s.buffer_until.clone();
        self.force_logout = s.force_logout;
        self.chat_allowed = s.chat_allowed;
        self.voice_allowed = s.voice_allowed;
        self.warning = s.warning;
        self.extended_used_today = s.extended_used_today;
    }

    fn status_frame(&self) -> Frame {
        Frame::new(
            0,
            s2c::PARENTAL_STATUS,
            serde_json::json!({
                "remaining_seconds": self.remaining_seconds,
                "blocked": self.blocked,
                "buffer_until": self.buffer_until,
                "warning": self.warning,
                "chat_allowed": self.chat_allowed || self.temp_chat,
                "voice_allowed": self.voice_allowed || self.temp_voice,
                "extended_used_today": self.extended_used_today,
            }),
        )
    }
}

pub struct Parental {
    states: Mutex<HashMap<String, ParentalPlayerState>>,
    auth: AuthApi,
}

pub type SharedParental = Arc<Parental>;

pub fn new_shared(auth: AuthApi) -> SharedParental {
    Arc::new(Parental {
        states: Mutex::new(HashMap::new()),
        auth,
    })
}

fn send_to(tx: &mpsc::UnboundedSender<String>, frame: &Frame) {
    let _ = tx.send(frame.encode());
}

/// Elternkontrolle für einen frisch registrierten Spieler anhängen.
/// Ok(()) = Einstieg erlaubt; Err(reason) = verweigern + schließen.
/// Auth-API-Fehler beim Login sind fail-closed.
pub async fn attach(
    parental: &SharedParental,
    tx: &mpsc::UnboundedSender<String>,
    player_id: &str,
    account_id: u32,
    session_id: &str,
) -> Result<(), String> {
    if !parental.auth.enabled() || account_id == 0 {
        return Ok(());
    }
    let s = parental
        .auth
        .parental_status(
            account_id,
            if session_id.is_empty() {
                None
            } else {
                Some(session_id)
            },
        )
        .await
        .map_err(|e| {
            log::error!("parental status at hello: {e}");
            "status_unavailable".to_string()
        })?;
    let mut st = ParentalPlayerState::fresh(account_id);
    st.apply_api(&s);
    // State immer speichern (wie Übergangsstand); STATUS nur bei
    // beaufsichtigten Spielern senden.
    parental
        .states
        .lock()
        .await
        .insert(player_id.to_string(), st.clone());
    if !st.enabled {
        return Ok(());
    }
    send_to(tx, &st.status_frame());
    if st.blocked || st.force_logout {
        send_to(
            tx,
            &Frame::new(
                0,
                s2c::PARENTAL_BLOCKED,
                serde_json::json!({"reason": "blocked"}),
            ),
        );
        return Err("blocked".to_string());
    }
    Ok(())
}

/// Sitzungs-State aufräumen (Disconnect).
pub async fn detach(parental: &SharedParental, player_id: &str) {
    parental.states.lock().await.remove(player_id);
}

/// Test-only: seed a parent-locked state (enabled, chat locked) so the
/// communication gate can be exercised offline without the Auth-API.
#[cfg(test)]
pub async fn seed_chat_locked(parental: &SharedParental, player_id: &str) {
    parental.states.lock().await.insert(
        player_id.to_string(),
        ParentalPlayerState {
            account_id: 0,
            enabled: true,
            remaining_seconds: 0,
            blocked: false,
            buffer_until: None,
            force_logout: false,
            chat_allowed: false,
            voice_allowed: false,
            warning: false,
            extended_used_today: false,
            temp_chat: false,
            temp_voice: false,
        },
    );
}

/// Chat-Gate (temporäre Freischaltung inklusive).
pub async fn chat_allowed(parental: &SharedParental, player_id: &str) -> bool {
    match parental.states.lock().await.get(player_id) {
        None => true,
        Some(st) if !st.enabled => true,
        Some(st) => st.chat_allowed || st.temp_chat,
    }
}

/// Voice-Gate (für das spätere Voice-System; Flag schon heute da).
#[allow(dead_code)]
pub async fn voice_allowed(parental: &SharedParental, player_id: &str) -> bool {
    match parental.states.lock().await.get(player_id) {
        None => true,
        Some(st) if !st.enabled => true,
        Some(st) => st.voice_allowed || st.temp_voice,
    }
}

fn valid_pin(pin: &str) -> bool {
    (4..=16).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

/// C2S.PARENTAL: {action:'extend'|'unlock_chat'|'unlock_voice', pin}.
/// Antwort via S2C.PARENTAL_RESULT (+ ggf. PARENTAL_STATUS).
pub async fn handle_message(
    parental: &SharedParental,
    tx: &mpsc::UnboundedSender<String>,
    player_id: &str,
    seq: i64,
    action: &str,
    pin: &str,
) {
    let result =
        |data: serde_json::Value| send_to(tx, &Frame::new(seq, s2c::PARENTAL_RESULT, data));
    let mut states = parental.states.lock().await;
    let Some(st) = states.get_mut(player_id) else {
        result(serde_json::json!({"ok": false, "reason": "not_supervised"}));
        return;
    };
    if !parental.auth.enabled() || !st.enabled {
        result(serde_json::json!({"ok": false, "reason": "not_supervised"}));
        return;
    }
    if !valid_pin(pin) {
        result(serde_json::json!({"ok": false, "reason": "bad_pin"}));
        return;
    }
    match action {
        "extend" => match parental.auth.use_extension(st.account_id, pin).await {
            Ok(r) => {
                st.remaining_seconds = r.remaining_seconds;
                st.extended_used_today = true;
                send_to(tx, &st.status_frame());
                result(serde_json::json!({
                    "ok": true, "unlocked": "extend",
                    "remaining_seconds": r.remaining_seconds, "extended_used_today": true
                }));
            }
            Err(e) => {
                if e.status() == Some(409) {
                    result(serde_json::json!({"ok": false, "reason": "extension_used"}));
                } else {
                    log::error!("parental action: {e}");
                    result(serde_json::json!({"ok": false, "reason": "service_unavailable"}));
                }
            }
        },
        "unlock_chat" | "unlock_voice" => {
            match parental.auth.verify_pin(st.account_id, pin).await {
                Ok(v) if v.valid => {
                    if action == "unlock_chat" {
                        st.temp_chat = true;
                    } else {
                        st.temp_voice = true;
                    }
                    send_to(tx, &st.status_frame());
                    result(serde_json::json!({"ok": true, "unlocked": action}));
                }
                Ok(_) => result(serde_json::json!({"ok": false, "reason": "bad_pin"})),
                Err(e) => {
                    if e.status() == Some(401) {
                        result(serde_json::json!({"ok": false, "reason": "bad_pin"}));
                    } else {
                        log::error!("parental action: {e}");
                        result(serde_json::json!({"ok": false, "reason": "service_unavailable"}));
                    }
                }
            }
        }
        _ => result(serde_json::json!({"ok": false, "reason": "unknown_action"})),
    }
}

/// Ein Poll-Durchlauf: Status je beaufsichtigtem Spieler laden, STATUS
/// senden, bei force_logout kicken (BLOCKED + Disconnect).
/// Netz-/API-Fehler: letzten bekannten State behalten (kein Kick).
pub async fn poll_once(parental: &SharedParental, shared: &Shared) {
    if !parental.auth.enabled() {
        return;
    }
    // Snapshot der zu pollenden Spieler (kein Lock über .await halten).
    let jobs: Vec<(String, u32, String, mpsc::UnboundedSender<String>)> = {
        let world = shared.lock().await;
        let states = parental.states.lock().await;
        world
            .players
            .values()
            .filter_map(|p| {
                states.get(&p.id).filter(|st| st.enabled).map(|_| {
                    (
                        p.id.clone(),
                        p.account_id,
                        p.session_id.clone(),
                        p.tx.clone(),
                    )
                })
            })
            .collect()
    };
    let mut kicks: Vec<String> = Vec::new();
    for (pid, account_id, session_id, tx) in jobs {
        let session = if session_id.is_empty() {
            None
        } else {
            Some(session_id.as_str())
        };
        match parental.auth.parental_status(account_id, session).await {
            Ok(s) => {
                let mut states = parental.states.lock().await;
                if let Some(st) = states.get_mut(&pid) {
                    let force = s.force_logout;
                    st.apply_api(&s);
                    send_to(&tx, &st.status_frame());
                    if force {
                        send_to(
                            &tx,
                            &Frame::new(
                                0,
                                s2c::PARENTAL_BLOCKED,
                                serde_json::json!({"reason": "buffer_expired"}),
                            ),
                        );
                        kicks.push(pid);
                    }
                }
            }
            Err(e) => log::error!("parental poll: {e}"),
        }
    }
    if !kicks.is_empty() {
        kick_owner_connections(shared, &kicks).await;
    }
}

/// Force-Logout (AUTH-03): Schließt gezielt die aktuelle Eigentümer-Verbindung
/// des Kick-Spielers. KEIN vorzeitiges charakterweites Cleanup — der zentrale
/// Disconnect-Pfad dieser Verbindung (finaler Persistenz-Flush, `logout_at`,
/// verbindungsspezifischer Registry-Cleanup, `detach`, Gruppenstatus) läuft
/// regulär in net.rs, sobald das Ende der Verbindung dort aufläuft.
pub async fn kick_owner_connections(shared: &Shared, kicks: &[String]) {
    let mut world = shared.lock().await;
    for pid in kicks {
        match crate::world::conn_of(&world, pid) {
            Some(conn) => crate::world::close_conn(&mut world, conn),
            None => log::info!("parental kick {pid}: keine Eigentümer-Verbindung"),
        }
    }
}

/// 10-s-Poller als Hintergrund-Task (Abbruch via JoinHandle::abort).
pub fn start_poller(parental: SharedParental, shared: Shared) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            poll_once(&parental, &shared).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_format() {
        assert!(valid_pin("1234"));
        assert!(valid_pin("0000000000000000"));
        assert!(!valid_pin("123"));
        assert!(!valid_pin("12345678901234567"));
        assert!(!valid_pin("12ab"));
        assert!(!valid_pin(""));
    }

    #[test]
    fn status_frame_shape() {
        let mut st = ParentalPlayerState::fresh(7);
        st.enabled = true;
        st.remaining_seconds = 3600;
        let f = st.status_frame();
        assert_eq!(f.msg_type, s2c::PARENTAL_STATUS);
        let v: serde_json::Value = serde_json::from_str(&f.encode()).unwrap();
        assert_eq!(v["data"]["remaining_seconds"], 3600);
        assert_eq!(v["data"]["chat_allowed"], true);
    }

    /// AUTH-03: Der Force-Logout schließt ausschließlich die aktuelle
    /// Eigentümer-Verbindung und führt KEIN vorzeitiges charakterweites
    /// Cleanup aus: Player und `by_conn`-Zuordnung bleiben bestehen, bis der
    /// zentrale Disconnect-Pfad dieser Verbindung in net.rs gelaufen ist.
    #[tokio::test]
    async fn force_logout_closes_owner_without_premature_cleanup() {
        let shared = crate::world::new_shared();
        let (close_tx, close_rx) = tokio::sync::oneshot::channel::<()>();
        {
            let mut world = shared.lock().await;
            let (tx, _player_rx) = mpsc::unbounded_channel();
            let p = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 100,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-1".into(),
                entities: Default::default(),
                last_activity: std::time::Instant::now(),
                tx,
                char_class: "Adventurer".into(),
                class: crate::class::ClassStatus::Adventurer,
                faction_transition: false,
                level: 1,
                exp: 0,
                free_attr_points: 0,
                rested_pool: 0,
                idia: 0,
                armor: 0,
                weapon_skill: 1,
                combat: None,
                last_strike: None,
                mana: 50,
                max_mana: 50,
                effects: Vec::new(),
                cooldowns: Default::default(),
                active_cast: None,
                learned_abilities: Default::default(),
                attributes: Default::default(),
                max_hp_base: 100,
                max_mana_base: 50,
                sitting: false,
                hp_regen_bonus: 0.0,
                mana_regen_bonus: 0.0,
                hp_regen_carry: 0.0,
                mana_regen_carry: 0.0,
                inventory: Default::default(),
                quests: Default::default(),
                dirty: Default::default(),
                persist_generation: 0,
                persist_revision: 0,
            };
            world.players.insert("hero".into(), p);
            world.by_conn.insert(42, "hero".into());
            world.closers.insert(42, close_tx);
        }
        kick_owner_connections(&shared, &["hero".to_string()]).await;
        // Closer der Eigentümer-Verbindung wurde signalisiert …
        assert!(
            close_rx.await.is_ok(),
            "Owner-Verbindung wurde nicht geschlossen"
        );
        // … aber es gab KEIN vorzeitiges charakterweites Cleanup.
        let world = shared.lock().await;
        assert!(world.players.contains_key("hero"), "Player entfernt");
        assert!(
            crate::world::is_owner(&world, 42, "hero"),
            "Eigentümer entfernt"
        );
        assert_eq!(world.by_conn.len(), 1);
    }

    // ── Chat-Doppelecho (FIX_SERVER_CHAT_DOUBLE_ECHO) ────────────────────
    // handle_chat lieferte eigene Nachrichten doppelt (Radius-Broadcast
    // inklusive Sender plus direktes Echo). Nachweis am echten Handler:
    // Sender aus dem Broadcast, Echo genau einmal, Rest unveraendert.

    fn chat_test_parental() -> SharedParental {
        let auth = AuthApi::new(&crate::config::AuthApiConfig {
            url: String::new(),
            service_id: String::new(),
            secret: String::new(),
        })
        .expect("authapi client");
        new_shared(auth)
    }

    fn chat_mk_player(
        id: &str,
        x: f64,
        y: f64,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> crate::world::Player {
        crate::world::Player {
            id: id.into(),
            name: id.into(),
            x,
            y,
            face: 0.0,
            ping_ms: 0,
            zone_id: 0,
            hp: 100,
            max_hp: 100,
            lang: "de".into(),
            account_id: 7,
            session_id: "sess-1".into(),
            entities: Default::default(),
            last_activity: std::time::Instant::now(),
            tx,
            char_class: "Adventurer".into(),
            class: crate::class::ClassStatus::Adventurer,
            faction_transition: false,
            level: 1,
            exp: 0,
            free_attr_points: 0,
            rested_pool: 0,
            idia: 0,
            armor: 0,
            weapon_skill: 1,
            combat: None,
            last_strike: None,
            mana: 50,
            max_mana: 50,
            effects: Vec::new(),
            cooldowns: Default::default(),
            active_cast: None,
            learned_abilities: Default::default(),
            attributes: Default::default(),
            max_hp_base: 100,
            max_mana_base: 50,
            sitting: false,
            hp_regen_bonus: 0.0,
            mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0,
            mana_regen_carry: 0.0,
            inventory: Default::default(),
            quests: Default::default(),
            dirty: Default::default(),
            persist_generation: 0,
            persist_revision: 0,
        }
    }

    /// a (Sender, conn 7) bei (0,0), b (nah, conn 8) bei (1,0), c (fern,
    /// conn 9) bei (50,50). Gibt (rx_a, rx_b, rx_c, tx_a) zurueck.
    async fn chat_world() -> (
        Shared,
        tokio::sync::mpsc::UnboundedReceiver<String>,
        tokio::sync::mpsc::UnboundedReceiver<String>,
        tokio::sync::mpsc::UnboundedReceiver<String>,
        tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        let shared = crate::world::new_shared();
        let (tx_a, rx_a) = mpsc::unbounded_channel();
        let (tx_b, rx_b) = mpsc::unbounded_channel();
        let (tx_c, rx_c) = mpsc::unbounded_channel();
        {
            let mut world = shared.lock().await;
            world
                .players
                .insert("a".into(), chat_mk_player("a", 0.0, 0.0, tx_a.clone()));
            world
                .players
                .insert("b".into(), chat_mk_player("b", 1.0, 0.0, tx_b));
            world
                .players
                .insert("c".into(), chat_mk_player("c", 50.0, 50.0, tx_c));
            world.by_conn.insert(7, "a".into());
            world.by_conn.insert(8, "b".into());
            world.by_conn.insert(9, "c".into());
        }
        (shared, rx_a, rx_b, rx_c, tx_a)
    }

    fn drain_values(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    ) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        while let Ok(s) = rx.try_recv() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                out.push(v);
            }
        }
        out
    }

    fn chat_only(frames: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        frames
            .into_iter()
            .filter(|v| v.get("type") == Some(&serde_json::json!(crate::protocol::s2c::CHAT)))
            .collect()
    }

    async fn chat_send(
        parental: &SharedParental,
        shared: &Shared,
        tx_a: &tokio::sync::mpsc::UnboundedSender<String>,
        text: &str,
    ) {
        crate::handlers::handle_chat(
            parental,
            shared,
            tx_a,
            7,
            1,
            &serde_json::json!({"channel": "lokal", "text": text}),
            20.0,
        )
        .await;
    }

    /// Eine Sendung: Sender genau einmal (Echo), naher Empfaenger genau
    /// einmal, ferner Empfaenger nichts (Reichweite unveraendert).
    #[tokio::test]
    async fn chat_single_send_sender_once_near_once_far_none() {
        let parental = chat_test_parental();
        let (shared, mut rx_a, mut rx_b, mut rx_c, tx_a) = chat_world().await;
        chat_send(&parental, &shared, &tx_a, "hi").await;
        let a = chat_only(drain_values(&mut rx_a));
        let b = chat_only(drain_values(&mut rx_b));
        let c = chat_only(drain_values(&mut rx_c));
        assert_eq!(a.len(), 1, "Sender erhaelt genau ein Echo");
        assert_eq!(b.len(), 1, "naher Empfaenger erhaelt genau eine Zustellung");
        assert!(c.is_empty(), "ferner Empfaenger ausserhalb Reichweite");
        assert_eq!(a[0]["data"]["from"], "a");
        assert_eq!(a[0]["data"]["channel"], "lokal");
        assert_eq!(a[0]["data"]["text"], "hi");
    }

    /// Zwei gleiche Sendungen: jeweils zwei Zustellungen (kein
    /// Text-Vergleich, keine Deduplication).
    #[tokio::test]
    async fn chat_two_identical_sends_deliver_twice_each() {
        let parental = chat_test_parental();
        let (shared, mut rx_a, mut rx_b, _rx_c, tx_a) = chat_world().await;
        chat_send(&parental, &shared, &tx_a, "hi").await;
        chat_send(&parental, &shared, &tx_a, "hi").await;
        assert_eq!(chat_only(drain_values(&mut rx_a)).len(), 2);
        assert_eq!(chat_only(drain_values(&mut rx_b)).len(), 2);
    }

    /// Leere Nachricht: keine Chat-Zustellung an niemanden.
    #[tokio::test]
    async fn chat_empty_text_delivers_nothing() {
        let parental = chat_test_parental();
        let (shared, mut rx_a, mut rx_b, mut rx_c, tx_a) = chat_world().await;
        chat_send(&parental, &shared, &tx_a, "").await;
        assert!(chat_only(drain_values(&mut rx_a)).is_empty());
        assert!(chat_only(drain_values(&mut rx_b)).is_empty());
        assert!(chat_only(drain_values(&mut rx_c)).is_empty());
    }

    /// Gesperrter Chat (Parental): keine Chat-Zustellung, nur
    /// PARENTAL_RESULT am Sender (Chat-/Parental-Pruefung unveraendert).
    #[tokio::test]
    async fn chat_locked_delivers_no_chat() {
        let parental = chat_test_parental();
        let (shared, mut rx_a, mut rx_b, _rx_c, tx_a) = chat_world().await;
        let mut st = ParentalPlayerState::fresh(7);
        st.enabled = true;
        st.chat_allowed = false;
        parental
            .states
            .lock()
            .await
            .insert("a".into(), st);
        chat_send(&parental, &shared, &tx_a, "hi").await;
        let all_a = drain_values(&mut rx_a);
        assert!(chat_only(all_a.clone()).is_empty());
        assert_eq!(all_a.len(), 1, "genau eine Antwort (Ablehnung)");
        assert_eq!(all_a[0]["type"], crate::protocol::s2c::PARENTAL_RESULT);
        assert_eq!(all_a[0]["data"]["reason"], "chat_locked");
        assert!(chat_only(drain_values(&mut rx_b)).is_empty());
    }
}
