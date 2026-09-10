// targeting — Zielvalidierung für Ability-System (Combat V3).
//
// Gemeinsame Prüfungen: Ziel existiert, ist lebendig, ist erreichbar
// (Reichweite), ist ein gültiges Ziel (feindlich/freundlich gemäß
// Fähigkeit).
use crate::world::World;

/// Ergebnis einer Zielvalidierung.
#[derive(Debug, Clone)]
pub struct TargetValidation {
    pub valid: bool,
    pub reason: Option<String>,
}

impl TargetValidation {
    fn ok() -> Self { Self { valid: true, reason: None } }
    fn fail(reason: &str) -> Self { Self { valid: false, reason: Some(reason.to_string()) } }
}

/// Validiert ein einzelnes Ziel für eine feindliche Fähigkeit.
/// Prüft: Ziel existiert, ist lebendig, ist erreichbar (Reichweite).
pub fn validate_single_hostile(
    world: &World,
    caster_id: &str,
    target_id: &str,
    range: f64,
) -> TargetValidation {
    if target_id.is_empty() {
        return TargetValidation::fail("no_target");
    }
    if target_id == caster_id {
        return TargetValidation::fail("cannot_target_self");
    }
    // Spieler
    if let Some(target) = world.players.get(target_id) {
        if target.hp <= 0 {
            return TargetValidation::fail("target_dead");
        }
        if let Some(caster) = world.players.get(caster_id) {
            let dx = caster.x - target.x;
            let dy = caster.y - target.y;
            if dx * dx + dy * dy > range * range {
                return TargetValidation::fail("out_of_range");
            }
            // Sichtlinie (derzeit immer frei, siehe line_of_sight).
            if !line_of_sight(world, caster.x, caster.y, target.x, target.y) {
                return TargetValidation::fail("no_line_of_sight");
            }
        } else {
            return TargetValidation::fail("caster_not_found");
        }
        return TargetValidation::ok();
    }
    // NPC
    if let Some(npc) = world.npcs.get(target_id) {
        if npc.hp <= 0 || npc.status != crate::npc::NpcStatus::Alive {
            return TargetValidation::fail("target_dead");
        }
        if !npc.effective_attackable() {
            return TargetValidation::fail("target_not_attackable");
        }
        if let Some(caster) = world.players.get(caster_id) {
            let dx = caster.x - npc.x;
            let dy = caster.y - npc.y;
            if dx * dx + dy * dy > range * range {
                return TargetValidation::fail("out_of_range");
            }
            if !line_of_sight(world, caster.x, caster.y, npc.x, npc.y) {
                return TargetValidation::fail("no_line_of_sight");
            }
        } else {
            return TargetValidation::fail("caster_not_found");
        }
        return TargetValidation::ok();
    }
    TargetValidation::fail("target_not_found")
}

/// Validiert ein einzelnes Ziel für eine freundliche Fähigkeit.
/// Prüft: Ziel existiert, ist lebendig, ist erreichbar, ist freundlich
/// (andere Fraktion = feindlich; ohne Fraktions-Daten: alle Spieler sind
/// potenziell freundlich, solange kein PvP implementiert ist).
pub fn validate_single_friendly(
    world: &World,
    caster_id: &str,
    target_id: &str,
    range: f64,
) -> TargetValidation {
    if target_id.is_empty() {
        return TargetValidation::fail("no_target");
    }
    if target_id == caster_id {
        return TargetValidation::fail("cannot_target_self");
    }
    if let Some(target) = world.players.get(target_id) {
        if target.hp <= 0 {
            return TargetValidation::fail("target_dead");
        }
        if let Some(caster) = world.players.get(caster_id) {
            let dx = caster.x - target.x;
            let dy = caster.y - target.y;
            if dx * dx + dy * dy > range * range {
                return TargetValidation::fail("out_of_range");
            }
        }
        return TargetValidation::ok();
    }
    // NPCs als freundliche Ziele (Heilung): grundsätzlich erlaubt, solange
    // lebendig; Reinforcement durch Frakton wird später geprüft.
    if let Some(npc) = world.npcs.get(target_id) {
        if npc.hp <= 0 || npc.status != crate::npc::NpcStatus::Alive {
            return TargetValidation::fail("target_dead");
        }
        if let Some(caster) = world.players.get(caster_id) {
            let dx = caster.x - npc.x;
            let dy = caster.y - npc.y;
            if dx * dx + dy * dy > range * range {
                return TargetValidation::fail("out_of_range");
            }
        }
        return TargetValidation::ok();
    }
    TargetValidation::fail("target_not_found")
}

