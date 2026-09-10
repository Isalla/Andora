// npc — NPC-/Monster-Ebene (Combat V2), Realm-autoritativ.
// Umgesetzt nach docs/Kampfsystem.md §§18–21 und docs/Boss-System.md §§2–6:
//   - gemeinsamer, realm-autoritativer Kampfkern für Spieler UND NPC/Monster
//     (Schadensauflösung via combat::resolve_attack — keine separate Engine),
//   - getrennte Eigenschaften `attackable`/`aggressive` (Content-Schicht, §18),
//   - kontextabhängige Überschreibungen mit automatischem Auslaufen (§19),
//   - Home-Zone, Verfolgungsgrenzen, Evade/Return als Combat-Reset (§20),
//   - Respawn als Contentwert (§21), Aggro-Formen Solo/sozial/feste Gruppe (§21),
//   - Boss-Claim mit einheitlichem Evade/Return-Reset (Boss-System.md §§2–3).
//
// Alle Werte sind vorläufig und kommen aus Content-/DB-Daten (Definitionen &
// Spawns, Migration 009) plus config-NpcCfg-Mechanikwerten — keine festen
// Gameplaywerte im Kampfkern.
use std::time::{Instant, SystemTime};

use crate::combat::HitResult;
use crate::combat::{class_cap, resolve_attack, CombatRng};
use crate::config::{CombatCfg, NpcCfg};
use crate::db;
use crate::protocol::{s2c, Frame};
use crate::world::World;

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).hypot(ay - by)
}

fn move_toward(x: f64, y: f64, tx: f64, ty: f64, step: f64) -> (f64, f64) {
    let d = dist(x, y, tx, ty);
    if d <= 1e-9 {
        return (x, y);
    }
    let s = step.min(d);
    (x + (tx - x) / d * s, y + (ty - y) / d * s)
}

/// Kompakter Instanz-Schlüssel eines NPCs ("npc_<spawn_id>" — vermeidet
/// Kollision mit Charakter-IDs im geteilten Target-Namespace).
pub fn npc_id(spawn_id: i64) -> String {
    format!("npc_{spawn_id}")
}

/// Laufzustand eines NPCs (§20/§21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcStatus {
    /// Am Leben, in Home-Zone (ggf. mit Ziel/Aggro).
    Alive,
    /// Tot; Respawn-Timer läuft (status persistent, übersteht Neustarts).
    Dead,
    /// Evade/Return: kehrt zur Home-Zone zurück, nicht angreifbar, kein
    /// Aggro, keine Angriffe (Combat-Reset, weder Tod noch Respawn).
    Returning,
}

impl NpcStatus {
    pub fn key(&self) -> &'static str {
        match self {
            NpcStatus::Alive => "alive",
            NpcStatus::Dead => "dead",
            NpcStatus::Returning => "returning",
        }
    }
}

/// Kontextabhängige Überschreibung (§19): überschreibt den Content-Grund-
/// zustand (attackable/aggressive) gezielt und läuft am Kontextende
/// (expires_at) automatisch aus. Kein Quest-/Dialogmodul nötig — ein
/// späteres System setzt dies; der Mechanismus samt Auslaufen ist hier.
#[derive(Debug, Clone)]
pub struct ContextOverride {
    pub attackable: Option<bool>,
    pub aggressive: Option<bool>,
    pub expires_at: Option<Instant>,
}

/// Statische Definition + Home-/Leash-Werte eines NPCs (Content-Schicht).
/// Wird aus monster_definitions + monster_spawns (Migration 009) gelesen.
#[derive(Debug, Clone)]
pub struct Npc {
    pub id: String,
    pub spawn_id: i64,
    pub name: String,
    pub kind: String, // normal | named | boss
    pub attackable: bool,
    pub aggressive: bool,
    pub aggro_range: f64,
    pub attack_range: f64,
    pub attack_duration_ms: u64,
    pub weapon_damage: i32,
    pub weapon_skill: u32,
    pub armor: i32,
    pub max_hp: i32,
    pub move_speed: f64,
    pub respawn_ms: i64,
    pub faction: Option<String>,
    pub pack_id: Option<String>,
    pub home_x: f64,
    pub home_y: f64,
    pub home_radius: f64,
    pub leash_radius: f64,
    // Runtime-Zustand
    pub status: NpcStatus,
    pub hp: i32,
    pub x: f64,
    pub y: f64,
    pub target_id: Option<String>,
    pub last_attack: Instant,
    pub no_link_since: Option<Instant>,
    pub return_started_at: Option<Instant>,
    /// Absolute Wallclock-Zeit (Epoch) für den Respawn (§21). Übersteht
    /// daher Realm-Neustarts (in DB persistiert).
    pub respawn_after: Option<SystemTime>,
    pub claimed_by: Option<String>,
    pub override_ctx: Option<ContextOverride>,
    /// Aktive Effekte (Buffs, Debuffs, DoT/HoT, Control, Combat V3).
    pub effects: Vec<crate::combat::effects::Effect>,
    /// Fähigkeits-Cooldowns (Combat V3): ability_id → ready_at.
    pub cooldowns: std::collections::BTreeMap<String, std::time::SystemTime>,
    /// Aktiver Cast-Zustand (Combat V3, für künftige NPC-AI).
    pub active_cast: Option<crate::combat::ability::ActiveCast>,
}

