// world — Spieler-Registry + AOFB-Tick (Port von src/realm world.ts).
// Versand pro Spieler über einen MPSC-Kanal (Trennung Spielzustand /
// Socket-IO; ohne echte Sockets testbar). Disconnect = Kanal zu.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, Mutex};

use crate::combat::CombatState;
use crate::protocol::{s2c, Frame};

/// Autoritativer Spieler-State auf dem Server. Felder hp/max_hp/lang
/// werden heute gesetzt und von künftigen Systemen (Combat, Level)
/// gelesen — kein Feld entfernen.
///
#[allow(dead_code)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub face: f64,
    pub ping_ms: u32,
    pub zone_id: u32,
    pub hp: i32,
    pub max_hp: i32,
    pub lang: String,
    /// Auth-API-Account (0 = unbekannt/Dev ohne Auth-API).
    pub account_id: u32,
    /// Login-Session ("" = keine); Basis für Elternkontroll-Polling.
    pub session_id: String,
    /// IDs, die dieser Spieler gerade sieht.
    pub entities: HashSet<String>,
    pub last_activity: Instant,
    pub tx: mpsc::UnboundedSender<String>,
    /// Klasse (char_class aus der DB; für Rüstungs-Caps im Kampf).
    pub char_class: String,
    pub level: u32,
    /// Aktueller Rüstungswert (relevante physische Rüstung).
    pub armor: i32,
    /// Level des relevanten Waffen-/Kampfskills (startet bei 1).
    pub weapon_skill: u32,
    /// Aktueller Auto-Angriff (Combat V1): None = nicht im Kampf.
    pub combat: Option<CombatState>,
}

impl Player {
    pub fn send(&self, frame: &Frame) {
        let _ = self.tx.send(frame.encode());
    }
}

#[derive(Debug, Default, Clone)]
pub struct TickStat {
    pub last_ms: f64,
    pub avg_ms: f64,
    pub count: u64,
}

pub struct World {
    pub players: HashMap<String, Player>,
    /// Verbindung (interne Conn-ID) → Spieler-ID.
    pub by_conn: HashMap<u64, String>,
    /// Schließ-Signale je Verbindung (Socket-Closes laufen über net.rs).
    pub closers: HashMap<u64, tokio::sync::oneshot::Sender<()>>,
    pub tick: TickStat,
    pub started: Instant,
}

impl World {
    pub fn new() -> Self {
        Self {
            players: HashMap::new(),
            by_conn: HashMap::new(),
            closers: HashMap::new(),
            tick: TickStat::default(),
            started: Instant::now(),
        }
    }
}

/// Verbindungs-ID eines Spielers (für gezielte Socket-Closes).
pub fn conn_of(world: &World, player_id: &str) -> Option<u64> {
    world
        .by_conn
        .iter()
        .find_map(|(c, p)| (p == player_id).then_some(*c))
}

/// Socket einer Verbindung schließen (HELLO-Ablehnung, Force-Logout).
/// Danach räumt der Disconnect-Pfad in net.rs auf.
pub fn close_conn(world: &mut World, conn_id: u64) {
    if let Some(tx) = world.closers.remove(&conn_id) {
        let _ = tx.send(());
    }
}

pub type Shared = Arc<Mutex<World>>;

pub fn new_shared() -> Shared {
    Arc::new(Mutex::new(World::new()))
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).hypot(ay - by)
}

/// Sendet SPAWN + STATE an o (falls p noch nicht sichtbar).
pub fn ensure_visible(o: &mut Player, p: &Player) {
    if !o.entities.contains(&p.id) {
        o.send(&Frame::new(
            0,
            s2c::SPAWN,
            serde_json::json!({"id": p.id, "kind": "player", "x": p.x, "y": p.y, "face": p.face}),
        ));
        o.entities.insert(p.id.clone());
    }
    o.send(&Frame::new(
        0,
        s2c::STATE,
        serde_json::json!({
            "id": p.id, "x": p.x, "y": p.y, "face": p.face,
            "hp": p.hp, "max_hp": p.max_hp
        }),
    ));
}