/// Prüft Caster-Status: tot, betäubt (Stun) oder geschwiegen (Silence)
/// verhindern das Wirken (Ability-System.md §6).
pub fn validate_caster_status(world: &World, caster_id: &str) -> TargetValidation {
    if let Some(player) = world.players.get(caster_id) {
        if player.hp <= 0 {
            return TargetValidation::fail("caster_dead");
        }
        if crate::combat::effects::is_stunned(&player.effects) {
            return TargetValidation::fail("stunned");
        }
        if crate::combat::effects::is_silenced(&player.effects) {
            return TargetValidation::fail("silenced");
        }
        return TargetValidation::ok();
    }
    TargetValidation::fail("caster_not_found")
}

/// Prüft Reichweite Caster ↔ Ground-Position (AoE: ground).
pub fn validate_ground_position(
    world: &World,
    caster_id: &str,
    x: f64,
    y: f64,
    max_range: f64,
) -> TargetValidation {
    if let Some(caster) = world.players.get(caster_id) {
        let dx = caster.x - x;
        let dy = caster.y - y;
        if dx * dx + dy * dy > max_range * max_range {
            return TargetValidation::fail("out_of_range");
        }
        return TargetValidation::ok();
    }
    TargetValidation::fail("caster_not_found")
}

/// Einfache Sichtlinien-Prüfung (vorläufig: immer true, da keine
/// Hindernisse in der Welt existieren; Interface für später).
pub fn line_of_sight(_world: &World, _from_x: f64, _from_y: f64, _to_x: f64, _to_y: f64) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::effects::EffectKind;
    use crate::combat::effects::Effect;
    use crate::world::Player;
    use std::collections::HashSet;
    use std::time::Instant;
    use tokio::sync::mpsc;

    fn make_player(id: &str, hp: i32, x: f64, y: f64) -> Player {
        let (tx, _rx) = mpsc::unbounded_channel();
        Player {
            id: id.into(),
            name: id.into(),
            x, y,
            face: 0.0,
            ping_ms: 0,
            zone_id: 0,
            hp, max_hp: hp,
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
            mana: 50,
            max_mana: 50,
            effects: Vec::new(),
            cooldowns: std::collections::BTreeMap::new(),
            active_cast: None,
            learned_abilities: HashSet::new(),
            sitting: false,
            attributes: Default::default(),
            max_hp_base: hp,
            max_mana_base: 50,
            hp_regen_bonus: 0.0,
            mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0,
            mana_regen_carry: 0.0,
        }
    }

    #[test]
    fn hostile_target_valid() {
        let mut w = World::new();
        let a = make_player("a", 100, 0.0, 0.0);
        let b = make_player("b", 100, 5.0, 0.0);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);
        let r = validate_single_hostile(&w, "a", "b", 10.0);
        assert!(r.valid);
    }

    #[test]
    fn hostile_target_out_of_range() {
        let mut w = World::new();
        let a = make_player("a", 100, 0.0, 0.0);
        let b = make_player("b", 100, 100.0, 0.0);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);
        let r = validate_single_hostile(&w, "a", "b", 10.0);
        assert!(!r.valid);
        assert_eq!(r.reason.as_deref(), Some("out_of_range"));
    }

    #[test]
    fn caster_stunned() {
        let mut w = World::new();
        let mut a = make_player("a", 100, 0.0, 0.0);
        a.effects.push(Effect {
            id: "e1".into(),
            effect_id: "stun".into(),
            group: "crowd".into(),
            source_entity: "b".into(),
            source_kind: crate::combat::effects::SourceKind::Ability,
            target_entity: "a".into(),
            kind: EffectKind::Stun,
            started_at: Instant::now(),
            duration_ms: 3000,
            tick_ms: 0,
            next_tick_at: None,
            value: 3.0,
            interrupts_on_damage: false,
        });
        w.players.insert("a".into(), a);
        let r = validate_caster_status(&w, "a");
        assert!(!r.valid);
        assert_eq!(r.reason.as_deref(), Some("stunned"));
    }

    #[test]
    fn friendly_target_valid() {
        let mut w = World::new();
        let a = make_player("a", 100, 0.0, 0.0);
        let b = make_player("b", 50, 3.0, 0.0);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);
        let r = validate_single_friendly(&w, "a", "b", 15.0);
        assert!(r.valid);
    }

    #[test]
    fn ground_position_out_of_range() {
        let mut w = World::new();
        let a = make_player("a", 100, 0.0, 0.0);
        w.players.insert("a".into(), a);
        let r = validate_ground_position(&w, "a", 50.0, 0.0, 10.0);
        assert!(!r.valid);
    }
}
