// ability — Fähigkeits-Engine (Combat V3, Ability-System.md).
//
// Struktur:
//   AbilityDef    — Content-Definition (aus DB / später Lua)
//   ActiveCast    — Aktiver Cast-Zustand eines Spielers
//   ability_tick  — Welt-Tick für Cast-Progress, DoT/HoT, Effekte
//
// Architektur:
//   - Gleiche Ability-Grundlage für Spieler, NPCs, Named, Bosse (§15)
//   - Kein separates System (§15)
//   - Realm bleibt autoritativ
//   - Content-Werte aus ability_definitions (Vorläufig, Lua-Replace later)
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime};

use crate::protocol::{s2c, Frame};
use crate::world::World;
use super::cooldowns;
use super::effects::{self, Effect, EffectKind, SourceKind};
use super::events::{self, AbilityOutcome, CombatEvent};
use super::targeting;
use super::aoe::{self, AoeType};

/// Content-Definition einer Fähigkeit (aus ability_definitions, Migration 010).
#[derive(Debug, Clone)]
pub struct AbilityDef {
    pub id: String,
    #[allow(dead_code)] // Content-Feld: Client-Anzeige/Balancing (noch ungenutzt)
    pub name: String,
    pub exec_type: String,       // instant | cast | channel
    #[allow(dead_code)] // Content-Feld: semantische Einordnung (noch ungenutzt)
    pub semantic_category: String,
    pub mana_cost: u32,
    pub cooldown_ms: u64,
    #[allow(dead_code)] // Persistente Cooldowns: Reset-Logik in on_death (TODO)
    pub cooldown_persistent: bool,
    pub cast_time_ms: u64,
    pub range: f64,
    pub aoe_type: String,
    pub aoe_radius: f64,
    pub host_effect: bool,       // true = feindlich
    pub effect_kind: String,
    pub effect_value: f64,
    pub duration_ms: u64,
    pub tick_ms: u64,
    pub effect_group: Option<String>,
}

/// Aktiver Cast-Zustand eines Spielers.
#[derive(Debug, Clone)]
pub struct ActiveCast {
    pub ability_id: String,
    pub target_id: Option<String>,
    pub ground_x: Option<f64>,
    pub ground_y: Option<f64>,
    pub cast_end: Instant,
    pub start_x: f64,
    pub start_y: f64,
}

/// Registrierter Ability-Satz (aus DB geladen).
#[derive(Debug, Default, Clone)]
pub struct AbilityRegistry {
    pub defs: HashMap<String, AbilityDef>,
}

impl AbilityRegistry {
    pub fn new() -> Self { Self { defs: HashMap::new() } }

    pub fn get(&self, id: &str) -> Option<&AbilityDef> {
        self.defs.get(id)
    }

    pub fn register(&mut self, def: AbilityDef) {
        self.defs.insert(def.id.clone(), def);
    }
}

/// Gespeicherter Eintrag aus `ability_definitions` (Migration 010).
#[derive(Debug, Clone)]
pub struct AbilityDefRow {
    pub id: String,
    pub name: String,
    pub exec_type: String,
    pub semantic_category: String,
    pub mana_cost: i32,
    pub cooldown_ms: i32,
    pub cooldown_persistent: i32,
    pub cast_time_ms: i32,
    pub range: f64,
    pub aoe_type: String,
    pub aoe_radius: f64,
    pub host_effect: i32,
    pub effect_kind: String,
    pub effect_value: f64,
    pub duration_ms: i32,
    pub tick_ms: i32,
    pub effect_group: Option<String>,
}

/// Baut eine AbilityDef aus einem DB-Row.
pub fn build_ability_def(row: &AbilityDefRow) -> AbilityDef {
    AbilityDef {
        id: row.id.clone(),
        name: row.name.clone(),
        exec_type: row.exec_type.clone(),
        semantic_category: row.semantic_category.clone(),
        mana_cost: row.mana_cost.max(0) as u32,
        cooldown_ms: row.cooldown_ms.max(0) as u64,
        cooldown_persistent: row.cooldown_persistent != 0,
        cast_time_ms: row.cast_time_ms.max(0) as u64,
        range: row.range,
        aoe_type: row.aoe_type.clone(),
        aoe_radius: row.aoe_radius,
        host_effect: row.host_effect != 0,
        effect_kind: row.effect_kind.clone(),
        effect_value: row.effect_value,
        duration_ms: row.duration_ms.max(0) as u64,
        tick_ms: row.tick_ms.max(0) as u64,
        effect_group: row.effect_group.clone(),
    }
}

