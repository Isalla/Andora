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
use crate::world::{disconnect_player, Shared};

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
    Arc::new(Parental { states: Mutex::new(HashMap::new()), auth })
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
        .parental_status(account_id, if session_id.is_empty() { None } else { Some(session_id) })
        .await
        .map_err(|e| {
            log::error!("parental status at hello: {e}");
            "status_unavailable".to_string()
        })?;
    let mut st = ParentalPlayerState::fresh(account_id);
    st.apply_api(&s);
    // State immer speichern (wie Übergangsstand); STATUS nur bei
    // beaufsichtigten Spielern senden.
    parental.states.lock().await.insert(player_id.to_string(), st.clone());
    if !st.enabled {
        return Ok(());
    }
    send_to(tx, &st.status_frame());
    if st.blocked || st.force_logout {
        send_to(tx, &Frame::new(0, s2c::PARENTAL_BLOCKED, serde_json::json!({"reason": "blocked"})));
        return Err("blocked".to_string());
    }
    Ok(())
}

/// Sitzungs-State aufräumen (Disconnect).
pub async fn detach(parental: &SharedParental, player_id: &str) {
    parental.states.lock().await.remove(player_id);
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
    let result = |data: serde_json::Value| send_to(tx, &Frame::new(seq, s2c::PARENTAL_RESULT, data));
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
        "unlock_chat" | "unlock_voice" => match parental.auth.verify_pin(st.account_id, pin).await {
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
        },
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
                    (p.id.clone(), p.account_id, p.session_id.clone(), p.tx.clone())
                })
            })
            .collect()
    };
    let mut kicks: Vec<String> = Vec::new();
    for (pid, account_id, session_id, tx) in jobs {
        let session = if session_id.is_empty() { None } else { Some(session_id.as_str()) };
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
        let mut world = shared.lock().await;
        let mut states = parental.states.lock().await;
        for pid in kicks {
            if let Some(conn) = crate::world::conn_of(&world, &pid) {
                crate::world::close_conn(&mut world, conn);
            }
            disconnect_player(&mut world, &pid);
            states.remove(&pid);
        }
    }
}

/// 10-s-Poller als Hintergrund-Task (Abbruch via JoinHandle::abort).
pub fn start_poller(
    parental: SharedParental,
    shared: Shared,
) -> tokio::task::JoinHandle<()> {
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
}
