// world — Spieler-Registry + AOFB-Tick (Port von src/realm world.ts).
// Versand pro Spieler über einen MPSC-Kanal (Trennung Spielzustand /
// Socket-IO; ohne echte Sockets testbar). Disconnect = Kanal zu.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, Mutex};

use crate::combat::CombatState;
use crate::combat::ability::ActiveCast;
use crate::combat::effects::Effect;
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
    /// Klasse (char_class aus der DB).
    pub char_class: String,
    /// Typsichere Klassenbasis (docs/Klassensystem.md): permanente
    /// Grundklasse oder Adventurer; kanonischer Zustand für Tutorialphase,
    /// Hauptattribute-Metadaten, L9-Wahl und L10-Progressions-Hook.
    /// (Aus `char_class` abgeleitet; Tutorialphase L1–8 ist für Adventurer
    /// levelabgeleitet, benötigt kein zusätzliches DB-Feld.)
    pub class: crate::class::ClassStatus,
    /// Fraktions-Übergangs-Hook (docs/Klassensystem.md, L10-Regel): true,
    /// wenn der Charakter nach Fraktionswahl in ein Fraktionsgebiet
    /// gewechselt ist; sonst false. Fraktions-/Zonensystem folgt technisch.
    pub faction_transition: bool,
    pub level: u32,
    /// Gesammelte Erfahrungspunkte (Gruppensystem V1, §7).
    pub exp: i64,
    /// Freie Attributpunkte (docs/Erfahrung_und_Progressionssystem.md §5):
    /// bei Levelaufstieg gutgeschrieben, per Attributs-UI verbrauchbar.
    pub free_attr_points: u32,
    /// Rested-EXP-Pool (docs/Erfahrung_und_Progressionssystem.md §12):
    /// offline bis zu 50 % der aktuellen Level-Anforderung; wird nur durch
    /// Kill-EXP abgerufen (Kill EXP System V1).
    pub rested_pool: i64,
    /// Geldstand (Loot System V1); wird bei Disconnect persistiert.
    pub gold: i64,
    /// Aktueller Rüstungswert (relevante physische Rüstung).
    pub armor: i32,
    /// Level des relevanten Waffen-/Kampfskills (startet bei 1).
    pub weapon_skill: u32,
    /// Aktueller Auto-Angriff (Combat V1): None = nicht im Kampf.
    pub combat: Option<CombatState>,
    /// Mana (Fähigkeits-Ressource, Ability-System.md §2).
    pub mana: i32,
    pub max_mana: i32,
    /// Aktive Effekte (Buffs, Debuffs, DoT/HoT, Control, Combat V3).
    pub effects: Vec<Effect>,
    /// Fähigkeits-Cooldowns: ability_id → ready_at (SystemTime).
    pub cooldowns: BTreeMap<String, std::time::SystemTime>,
    /// Aktiver Cast-Zustand (Combat V3): None = kein Cast aktiv.
    pub active_cast: Option<ActiveCast>,
    /// Gelernte Fähigkeiten (ability_id).
    pub learned_abilities: HashSet<String>,
    /// Grundattribute (docs/Attribute_und_Regeneration.md §§1–3).
    pub attributes: crate::attributes::Attributes,
    /// Basis-Max-HP vor Attributs-Bonus (für recompute_max_resources).
    pub max_hp_base: i32,
    /// Basis-Max-Mana vor Attributs-Bonus (für recompute_max_resources).
    pub max_mana_base: i32,
    /// Sitz-Zustand (docs/Attribute_und_Regeneration.md §5): 125 %
    /// Regeneration nur außerhalb des Kampfes.
    pub sitting: bool,
    /// Additive Regenerationsboni (absolute Werte, §7): Essen/Buffs/…
    /// wirken hier als +HP/s bzw. +Mana/s (Technik-Anschluss; das
    /// Consumable-System folgt später).
    pub hp_regen_bonus: f64,
    pub mana_regen_bonus: f64,
    /// Bruchteil-Carry der Regeneration (f64, §8): dezimale Raten
    /// ohne vorgezogenes Runden über Ticks.
    pub hp_regen_carry: f64,
    pub mana_regen_carry: f64,
    /// Inventory System V1 (docs/inventory_system.md): Grundinventar,
    /// Rucksäcke, Equipment, temporärer Sicherheits-Puffer. Wird bei HELLO
    /// aus realm_state geladen, bei Änderung/Disconnect persistiert.
    pub inventory: crate::inventory::InventoryState,
}

