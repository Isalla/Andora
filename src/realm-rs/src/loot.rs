// loot — Loot System V1 (docs/Lootsystem.md), Server-autoritativ.
// Umgesetzt nach docs/Lootsystem.md:
//   - Content-Schicht: Loot-Tabellen + unabhängige Einträge (Migration 017),
//   - unabhängige Würfe je Kill (kein "genau ein Eintrag", kein Pity/Luck),
//   - Claim (Lootsystem.md §Claim): erster gültiger Schaden am Monster
//     berechtigt; Gruppenmitglied → Gruppe als Einheit ("g:<id>"),
//   - FFA-V1: jeder Berechtigte darf frei einnehmen; KEINE Need/Greed-,
//     Leiter-, Würfel- oder Master-Loot-Mechanik (folgt in V2),
//   - Items/Gold als Bodenloot mit Despawn-Zeit; nur Truhen haben eine
//     Claim-Frist und werden danach öffentlich,
//   - Gold-Pickup: Aufteilung an die aktiven Gruppenmitglieder (stabil),
//   - Truhe: Inhalt wird beim SPAWN einmalig gerollt und serverseitig
//     gehalten; Öffnen erzeugt normalen Bodenloot (kein Inventory-Insert).
//
// Kein Teil enthält Gameplay-Balancing — Werte kommen aus Content (DB)
// und Config (LootCfg).
// Der Modulname (loot) kollidiert bewusst NICHT: alte, kaputte Variante
// dieses Moduls wurde vollständig ersetzt.
use std::collections::HashMap;
use std::time::Instant;

use crate::combat::CombatRng;
use crate::config::LootCfg;
use crate::group::{self, GroupManager};
use crate::world::World;

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).hypot(ay - by)
}

// ── Content-Schicht ───────────────────────────────────────────────────

/// Drop-Art eines Loot-Eintrags (Migration 017: Spalte kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootKind {
    Item,
    Gold,
    Chest,
}

impl LootKind {
    pub fn from_db(s: &str) -> Option<LootKind> {
        match s.trim().to_lowercase().as_str() {
            "item" => Some(LootKind::Item),
            "gold" => Some(LootKind::Gold),
            "chest" => Some(LootKind::Chest),
            _ => None,
        }
    }

    /// Protokoll-/JSON-Schlüssel (s2c::LOOT, kind).
    pub fn as_key(self) -> &'static str {
        match self {
            LootKind::Item => "item",
            LootKind::Gold => "gold",
            LootKind::Chest => "chest",
        }
    }
}

/// Ein Eintrag einer Loot-Tabelle (unabhängiger Wurf je Kill).
#[derive(Debug, Clone)]
pub struct LootEntry {
    pub kind: LootKind,
    /// item: fallengelassene Definition.
    pub item_id: Option<String>,
    pub min_quantity: i64,
    pub max_quantity: i64,
    /// Unabhängige Dropchance (0..1).
    pub chance: f64,
    /// chest: Inhaltstabelle (V1: nur item/gold; verschachtelte Truhe
    /// wird beim Rollen ignoriert).
    pub content_table_id: Option<i64>,
}

/// Eine benannte Loot-Tabelle (Content-Schicht, Migration 017).
#[derive(Debug, Clone)]
pub struct LootTable {
    /// Content-Id (Wird als Schlüssel im World gemappt; hier deklarativ).
    #[allow(dead_code)]
    pub id: i64,
    /// Content-Name (Diagnose/Admin; noch kein Laufzeit-Konsument).
    #[allow(dead_code)]
    pub name: String,
    pub entries: Vec<LootEntry>,
}

/// Ein rollfertiger Einzel-Baustein eines (Truhen-)Inhalts.
#[derive(Debug, Clone)]
pub enum LootPayload {
    Item { item_id: String, count: i64 },
    Gold { amount: i64 },
}

// ── Runtime: Bodenloot ────────────────────────────────────────────────

/// Server-autoritativer Boden-Loot-Drop. id = "loot_<n>" (world.loot_next_id).
#[derive(Debug, Clone)]
pub struct WorldLoot {
    pub id: String,
    pub kind: LootKind,
    pub x: f64,
    pub y: f64,
    /// item: Definition; sonst None.
    pub item_id: Option<String>,
    /// item: Stückzahl; gold: Betrag; chest: 0.
    pub count: i64,
    /// Claim: "g:<gid>" (Gruppe) oder rohe Spieler-ID. None = für alle.
    pub claimed_by: Option<String>,
    /// Truhe: beim SPAWN einmalig ausgewürfelter Inhalt (serverseitig).
    pub content: Vec<LootPayload>,
    /// Nur für Diagnose-/Debug-Ausgaben vorgesehen.
    #[allow(dead_code)]
    pub spawned_at: Instant,
    /// items/gold: Despawnzeitpunkt.
    pub despawn_at: Instant,
    /// nur Truhe: Ende der Claim-Frist (danach öffentlich).
    pub chest_claim_until: Option<Instant>,
    /// nur Truhe: Gesamtlebensdauer (Truhe verschwindet unabhängig).
    pub chest_despawn_at: Option<Instant>,
}