/// Welt-Tick: AOFB-Broadcast (SPAWN/STATE/DESPAWN) für alle Spieler.
/// Wie im Übergangsstand O(list²), ausreichend für die Kanalgröße
/// (40–70 Spieler). Arbeitet auf einem Positions-Snapshot, damit keine
/// Borrow-Konflikte zwischen Leser (p) und Schreiber (q) entstehen.
pub fn world_tick(world: &mut World, aofb_radius: f64) {
    let t0 = Instant::now();
    let snap: Vec<(String, f64, f64, f64, i32, i32)> = world
        .players
        .values()
        .map(|p| (p.id.clone(), p.x, p.y, p.face, p.hp, p.max_hp))
        .collect();
    for (qid, qx, qy, _, _, _) in &snap {
        let mut now_visible = HashSet::new();
        for (pid, px, py, _, _, _) in &snap {
            if pid == qid {
                continue;
            }
            if dist(*px, *py, *qx, *qy) <= aofb_radius {
                now_visible.insert(pid.clone());
            }
        }
        let Some(q) = world.players.get_mut(qid.as_str()) else {
            continue;
        };
        for (pid, px, py, face, hp, max_hp) in &snap {
            if pid == qid || !now_visible.contains(pid) {
                continue;
            }
            if !q.entities.contains(pid) {
                q.send(&Frame::new(
                    0,
                    s2c::SPAWN,
                    serde_json::json!({"id": pid, "kind": "player", "x": px, "y": py, "face": face}),
                ));
                q.entities.insert(pid.clone());
            }
            q.send(&Frame::new(
                0,
                s2c::STATE,
                serde_json::json!({
                    "id": pid, "x": px, "y": py, "face": face,
                    "hp": hp, "max_hp": max_hp
                }),
            ));
        }
        let stale: Vec<String> = q
            .entities
            .iter()
            .filter(|e| !now_visible.contains(*e))
            .cloned()
            .collect();
        for eid in stale {
            q.send(&Frame::new(0, s2c::DESPAWN, serde_json::json!({"id": eid})));
            q.entities.remove(&eid);
        }
        for id in &now_visible {
            q.entities.insert(id.clone());
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    world.tick.last_ms = ms;
    world.tick.count += 1;
    world.tick.avg_ms += (ms - world.tick.avg_ms) / world.tick.count as f64;
}

/// Entfernt einen Spieler und benachrichtigt alle, die ihn sahen
/// (DESPAWN). Der Socket wird geschlossen, sobald die Kanal-Sender
/// wegfallen (Forward-Task in net.rs beendet sich dann selbst).
/// Gibt true zurück, wenn der Spieler existierte.
pub fn disconnect_player(world: &mut World, player_id: &str) -> bool {
    let Some(me) = world.players.remove(player_id) else {
        return false;
    };
    world.by_conn.retain(|_, pid| pid != player_id);
    let frame = Frame::new(0, s2c::DESPAWN, serde_json::json!({"id": me.id}));
    for q in world.players.values() {
        if q.entities.contains(&me.id) {
            q.send(&frame);
        }
    }
    // Eigene entities-Sicht der anderen bereinigen.
    for q in world.players.values_mut() {
        q.entities.remove(&me.id);
    }
    true
}

/// MOVE-Anwendung mit Speed-Cap (max 210 m/s, skaliert aufs Tick-
/// Intervall — keine Teleports). Reine Funktion, testbar.
pub fn apply_move(x: f64, y: f64, dx: f64, dy: f64, tick_ms: u64) -> (f64, f64) {
    let len = dx.hypot(dy);
    if len == 0.0 {
        return (x, y);
    }
    let max_step = 210.0 * (tick_ms as f64 / 1000.0);
    let step = len.min(max_step);
    (x + dx / len * step, y + dy / len * step)
}

/// Chat-Text kürzen (240 Zeichen, wie Übergangsstand).
pub fn truncate_chat(text: &str) -> String {
    text.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_player(id: &str, x: f64, y: f64) -> (Player, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Player {
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
                account_id: 0,
                session_id: String::new(),
                entities: HashSet::new(),
                last_activity: Instant::now(),
                tx,
                char_class: "Warrior".into(),
                level: 1,
                armor: 0,
                weapon_skill: 1,
                combat: None,
            },
            rx,
        )
    }

    #[test]
    fn tick_spawns_and_despawns_by_radius() {
        let mut w = World::new();
        let (a, mut ra) = test_player("a", 0.0, 0.0);
        let (b, _rb) = test_player("b", 5.0, 0.0);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);
        world_tick(&mut w, 20.0);
        // a sieht b: SPAWN + STATE (Reihenfolge: erst SPAWN, dann STATE je Tick).
        let m1 = ra.try_recv().unwrap();
        let m2 = ra.try_recv().unwrap();
        assert!(m1.contains("\"type\":2"), "erst SPAWN, got {m1}");
        assert!(m2.contains("\"type\":4"), "dann STATE, got {m2}");
        assert!(w.players["a"].entities.contains("b"));
        // b weit weg -> DESPAWN.
        w.players.get_mut("b").unwrap().x = 1000.0;
        world_tick(&mut w, 20.0);
        let mut saw_despawn = false;
        while let Ok(m) = ra.try_recv() {
            if m.contains("\"type\":3") {
                saw_despawn = true;
            }
        }
        assert!(saw_despawn);
        assert!(!w.players["a"].entities.contains("b"));
    }

    #[test]
    fn move_cap_blocks_teleport() {
        // Tick 100ms -> max 21 m pro MOVE.
        let (x, y) = apply_move(0.0, 0.0, 1000.0, 0.0, 100);
        assert!((x - 21.0).abs() < 1e-9 && y == 0.0);
        let (x, y) = apply_move(0.0, 0.0, 3.0, 4.0, 100);
        assert!((x - 3.0).abs() < 1e-9 && (y - 4.0).abs() < 1e-9);
        let (x, y) = apply_move(1.0, 1.0, 0.0, 0.0, 100);
        assert_eq!((x, y), (1.0, 1.0));
    }

    #[test]
    fn chat_truncated_to_240_chars() {
        assert_eq!(truncate_chat(&"x".repeat(300)).chars().count(), 240);
        assert_eq!(truncate_chat("hi"), "hi");
    }
}
