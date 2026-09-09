// events — Strukturierte Kampf-Ergebnisse und S2C-Frames (Combat V3).
//
// Realm-bestätigte Ergebnisse (Ability-System.md §14) werden als
// `CombatEvent` dargestellt und in S2C-Frames umgewandelt.
// Zusätzliche S2C-Typen: ABILITY (16), EFFECT (17).
use crate::protocol::{s2c, Frame};

/// Ausführungsergebnis einer Fähigkeit (realm-bestätigt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbilityOutcome {
    Started,
    Interrupted,
    Succeeded,
    Failed,
}

impl AbilityOutcome {
    pub fn key(&self) -> &'static str {
        match self {
            AbilityOutcome::Started => "started",
            AbilityOutcome::Interrupted => "interrupted",
            AbilityOutcome::Succeeded => "succeeded",
            AbilityOutcome::Failed => "failed",
        }
    }
}

/// Strukturiertes Kampf-Ergebnis des Realms.
/// Wird in tests als Ereignisfolge geprüft; live als S2C-Frames versendet.
#[derive(Debug, Clone)]
pub enum CombatEvent {
    AbilityResult {
        caster_id: String,
        ability_id: String,
        outcome: AbilityOutcome,
        reason: Option<String>,
        target_id: Option<String>,
    },
    EffectApplied {
        entity_id: String,
        effect_id: String,
        group: String,
        kind: String,
        duration_left_ms: u64,
    },
    EffectRemoved {
        entity_id: String,
        effect_id: String,
        group: String,
    },
    DamageApplied {
        target_id: String,
        amount: i32,
        from_id: String,
    },
    HealApplied {
        target_id: String,
        amount: i32,
        from_id: String,
    },
    TargetDied {
        target_id: String,
        killer_id: String,
    },
}

/// Erzeugt den S2C-ABILITY-Frame.
pub fn ability_frame(caster_id: &str, ability_id: &str, outcome: AbilityOutcome, reason: Option<&str>, target_id: Option<&str>) -> Frame {
    Frame::new(
        0,
        s2c::ABILITY,
        serde_json::json!({
            "caster_id": caster_id,
            "ability_id": ability_id,
            "outcome": outcome.key(),
            "reason": reason,
            "target_id": target_id,
        }),
    )
}

/// Erzeugt den S2C-EFFECT-Frame (applied oder removed).
pub fn effect_frame(entity_id: &str, action: &str, effect_id: &str, group: &str, kind: &str, duration_left_ms: u64) -> Frame {
    Frame::new(
        0,
        s2c::EFFECT,
        serde_json::json!({
            "entity_id": entity_id,
            "action": action,
            "effect_id": effect_id,
            "group": group,
            "kind": kind,
            "duration_left_ms": duration_left_ms,
        }),
    )
}

/// Broadcastet einen Frame an alle Spieler in Reichweite eines bestimmten Ortes.
pub fn broadcast_aofb(world: &crate::world::World, x: f64, y: f64, aofb: f64, frame: &Frame) {
    for p in world.players.values() {
        let dx = p.x - x;
        let dy = p.y - y;
        if dx * dx + dy * dy <= aofb * aofb {
            p.send(frame);
        }
    }
}

/// Broadcastet einen Frame an alle Spieler.
pub fn broadcast_all(world: &crate::world::World, frame: &Frame) {
    for p in world.players.values() {
        p.send(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_frame_has_correct_type() {
        let f = ability_frame("p1", "fire_bolt", AbilityOutcome::Succeeded, None, Some("npc_1"));
        let v: serde_json::Value = serde_json::from_str(&f.encode()).unwrap();
        assert_eq!(v["type"], s2c::ABILITY);
        assert_eq!(v["data"]["outcome"], "succeeded");
    }

    #[test]
    fn effect_frame_has_correct_type() {
        let f = effect_frame("p1", "applied", "e1", "doom", "debuff", 6000);
        let v: serde_json::Value = serde_json::from_str(&f.encode()).unwrap();
        assert_eq!(v["type"], s2c::EFFECT);
        assert_eq!(v["data"]["action"], "applied");
        assert_eq!(v["data"]["duration_left_ms"], 6000);
    }
}