// ── Würfeln (Content → Drops) ─────────────────────────────────────────

/// Gleichverteilte ganze Zahl in [min, max] (max >= min, sonst min).
fn quantity_in(rng: &mut dyn CombatRng, min: i64, max: i64) -> i64 {
    if max <= min {
        return min.max(0);
    }
    min.max(0) + (rng.next() * (max - min + 1) as f64) as i64
}

/// Rollt einen Truhen-Inhalt (V1: nur item/gold möglich; chest-Einträge
/// in Inhaltstabellen werden ignoriert — keine Rekursion).
fn roll_payloads(
    table: &LootTable,
    tables: &HashMap<i64, LootTable>,
    rng: &mut dyn CombatRng,
) -> Vec<LootPayload> {
    let mut out = Vec::new();
    for e in &table.entries {
        if rng.next() >= e.chance {
            continue;
        }
        match e.kind {
            LootKind::Item => {
                if let Some(item_id) = &e.item_id {
                    let c = quantity_in(rng, e.min_quantity, e.max_quantity);
                    if c > 0 {
                        out.push(LootPayload::Item {
                            item_id: item_id.clone(),
                            count: c,
                        });
                    }
                }
            }
            LootKind::Gold => {
                let a = quantity_in(rng, e.min_quantity, e.max_quantity);
                if a > 0 {
                    out.push(LootPayload::Gold { amount: a });
                }
            }
            LootKind::Chest => {
                // Verschachtelte Truhen sind V2 — Inhalt wird ignoriert.
                let _ = tables;
            }
        }
    }
    out
}

/// Gerollter Drop-Punkt beim NPC-Tod (ein Eintrag der Tabelle).
#[derive(Debug, Clone)]
pub enum RolledDrop {
    Item { item_id: String, count: i64 },
    Gold { amount: i64 },
    Chest { content: Vec<LootPayload> },
}

/// Rollt alle unabhängigen Einträge einer Tabelle für einen Kill.
/// Truhen mit leerem Inhalt werden NICHT erzeugt (eine leere Truhe
/// erscheint nie — "leere Truhe despawnt sofort").
pub fn roll_drops(
    table: &LootTable,
    tables: &HashMap<i64, LootTable>,
    rng: &mut dyn CombatRng,
) -> Vec<RolledDrop> {
    let mut out = Vec::new();
    for e in &table.entries {
        if rng.next() >= e.chance {
            continue;
        }
        match e.kind {
            LootKind::Item => {
                if let Some(item_id) = &e.item_id {
                    let c = quantity_in(rng, e.min_quantity, e.max_quantity);
                    if c > 0 {
                        out.push(RolledDrop::Item {
                            item_id: item_id.clone(),
                            count: c,
                        });
                    }
                }
            }
            LootKind::Gold => {
                let a = quantity_in(rng, e.min_quantity, e.max_quantity);
                if a > 0 {
                    out.push(RolledDrop::Gold { amount: a });
                }
            }
            LootKind::Chest => {
                if let Some(tid) = e.content_table_id {
                    if let Some(t) = tables.get(&tid) {
                        let content = roll_payloads(t, tables, rng);
                        if !content.is_empty() {
                            out.push(RolledDrop::Chest { content });
                        }
                    }
                }
            }
        }
    }
    out
}

// ── Spawn ─────────────────────────────────────────────────────────────

/// Erzeugt einen Boden-Loot-Drop in der Welt. Gibt die neue Loot-ID zurück.
pub fn spawn_drop(
    world: &mut World,
    kind: LootKind,
    x: f64,
    y: f64,
    item_id: Option<String>,
    count: i64,
    content: Vec<LootPayload>,
    claimed_by: Option<String>,
    cfg: &LootCfg,
    now: Instant,
) -> String {
    let id = format!("loot_{}", world.loot_next_id);
    world.loot_next_id += 1;
    let (despawn_at, chest_claim_until, chest_despawn_at) = match kind {
        LootKind::Chest => (
            now + std::time::Duration::from_millis(cfg.chest_despawn_ms),
            Some(now + std::time::Duration::from_millis(cfg.chest_claim_ms)),
            Some(now + std::time::Duration::from_millis(cfg.chest_despawn_ms)),
        ),
        _ => (
            now + std::time::Duration::from_millis(cfg.despawn_ms),
            None,
            None,
        ),
    };
    world.loot_drops.insert(
        id.clone(),
        WorldLoot {
            id: id.clone(),
            kind,
            x,
            y,
            item_id,
            count,
            claimed_by,
            content,
            spawned_at: now,
            despawn_at,
            chest_claim_until,
            chest_despawn_at,
        },
    );
    id
}