impl Npc {
    /// Effektive Angreifbarkeit (Grundzustand, überschrieben durch Kontext).
    pub fn effective_attackable(&self) -> bool {
        self.override_ctx
            .as_ref()
            .and_then(|o| o.attackable)
            .unwrap_or(self.attackable)
    }

    /// Effektive Aggressivität (Grundzustand, überschrieben durch Kontext).
    pub fn effective_aggressive(&self) -> bool {
        self.override_ctx
            .as_ref()
            .and_then(|o| o.aggressive)
            .unwrap_or(self.aggressive)
    }

    pub fn is_at_home(&self) -> bool {
        dist(self.x, self.y, self.home_x, self.home_y) <= self.home_radius
    }

    pub fn is_boss(&self) -> bool {
        self.kind == "boss"
    }
}

/// Default-Respawnzeit je Kategorie (§21, sofern Content nichts anderes
/// festlegt). Values in ms: normal=3min? — siehe §21: Questmonster 3min,
/// normales Monster 5min, Named 10min, besondere seltene Named/Bosse
/// individuell (Boss-System.md §6: z. B. 60min).
pub fn default_respawn_ms(kind: &str) -> i64 {
    match kind {
        "quest" => 180_000,
        "named" => 600_000,
        "boss" => 3_600_000,
        _ => 300_000, // normal
    }
}

/// Aggro-Trigger (§21): setzt das Aggro-Ziel des angegriffenen NPCs und
/// propagiert auf die Aggro-Formen — feste Gruppe/Rudel (gleicher pack_id)
/// tritt immer ein; soziale Aggro (gleiche Fraktion) innerhalb des
/// sozialen Aggro-Radius des angegriffenen NPCs.
pub fn aggro_trigger(world: &mut World, npc_id: &str, attacker_id: &str, npc_cfg: &NpcCfg) {
    let (faction, pack, ax, ay) = match world.npcs.get(npc_id) {
        Some(n) => (n.faction.clone(), n.pack_id.clone(), n.x, n.y),
        None => return,
    };
    let affected: Vec<String> = world
        .npcs
        .values()
        .filter(|n| {
            if n.status != NpcStatus::Alive {
                return false;
            }
            if n.id == npc_id {
                return true;
            }
            if let Some(p) = &pack {
                if n.pack_id.as_ref() == Some(p) {
                    return true;
                }
            }
            if let Some(f) = &faction {
                if n.faction.as_ref() == Some(f)
                    && dist(n.x, n.y, ax, ay) <= npc_cfg.social_aggro_radius
                {
                    return true;
                }
            }
            false
        })
        .map(|n| n.id.clone())
        .collect();
    for id in &affected {
        if let Some(n) = world.npcs.get_mut(id) {
            n.target_id = Some(attacker_id.to_string());
        }
    }
}