/// Validiert und startet eine Fähigkeit. Gibt eine Liste von CombatEvents zurück.
pub fn start_ability(
    world: &mut World,
    registry: &AbilityRegistry,
    caster_id: &str,
    ability_id: &str,
    target_id: Option<&str>,
    ground_x: Option<f64>,
    ground_y: Option<f64>,
    now: Instant,
    wall_now: SystemTime,
) -> Vec<CombatEvent> {
    let mut events = Vec::new();

    // 1) Caster-Status prüfen
    let status = targeting::validate_caster_status(world, caster_id);
    if !status.valid {
        events.push(CombatEvent::AbilityResult {
            caster_id: caster_id.into(),
            ability_id: ability_id.into(),
            outcome: AbilityOutcome::Failed,
            reason: status.reason,
            target_id: target_id.map(|s| s.into()),
        });
        return events;
    }

    // 2) Ability-Definition laden
    let def = match registry.get(ability_id) {
        Some(d) => d.clone(),
        None => {
            events.push(CombatEvent::AbilityResult {
                caster_id: caster_id.into(),
                ability_id: ability_id.into(),
                outcome: AbilityOutcome::Failed,
                reason: Some("ability_not_found".into()),
                target_id: target_id.map(|s| s.into()),
            });
            return events;
        }
    };

    // 3) Caster hat diese Fähigkeit gelernt?
    let has_ability = world.players.get(caster_id)
        .map(|p| p.learned_abilities.contains(ability_id))
        .unwrap_or(false);
    if !has_ability {
        events.push(CombatEvent::AbilityResult {
            caster_id: caster_id.into(),
            ability_id: ability_id.into(),
            outcome: AbilityOutcome::Failed,
            reason: Some("ability_not_learned".into()),
            target_id: target_id.map(|s| s.into()),
        });
        return events;
    }

    // 4) Mana prüfen
    let has_mana = world.players.get(caster_id)
        .map(|p| p.mana >= def.mana_cost as i32)
        .unwrap_or(false);
    if !has_mana {
        events.push(CombatEvent::AbilityResult {
            caster_id: caster_id.into(),
            ability_id: ability_id.into(),
            outcome: AbilityOutcome::Failed,
            reason: Some("not_enough_mana".into()),
            target_id: target_id.map(|s| s.into()),
        });
        return events;
    }

    // 5) Cooldown prüfen
    let cooldowns_ready = world.players.get(caster_id)
        .map(|p| cooldowns::is_ready(&p.cooldowns, ability_id, wall_now))
        .unwrap_or(true);
    if !cooldowns_ready {
        events.push(CombatEvent::AbilityResult {
            caster_id: caster_id.into(),
            ability_id: ability_id.into(),
            outcome: AbilityOutcome::Failed,
            reason: Some("on_cooldown".into()),
            target_id: target_id.map(|s| s.into()),
        });
        return events;
    }

    // 6) Zielvalidierung (Single-Target)
    if def.aoe_type == "single" || def.host_effect {
        let validation = if def.host_effect {
            targeting::validate_single_hostile(world, caster_id, target_id.unwrap_or(""), def.range)
        } else {
            targeting::validate_single_friendly(world, caster_id, target_id.unwrap_or(""), def.range)
        };
        if !validation.valid {
            events.push(CombatEvent::AbilityResult {
                caster_id: caster_id.into(),
                ability_id: ability_id.into(),
                outcome: AbilityOutcome::Failed,
                reason: validation.reason,
                target_id: target_id.map(|s| s.into()),
            });
            return events;
        }
    } else if def.aoe_type == "ground" {
        let validation = targeting::validate_ground_position(
            world, caster_id,
            ground_x.unwrap_or(0.0), ground_y.unwrap_or(0.0),
            def.range,
        );
        if !validation.valid {
            events.push(CombatEvent::AbilityResult {
                caster_id: caster_id.into(),
                ability_id: ability_id.into(),
                outcome: AbilityOutcome::Failed,
                reason: validation.reason,
                target_id: None,
            });
            return events;
        }
    }

    // 7) Mana abziehen
    if let Some(player) = world.players.get_mut(caster_id) {
        player.mana -= def.mana_cost as i32;
    }

    let caster_x = world.players.get(caster_id).map(|p| p.x).unwrap_or(0.0);
    let caster_y = world.players.get(caster_id).map(|p| p.y).unwrap_or(0.0);

    // 8) Sofortfähigkeiten direkt ausführen
    if def.exec_type == "instant" {
        execute_instant(world, registry, caster_id, caster_x, caster_y, &def, target_id, ground_x, ground_y, now, wall_now, &mut events);
        return events;
    }

    // 9) Cast starten (cast / channel)
    let cast_end = now + Duration::from_millis(def.cast_time_ms);
    let cast = ActiveCast {
        ability_id: ability_id.into(),
        target_id: target_id.map(|s| s.into()),
        ground_x,
        ground_y,
        cast_end,
        start_x: caster_x,
        start_y: caster_y,
    };
    if let Some(player) = world.players.get_mut(caster_id) {
        player.active_cast = Some(cast);
    }

    events.push(CombatEvent::AbilityResult {
        caster_id: caster_id.into(),
        ability_id: ability_id.into(),
        outcome: AbilityOutcome::Started,
        reason: None,
        target_id: target_id.map(|s| s.into()),
    });

    events
}

/// Führt eine Sofortfähigkeit aus.
fn execute_instant(
    world: &mut World,
    _registry: &AbilityRegistry,
    caster_id: &str,
    _caster_x: f64,
    _caster_y: f64,
    def: &AbilityDef,
    target_id: Option<&str>,
    ground_x: Option<f64>,
    ground_y: Option<f64>,
    now: Instant,
    wall_now: SystemTime,
    events: &mut Vec<CombatEvent>,
) {
    // AoE-Ziele bestimmen
    let aoe_t = AoeType::from_str(&def.aoe_type);
    let aoe_result = aoe::select_targets(
        world, caster_id, aoe_t, target_id, ground_x, ground_y,
        def.aoe_radius, def.host_effect,
    );

    // Cooldown starten
    if let Some(player) = world.players.get_mut(caster_id) {
        cooldowns::start(&mut player.cooldowns, def.id.clone(), def.cooldown_ms, wall_now);
    }

    // Effekte auf Ziele anwenden
    for target_id in &aoe_result.targets {
        apply_ability_effect(world, caster_id, target_id, def, now, events);
    }

    events.push(CombatEvent::AbilityResult {
        caster_id: caster_id.into(),
        ability_id: def.id.clone(),
        outcome: AbilityOutcome::Succeeded,
        reason: None,
        target_id: target_id.map(|s| s.into()),
    });
}