/// NPC-Tod (Combat V2): alle unabhängigen Drops der Loot-Tabelle spawnen.
/// Gibt die Anzahl erzeugter Drops zurück (0 = kein Loot/leer).
pub fn spawn_npc_loot(
    world: &mut World,
    loot_table_id: Option<i64>,
    claim: &Option<String>,
    x: f64,
    y: f64,
    cfg: &LootCfg,
    now: Instant,
    rng: &mut dyn CombatRng,
) -> usize {
    let Some(tid) = loot_table_id else {
        return 0; // Monster ohne Loot-Tabelle → nichts.
    };
    let Some(table) = world.loot_tables.get(&tid).cloned() else {
        log::error!("Nicht geladene Loot-Tabelle {tid} — kein Loot");
        return 0;
    };
    let mut spawned = 0;
    for drop in roll_drops(&table, &world.loot_tables, rng) {
        match drop {
            RolledDrop::Item { item_id, count } => {
                let _ = spawn_drop(
                    world,
                    LootKind::Item,
                    x,
                    y,
                    Some(item_id),
                    count,
                    Vec::new(),
                    claim.clone(),
                    cfg,
                    now,
                );
                spawned += 1;
            }
            RolledDrop::Gold { amount } => {
                let _ = spawn_drop(
                    world,
                    LootKind::Gold,
                    x,
                    y,
                    None,
                    amount,
                    Vec::new(),
                    claim.clone(),
                    cfg,
                    now,
                );
                spawned += 1;
            }
            RolledDrop::Chest { content } => {
                let _ = spawn_drop(
                    world,
                    LootKind::Chest,
                    x,
                    y,
                    None,
                    0,
                    content,
                    claim.clone(),
                    cfg,
                    now,
                );
                spawned += 1;
            }
        }
    }
    spawned
}

// ── Tick ──────────────────────────────────────────────────────────────

/// Despawn von abgelaufenen Drops (items/gold: despawn_at; Truhe:
/// chest_despawn_at). Die Sichtbarkeit räumt world_tick (DESPAWN) auf.
pub fn loot_tick(world: &mut World, _cfg: &LootCfg, now: Instant) {
    let expired: Vec<String> = world
        .loot_drops
        .iter()
        .filter(|(_, l)| {
            let at = l.chest_despawn_at.unwrap_or(l.despawn_at);
            now >= at
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        world.loot_drops.remove(&id);
    }
}

// ── Sichtbarkeit + Aufnahme ───────────────────────────────────────────

/// s2c::LOOT-Payload für einen Drop.
pub fn loot_json(l: &WorldLoot) -> serde_json::Value {
    let mut v = serde_json::json!({
        "id": l.id, "kind": l.kind.as_key(), "x": l.x, "y": l.y,
        "claimed": l.claimed_by.is_some(),
    });
    match l.kind {
        LootKind::Item => {
            v["item_id"] = serde_json::Value::from(l.item_id.clone().unwrap_or_default());
            v["count"] = serde_json::Value::from(l.count);
        }
        LootKind::Gold => {
            v["gold"] = serde_json::Value::from(l.count);
        }
        LootKind::Chest => {}
    }
    v
}

/// Claim-Freigabe (Lootsystem.md §Claim): gegen einen Claim sind nur die
/// Eigentümer berechtigt. Truhen werden nach Ablauf der Claim-Frist
/// öffentlich; Items/Gold niemals automatisch.
pub fn can_take(l: &WorldLoot, actor: &str, groups: &GroupManager, now: Instant) -> bool {
    if l.claimed_by.is_none() {
        return true;
    }
    if l.kind == LootKind::Chest {
        if let Some(until) = l.chest_claim_until {
            if now >= until {
                return true;
            }
        }
    }
    let Some(claim) = &l.claimed_by else {
        return true;
    };
    if let Some(gid) = group::decode_group_claim(claim) {
        // Gruppenmitglied des Claim-Eigentümers.
        groups.group_of(actor) == Some(gid)
    } else {
        // Einzelspieler-Claim: nur der Eigentümer (nicht die Gruppe).
        claim == actor
    }
}

/// Empfänger einer Gold-Aufteilung: Gruppen-Claim → aktive Mitglieder,
/// Spieler-Claim → dieser, kein Claim → Aufnehmer (Fallback).
fn gold_recipients(
    world: &World,
    groups: &GroupManager,
    claim: &Option<String>,
    fallback: &str,
) -> Vec<String> {
    match claim {
        Some(c) if group::is_group_claim(c) => {
            if let Some(gid) = group::decode_group_claim(c) {
                let positions: std::collections::HashMap<String, (f64, f64)> = world
                    .players
                    .iter()
                    .map(|(id, p)| (id.clone(), (p.x, p.y)))
                    .collect();
                groups.active_members(gid, &positions, groups.cfg.range)
            } else {
                Vec::new()
            }
        }
        Some(pid) => vec![pid.clone()],
        None => vec![fallback.to_string()],
    }
}

/// Ergebnis eines Pickup-Versuchs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickupResult {
    /// Aufgenommen (Item ganz/teilweise, Gold verteilt, Truhe geöffnet).
    PickedUp,
    /// Kein Drop unter dieser ID (mehr).
    Missing,
    /// Spieler darf den Drop nicht aufnehmen (Claim).
    NotClaimed,
    /// Außerhalb der Pickup-Reichweite.
    OutOfRange,
}

