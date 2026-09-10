// attributes — Sieben Grundattribute (Realm-autoritativ, technische Grundlage).
//
// Wachstums-Modul: Viele Werte sind Hooks für später gesetzte Systeme
// (Rassensystem §2, Ausrüstungsboni §3, fernkampf-/lauf-/movementsystems
// ohne Ausführungspfad). Daher keine dead_code-Wartung für gewollte Hooks.
#![allow(dead_code)]
//
// Umgesetzt nach docs/Attribute_und_Regeneration.md (§1–3):
// - Attribute sind ganzzahlige Punkte; abgeleitete Werte intern f64 (§11).
// - Aktive Wirkungen (erster Balancing-Stand, §11):
//   - Kraft:         +0,5 % Nahkampfschaden / Punkt
//   - Konstitution:  +10 Max-HP / Punkt
//   - Geschicklichkeit: +0,5 % Fernkampfschaden / Punkt
//   - Intelligenz:   +0,5 % magischer Schaden / Punkt
//   - Weisheit:      +20 Max-Mana / Punkt
//   - Glück:         +0,1 Prozentpunkte Crit-Chance / Punkt
//     (Aufschlag auf den bestehenden Crit-Wurf, kein paralleles System)
//   - Ausdauer:      +0,2 % auf den vorhandenen Rüstungswert / Punkt
//     (wirkt auf den Rüstungswert; Umrechnung in Schadensreduktion
//      bleibt in Kampfsystem.md §7); Laufdauer +1 % / Punkt
//     (vorbereitet, inaktiv — kein Bewegungssystem).
// - Rassenverteilung (§2) folgt mit dem Rassensystem (keine Zahlen erfunden).
// - Ausrüstungsboni (§3): additiver Hook, konkrete Items folgen separat.
// - Alle Konstanten zentral hier (§11, datengetrieben, anpassbar ohne
//   Architekturumbau).
use crate::world::Player;

/// Die sieben Grundattribute (ganzzahlige Punkte, §1).
#[derive(Debug, Clone, Copy, Default)]
pub struct Attributes {
    pub strength: i32,
    pub constitution: i32,
    // Vorbereitet: Fernkampfsystem folgt (Hook, kein erfundener Pfad).
    #[allow(dead_code)]
    pub dexterity: i32,
    pub intelligence: i32,
    pub wisdom: i32,
    pub luck: i32,
    pub endurance: i32,
}

/// Additive Ausrüstungsboni (§3): Hook, konkrete Items folgen separat.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AttributeBonuses {
    pub strength: i32,
    pub constitution: i32,
    pub dexterity: i32,
    pub intelligence: i32,
    pub wisdom: i32,
    pub luck: i32,
    pub endurance: i32,
}

impl Attributes {
    /// Hook für Ausrüstungsboni (§3); konkrete Items folgen separat.
    #[allow(dead_code)]
    pub fn effective(&self, bonus: &AttributeBonuses) -> Self {
        Self {
            strength: (self.strength + bonus.strength).max(0),
            constitution: (self.constitution + bonus.constitution).max(0),
            dexterity: (self.dexterity + bonus.dexterity).max(0),
            intelligence: (self.intelligence + bonus.intelligence).max(0),
            wisdom: (self.wisdom + bonus.wisdom).max(0),
            luck: (self.luck + bonus.luck).max(0),
            endurance: (self.endurance + bonus.endurance).max(0),
        }
    }
}

// === Erster Balancing-Stand (§11) ===

pub const MELEE_PERMILLE_PER_POINT: f64 = 5.0;
pub const RANGED_PERMILLE_PER_POINT: f64 = 5.0;
pub const MAGIC_PERMILLE_PER_POINT: f64 = 5.0;
pub const MAX_HP_PER_POINT: i32 = 10;
pub const MAX_MANA_PER_POINT: i32 = 20;
pub const CRIT_PERMILLE_PER_POINT: f64 = 1.0;
pub const ARMOR_PERMILLE_PER_POINT: f64 = 2.0;
pub const RUN_DURATION_PERCENT_PER_POINT: f64 = 1.0;

// === Abgeleitete Werte (f64, keine unnötige Rundung) ===

pub fn melee_damage_multiplier(strength: i32) -> f64 {
    1.0 + (strength.max(0) as f64) * (MELEE_PERMILLE_PER_POINT / 1000.0)
}

pub fn ranged_damage_multiplier(dexterity: i32) -> f64 {
    1.0 + (dexterity.max(0) as f64) * (RANGED_PERMILLE_PER_POINT / 1000.0)
}

pub fn magic_damage_multiplier(intelligence: i32) -> f64 {
    1.0 + (intelligence.max(0) as f64) * (MAGIC_PERMILLE_PER_POINT / 1000.0)
}

pub fn melee_damage_permille(strength: i32) -> u32 {
    (strength.max(0) as f64 * MELEE_PERMILLE_PER_POINT).round() as u32
}

pub fn max_hp_bonus(constitution: i32) -> i32 {
    constitution.max(0) * MAX_HP_PER_POINT
}