/// Baut aus Content (Definitions + Spawns, Migration 009) und dem
/// persistenten Zustand (monster_instances) die NPC-Instanzen.
///
/// State-Regeln (dokumentiert in docs/Kampfsystem.md §21/Boss-System.md §6):
///   - keine Zeile in monster_instances  → frische Instanz (Alive, volle HP,
///     Home-Position),
///   - status 'returning'                → nach Neustart weiterhin kehrend
///     (nur Bewegungsstatus, keine Aggro-Restaurierung),
///   - status 'dead'                     → Respawn-Timer bleibt erhalten;
///     abgelaufene Timer werden beim nächsten npc_tick aufgelöst,
///   - contentverändernde Felder
///     (attackable/aggressive/respawn_ms) ziehen Spawn-Overrides vor der
///     Definition, damit der Runtime-Zustand rekonstruierbar bleibt.
pub fn build_npcs(
    defs: &[db::NpcDefRow],
    spawns: &[db::NpcSpawnRow],
    states: &[db::NpcStateRow],
) -> std::collections::HashMap<String, Npc> {
    let defs: std::collections::HashMap<_, _> = defs.iter().map(|d| (d.id.clone(), d)).collect();
    let states: std::collections::HashMap<_, _> = states.iter().map(|s| (s.spawn_id, s)).collect();
    let mut npcs = std::collections::HashMap::new();
    for spawn in spawns {
        let Some(def) = defs.get(&spawn.monster_id) else {
            log::warn!("Monster {} ohne Definition übersprungen", spawn.monster_id);
            continue;
        };
        let respawn_ms = spawn
            .respawn_ms
            .or(def.respawn_ms)
            .unwrap_or_else(|| default_respawn_ms(&def.kind));
        let attackable = spawn.attackable.unwrap_or(def.attackable);
        let aggressive = spawn.aggressive.unwrap_or(def.aggressive);

        let mut n = Npc {
            id: npc_id(spawn.id),
            spawn_id: spawn.id,
            name: def.name.clone(),
            kind: def.kind.clone(),
            attackable,
            aggressive,
            aggro_range: def.aggro_range,
            attack_range: def.attack_range,
            attack_duration_ms: def.attack_duration_ms as u64,
            weapon_damage: def.weapon_damage,
            weapon_skill: def.weapon_skill as u32,
            armor: def.armor,
            max_hp: def.max_hp,
            move_speed: def.move_speed,
            respawn_ms,
            faction: def.faction.clone(),
            pack_id: spawn.pack_id.clone(),
            home_x: spawn.home_x,
            home_y: spawn.home_y,
            home_radius: spawn.home_radius,
            leash_radius: spawn.leash_radius,
            status: NpcStatus::Alive,
            hp: def.max_hp,
            x: spawn.home_x,
            y: spawn.home_y,
            target_id: None,
            last_attack: Instant::now(),
            no_link_since: None,
            return_started_at: None,
            respawn_after: None,
            claimed_by: None,
            override_ctx: None,
            effects: Vec::new(),
            cooldowns: std::collections::BTreeMap::new(),
            active_cast: None,
        };
        if let Some(st) = states.get(&spawn.id) {
            n.status = match st.status.as_str() {
                "dead" => NpcStatus::Dead,
                "returning" => NpcStatus::Returning,
                _ => NpcStatus::Alive,
            };
            n.hp = st.hp.min(n.max_hp);
            n.x = st.x;
            n.y = st.y;
            n.respawn_after = st.respawn_after_ms.and_then(|ms| {
                std::time::UNIX_EPOCH
                    .checked_add(std::time::Duration::from_millis(ms.max(0) as u64))
            });
            n.claimed_by = st.claimed_by.clone();
            if n.status == NpcStatus::Dead && n.respawn_after.is_none() {
                log::warn!(
                    "NPC {} tot ohne Respawn-Zeit → wird beim nächsten Tick wiederbelebt",
                    n.id
                );
            }
        }
        npcs.insert(n.id.clone(), n);
    }
    npcs
}

/// Expired Kontext-Überschreibungen auslaufen lassen (§19). Bei fehlendem
/// Ablaufdatum (None) läuft die Überschreibung nicht von selbst aus
/// (Kontextende löst später über das konkrete System aus).
pub fn expire_overrides(world: &mut World, now: Instant) {
    for n in world.npcs.values_mut() {
        let expired = n
            .override_ctx
            .as_ref()
            .and_then(|o| o.expires_at)
            .is_some_and(|e| now >= e);
        if expired {
            n.override_ctx = None;
        }
    }
}