/// Server-autoritativer Pickup. Item: zuerst Stacks, dann freie Slots
/// (try_add) — Restmenge bleibt als Drop liegen. Gold: wird an die aktiven
/// Gruppenmitglieder aufgeteilt und der Drop entfernt. Truhe: geöffnet,
/// der beim Spawn gerollte Inhalt erscheint als normaler Bodenloot.
pub fn attempt_pickup(
    world: &mut World,
    actor: &str,
    groups: &GroupManager,
    loot_id: &str,
    now: Instant,
    cfg: &LootCfg,
) -> PickupResult {
    let Some(l) = world.loot_drops.get(loot_id) else {
        return PickupResult::Missing;
    };
    let Some(p) = world.players.get(actor) else {
        return PickupResult::Missing;
    };
    if dist(p.x, p.y, l.x, l.y) > cfg.pickup_radius {
        return PickupResult::OutOfRange;
    }
    if !can_take(l, actor, groups, now) {
        return PickupResult::NotClaimed;
    }
    // Daten für die Mutationen klonen (Borrow des Drops separat halten).
    let kind = l.kind;
    let item_id = l.item_id.clone();
    let count = l.count;
    let claimed_by = l.claimed_by.clone();
    let content = l.content.clone();
    let x = l.x;
    let y = l.y;

    match kind {
        LootKind::Item => {
            let Some(def) = world
                .item_definitions
                .get(&item_id.clone().unwrap_or_default())
                .cloned()
            else {
                // Unbekannte Definition → Loot bleibt liegen.
                return PickupResult::PickedUp;
            };
            let outcome = {
                let p = world.players.get_mut(actor).expect("actor checked");
                let outcome = p.inventory.try_add(&def, count);
                if outcome.accepted > 0 {
                    p.mark_dirty(crate::persist::PersistComponent::Inventory);
                }
                outcome
            };
            let remaining = outcome.remainder.max(0);
            let drop = world.loot_drops.get_mut(loot_id);
            if remaining == 0 {
                world.loot_drops.remove(loot_id);
            } else if let Some(d) = drop {
                d.count = remaining;
            }
            PickupResult::PickedUp
        }
        LootKind::Gold => {
            let recipients = gold_recipients(world, groups, &claimed_by, actor);
            let amounts = group::split_exp_equally(count.max(0), recipients.len());
            for (pid, amt) in recipients.iter().zip(&amounts) {
                if let Some(p) = world.players.get_mut(pid) {
                    if *amt > 0 {
                        p.idia += *amt;
                        p.mark_dirty(crate::persist::PersistComponent::Idia);
                    }
                }
            }
            world.loot_drops.remove(loot_id);
            PickupResult::PickedUp
        }
        LootKind::Chest => {
            // Inhalt beim Spawn gerollt → als normaler Bodenloot erscheinen.
            for payload in content {
                match payload {
                    LootPayload::Item { item_id, count } => {
                        let _ = spawn_drop(
                            world,
                            LootKind::Item,
                            x,
                            y,
                            Some(item_id),
                            count,
                            Vec::new(),
                            claimed_by.clone(),
                            cfg,
                            now,
                        );
                    }
                    LootPayload::Gold { amount } => {
                        let _ = spawn_drop(
                            world,
                            LootKind::Gold,
                            x,
                            y,
                            None,
                            amount,
                            Vec::new(),
                            claimed_by.clone(),
                            cfg,
                            now,
                        );
                    }
                }
            }
            world.loot_drops.remove(loot_id);
            PickupResult::PickedUp
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::GroupCfg;
    use crate::item::{ItemCategory, ItemDefinition};
    use std::collections::VecDeque;
    use tokio::sync::mpsc;

    /// Scripted-RNG (Determinismus).
    struct ScriptedRng(VecDeque<f64>);
    impl ScriptedRng {
        fn from(v: &[f64]) -> Self {
            Self(v.iter().copied().collect())
        }
    }
    impl CombatRng for ScriptedRng {
        fn next(&mut self) -> f64 {
            self.0.pop_front().expect("scripted rng exhausted")
        }
    }

    fn player(id: &str, x: f64, y: f64) -> (crate::world::Player, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
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
                account_id: 0,
                session_id: String::new(),
                entities: Default::default(),
                last_activity: Instant::now(),
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
                cooldowns: std::collections::BTreeMap::new(),
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
                inventory: crate::inventory::InventoryState::new(2),
                quests: Default::default(),
                dirty: Default::default(),
                persist_generation: 0,
                persist_revision: 0,
            },
            rx,
        )
    }

    fn def(item_id: &str, max_stack: i64) -> ItemDefinition {
        let mut d = ItemDefinition::new(item_id, item_id, ItemCategory::RawMaterial);
        d.max_stack = max_stack;
        d
    }

    fn entry(kind: LootKind, item_id: Option<&str>, min: i64, max: i64, chance: f64) -> LootEntry {
        LootEntry {
            kind,
            item_id: item_id.map(String::from),
            min_quantity: min,
            max_quantity: max,
            chance,
            content_table_id: None,
        }
    }

    fn chest_entry(content_table_id: i64, chance: f64) -> LootEntry {
        LootEntry {
            kind: LootKind::Chest,
            item_id: None,
            min_quantity: 0,
            max_quantity: 0,
            chance,
            content_table_id: Some(content_table_id),
        }
    }

    fn table(id: i64, entries: Vec<LootEntry>) -> LootTable {
        LootTable {
            id,
            name: format!("t{id}"),
            entries,
        }
    }

    fn world_with(hide_stack: i64) -> World {
        let mut w = World::new();
        w.item_definitions
            .insert("wolf_hide".into(), def("wolf_hide", hide_stack));
        w.item_definitions
            .insert("wolf_meat".into(), def("wolf_meat", 50));
        w
    }

    fn drop(
        w: &mut World,
        kind: LootKind,
        item_id: Option<&str>,
        count: i64,
        claim: Option<&str>,
    ) -> String {
        spawn_drop(
            w,
            kind,
            0.0,
            0.0,
            item_id.map(String::from),
            count,
            Vec::new(),
            claim.map(String::from),
            &LootCfg::default(),
            Instant::now(),
        )
    }

    fn groups() -> GroupManager {
        GroupManager::new(GroupCfg::default())
    }

    fn rng(v: &[f64]) -> ScriptedRng {
        ScriptedRng::from(v)
    }

    /// Gruppe aus alice (Leiter) + Mitgliedern; gibt die Group-ID zurück.
    fn group_with(g: &mut GroupManager, leader: &str, members: &[&str]) -> u64 {
        let now = Instant::now();
        let gid = g.create_group(leader, now).unwrap();
        for m in members {
            g.invite(gid, leader, m, now).unwrap();
            g.accept_invite(gid, m, now).unwrap();
        }
        gid
    }

    // ── Würfeln ──────────────────────────────────────────────────────

    #[test]
    fn npc_without_loot_table_drops_nothing() {
        let mut w = world_with(100);
        let ids_before = w.loot_drops.len();
        let n = spawn_npc_loot(
            &mut w,
            None,
            &None,
            0.0,
            0.0,
            &LootCfg::default(),
            Instant::now(),
            &mut rng(&[0.99]),
        );
        assert_eq!(n, 0);
        assert_eq!(w.loot_drops.len(), ids_before);
    }

    #[test]
    fn entries_roll_independently() {
        let mut w = world_with(100);
        w.loot_tables.insert(
            1,
            table(
                1,
                vec![
                    entry(LootKind::Item, Some("wolf_hide"), 1, 1, 0.5),
                    entry(LootKind::Gold, None, 10, 10, 0.5),
                ],
            ),
        );
        // Je Eintrag: Chance-Wurf, dann (nur bei Item/Gold mit min<max) Menge.
        let n = spawn_npc_loot(
            &mut w,
            Some(1),
            &None,
            0.0,
            0.0,
            &LootCfg::default(),
            Instant::now(),
            &mut rng(&[0.1, 0.2]),
        );
        assert_eq!(n, 2, "beide Einträge treffen unabhängig");
        let kinds: Vec<LootKind> = w.loot_drops.values().map(|l| l.kind).collect();
        assert!(kinds.contains(&LootKind::Item));
        assert!(kinds.contains(&LootKind::Gold));
        let item = w
            .loot_drops
            .values()
            .find(|l| l.kind == LootKind::Item)
            .unwrap();
        assert_eq!(item.count, 1);
        let gold = w
            .loot_drops
            .values()
            .find(|l| l.kind == LootKind::Gold)
            .unwrap();
        assert_eq!(gold.count, 10);
    }

    #[test]
    fn loot_id_is_distinct_from_item_id() {
        let mut w = world_with(100);
        w.loot_tables.insert(
            1,
            table(1, vec![entry(LootKind::Item, Some("wolf_hide"), 1, 1, 1.0)]),
        );
        spawn_npc_loot(
            &mut w,
            Some(1),
            &None,
            0.0,
            0.0,
            &LootCfg::default(),
            Instant::now(),
            &mut rng(&[0.5]),
        );
        let l = w.loot_drops.values().next().expect("ein Drop");
        assert!(l.id.starts_with("loot_"));
        assert_ne!(l.id, "wolf_hide");
        assert_eq!(l.item_id.as_deref(), Some("wolf_hide"));
    }

    // ── Pickup: Gold ─────────────────────────────────────────────────

    #[test]
    fn solo_player_picks_up_gold() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        let id = drop(&mut w, LootKind::Gold, None, 50, Some("alice"));
        let r = attempt_pickup(
            &mut w,
            "alice",
            &groups(),
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r, PickupResult::PickedUp);
        assert_eq!(w.players["alice"].idia, 50);
        assert!(!w.loot_drops.contains_key(&id), "Gold-Drop entfernt");
    }

    #[test]
    fn group_members_split_gold() {
        let mut w = world_with(100);
        for (id, x) in [("alice", 0.0), ("bob", 10.0)] {
            let (p, _rx) = player(id, x, 0.0);
            w.players.insert(id.into(), p);
        }
        let mut g = groups();
        let gid = group_with(&mut g, "alice", &["bob"]);

        let id = drop(
            &mut w,
            LootKind::Gold,
            None,
            100,
            Some(&crate::group::encode_group_claim(gid)),
        );
        let r = attempt_pickup(
            &mut w,
            "alice",
            &g,
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r, PickupResult::PickedUp);
        assert_eq!(w.players["alice"].idia, 50);
        assert_eq!(w.players["bob"].idia, 50);
        assert!(!w.loot_drops.contains_key(&id));
    }

    #[test]
    fn remainder_gold_goes_to_first_recipients_in_stable_order() {
        let mut w = world_with(100);
        for id in ["alice", "bob", "carol"] {
            let (p, _rx) = player(id, 0.0, 0.0);
            w.players.insert(id.into(), p);
        }
        let mut g = groups();
        let gid = group_with(&mut g, "alice", &["bob", "carol"]);

        let now = Instant::now();
        let id = drop(
            &mut w,
            LootKind::Gold,
            None,
            10,
            Some(&crate::group::encode_group_claim(gid)),
        );
        assert_eq!(
            attempt_pickup(&mut w, "bob", &g, &id, now, &LootCfg::default()),
            PickupResult::PickedUp
        );
        // 10 über 3 Empfänger → [4,3,3] in stabiler (BTreeMap-)Reihenfolge.
        assert_eq!(w.players["alice"].idia, 4);
        assert_eq!(w.players["bob"].idia, 3);
        assert_eq!(w.players["carol"].idia, 3);
    }

    // ── Pickup: Claim ────────────────────────────────────────────────

    #[test]
    fn unclaimed_loot_cannot_be_taken_by_others() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        let (q, _rx2) = player("mallory", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        w.players.insert("mallory".into(), q);
        let id = drop(&mut w, LootKind::Gold, None, 50, Some("alice"));
        let r = attempt_pickup(
            &mut w,
            "mallory",
            &groups(),
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r, PickupResult::NotClaimed);
        assert_eq!(w.players["mallory"].idia, 0);
        assert!(w.loot_drops.contains_key(&id), "Loot bleibt liegen");
    }

    #[test]
    fn group_member_can_take_group_claimed_loot() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        let (q, _rx2) = player("bob", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        w.players.insert("bob".into(), q);
        let mut g = groups();
        let gid = group_with(&mut g, "alice", &["bob"]);
        // Claim einer Gruppe → Mitglied darf einnehmen (FFA-V1).
        let id = drop(
            &mut w,
            LootKind::Item,
            Some("wolf_hide"),
            5,
            Some(&crate::group::encode_group_claim(gid)),
        );
        let r = attempt_pickup(&mut w, "bob", &g, &id, Instant::now(), &LootCfg::default());
        assert_eq!(r, PickupResult::PickedUp);
        assert_eq!(w.players["bob"].inventory.count_of("wolf_hide"), 5);
    }

    #[test]
    fn group_dissolution_transfers_loot_claim() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        let (q, _rx2) = player("bob", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        w.players.insert("bob".into(), q);
        let mut g = groups();
        let gid = group_with(&mut g, "alice", &["bob"]);
        let id = drop(
            &mut w,
            LootKind::Item,
            Some("wolf_hide"),
            1,
            Some(&crate::group::encode_group_claim(gid)),
        );
        // bob (Mitglied) verlässt die 2er-Gruppe → Auflösung.
        // Letztes verbleibendes Mitglied ist alice.
        g.leave(gid, "bob", Instant::now()).unwrap();
        // main.rs-Loop: Claim "g:<gid>" geht aufs letzte Mitglied über.
        for (dgid, last_pid) in g.take_dissolutions() {
            let group_claim = format!("g:{dgid}");
            for l in w.loot_drops.values_mut() {
                if l.claimed_by.as_deref() == Some(group_claim.as_str()) {
                    // Kein Mitglied übrig → Claim entfällt ersatzlos.
                    l.claimed_by = if last_pid.is_empty() {
                        None
                    } else {
                        Some(last_pid.clone())
                    };
                }
            }
        }
        assert_eq!(w.loot_drops[&id].claimed_by.as_deref(), Some("alice"));
        // Der überlebende Claimer kann den Loot weiterhin aufnehmen.
        assert_eq!(
            attempt_pickup(
                &mut w,
                "alice",
                &g,
                &id,
                Instant::now(),
                &LootCfg::default()
            ),
            PickupResult::PickedUp
        );
    }

    // ── Pickup: Item / Inventar ──────────────────────────────────────

    #[test]
    fn full_inventory_leaves_loot_on_ground() {
        let mut w = world_with(1); // wolf_hide: max_stack 1
        let (mut p, _rx) = player("alice", 0.0, 0.0); // 2 Basis-Slots
        let _ = p.inventory.try_add(&def("wolf_hide", 1), 2); // beide voll
        w.players.insert("alice".into(), p);
        let id = drop(&mut w, LootKind::Item, Some("wolf_hide"), 3, Some("alice"));
        let r = attempt_pickup(
            &mut w,
            "alice",
            &groups(),
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r, PickupResult::PickedUp);
        assert_eq!(w.players["alice"].inventory.count_of("wolf_hide"), 2);
        assert!(w.loot_drops.contains_key(&id), "nichts passte, Drop bleibt");
        assert_eq!(w.loot_drops[&id].count, 3, "Menge unverändert");
    }

    #[test]
    fn partial_pickup_keeps_remainder_in_world() {
        let mut w = world_with(5); // max_stack 5
        let (mut p, _rx) = player("alice", 0.0, 0.0); // 2 Basis-Slots
        let _ = p.inventory.try_add(&def("wolf_hide", 5), 3); // 3/5 im Stack
        w.players.insert("alice".into(), p);
        let id = drop(&mut w, LootKind::Item, Some("wolf_hide"), 10, Some("alice"));
        let r = attempt_pickup(
            &mut w,
            "alice",
            &groups(),
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r, PickupResult::PickedUp);
        // slot0 wird auf 5/5 gefüllt, slot1 nimmt ein weiteres 5er-Stack →
        // 7 aufgenommen, 3 bleiben am Boden.
        assert_eq!(w.players["alice"].inventory.count_of("wolf_hide"), 10);
        assert!(w.loot_drops.contains_key(&id));
        assert_eq!(w.loot_drops[&id].count, 3, "Rest liegt am Boden");
        // Zweiter Versuch ohne freien Slot nimmt nichts weiter auf.
        let r2 = attempt_pickup(
            &mut w,
            "alice",
            &groups(),
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r2, PickupResult::PickedUp);
        assert_eq!(w.players["alice"].inventory.count_of("wolf_hide"), 10);
        assert_eq!(w.loot_drops[&id].count, 3, "Rest bleibt");
    }

    #[test]
    fn gold_pickup_marks_gold_dirty() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        let id = drop(&mut w, LootKind::Gold, None, 50, Some("alice"));
        let r = attempt_pickup(
            &mut w,
            "alice",
            &groups(),
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r, PickupResult::PickedUp);
        let p = &w.players["alice"];
        assert_eq!(p.idia, 50);
        assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Idia));
        assert_eq!(p.persist_generation, 1);
    }

    #[test]
    fn item_pickup_marks_inventory_dirty_when_items_are_accepted() {
        let mut w = world_with(50);
        let (p, _rx) = player("alice", 0.0, 0.0); // 2 Basis-Slots
        w.players.insert("alice".into(), p);
        let id = drop(&mut w, LootKind::Item, Some("wolf_hide"), 3, Some("alice"));
        let r = attempt_pickup(
            &mut w,
            "alice",
            &groups(),
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r, PickupResult::PickedUp);
        let p = &w.players["alice"];
        assert_eq!(p.inventory.count_of("wolf_hide"), 3);
        assert!(p
            .dirty
            .is_dirty(crate::persist::PersistComponent::Inventory));
    }

    #[test]
    fn item_pickup_without_accepted_items_does_not_mark_dirty() {
        // Voller Inventar → nichts aufgenommen → keine RAM-Mutation → kein
        // Inventory-Dirty (der periodische Flush würde nichts Neues schreiben).
        let mut w = world_with(1);
        let (mut p, _rx) = player("alice", 0.0, 0.0);
        let _ = p.inventory.try_add(&def("wolf_hide", 1), 2); // beide Slots voll
        w.players.insert("alice".into(), p);
        let id = drop(&mut w, LootKind::Item, Some("wolf_hide"), 3, Some("alice"));
        let _ = attempt_pickup(
            &mut w,
            "alice",
            &groups(),
            &id,
            Instant::now(),
            &LootCfg::default(),
        );
        let p = &w.players["alice"];
        assert!(!p.dirty.any());
        assert_eq!(p.persist_generation, 0);
    }

    // ── Truhen ───────────────────────────────────────────────────────

    #[test]
    fn chest_content_rolled_once_at_spawn() {
        let mut w = world_with(100);
        w.loot_tables.insert(
            1,
            table(1, vec![entry(LootKind::Item, Some("wolf_hide"), 2, 2, 1.0)]),
        );
        let id = spawn_drop(
            &mut w,
            LootKind::Chest,
            0.0,
            0.0,
            None,
            0,
            vec![
                LootPayload::Item {
                    item_id: "wolf_hide".into(),
                    count: 2,
                },
                LootPayload::Gold { amount: 30 },
            ],
            Some("alice".into()),
            &LootCfg::default(),
            Instant::now(),
        );
        assert_eq!(w.loot_drops[&id].kind, LootKind::Chest);
        assert_eq!(
            w.loot_drops[&id].content.len(),
            2,
            "Inhalt beim Spawn fixiert"
        );
    }

    #[test]
    fn opening_chest_does_not_re_roll() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        let id = drop(&mut w, LootKind::Chest, None, 0, Some("alice"));
        // Inhalt beim Spawn fixieren (simuliert gewürfelten Inhalt).
        let cid = w.loot_drops[&id].id.clone();
        w.loot_drops.get_mut(&cid).unwrap().content = vec![LootPayload::Item {
            item_id: "wolf_hide".into(),
            count: 2,
        }];
        let r = attempt_pickup(
            &mut w,
            "alice",
            &groups(),
            &cid,
            Instant::now(),
            &LootCfg::default(),
        );
        assert_eq!(r, PickupResult::PickedUp);
        // Der gerollte Inhalt erscheint als Bodenloot (kein Inventory-Insert).
        assert_eq!(w.players["alice"].inventory.count_of("wolf_hide"), 0);
        let new_item = w.loot_drops.values().find(|l| l.kind == LootKind::Item);
        assert!(new_item.is_some(), "Inhalt wird normaler Bodenloot");
        assert!(!w.loot_drops.contains_key(&cid), "Truhe weg");
    }

    #[test]
    fn chest_becomes_public_after_claim_window() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        let (q, _rx2) = player("mallory", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        w.players.insert("mallory".into(), q);
        let cfg = LootCfg {
            chest_claim_ms: 1000,
            ..LootCfg::default()
        };
        let now = Instant::now();
        let id = spawn_drop(
            &mut w,
            LootKind::Chest,
            0.0,
            0.0,
            None,
            0,
            vec![LootPayload::Gold { amount: 10 }],
            Some("alice".into()),
            &cfg,
            now,
        );
        // Vor Ablauf: Claim blockiert den Fremden.
        assert_eq!(
            attempt_pickup(&mut w, "mallory", &groups(), &id, now, &cfg),
            PickupResult::NotClaimed
        );
        // Nach Ablauf: öffentlich.
        let later = now + std::time::Duration::from_millis(1_500);
        assert_eq!(
            attempt_pickup(&mut w, "mallory", &groups(), &id, later, &cfg),
            PickupResult::PickedUp
        );
        assert!(!w.loot_drops.contains_key(&id));
    }

    #[test]
    fn item_loot_never_becomes_public() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        let (q, _rx2) = player("mallory", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        w.players.insert("mallory".into(), q);
        let cfg = LootCfg {
            chest_claim_ms: 1000,
            ..LootCfg::default()
        };
        let now = Instant::now();
        let id = drop_with(
            &mut w,
            LootKind::Item,
            "wolf_hide",
            1,
            Some("alice"),
            &cfg,
            now,
        );
        let later = now + std::time::Duration::from_millis(10_000);
        assert_eq!(
            attempt_pickup(&mut w, "mallory", &groups(), &id, later, &cfg),
            PickupResult::NotClaimed
        );
        assert!(w.loot_drops.contains_key(&id), "Item bleibt geclaimed");
    }

    #[test]
    fn gold_loot_never_becomes_public() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        let (q, _rx2) = player("mallory", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        w.players.insert("mallory".into(), q);
        let cfg = LootCfg {
            chest_claim_ms: 1000,
            ..LootCfg::default()
        };
        let now = Instant::now();
        let id = drop_with(&mut w, LootKind::Gold, "", 10, Some("alice"), &cfg, now);
        let later = now + std::time::Duration::from_millis(10_000);
        assert_eq!(
            attempt_pickup(&mut w, "mallory", &groups(), &id, later, &cfg),
            PickupResult::NotClaimed
        );
        assert!(w.loot_drops.contains_key(&id));
    }

    #[test]
    fn empty_chest_never_spawns() {
        let mut w = world_with(100);
        // Inhaltstabelle mit zwei sicher verfehlten Einträgen.
        w.loot_tables.insert(
            1,
            table(
                1,
                vec![
                    entry(LootKind::Item, Some("wolf_hide"), 1, 1, 0.0),
                    entry(LootKind::Gold, None, 5, 5, 0.0),
                ],
            ),
        );
        // Chest-Chance 100 %, aber gerollter Inhalt ist leer → kein Drop.
        w.loot_tables.insert(2, table(2, vec![chest_entry(1, 1.0)]));
        let n = spawn_npc_loot(
            &mut w,
            Some(2),
            &Some("alice".into()),
            0.0,
            0.0,
            &LootCfg::default(),
            Instant::now(),
            &mut rng(&[0.5, 0.9, 0.9]),
        );
        assert_eq!(n, 0, "leere Truhe erscheint nie");
        assert!(w.loot_drops.is_empty());
    }

    // ── Despawn ──────────────────────────────────────────────────────

    #[test]
    fn expiring_loot_despawns_by_cfg() {
        let mut w = world_with(100);
        let (p, _rx) = player("alice", 0.0, 0.0);
        w.players.insert("alice".into(), p);
        let now = Instant::now();
        let cfg = LootCfg {
            despawn_ms: 500,
            ..LootCfg::default()
        };
        let id = drop_with(&mut w, LootKind::Gold, "", 10, Some("alice"), &cfg, now);
        assert!(w.loot_drops.contains_key(&id));
        loot_tick(&mut w, &cfg, now + std::time::Duration::from_millis(600));
        assert!(
            !w.loot_drops.contains_key(&id),
            "abgelaufener Drop entfernt"
        );
    }

    // ── Hilfsfunktionen ──────────────────────────────────────────────

    fn drop_with(
        w: &mut World,
        kind: LootKind,
        item_id: &str,
        count: i64,
        claim: Option<&str>,
        cfg: &LootCfg,
        now: Instant,
    ) -> String {
        spawn_drop(
            w,
            kind,
            0.0,
            0.0,
            match kind {
                LootKind::Item => Some(item_id.to_string()),
                _ => None,
            },
            count,
            Vec::new(),
            claim.map(String::from),
            cfg,
            now,
        )
    }
}