pub fn max_mana_bonus(wisdom: i32) -> i32 {
    wisdom.max(0) * MAX_MANA_PER_POINT
}

pub fn crit_bonus_permille(luck: i32) -> u32 {
    (luck.max(0) as f64 * CRIT_PERMILLE_PER_POINT).round() as u32
}

pub fn effective_armor(base: i32, endurance: i32) -> i32 {
    if base <= 0 {
        return 0;
    }
    let eff = base as f64 * (1.0 + endurance.max(0) as f64 * (ARMOR_PERMILLE_PER_POINT / 1000.0));
    eff.round().max(0.0) as i32
}

pub fn run_duration_percent(endurance: i32) -> f64 {
    endurance.max(0) as f64 * RUN_DURATION_PERCENT_PER_POINT
}

/// Rechnet Max-HP/Max-Mana aus Attributen neu und deckelt die aktuellen
/// Pools (§1). Einmalig beim Charakter laden aufrufen.
pub fn recompute_max_resources(p: &mut Player) {
    p.max_hp = p.max_hp_base + max_hp_bonus(p.attributes.constitution);
    p.max_mana = p.max_mana_base + max_mana_bonus(p.attributes.wisdom);
    p.hp = p.hp.min(p.max_hp);
    p.mana = p.mana.min(p.max_mana);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tokio::sync::mpsc;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn test_player(
        hp: i32, max_hp: i32, mana: i32, max_mana: i32,
        max_hp_base: i32, max_mana_base: i32,
        attrs: Attributes,
    ) -> Player {
        let (tx, _rx) = mpsc::unbounded_channel();
        Player {
            id: "t".into(), name: "t".into(),
            x: 0.0, y: 0.0, face: 0.0, ping_ms: 0, zone_id: 0,
            hp, max_hp, lang: "de".into(),
            account_id: 0, session_id: String::new(),
            entities: HashSet::new(), last_activity: std::time::Instant::now(), tx,
            char_class: "Adventurer".into(),
            class: crate::class::ClassStatus::Adventurer,
            faction_transition: false,
            level: 1,
            armor: 0,
            weapon_skill: 1, combat: None,
            mana, max_mana,
            effects: Vec::new(), cooldowns: Default::default(),
            active_cast: None, learned_abilities: HashSet::new(),
            attributes: attrs,
            max_hp_base, max_mana_base,
            sitting: false,
            hp_regen_bonus: 0.0, mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0, mana_regen_carry: 0.0,
        }
    }

    #[test]
    fn constitution_raises_max_hp() {
        let mut p = test_player(
            100, 100, 50, 50, 100, 50,
            Attributes { constitution: 15, ..Default::default() },
        );
        recompute_max_resources(&mut p);
        assert_eq!(p.max_hp, 250);
        assert_eq!(p.hp.min(p.max_hp), p.hp);
    }

    #[test]
    fn wisdom_raises_max_mana() {
        let mut p = test_player(
            100, 100, 50, 50, 100, 50,
            Attributes { wisdom: 12, ..Default::default() },
        );
        recompute_max_resources(&mut p);
        assert_eq!(p.max_mana, 290);
    }

    #[test]
    fn strength_raises_melee_damage() {
        assert!(approx(melee_damage_multiplier(10), 1.05));
        assert!(approx(melee_damage_multiplier(0), 1.0));
        assert!(approx(melee_damage_multiplier(20), 1.1));
    }

    #[test]
    fn dexterity_ranged_pure() {
        assert!(approx(ranged_damage_multiplier(10), 1.05));
        assert!(approx(ranged_damage_multiplier(0), 1.0));
    }

    #[test]
    fn intelligence_raises_magic_damage() {
        assert!(approx(magic_damage_multiplier(10), 1.05));
    }

    #[test]
    fn luck_raises_crit_permille() {
        assert_eq!(crit_bonus_permille(0), 0);
        assert_eq!(crit_bonus_permille(10), 10);
        assert_eq!(crit_bonus_permille(150), 150);
    }

    #[test]
    fn endurance_raises_effective_armor() {
        assert_eq!(effective_armor(100, 10), 102);
        assert_eq!(effective_armor(0, 10), 0);
        assert_eq!(effective_armor(50, 0), 50);
        assert_eq!(effective_armor(50, 50), 55);
    }

    #[test]
    fn recompute_clamps_current() {
        let mut p = test_player(
            300, 100, 200, 50, 100, 50,
            Attributes { constitution: 1, wisdom: 1, ..Default::default() },
        );
        recompute_max_resources(&mut p);
        assert_eq!(p.max_hp, 110);
        assert_eq!(p.hp, 110);
        assert_eq!(p.max_mana, 70);
        assert_eq!(p.mana, 70);
    }

    #[test]
    fn run_duration_is_prepared() {
        assert!(approx(run_duration_percent(10), 10.0));
    }

    #[test]
    fn precision_no_rounding_loss() {
        let m = melee_damage_multiplier(10);
        let dmg = (100.0_f64 * m).round() as i32;
        assert_eq!(dmg, 105);
    }
}