/// Wendet den Effekt einer Fähigkeit auf ein Ziel an.
fn apply_ability_effect(
    world: &mut World,
    caster_id: &str,
    target_id: &str,
    def: &AbilityDef,
    now: Instant,
    events: &mut Vec<CombatEvent>,
) {
    let kind = effects::parse_effect_kind(&def.effect_kind);

    // Direktschaden (ohne Effekt-Tick) — skaliert mit Intelligenz (§11).
    if def.effect_kind == "damage" && def.duration_ms == 0 {
        let mult = world.players.get(caster_id)
            .map(|p| crate::attributes::magic_damage_multiplier(p.attributes.intelligence))
            .unwrap_or(1.0);
        let amount = (def.effect_value * mult).round() as i32;
        apply_damage(world, caster_id, target_id, amount, events, now);
        return;
    }

    // Direktheilung (ohne Effekt-Tick)
    if def.effect_kind == "heal" && def.duration_ms == 0 {
        let amount = def.effect_value.round() as i32;
        apply_heal(world, caster_id, target_id, amount, events);
        return;
    }

    // Zeit-/Tick-Effekt (DoT, HoT, Buff, Debuff, Stun, etc.)
    // Magischer Schaden wird hier skaliert (§11): effektiver Wert wird
    // in den Effect geschrieben, damit die Ticks den Skalierungswert nutzen.
    let mult = if kind == EffectKind::Dot {
        world.players.get(caster_id)
            .map(|p| crate::attributes::magic_damage_multiplier(p.attributes.intelligence))
            .unwrap_or(1.0)
    } else {
        1.0
    };
    let scaled_value = def.effect_value * mult;
    let group = def.effect_group.clone().unwrap_or_default();
    let effect_id = format!("{}:{}", caster_id, def.id);
    let source_kind = SourceKind::Ability;

    let effect = Effect {
        id: effect_id.clone(),
        effect_id: def.id.clone(),
        group: group.clone(),
        source_entity: caster_id.into(),
        source_kind,
        target_entity: target_id.into(),
        kind,
        started_at: now,
        duration_ms: def.duration_ms,
        tick_ms: def.tick_ms,
        next_tick_at: if def.tick_ms > 0 && kind.tickable() {
            now.checked_add(Duration::from_millis(def.tick_ms)).or(Some(now))
        } else {
            None
        },
        value: scaled_value,
        interrupts_on_damage: kind == EffectKind::Root,
    };

    // Auf Spieler anwenden
    if let Some(player) = world.players.get_mut(target_id) {
        let removed = effects::apply_effect(&mut player.effects, effect.clone());
        for r in &removed {
            events.push(CombatEvent::EffectRemoved {
                entity_id: target_id.into(),
                effect_id: r.id.clone(),
                group: r.group.clone(),
            });
        }
        events.push(CombatEvent::EffectApplied {
            entity_id: target_id.into(),
            effect_id: effect_id,
            group,
            kind: kind.key().into(),
            duration_left_ms: def.duration_ms,
        });
    } else if let Some(npc) = world.npcs.get_mut(target_id) {
        let removed = effects::apply_effect(&mut npc.effects, effect.clone());
        for r in &removed {
            events.push(CombatEvent::EffectRemoved {
                entity_id: target_id.into(),
                effect_id: r.id.clone(),
                group: r.group.clone(),
            });
        }
        events.push(CombatEvent::EffectApplied {
            entity_id: target_id.into(),
            effect_id: effect_id,
            group,
            kind: kind.key().into(),
            duration_left_ms: def.duration_ms,
        });
    }
}