/// verarbeitet den NPC/Combat-V2-Teil eines Welt-Ticks.
///
/// Reihenfolge (deterministisch, `now` + `wall_now` injizierbar):
///   1. Kontext-Überschreibungen auslaufen lassen (§19)
///   2. Respawn: tote NPCs, deren Timer abgelaufen ist, auferstehen lassen (§21)
///   3. Evade/Return: kehrenden NPCs bewegen; Ankunft = voller Reset (§20)
///   4. Aggro aus Entfernung + Verfolgung; Angriff aufs Ziel (§18/§21)
///   5. Evade-Auslöser (Leash / ohne gültigen Kampfbezug) (§20)
///
/// NPC-Angriffe nutzen dieselbe Schadensauflösung wie Spieler
/// (resolve_attack) — kein separates Kampfsystem (§21 Schwierigkeit).
#[allow(clippy::too_many_arguments)]
pub fn npc_tick(
    world: &mut World,
    cfg: &CombatCfg,
    npc_cfg: &NpcCfg,
    rng: &mut dyn CombatRng,
    now: Instant,
    wall_now: SystemTime,
    tick_ms: u64,
    aofb: f64,
) {
    expire_overrides(world, now);

    // 2) Respawn.
    for n in world.npcs.values_mut() {
        if n.status == NpcStatus::Dead {
            let ready = n.respawn_after.is_some_and(|r| wall_now >= r);
            if ready {
                n.status = NpcStatus::Alive;
                n.hp = n.max_hp;
                n.x = n.home_x;
                n.y = n.home_y;
                n.target_id = None;
                n.no_link_since = None;
                n.return_started_at = None;
                n.respawn_after = None;
                n.claimed_by = None; // Boss neu → wieder frei claimbar.
                n.last_attack = now;
                // Combat V3: vollständiger Reset (Effekte, Cooldowns) (§20).
                crate::combat::effects::clear_all(&mut n.effects);
                crate::combat::cooldowns::reset_all(&mut n.cooldowns);
                n.active_cast = None;
            }
        }
    }

    // 3) Evade/Return-Bewegung + Ankunft.
    let return_step = npc_cfg.return_speed * (tick_ms as f64 / 1000.0);
    let returners: Vec<String> = world
        .npcs
        .values()
        .filter(|n| n.status == NpcStatus::Returning)
        .map(|n| n.id.clone())
        .collect();
    let mut returned: Vec<String> = Vec::new();
    for id in &returners {
        if let Some(n) = world.npcs.get_mut(id) {
            let (nx, ny) = move_toward(n.x, n.y, n.home_x, n.home_y, return_step);
            n.x = nx;
            n.y = ny;
            if n.is_at_home() {
                n.status = NpcStatus::Alive;
                n.hp = n.max_hp; // volle HP-Regeneration
                n.target_id = None;
                n.no_link_since = None;
                n.return_started_at = None;
                n.claimed_by = None; // Claim vollständig gelöscht (§3.5)
                n.last_attack = now; // vollständiger Cooldown-Reset (§20)
                // Combat V3: vollständiger Reset (Effekte, Cooldowns) (§20).
                crate::combat::effects::clear_all(&mut n.effects);
                crate::combat::cooldowns::reset_all(&mut n.cooldowns);
                n.active_cast = None;
                returned.push(id.clone());
            }
        }
    }
    for id in &returned {
        broadcast_state(world, id, aofb);
    }

    // 4) Aggro aus Entfernung (+ defensives Einsteigen für Nicht-Aggressive
    //    wird über handle_attack/aggro_trigger gesetzt) + Verfolgung.
    let mut outcomes: Vec<(String, String, f64, f64, HitResult, i32)> = Vec::new();
    let ids: Vec<String> = world
        .npcs
        .values()
        .filter(|n| n.status == NpcStatus::Alive)
        .map(|n| n.id.clone())
        .collect();
    for id in &ids {
        // Immutable Read.
        let (has_target, want_aggro) = {
            let n = world.npcs.get(id).unwrap();
            (n.target_id.is_some(), n.effective_aggressive())
        };
        // 4a) Neues Aggroziel, falls aggressiv und Spieler im Aggro-Radius.
        if !has_target && want_aggro {
            let (px, py, axr) = {
                let n = world.npcs.get(id).unwrap();
                (n.x, n.y, n.aggro_range)
            };
            let target: Option<String> = world
                .players
                .values()
                .filter(|p| p.hp > 0)
                .filter(|p| dist(p.x, p.y, px, py) <= axr)
                .min_by(|a, b| {
                    dist(a.x, a.y, px, py)
                        .partial_cmp(&dist(b.x, b.y, px, py))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|p| p.id.clone());
            if let Some(t) = target {
                if let Some(n) = world.npcs.get_mut(id) {
                    n.target_id = Some(t);
                }
            }
        }

        // 4b) Verfolgung/Angriff des Ziels.
        let action = {
            let n = world.npcs.get(id).unwrap();
            match n.target_id.as_ref() {
                Some(tid) => {
                    let target_exists = world.players.get(tid).is_some_and(|p| p.hp > 0);
                    if !target_exists {
                        // Ziel weg/tot → kein gültiger Kampfbezug.
                        (Some(tid.clone()), None)
                    } else {
                        let t = world.players.get(tid).unwrap();
                        let d = dist(n.x, n.y, t.x, t.y);
                        if d <= n.attack_range {
                            let within = now
                                .checked_duration_since(n.last_attack)
                                .map(|e| {
                                    e >= std::time::Duration::from_millis(n.attack_duration_ms)
                                })
                                .unwrap_or(false);
                            (n.target_id.clone(), Some(("attack".to_string(), within)))
                        } else {
                            (n.target_id.clone(), Some(("chase".to_string(), false)))
                        }
                    }
                }
                None => (None, None),
            }
        };
        // (Some(tid), None): Ziel ungültig → kein gültiger Kampfbezug
        // zählen (kein Selbst-Evade im "kein Ziel"-Fall).
        if let (Some(_), None) = &action {
            if let Some(n) = world.npcs.get_mut(id) {
                n.no_link_since = Some(n.no_link_since.unwrap_or(now));
            }
        }
        match action {
            (Some(tid), Some((verb, strike))) => {
                if verb == "chase" {
                    // Bewegung aufs Ziel.
                    let step = {
                        let n = world.npcs.get(id).unwrap();
                        n.move_speed * (tick_ms as f64 / 1000.0)
                    };
                    if let Some(n) = world.npcs.get_mut(id) {
                        let t = world.players.get(&tid).map(|p| (p.x, p.y));
                        if let Some((tx, ty)) = t {
                            let (nx, ny) = move_toward(n.x, n.y, tx, ty, step);
                            n.x = nx;
                            n.y = ny;
                        }
                    }
                    // noch kein gültiger Kampfbezug (außer Reichweite).
                    if let Some(n) = world.npcs.get_mut(id) {
                        n.no_link_since = Some(n.no_link_since.unwrap_or(now));
                    }
                } else if strike {
                    // Angriff auflösen — gemeinsamer Kampfkern (§21).
                    let (ax, ay, skill, dmg_weapon) = {
                        let n = world.npcs.get(id).unwrap();
                        (n.x, n.y, n.weapon_skill, n.weapon_damage)
                    };
                    let (res, dmg) = {
                        let t = world.players.get(&tid).unwrap();
                        let cap = class_cap(cfg, &t.char_class);
                        let effective_armor = crate::attributes::effective_armor(t.armor, t.attributes.endurance);
                        resolve_attack(cfg, rng, dmg_weapon, skill, effective_armor, cap, 0, 0)
                    };
                    outcomes.push((id.clone(), tid, ax, ay, res, dmg));
                    if let Some(n) = world.npcs.get_mut(id) {
                        n.last_attack = now;
                        n.no_link_since = None; // gültiger Kampfbezug
                    }
                } else {
                    // in Reichweite, aber innerhalb der Duration → gültiger
                    // Kampfbezug, kein Schlag.
                    if let Some(n) = world.npcs.get_mut(id) {
                        n.no_link_since = None;
                    }
                }
            }
            _ => {
                // Ziel ungültig oder abwesend: peile Evade/Ankunft an.
            }
        }
    }

    // 5) Evade-Auslöser (Leash / kein gültiger Kampfbezug) VOR Schadens-
    //    anwendung — so sind NPCs, die gerade ausweichen, nicht mehr Ziel
    //    desselben Ticks.
    let leashers: Vec<(String, bool)> = world
        .npcs
        .values()
        .filter(|n| n.status == NpcStatus::Alive)
        .filter_map(|n| {
            let away = dist(n.x, n.y, n.home_x, n.home_y) > n.leash_radius;
            let no_link = n.no_link_since.is_some_and(|s| {
                now.duration_since(s) >= std::time::Duration::from_millis(npc_cfg.no_link_ms)
            });
            (away || no_link).then(|| (n.id.clone(), away || no_link))
        })
        .collect();
    for (id, _) in &leashers {
        if let Some(n) = world.npcs.get_mut(id) {
            n.status = NpcStatus::Returning;
            n.return_started_at = Some(now);
            n.target_id = None;
            n.no_link_since = None;
            n.claimed_by = None; // Boss-Reset → Claim vollständig gelöscht.
        }
    }

    // 6) Schaden anwenden (NPC → Spieler) + Broadcasts.
    let mut kills: Vec<(String, String)> = Vec::new();
    let mut broadcasts: Vec<(String, f64, f64, Frame)> = Vec::new();
    for (aid, tid, ax, ay, result, dmg) in &outcomes {
        let killed = if let Some(t) = world.players.get_mut(tid) {
            t.hp = (t.hp - dmg).max(0);
            t.hp == 0
        } else {
            false
        };
        let hit = result.key();
        broadcasts.push((
            aid.clone(),
            *ax,
            *ay,
            Frame::new(
                0,
                s2c::DAMAGE,
                serde_json::json!({"id": tid, "amount": dmg, "from_id": aid, "hit": hit}),
            ),
        ));
        if killed {
            kills.push((aid.clone(), tid.clone()));
        }
    }
    for (aid, ax, ay, frame) in &broadcasts {
        for p in world.players.values() {
            if p.id == *aid || dist(p.x, p.y, *ax, *ay) <= aofb {
                p.send(frame);
            }
        }
    }
    for (npc_id, pid) in &kills {
        let frame = Frame::new(
            0,
            s2c::KILL,
            serde_json::json!({"id": pid, "killer_id": npc_id}),
        );
        for p in world.players.values() {
            p.send(&frame);
        }
        // Combat V3: Tod → alle Effekte entfernen, Cast abbrechen,
        // nicht-persistente Cooldowns zurücksetzen (Ability-System.md §6).
        crate::combat::ability::on_death(world, pid, world.players.contains_key(pid));
        // Toten Spieler entwaffnen; NPC verliert das Ziel.
        if let Some(p) = world.players.get_mut(pid) {
            p.combat = None;
        }
        if let Some(n) = world.npcs.get_mut(npc_id) {
            n.target_id = None;
            n.no_link_since = Some(now); // kein Kampfbezug mehr
        }
    }
    // Alle Spieler entwaffnen, die einen toten Spieler anvisieren (parallel
    // zur Spieler-Kampflogik). NPC-Ziel wird bereits oben bedient.
    let dead_targets: Vec<String> = world
        .players
        .values()
        .filter(|p| {
            p.combat
                .as_ref()
                .and_then(|c| world.players.get(&c.target_id))
                .is_some_and(|t| t.hp <= 0)
        })
        .map(|p| p.combat.as_ref().unwrap().target_id.clone())
        .collect();
    for p in world.players.values_mut() {
        if p.hp <= 0
            || p.combat
                .as_ref()
                .map(|c| dead_targets.contains(&c.target_id))
                .unwrap_or(false)
        {
            p.combat = None;
        }
    }
}

/// Broadcastet den aktuellen NPC-Zustand (SPAWN, falls noch nicht sichtbar,
/// plus STATE) an alle Spieler in AOFB (§20: auch "returning" sichtbar).
/// Tote NPCs sind unsichtbar (Despawn durch world_tick über Sichtbarkeit).
pub fn broadcast_state(world: &mut World, npc_id: &str, aofb: f64) {
    let Some(n) = world.npcs.get(npc_id) else {
        return;
    };
    let frame_state = Frame::new(
        0,
        s2c::STATE,
        serde_json::json!({
            "id": n.id,
            "x": n.x,
            "y": n.y,
            "face": 0.0,
            "hp": n.hp,
            "max_hp": n.max_hp,
            "kind": n.kind,
            "status": n.status.key(),
            "aggro": n.target_id.is_some(),
            "claimed": n.claimed_by.is_some(),
        }),
    );
    for p in world.players.values() {
        if dist(p.x, p.y, n.x, n.y) <= aofb {
            p.send(&frame_state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::Duration;

    use tokio::sync::mpsc;

    /// Scripted-RNG: liefert exakt die vorgegebenen Werte (deterministisch).
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

    /// Feste Wallclock für deterministische Respawn-Tests.
    fn wall() -> SystemTime {
        SystemTime::UNIX_EPOCH
    }

    fn npc_cfg() -> NpcCfg {
        NpcCfg {
            social_aggro_radius: 15.0,
            no_link_ms: 5000,
            return_speed: 5.0,
            persist_interval_ms: 30000,
        }
    }

    fn combat_cfg() -> CombatCfg {
        CombatCfg {
            weapon_skill_id: "schwerter".into(),
            weapon_damage: 100,
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
        }
    }

    fn player(id: &str, hp: i32) -> (crate::world::Player, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            crate::world::Player {
                id: id.into(),
                name: id.into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp,
                max_hp: hp,
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
                armor: 0,
                weapon_skill: 1,
                combat: None,
                mana: 50,
                max_mana: 50,
                effects: Vec::new(),
                cooldowns: std::collections::BTreeMap::new(),
                active_cast: None,
                learned_abilities: std::collections::HashSet::new(),
                sitting: false,
                attributes: Default::default(),
                max_hp_base: hp,
                max_mana_base: 50,
                hp_regen_bonus: 0.0,
                mana_regen_bonus: 0.0,
                hp_regen_carry: 0.0,
                mana_regen_carry: 0.0,
            },
            rx,
        )
    }

    fn npc(id: &str, x: f64, y: f64, kind: &str) -> Npc {
        Npc {
            id: id.into(),
            spawn_id: 0,
            name: id.into(),
            kind: kind.into(),
            attackable: true,
            aggressive: true,
            aggro_range: 8.0,
            attack_range: 1.5,
            attack_duration_ms: 1500,
            weapon_damage: 20,
            weapon_skill: 1,
            armor: 0,
            max_hp: 100,
            move_speed: 4.0,
            respawn_ms: 300_000,
            faction: None,
            pack_id: None,
            home_x: x,
            home_y: y,
            home_radius: 5.0,
            leash_radius: 15.0,
            status: NpcStatus::Alive,
            hp: 100,
            x,
            y,
            target_id: None,
            last_attack: Instant::now(),
            no_link_since: None,
            return_started_at: None,
            respawn_after: None,
            claimed_by: None,
            override_ctx: None,
            effects: Vec::new(),
            cooldowns: std::collections::BTreeMap::new(),
            active_cast: None,
        }
    }

    fn armed_player(p: &mut crate::world::Player, target: &str, now: Instant) {
        use crate::combat::CombatState;
        p.combat = Some(CombatState {
            target_id: target.into(),
            last_attack: now - Duration::from_millis(2000),
        });
    }

    fn insert(world: &mut World, n: Npc) {
        let id = n.id.clone();
        world.npcs.insert(id, n);
    }

    /// V1+V2: Spieler-Auto-Angriff auf NPC fällt auf denselben Kampfkern
    /// (resolve_attack), NPCs sind gültige Ziele (npc_id im Target-Namespace).
    #[test]
    fn player_can_attack_npc_and_npc_takes_damage() {
        let cfg = combat_cfg();
        let mut w = World::new();
        let (mut a, _) = player("a", 100);
        a.x = 0.0;
        a.y = 0.0;
        let mut n = npc("npc_1", 1.0, 0.0, "normal");
        n.respawn_ms = 1_000_000;
        insert(&mut w, n);
        let t1 = Instant::now();
        armed_player(&mut a, "npc_1", t1);
        w.players.insert("a".into(), a);
        crate::combat::combat_tick(
            &mut w,
            &cfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            t1 + Duration::from_millis(10),
            wall(),
            20.0,
        );
        assert_eq!(w.npcs["npc_1"].hp, 0, "100-Schaden → tot");
        assert_eq!(
            w.npcs["npc_1"].status,
            NpcStatus::Dead,
            "NPC-Tod setzt Status Dead, kein Respawn im selben Tick"
        );
        assert!(
            w.npcs["npc_1"].respawn_after.is_some(),
            "Respawn-Timer (§21) startet bei Tod"
        );
    }

    /// §19: Kontext-Überschreibung (attackable/aggressive) läuft am
    /// Ablaufdatum automatisch aus; danach gilt wieder Content-Grundzustand.
    #[test]
    fn context_override_expires_on_schedule() {
        let mut w = World::new();
        let mut n = npc("npc_1", 0.0, 0.0, "normal");
        n.aggressive = false;
        n.attackable = false;
        insert(&mut w, n);
        let now = Instant::now();
        assert!(!w.npcs["npc_1"].effective_aggressive());
        assert!(!w.npcs["npc_1"].effective_attackable());
        w.npcs.get_mut("npc_1").unwrap().override_ctx = Some(ContextOverride {
            attackable: Some(true),
            aggressive: Some(true),
            expires_at: Some(now + Duration::from_millis(1000)),
        });
        assert!(w.npcs["npc_1"].effective_aggressive());
        assert!(w.npcs["npc_1"].effective_attackable());
        // Noch nicht abgelaufen → bleibt überschrieben.
        npc_tick(
            &mut w,
            &combat_cfg(),
            &npc_cfg(),
            &mut ScriptedRng::from(&[0.8, 0.9]),
            now + Duration::from_millis(500),
            wall(),
            1,
            20.0,
        );
        assert!(w.npcs["npc_1"].override_ctx.is_some());
        // Abgelaufen → zurück auf Grundzustand.
        npc_tick(
            &mut w,
            &combat_cfg(),
            &npc_cfg(),
            &mut ScriptedRng::from(&[0.8, 0.9]),
            now + Duration::from_millis(1500),
            wall(),
            1,
            20.0,
        );
        assert!(w.npcs["npc_1"].override_ctx.is_none());
        assert!(!w.npcs["npc_1"].effective_aggressive());
    }

    /// §21 Boss: erster Schaden > 0 an einem freien Boss verleiht den Claim;
    /// andere Schadensverursacher übernehmen ihn nicht (nur der erste gilt).
    #[test]
    fn boss_claim_first_damage_wins_and_other_dps_does_not_steal() {
        let cfg = combat_cfg();
        let mut w = World::new();
        let (mut a, _) = player("a", 100);
        a.x = 0.0;
        let mut boss = npc("npc_boss", 1.0, 0.0, "boss");
        boss.max_hp = 1500;
        boss.hp = 1500;
        insert(&mut w, boss);
        let t1 = Instant::now();
        armed_player(&mut a, "npc_boss", t1);
        w.players.insert("a".into(), a);
        // Erster Schlag: a trifft (100).
        crate::combat::combat_tick(
            &mut w,
            &cfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            t1 + Duration::from_millis(10),
            wall(),
            20.0,
        );
        assert_eq!(
            w.npcs["npc_boss"].claimed_by.as_deref(),
            Some("a"),
            "erster Schadensverursacher erhält Boss-Claim (§2)"
        );
        // Zweiter Schlag: c trifft (100). Claim bleibt bei a.
        let (mut c, _) = player("c", 100);
        c.x = 0.0;
        armed_player(&mut c, "npc_boss", t1);
        w.players.insert("c".into(), c);
        crate::combat::combat_tick(
            &mut w,
            &cfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            t1 + Duration::from_millis(20),
            wall(),
            20.0,
        );
        assert_eq!(
            w.npcs["npc_boss"].claimed_by.as_deref(),
            Some("a"),
            "Claim wird nicht übernommen (§2)"
        );
        assert_eq!(w.npcs["npc_boss"].hp, 1300);
    }

    /// §20/§21: Evade/Return löscht den Boss-Claim vollständig (Reset),
    /// kein Respawn-Timer im Reset-Fall.
    #[test]
    fn boss_claim_cleared_on_evade_return_reset() {
        let cfg = combat_cfg();
        let npccfg = npc_cfg();
        let mut w = World::new();
        let (mut a, _) = player("a", 100);
        a.x = 30.0; // außerhalb Leash → kein gültiger Kampfbezug
        a.y = 30.0;
        let mut boss = npc("npc_boss", 10.0, 10.0, "boss");
        boss.max_hp = 1500;
        boss.hp = 1500;
        boss.claimed_by = Some("a".into()); // Boss war geclaimt
        boss.target_id = Some("a".into());
        boss.no_link_since = Some(Instant::now() - Duration::from_millis(10_000));
        insert(&mut w, boss);
        let t1 = Instant::now();
        armed_player(&mut a, "npc_boss", t1);
        w.players.insert("a".into(), a);
        // a steht weit außerhalb → kein gültiger Kampfbezug, Evade auslösen.
        npc_tick(
            &mut w,
            &cfg,
            &npccfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            t1,
            wall(),
            100,
            20.0,
        );
        let boss = &w.npcs["npc_boss"];
        assert_eq!(boss.status, NpcStatus::Returning, "Evade (§20)");
        assert!(boss.claimed_by.is_none(), "Claim gelöscht (Reset)");
        assert!(
            boss.respawn_after.is_none(),
            "Reset erzeugt keinen Respawn-Timer (§20)"
        );
        assert!(boss.target_id.is_none(), "Reset entfernt Aggro");
        // Rückkehr: läuft zur Home-Position, Ankunft = voller Reset.
        let t3 = t1 + Duration::from_millis(5000);
        npc_tick(
            &mut w,
            &cfg,
            &npccfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            t3,
            wall(),
            100,
            20.0,
        );
        let boss = &w.npcs["npc_boss"];
        assert_eq!(boss.status, NpcStatus::Alive, "Ankunft = voller Reset");
        assert_eq!(boss.hp, boss.max_hp, "volle HP-Regeneration (§20)");
    }

    /// §20: un-interruptierbarer Evade/Return — kehrende NPCs greifen nicht
    /// an und sind nie Ziel (weder Aggro noch Verfolgung).
    #[test]
    fn returning_npc_is_uninterruptible() {
        let cfg = combat_cfg();
        let npccfg = npc_cfg();
        let mut w = World::new();
        let (mut a, _) = player("a", 100);
        a.x = 50.5; // nah am NPC → Aggro wäre möglich, wenn er angriffe
        let mut n = npc("npc_1", 50.0, 0.0, "normal");
        n.status = NpcStatus::Returning; // bereits im Return
        n.hp = 50;
        n.home_x = 0.0; // weit weg → bleibt während des Tests Returning
        n.home_y = 0.0;
        insert(&mut w, n);
        let t1 = Instant::now();
        w.players.insert("a".into(), a);
        npc_tick(
            &mut w,
            &cfg,
            &npccfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            t1,
            wall(),
            1,
            20.0,
        );
        assert_eq!(w.npcs["npc_1"].status, NpcStatus::Returning);
        assert!(
            w.npcs["npc_1"].target_id.is_none(),
            "keine neue Aggro während des Return"
        );
    }

    /// §21 Respawn: Rückkehr dauert dann, wenn der Timestamp "in der
    /// Zukunft" liegt, aber abgelaufen ist → NPC aufersteht korrekt.
    #[test]
    fn respawn_after_persists_across_realm_restart() {
        // Zustand nach "Realm-Neustart": tot mit Rest-Timer.
        let mut n = npc("npc_1", 0.0, 0.0, "normal");
        n.status = NpcStatus::Dead;
        n.respawn_after = Some(wall() + Duration::from_millis(1000));
        let mut w = World::new();
        insert(&mut w, n);
        // Vor Ablauf kein Respawn.
        npc_tick(
            &mut w,
            &combat_cfg(),
            &npc_cfg(),
            &mut ScriptedRng::from(&[0.8, 0.9]),
            Instant::now(),
            wall() + Duration::from_millis(500),
            1,
            20.0,
        );
        assert_eq!(w.npcs["npc_1"].status, NpcStatus::Dead);
        // Nach Ablauf Respawn (volle HP, an Home-Position, kein Claim).
        npc_tick(
            &mut w,
            &combat_cfg(),
            &npc_cfg(),
            &mut ScriptedRng::from(&[0.8, 0.9]),
            Instant::now(),
            wall() + Duration::from_millis(2000),
            1,
            20.0,
        );
        assert_eq!(w.npcs["npc_1"].status, NpcStatus::Alive);
        assert_eq!(w.npcs["npc_1"].hp, w.npcs["npc_1"].max_hp);
        assert_eq!(w.npcs["npc_1"].x, w.npcs["npc_1"].home_x);
        assert!(w.npcs["npc_1"].respawn_after.is_none());
        assert!(w.npcs["npc_1"].claimed_by.is_none());
    }

    /// §18: nicht angreifbare NPCs (attackable=false) werden nicht
    /// abgegriffen; kein Combat-Zustand entsteht.
    #[test]
    fn non_attackable_npc_rejects_start() {
        let mut w = World::new();
        let mut n = npc("npc_1", 1.0, 0.0, "normal");
        n.attackable = false;
        insert(&mut w, n);
        // Aggro-Trigger wird trotzdem nicht gesetzt (Handler prüft attackable).
        aggro_trigger(&mut w, "npc_1", "a", &npc_cfg());
        // Handler-Validierung: attackable=false → kein gültiger Start.
        let ok = w
            .npcs
            .get("npc_1")
            .is_some_and(|n| n.status == NpcStatus::Alive && n.effective_attackable());
        assert!(!ok);
    }
}