impl Player {
    pub fn send(&self, frame: &Frame) {
        let _ = self.tx.send(frame.encode());
    }

    /// Reiner Progressions-Zustand (docs/Erfahrung_und_Progressionssystem.md)
    /// für die zentrale Berechnungslogik (src/progression.rs).
    pub fn progression_state(&self) -> crate::progression::Progression {
        crate::progression::Progression {
            level: self.level,
            exp: self.exp,
            free_attr_points: self.free_attr_points,
            rested_pool: self.rested_pool,
            class: self.class,
            faction_transition: self.faction_transition,
        }
    }

    /// Schreibt das Ergebnis einer Progressions-Berechnung zurück.
    pub fn apply_progression(&mut self, prog: crate::progression::Progression) {
        self.level = prog.level;
        self.exp = prog.exp;
        self.free_attr_points = prog.free_attr_points;
        self.rested_pool = prog.rested_pool;
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
    /// NPC-/Monster-Registry (Combat V2): key = npc_id().
    pub npcs: HashMap<String, crate::npc::Npc>,
    /// Statische Item-Definitionen (Item System V1, Content-Schicht).
    /// Wird beim Start aus item_definitions geladen; Konsum durch
    /// Inventory/Crafting/Loot folgt in späteren Systemen.
    #[allow(dead_code)]
    pub item_definitions: HashMap<String, crate::item::ItemDefinition>,
    /// Loot-Tabellen (Loot System V1, Content-Schicht, Migration 017).
    pub loot_tables: HashMap<i64, crate::loot::LootTable>,
    /// Aktiver Boden-Loot (Loot System V1): id = "loot_<n>".
    pub loot_drops: HashMap<String, crate::loot::WorldLoot>,
    /// Monoton steigender Zähler für Loot-IDs.
    pub loot_next_id: i64,
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
            npcs: HashMap::new(),
            item_definitions: HashMap::new(),
            loot_tables: HashMap::new(),
            loot_drops: HashMap::new(),
            loot_next_id: 1,
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

/// Welt-Tick: AOFB-Broadcast (SPAWN/STATE/DESPAWN) für alle Spieler UND
/// NPCs (Combat V2). Wie im Übergangsstand O(list²), ausreichend für die
/// Kanalgröße (40–70 Spieler). Arbeitet auf einem Positions-Snapshot, damit
/// keine Borrow-Konflikte zwischen Leser (p) und Schreiber (q) entstehen.
pub fn world_tick(world: &mut World, aofb_radius: f64) {
    let t0 = Instant::now();
    // Entity-Snapshot: Spieler + lebende/kehrende NPCs (tote sind unsichtbar).
    let mut snap: Vec<(String, String, f64, f64, f64, i32, i32)> = Vec::new();
    for p in world.players.values() {
        snap.push((
            p.id.clone(),
            "player".into(),
            p.x,
            p.y,
            p.face,
            p.hp,
            p.max_hp,
        ));
    }
    for n in world.npcs.values() {
        if n.status == crate::npc::NpcStatus::Dead {
            continue; // tot → unsichtbar (Respawn kommt später)
        }
        snap.push((n.id.clone(), "npc".into(), n.x, n.y, 0.0, n.hp, n.max_hp));
    }
    for l in world.loot_drops.values() {
        snap.push((l.id.clone(), "loot".into(), l.x, l.y, 0.0, 0, 0));
    }
    for (qid, _, qx, qy, _, _, _) in &snap {
        if !world.players.contains_key(qid) {
            continue; // nur Spieler empfangen Frames
        }
        let mut now_visible = HashSet::new();
        for (pid, _, px, py, _, _, _) in &snap {
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
        for (pid, kind, px, py, face, hp, max_hp) in &snap {
            if pid == qid || !now_visible.contains(pid) {
                continue;
            }
            if kind == "player" {
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
            } else if kind == "loot" {
                // Loot: LOOT-Frame bei jedem sichtbaren Spieler
                // (beim ersten Sichten einmalig; danach je Tick Mirror
                // von STATE; DESPAWN läuft über stale/entities).
                let Some(l) = world.loot_drops.get(pid.as_str()) else {
                    continue;
                };
                let payload = crate::loot::loot_json(l);
                if !q.entities.contains(pid) {
                    q.send(&Frame::new(0, s2c::LOOT, payload.clone()));
                    q.entities.insert(pid.clone());
                }
                q.send(&Frame::new(0, s2c::LOOT, payload));
            } else {
                let (status, aggro, claimed, name) = {
                    let n = world.npcs.get(pid.as_str());
                    (
                        n.map(|n| n.status.key()).unwrap_or("alive"),
                        n.map(|n| n.target_id.is_some()).unwrap_or(false),
                        n.map(|n| n.claimed_by.is_some()).unwrap_or(false),
                        n.map(|n| n.name.clone()).unwrap_or_default(),
                    )
                };
                if !q.entities.contains(pid) {
                    q.send(&Frame::new(
                        0,
                        s2c::SPAWN,
                        serde_json::json!({
                            "id": pid, "kind": "npc", "x": px, "y": py, "face": face,
                            "extra": {"status": status, "aggro": aggro, "claimed": claimed, "name": name}
                        }),
                    ));
                    q.entities.insert(pid.clone());
                }
                q.send(&Frame::new(
                    0,
                    s2c::STATE,
                    serde_json::json!({
                        "id": pid, "x": px, "y": py, "face": face,
                        "hp": hp, "max_hp": max_hp,
                        "kind": "npc", "status": status, "aggro": aggro, "claimed": claimed
                    }),
                ));
            }
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

/// HP-/Mana-Regeneration pro Tick (docs/Attribute_und_Regeneration.md
/// §§4–8): absolute Raten pro Sekunde × Zustandsmultiplikator
/// (Kampf 15 %, stehend 100 %, sitzend 125 % — Sitzbonus nie im Kampf),
/// intern f64 mit Carry je Ressource; gedeckelt auf 0 bzw. max. Tote
/// (hp == 0) regenerieren nicht (keine Wiederbelebung). Teilt sich die
/// gemeinsame Kernlogik in crate::regen (HP UND Mana, NPC-fähig).
pub fn world_regen_tick(world: &mut World, tick_ms: u64) {
    for p in world.players.values_mut() {
        crate::regen::apply_regen(p, tick_ms);
    }
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
                char_class: "Adventurer".into(),
                class: crate::class::ClassStatus::Adventurer,
                faction_transition: false,
                level: 1,
                exp: 0,
                free_attr_points: 0,
                rested_pool: 0,
                gold: 0,
                armor: 0,
                weapon_skill: 1,
                combat: None,
                mana: 50,
                max_mana: 50,
                effects: Vec::new(),
                cooldowns: std::collections::BTreeMap::new(),
                active_cast: None,
                learned_abilities: HashSet::new(),
                attributes: Default::default(),
                max_hp_base: 100,
                max_mana_base: 50,
                sitting: false,
                hp_regen_bonus: 0.0,
                mana_regen_bonus: 0.0,
                hp_regen_carry: 0.0,
                mana_regen_carry: 0.0,
                inventory: Default::default(),
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