/// Wendet Schaden an (Schadensreduktion: direkt, da magischer Schaden;
/// physische Abilities könnten später Rüstung nutzen).
fn apply_damage(
    world: &mut World,
    caster_id: &str,
    target_id: &str,
    amount: i32,
    events: &mut Vec<CombatEvent>,
    _now: Instant,
) {
    let killed = if let Some(player) = world.players.get_mut(target_id) {
        player.hp = (player.hp - amount).max(0);
        // Durch Schaden unterbrochene Effekte (Root, Sleep)
        let removed = effects::on_damage_taken(&mut player.effects, amount);
        for r in &removed {
            events.push(CombatEvent::EffectRemoved {
                entity_id: target_id.into(),
                effect_id: r.id.clone(),
                group: r.group.clone(),
            });
        }
        player.hp == 0
    } else if let Some(npc) = world.npcs.get_mut(target_id) {
        if npc.status == crate::npc::NpcStatus::Alive && npc.effective_attackable() {
            let was_alive = npc.hp > 0;
            // Boss-Claim (§2)
            if npc.is_boss() && npc.claimed_by.is_none() && amount > 0 {
                npc.claimed_by = Some(caster_id.into());
            }
            npc.hp = (npc.hp - amount).max(0);
            if npc.hp == 0 && was_alive {
                npc.status = crate::npc::NpcStatus::Dead;
                npc.target_id = None;
                npc.no_link_since = None;
                npc.claimed_by = None;
                let respawn_ms = npc.respawn_ms.max(0) as u64;
                npc.respawn_after = Some(
                    std::time::SystemTime::now()
                        .checked_add(Duration::from_millis(respawn_ms))
                        .unwrap_or(std::time::SystemTime::now()),
                );
                // Evade/Return-Reset: alle Cooldowns/Effekte (§20)
                effects::clear_all(&mut npc.effects);
                cooldowns::reset_all(&mut npc.cooldowns);
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    events.push(CombatEvent::DamageApplied {
        target_id: target_id.into(),
        amount,
        from_id: caster_id.into(),
    });

    if killed {
        events.push(CombatEvent::TargetDied {
            target_id: target_id.into(),
            killer_id: caster_id.into(),
        });
    }
}

/// Wendet Heilung an (geclampt auf max_hp).
fn apply_heal(
    world: &mut World,
    caster_id: &str,
    target_id: &str,
    amount: i32,
    events: &mut Vec<CombatEvent>,
) {
    let healed = if let Some(player) = world.players.get_mut(target_id) {
        let before = player.hp;
        player.hp = (player.hp + amount).min(player.max_hp);
        player.hp - before
    } else if let Some(npc) = world.npcs.get_mut(target_id) {
        if npc.status == crate::npc::NpcStatus::Alive {
            let before = npc.hp;
            npc.hp = (npc.hp + amount).min(npc.max_hp);
            npc.hp - before
        } else {
            0
        }
    } else {
        0
    };

    if healed > 0 {
        events.push(CombatEvent::HealApplied {
            target_id: target_id.into(),
            amount: healed,
            from_id: caster_id.into(),
        });
    }
}

/// Bricht den aktiven Cast eines Spielers ab (z. B. durch Bewegung, Stun).
pub fn interrupt_cast(world: &mut World, caster_id: &str) -> Option<CombatEvent> {
    let player = world.players.get_mut(caster_id)?;
    if player.active_cast.is_none() {
        return None;
    }
    let cast = player.active_cast.take().unwrap();
    Some(CombatEvent::AbilityResult {
        caster_id: caster_id.into(),
        ability_id: cast.ability_id,
        outcome: AbilityOutcome::Interrupted,
        reason: Some("movement".into()),
        target_id: cast.target_id,
    })
}

/// Welt-Tick für Cast-Progress, DoT/HoT, Effekte (Ability-System.md §1, §6–7).
/// Wird einmal pro Tick (tick_ms) aufgerufen.
pub fn ability_tick(
    world: &mut World,
    registry: &AbilityRegistry,
    now: Instant,
    wall_now: SystemTime,
    _tick_ms: u64,
    aofb: f64,
) {
    // 1) Cast-Progress für alle Spieler mit aktivem Cast
    let cast_completions: Vec<(String, ActiveCast)> = {
        // 0) Bewegungsprüfung für in-progress Casts: Bewegung bricht Cast ab
        let player_ids: Vec<String> = world.players.keys().cloned().collect();
        for pid in &player_ids {
            let caster_moved = {
                let player = match world.players.get(pid) {
                    Some(p) => p,
                    None => continue,
                };
                match &player.active_cast {
                    Some(cast) => {
                        let dx = player.x - cast.start_x;
                        let dy = player.y - cast.start_y;
                        dx * dx + dy * dy > 0.001
                    }
                    None => false,
                }
            };
            if caster_moved {
                if let Some(event) = interrupt_cast(world, pid) {
                    let (px, py) = world.players.get(pid).map(|p| (p.x, p.y)).unwrap_or((0.0, 0.0));
                    broadcast_combat_event(world, px, py, aofb, &event);
                }
            }
        }
        let mut completions = Vec::new();
        for (pid, player) in world.players.iter() {
            if let Some(cast) = &player.active_cast {
                if now >= cast.cast_end {
                    completions.push((pid.clone(), cast.clone()));
                }
            }
        }
        completions
    };

    for (caster_id, cast) in cast_completions {
        // Cast ist abgeschlossen — validiere erneut
        let def = match registry.get(&cast.ability_id) {
            Some(d) => d.clone(),
            None => {
                // Fähigkeit nicht mehr vorhanden
                if let Some(player) = world.players.get_mut(&caster_id) {
                    player.active_cast = None;
                }
                continue;
            }
        };

        // Bewegungsprüfung
        let caster_moved = world.players.get(&caster_id)
            .map(|p| {
                let dx = p.x - cast.start_x;
                let dy = p.y - cast.start_y;
                dx * dx + dy * dy > 0.001
            })
            .unwrap_or(true);

        if caster_moved {
            if let Some(player) = world.players.get_mut(&caster_id) {
                player.active_cast = None;
            }
            let frame = events::ability_frame(
                &caster_id, &cast.ability_id, AbilityOutcome::Interrupted,
                Some("movement"), cast.target_id.as_deref(),
            );
            events::broadcast_all(world, &frame);
            continue;
        }

        // Reichweiten-/Zielprüfung
        if def.aoe_type == "single" || def.host_effect {
            let target = cast.target_id.as_deref().unwrap_or("");
            let validation = if def.host_effect {
                targeting::validate_single_hostile(world, &caster_id, target, def.range)
            } else {
                targeting::validate_single_friendly(world, &caster_id, target, def.range)
            };
            if !validation.valid {
                if let Some(player) = world.players.get_mut(&caster_id) {
                    player.active_cast = None;
                }
                let frame = events::ability_frame(
                    &caster_id, &cast.ability_id, AbilityOutcome::Interrupted,
                    validation.reason.as_deref(), cast.target_id.as_deref(),
                );
                events::broadcast_all(world, &frame);
                continue;
            }
        }

        // Fähigkeit erfolgreich ausgeführt
        if let Some(player) = world.players.get_mut(&caster_id) {
            player.active_cast = None;
            // Cooldown starten
            cooldowns::start(&mut player.cooldowns, def.id.clone(), def.cooldown_ms, wall_now);
        }

        let caster_x = world.players.get(&caster_id).map(|p| p.x).unwrap_or(0.0);
        let caster_y = world.players.get(&caster_id).map(|p| p.y).unwrap_or(0.0);

        let aoe_t = AoeType::from_str(&def.aoe_type);
        let aoe_result = aoe::select_targets(
            world, &caster_id, aoe_t,
            cast.target_id.as_deref(), cast.ground_x, cast.ground_y,
            def.aoe_radius, def.host_effect,
        );

        for target_id in &aoe_result.targets {
            let mut single_events = Vec::new();
            apply_ability_effect(world, &caster_id, target_id, &def, now, &mut single_events);
            for event in &single_events {
                broadcast_combat_event(world, caster_x, caster_y, aofb, event);
            }
        }

        let frame = events::ability_frame(
            &caster_id, &def.id, AbilityOutcome::Succeeded,
            None, cast.target_id.as_deref(),
        );
        events::broadcast_all(world, &frame);
    }

    // 2) DoT/HoT-Ticks
    process_effect_ticks(world, registry, now, aofb);

    // 3) Abgelaufene Effekte entfernen
    process_expired_effects(world, now, aofb);
}

/// Verarbeitet DoT/HoT-Ticks aller Entitäten (effects::process_tick
/// entscheidet anhand `next_tick_at`, welcher Tick fällig ist).
fn process_effect_ticks(world: &mut World, _registry: &AbilityRegistry, now: Instant, aofb: f64) {
    // Spieler-Effects
    let player_ids: Vec<String> = world.players.keys().cloned().collect();
    for pid in player_ids {
        let mut tick_results = Vec::new();
        if let Some(player) = world.players.get_mut(&pid) {
            for e in player.effects.iter_mut() {
                if let Some(result) = effects::process_tick(e, now) {
                    tick_results.push(result);
                }
            }
        }
        let (player_x, player_y) = world.players.get(&pid).map(|p| (p.x, p.y)).unwrap_or((0.0, 0.0));
        for result in tick_results {
            let amount = result.value;
            let mut tick_events = Vec::new();
            if result.kind == EffectKind::Dot {
                apply_damage(world, "dot", &pid, amount, &mut tick_events, now);
            } else if result.kind == EffectKind::Hot {
                apply_heal(world, "hot", &pid, amount, &mut tick_events);
            }
            for event in &tick_events {
                broadcast_combat_event(world, player_x, player_y, aofb, event);
            }
        }
    }

    // NPC-Effects
    let npc_ids: Vec<String> = world.npcs.keys().cloned().collect();
    for nid in npc_ids {
        let mut tick_results = Vec::new();
        if let Some(npc) = world.npcs.get_mut(&nid) {
            for e in npc.effects.iter_mut() {
                if let Some(result) = effects::process_tick(e, now) {
                    tick_results.push(result);
                }
            }
        }
        let (npc_x, npc_y) = world.npcs.get(&nid).map(|n| (n.x, n.y)).unwrap_or((0.0, 0.0));
        for result in tick_results {
            let amount = result.value;
            let mut tick_events = Vec::new();
            if result.kind == EffectKind::Dot {
                apply_damage(world, "dot", &nid, amount, &mut tick_events, now);
            } else if result.kind == EffectKind::Hot {
                apply_heal(world, "hot", &nid, amount, &mut tick_events);
            }
            for event in &tick_events {
                broadcast_combat_event(world, npc_x, npc_y, aofb, event);
            }
        }
    }
}

/// Entfernt abgelaufene Effekte und broadcastet EFFECT_REMOVED.
fn process_expired_effects(world: &mut World, now: Instant, aofb: f64) {
    // Spieler
    let player_ids: Vec<String> = world.players.keys().cloned().collect();
    for pid in player_ids {
        let removed = {
            let player = match world.players.get_mut(&pid) {
                Some(p) => p,
                None => continue,
            };
            effects::remove_expired(&mut player.effects, now)
        };
        let px = world.players.get(&pid).map(|p| p.x).unwrap_or(0.0);
        let py = world.players.get(&pid).map(|p| p.y).unwrap_or(0.0);
        for r in &removed {
            let frame = events::effect_frame(&pid, "removed", &r.id, &r.group, r.kind.key(), 0);
            events::broadcast_aofb(world, px, py, aofb, &frame);
        }
    }

    // NPCs
    let npc_ids: Vec<String> = world.npcs.keys().cloned().collect();
    for nid in npc_ids {
        let removed = {
            let npc = match world.npcs.get_mut(&nid) {
                Some(n) => n,
                None => continue,
            };
            effects::remove_expired(&mut npc.effects, now)
        };
        let nx = world.npcs.get(&nid).map(|n| n.x).unwrap_or(0.0);
        let ny = world.npcs.get(&nid).map(|n| n.y).unwrap_or(0.0);
        for r in &removed {
            let frame = events::effect_frame(&nid, "removed", &r.id, &r.group, r.kind.key(), 0);
            events::broadcast_aofb(world, nx, ny, aofb, &frame);
        }
    }
}

/// Broadcastet ein CombatEvent als S2C-Frame.
pub(crate) fn broadcast_combat_event(world: &World, x: f64, y: f64, aofb: f64, event: &CombatEvent) {
    match event {
        CombatEvent::DamageApplied { target_id, amount, from_id } => {
            let frame = Frame::new(0, s2c::DAMAGE, serde_json::json!({
                "id": target_id, "amount": amount, "from_id": from_id, "hit": "normal"
            }));
            events::broadcast_aofb(world, x, y, aofb, &frame);
        }
        CombatEvent::HealApplied { target_id, amount, from_id } => {
            let frame = Frame::new(0, s2c::STATE, serde_json::json!({
                "id": target_id, "amount": amount, "from_id": from_id, "heal": true
            }));
            events::broadcast_aofb(world, x, y, aofb, &frame);
        }
        CombatEvent::EffectApplied { entity_id, effect_id, group, kind, duration_left_ms } => {
            let frame = events::effect_frame(entity_id, "applied", effect_id, group, kind, *duration_left_ms);
            events::broadcast_aofb(world, x, y, aofb, &frame);
        }
        CombatEvent::EffectRemoved { entity_id, effect_id, group } => {
            let frame = events::effect_frame(entity_id, "removed", effect_id, group, "", 0);
            events::broadcast_aofb(world, x, y, aofb, &frame);
        }
        CombatEvent::TargetDied { target_id, killer_id } => {
            let frame = Frame::new(0, s2c::KILL, serde_json::json!({
                "id": target_id, "killer_id": killer_id
            }));
            events::broadcast_all(world, &frame);
        }
        CombatEvent::AbilityResult { caster_id, ability_id, outcome, reason, target_id } => {
            let frame = events::ability_frame(caster_id, ability_id, *outcome, reason.as_deref(), target_id.as_deref());
            events::broadcast_all(world, &frame);
        }
    }
}

/// Aufräumen bei Tod einer Entität (Ability-System.md §6, §20).
pub fn on_death(world: &mut World, entity_id: &str, is_player: bool) {
    if is_player {
        if let Some(player) = world.players.get_mut(entity_id) {
            // Alle aktiven Buffs/Debuffs entfernen
            effects::clear_all(&mut player.effects);
            // Cast abbrechen
            player.active_cast = None;
            // Nicht-persistenten Cooldowns zurücksetzen
            let persistent: HashSet<String> = HashSet::new(); //TODO: aus Registry
            cooldowns::reset_non_persistent(&mut player.cooldowns, &persistent);
        }
    } else {
        if let Some(npc) = world.npcs.get_mut(entity_id) {
            effects::clear_all(&mut npc.effects);
            npc.active_cast = None;
            cooldowns::reset_all(&mut npc.cooldowns);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Player;
    use crate::npc::{Npc, NpcStatus};
    use std::collections::{BTreeMap, HashSet};
    use tokio::sync::mpsc;

    fn make_player(id: &str, hp: i32, mana: i32) -> Player {
        let (tx, _rx) = mpsc::unbounded_channel();
        Player {
            id: id.into(), name: id.into(),
            x: 0.0, y: 0.0, face: 0.0, ping_ms: 0, zone_id: 0,
            hp, max_hp: hp, lang: "de".into(),
            account_id: 0, session_id: String::new(),
            entities: HashSet::new(), last_activity: Instant::now(), tx,
            char_class: "Adventurer".into(),
            class: crate::class::ClassStatus::Adventurer,
            faction_transition: false,
            level: 1,
            exp: 0,
            gold: 0,
            armor: 0,
            weapon_skill: 1, combat: None,
            mana, max_mana: mana,
            effects: Vec::new(), cooldowns: BTreeMap::new(),
            active_cast: None,
            learned_abilities: HashSet::new(),
            sitting: false,
            attributes: Default::default(),
            max_hp_base: hp,
            max_mana_base: mana,
            hp_regen_bonus: 0.0,
            mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0,
            mana_regen_carry: 0.0,
            inventory: Default::default(),
        }
    }

    fn make_npc(id: &str, hp: i32, x: f64, y: f64) -> Npc {
        Npc {
            id: id.into(), spawn_id: 1, name: "Wolf".into(),
            kind: "normal".into(),
            attackable: true, aggressive: true,
            aggro_range: 8.0, attack_range: 1.5, attack_duration_ms: 1500,
            weapon_damage: 10, weapon_skill: 1, armor: 0,
            exp_reward: 0,
            loot_table_id: None,
            max_hp: hp, move_speed: 4.0, respawn_ms: 300000,
            faction: None, pack_id: None,
            home_x: x, home_y: y, home_radius: 5.0, leash_radius: 15.0,
            status: NpcStatus::Alive, hp,
            x, y,
            target_id: None, last_attack: Instant::now(),
            no_link_since: None, return_started_at: None,
            respawn_after: None, claimed_by: None,
            override_ctx: None,
            effects: Vec::new(), cooldowns: BTreeMap::new(),
            active_cast: None,
        }
    }

    fn fire_bolt_def() -> AbilityDef {
        AbilityDef {
            id: "fire_bolt".into(), name: "Feuerblitz".into(),
            exec_type: "instant".into(), semantic_category: "single_target_damage".into(),
            mana_cost: 8, cooldown_ms: 2000, cooldown_persistent: false,
            cast_time_ms: 0, range: 12.0,
            aoe_type: "single".into(), aoe_radius: 0.0,
            host_effect: true, effect_kind: "damage".into(),
            effect_value: 35.0, duration_ms: 0, tick_ms: 0,
            effect_group: None,
        }
    }

    fn soul_rend_def() -> AbilityDef {
        AbilityDef {
            id: "soul_rend".into(), name: "Seelenriss".into(),
            exec_type: "instant".into(), semantic_category: "debuff".into(),
            mana_cost: 10, cooldown_ms: 4000, cooldown_persistent: false,
            cast_time_ms: 0, range: 12.0,
            aoe_type: "single".into(), aoe_radius: 0.0,
            host_effect: true, effect_kind: "debuff".into(),
            effect_value: 8.0, duration_ms: 9000, tick_ms: 3000,
            effect_group: Some("doom".into()),
        }
    }

    fn healing_light_def() -> AbilityDef {
        AbilityDef {
            id: "healing_light".into(), name: "Heilendes Licht".into(),
            exec_type: "instant".into(), semantic_category: "heal".into(),
            mana_cost: 12, cooldown_ms: 3000, cooldown_persistent: false,
            cast_time_ms: 0, range: 15.0,
            aoe_type: "single".into(), aoe_radius: 0.0,
            host_effect: false, effect_kind: "heal".into(),
            effect_value: 40.0, duration_ms: 0, tick_ms: 0,
            effect_group: None,
        }
    }

    fn frost_nova_def() -> AbilityDef {
        AbilityDef {
            id: "frost_nova".into(), name: "Frost Nova".into(),
            exec_type: "cast".into(), semantic_category: "aoe_damage".into(),
            mana_cost: 20, cooldown_ms: 8000, cooldown_persistent: false,
            cast_time_ms: 2000, range: 10.0,
            aoe_type: "target_radius".into(), aoe_radius: 5.0,
            host_effect: true, effect_kind: "damage".into(),
            effect_value: 50.0, duration_ms: 0, tick_ms: 0,
            effect_group: None,
        }
    }

    fn make_world_with_ability(def: AbilityDef) -> (World, AbilityRegistry) {
        let mut w = World::new();
        let mut caster = make_player("a", 100, 50);
        caster.learned_abilities.insert(def.id.clone());
        w.players.insert("a".into(), caster);
        w.npcs.insert("npc_1".into(), make_npc("npc_1", 100, 5.0, 0.0));

        let mut reg = AbilityRegistry::new();
        reg.register(def);
        (w, reg)
    }

    #[test]
    fn instant_damage_basic() {
        let def = fire_bolt_def();
        let (mut w, reg) = make_world_with_ability(def);
        let now = Instant::now();
        let wall = SystemTime::now();
        let events = start_ability(&mut w, &reg, "a", "fire_bolt", Some("npc_1"), None, None, now, wall);
        assert!(events.iter().any(|e| matches!(e, CombatEvent::AbilityResult { outcome: AbilityOutcome::Succeeded, .. })));
        assert!(events.iter().any(|e| matches!(e, CombatEvent::DamageApplied { amount: 35, .. })));
        assert_eq!(w.npcs["npc_1"].hp, 65);
        assert_eq!(w.players["a"].mana, 42); // 50 - 8
    }

    #[test]
    fn not_enough_mana() {
        let def = fire_bolt_def();
        let (mut w, reg) = make_world_with_ability(def);
        w.players.get_mut("a").unwrap().mana = 5;
        let now = Instant::now();
        let wall = SystemTime::now();
        let events = start_ability(&mut w, &reg, "a", "fire_bolt", Some("npc_1"), None, None, now, wall);
        assert!(events.iter().any(|e| {
            if let CombatEvent::AbilityResult {
                outcome: AbilityOutcome::Failed,
                reason: Some(r),
                ..
            } = e
            {
                r == "not_enough_mana"
            } else {
                false
            }
        }));
    }

    #[test]
    fn cooldown_prevents_cast() {
        let def = fire_bolt_def();
        let (mut w, reg) = make_world_with_ability(def);
        let now = Instant::now();
        let wall = SystemTime::now();
        start_ability(&mut w, &reg, "a", "fire_bolt", Some("npc_1"), None, None, now, wall);
        let events2 = start_ability(&mut w, &reg, "a", "fire_bolt", Some("npc_1"), None, None, now, wall);
        assert!(events2.iter().any(|e| {
            if let CombatEvent::AbilityResult {
                outcome: AbilityOutcome::Failed,
                reason: Some(r),
                ..
            } = e
            {
                r == "on_cooldown"
            } else {
                false
            }
        }));
    }

    #[test]
    fn heal_clamped_at_max() {
        let def = healing_light_def();
        let (mut w, reg) = make_world_with_ability(def);
        w.players.get_mut("a").unwrap().hp = 90;
        let mut target = make_player("b", 100, 50);
        target.hp = 80;
        w.players.insert("b".into(), target);
        let now = Instant::now();
        let wall = SystemTime::now();
        let events = start_ability(&mut w, &reg, "a", "healing_light", Some("b"), None, None, now, wall);
        assert!(events.iter().any(|e| matches!(e, CombatEvent::HealApplied { amount: 20, .. }))); // 80 + 40 = 120, capped at 100
        assert_eq!(w.players["b"].hp, 100);
    }

    #[test]
    fn soul_rend_applies_debuff_effect() {
        let def = soul_rend_def();
        let (mut w, reg) = make_world_with_ability(def);
        let now = Instant::now();
        let wall = SystemTime::now();
        let events = start_ability(&mut w, &reg, "a", "soul_rend", Some("npc_1"), None, None, now, wall);
        assert!(events
            .iter()
            .any(|e| if let CombatEvent::EffectApplied { kind, .. } = e { kind == "debuff" } else { false }));
        assert!(!w.npcs["npc_1"].effects.is_empty());
    }

    #[test]
    fn same_group_replaces() {
        let def = soul_rend_def();
        let (mut w, reg) = make_world_with_ability(def);
        let now = Instant::now();
        let wall = SystemTime::now();
        start_ability(&mut w, &reg, "a", "soul_rend", Some("npc_1"), None, None, now, wall);
        start_ability(&mut w, &reg, "a", "soul_rend", Some("npc_1"), None, None, now, wall);
        // Nur ein Effekt pro Gruppe
        let doom: Vec<_> = w.npcs["npc_1"].effects.iter().filter(|e| e.group == "doom").collect();
        assert_eq!(doom.len(), 1);
    }

    #[test]
    fn cast_started_then_completes() {
        let def = frost_nova_def();
        let (mut w, reg) = make_world_with_ability(def);
        let now = Instant::now();
        let wall = SystemTime::now();
        let events = start_ability(&mut w, &reg, "a", "frost_nova", Some("npc_1"), None, None, now, wall);
        assert!(events.iter().any(|e| matches!(e, CombatEvent::AbilityResult { outcome: AbilityOutcome::Started, .. })));
        assert!(w.players["a"].active_cast.is_some());

        // Tick nach cast_time_ms
        let later = now + Duration::from_millis(2000);
        let wall2 = wall + Duration::from_millis(2000);
        ability_tick(&mut w, &reg, later, wall2, 100, 50.0);
        assert!(w.players["a"].active_cast.is_none());
        assert_eq!(w.npcs["npc_1"].hp, 50); // 100 - 50
    }

    #[test]
    fn movement_interrupts_cast() {
        let def = frost_nova_def();
        let (mut w, reg) = make_world_with_ability(def);
        let now = Instant::now();
        let wall = SystemTime::now();
        start_ability(&mut w, &reg, "a", "frost_nova", Some("npc_1"), None, None, now, wall);

        // Spieler bewegt sich
        w.players.get_mut("a").unwrap().x = 10.0;

        let later = now + Duration::from_millis(1000);
        let wall2 = wall + Duration::from_millis(1000);
        ability_tick(&mut w, &reg, later, wall2, 100, 50.0);
        assert!(w.players["a"].active_cast.is_none());
        assert_eq!(w.npcs["npc_1"].hp, 100); // kein Schaden
    }

    #[test]
    fn death_clears_effects() {
        let def = soul_rend_def();
        let (mut w, reg) = make_world_with_ability(def);
        let now = Instant::now();
        let wall = SystemTime::now();
        start_ability(&mut w, &reg, "a", "soul_rend", Some("npc_1"), None, None, now, wall);
        assert!(!w.npcs["npc_1"].effects.is_empty());
        on_death(&mut w, "npc_1", false);
        assert!(w.npcs["npc_1"].effects.is_empty());
    }

    #[test]
    fn out_of_range_fails() {
        let def = fire_bolt_def();
        let (mut w, reg) = make_world_with_ability(def);
        w.npcs.get_mut("npc_1").unwrap().x = 100.0;
        let now = Instant::now();
        let wall = SystemTime::now();
        let events = start_ability(&mut w, &reg, "a", "fire_bolt", Some("npc_1"), None, None, now, wall);
        assert!(events.iter().any(|e| {
            if let CombatEvent::AbilityResult {
                outcome: AbilityOutcome::Failed,
                reason: Some(r),
                ..
            } = e
            {
                r == "out_of_range"
            } else {
                false
            }
        }));
    }

    #[test]
    fn stunned_caster_fails() {
        let def = fire_bolt_def();
        let (mut w, reg) = make_world_with_ability(def);
        w.players.get_mut("a").unwrap().effects.push(Effect {
            id: "stun1".into(), effect_id: "stun".into(),
            group: "crowd".into(), source_entity: "b".into(),
            source_kind: SourceKind::Ability,
            target_entity: "a".into(),
            kind: EffectKind::Stun,
            started_at: Instant::now(), duration_ms: 3000,
            tick_ms: 0, next_tick_at: None, value: 0.0, interrupts_on_damage: false,
        });
        let now = Instant::now();
        let wall = SystemTime::now();
        let events = start_ability(&mut w, &reg, "a", "fire_bolt", Some("npc_1"), None, None, now, wall);
        assert!(events.iter().any(|e| {
            if let CombatEvent::AbilityResult {
                outcome: AbilityOutcome::Failed,
                reason: Some(r),
                ..
            } = e
            {
                r == "stunned"
            } else {
                false
            }
        }));
    }
}
