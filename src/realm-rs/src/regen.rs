// regen — Gemeinsames HP-/Mana-Regenerationssystem (Realm-autoritativ).
//
// Umgesetzt nach docs/Attribute_und_Regeneration.md (§§4–8):
// Grundformel:
//   Effektive Regeneration = (Basisregeneration + additive Boni) × Zustandsmultiplikator
// - absolute Werte pro Sekunde (kein Anteil des Max-Pools)
// - Zustandsmultiplikatoren: Kampf 15 %, stehend 100 %, sitzend 125 %
//   (Sitzbonus wirkt nie im Kampf)
// - Klassen-Basisregeneration Level 1 (Kämpfer 6/3, Magier 3/7, Priester 4/6,
//   Kundschafter 5/4) + Levelwachstum +0,2 HP/s und +0,2 Mana/s pro Level
// - intern mit f64 gerechnet; der Bruchteil wird je Ressourcen als Carry
//   getragen, damit dezimale Raten über viele Ticks ohne Rundungsverlust
//   aufsummiert werden (HP/Mana bleiben i32, kein vorgezogenes Runden).
//
// Die Werte sind erster Balancing-Stand (docs/Attribute_und_Regeneration.md
// §11): zentral an einer Stelle, damit spätere Anpassungen ohne
// Architekturumbau möglich sind.
use crate::world::Player;

/// Zustandsmultiplikator (docs/Attribute_und_Regeneration.md §5).
pub const REGEN_MULT_COMBAT: f64 = 0.15;
pub const REGEN_MULT_NORMAL: f64 = 1.0;
pub const REGEN_MULT_SITTING: f64 = 1.25;

/// Level-Zuwachs pro zusätzlichem Level (docs/Attribute_und_Regeneration.md §6).
pub const REGEN_LEVEL_GROWTH: f64 = 0.2;

/// Klassen-Basisregeneration pro Sekunde bei Level 1
/// (docs/Attribute_und_Regeneration.md §6).
#[derive(Debug, Clone, Copy)]
pub struct ClassRegen {
    pub hp: f64,
    pub mana: f64,
}

impl ClassRegen {
    pub const FIGHTER: Self = Self { hp: 6.0, mana: 3.0 };
    pub const MAGE: Self = Self { hp: 3.0, mana: 7.0 };
    pub const PRIEST: Self = Self { hp: 4.0, mana: 6.0 };
    pub const SCOUT: Self = Self { hp: 5.0, mana: 4.0 };
}

/// Basisregeneration je Klasse (Level 1). Erkannt werden deutsche
/// Klassennamen (Kämpfer/Magier/Priester/Kundschafter), legacy-Namen
/// (Warrior/Mage, wie in Kampf-/Armor-Logik) und typische
/// Subklassen-Bezeichnungen. Unbekannt → FIGHTER.
pub fn class_base_regen(class: &str) -> ClassRegen {
    match class.trim().to_lowercase().as_str() {
        "kämpfer" | "kampfer" | "krieger" | "paladin" | "warrior" => ClassRegen::FIGHTER,
        "magier" | "hexer" | "mentalist" | "mage" => ClassRegen::MAGE,
        "priester" | "priest" | "druide" | "templer" => ClassRegen::PRIEST,
        "kundschafter" | "scout" | "räuber" | "raeber" | "barde" => ClassRegen::SCOUT,
        _ => ClassRegen::FIGHTER,
    }
}

/// Multiplikator nach Zustand: im Kampf immer 15 %, sonst 125 % sitzend
/// bzw. 100 % stehend (Sitzbonus greift nie im Kampf).
pub fn regen_multiplier(in_combat: bool, sitting: bool) -> f64 {
    if in_combat {
        REGEN_MULT_COMBAT
    } else if sitting {
        REGEN_MULT_SITTING
    } else {
        REGEN_MULT_NORMAL
    }
}

/// Effektive Regeneration pro Sekunde (Grundformel, §4).
/// Basis = Klassenbasis + (Level-1 × Levelwachstum) + additiver Bonus.
pub fn effective_regen_per_sec(
    class: &str,
    level: u32,
    bonus: f64,
    in_combat: bool,
    sitting: bool,
) -> f64 {
    let base = class_base_regen(class);
    let level_bonus = (level.saturating_sub(1)) as f64 * REGEN_LEVEL_GROWTH;
    (base.hp + level_bonus + bonus.max(0.0)) * regen_multiplier(in_combat, sitting)
}

/// Effektive Mana-Regeneration pro Sekunde (gleiche Formel, §4).
pub fn effective_mana_regen_per_sec(
    class: &str,
    level: u32,
    bonus: f64,
    in_combat: bool,
    sitting: bool,
) -> f64 {
    let base = class_base_regen(class);
    let level_bonus = (level.saturating_sub(1)) as f64 * REGEN_LEVEL_GROWTH;
    (base.mana + level_bonus + bonus.max(0.0)) * regen_multiplier(in_combat, sitting)
}

/// Wendet die Regeneration für einen Tick an (Delta in Millisekunden):
/// rechnet intern f64 (Rate × t/1000), trägt den Bruchteil je Ressource als
/// Carry weiter, addiert nur volle Einheiten hinzu und deckelt bei max / 0.
/// Tote Charaktere (hp == 0) regenerieren NICHT (keine Wiederbelebung, §4).
pub fn apply_regen(player: &mut Player, tick_ms: u64) {
    if player.hp <= 0 {
        return;
    }
    let t = tick_ms as f64 / 1000.0;

    let in_combat = player.combat.is_some();
    let hp_rate = effective_regen_per_sec(
        &player.char_class,
        player.level,
        player.hp_regen_bonus,
        in_combat,
        player.sitting,
    );
    let mana_rate = effective_mana_regen_per_sec(
        &player.char_class,
        player.level,
        player.mana_regen_bonus,
        in_combat,
        player.sitting,
    );
    // §4: Regeneration wirkt nur, solange der Pool nicht voll ist.
    if player.hp < player.max_hp {
        let gain = player.hp_regen_carry + hp_rate * t;
        let whole = gain as i32; // nicht-negativ → Ganzzahlteil (floor)
        player.hp_regen_carry = gain - whole as f64;
        if whole > 0 {
            let new_hp = (player.hp + whole).min(player.max_hp);
            player.hp = new_hp;
            // §6/§39: HP-Regeneration ändert persistierbare Ressourcen →
            // Komponente `Resources` dirty markieren (Stufe B).
            player.mark_dirty(crate::persist::PersistComponent::Resources);
            if new_hp == player.max_hp {
                player.hp_regen_carry = 0.0; // voller Pool: kein Carry aufbauen
            }
        }
    }
    if player.mana < player.max_mana {
        let gain = player.mana_regen_carry + mana_rate * t;
        let whole = gain as i32;
        player.mana_regen_carry = gain - whole as f64;
        if whole > 0 {
            let new_mana = (player.mana + whole).min(player.max_mana);
            // Nur bei **wirklicher** Änderung markieren: die Regeneration ist
            // gedeckelt, `new_mana` kann also durchaus `player.mana` entsprechen.
            let mana_geaendert = new_mana != player.mana;
            player.mana = new_mana;
            // §6/§39: Mana-Regeneration ändert persistierbare Ressourcen →
            // Komponente `Resources` dirty markieren (Stufe B). Ohne diese
            // Markierung nimmt der periodische Lauf einen mana-regenerierten
            // Spieler nicht auf; bei einem Abbruch bliebe der regenerierte
            // Stand ungesichert. `mark_dirty` erhöht zusätzlich die Generation.
            if mana_geaendert {
                player.mark_dirty(crate::persist::PersistComponent::Resources);
            }
            if new_mana == player.max_mana {
                player.mana_regen_carry = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn p(class: &str, level: u32, hp: i32, max_hp: i32, mana: i32, max_mana: i32) -> Player {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let status = crate::class::ClassStatus::from_db_name(class);
        Player {
            id: "t".into(),
            name: "t".into(),
            x: 0.0,
            y: 0.0,
            face: 0.0,
            ping_ms: 0,
            zone_id: 0,
            hp,
            max_hp,
            lang: "de".into(),
            account_id: 0,
            session_id: String::new(),
            entities: Default::default(),
            last_activity: std::time::Instant::now(),
            tx,
            char_class: class.into(),
            class: status,
            faction_transition: false,
            level,
            exp: 0,
            free_attr_points: 0,
            rested_pool: 0,
            idia: 0,
            armor: 0,
            weapon_skill: 1,
            combat: None,
            last_strike: None,
            sitting: false,
            mana,
            max_mana,
            attributes: Default::default(),
            max_hp_base: max_hp,
            max_mana_base: max_mana,
            hp_regen_bonus: 0.0,
            mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0,
            mana_regen_carry: 0.0,
            inventory: Default::default(),
            quests: Default::default(),
            dirty: Default::default(),
            persist_generation: 0,
            persist_revision: 0,
            effects: Vec::new(),
            cooldowns: Default::default(),
            active_cast: None,
            learned_abilities: Default::default(),
        }
    }

    fn in_combat(p: &mut Player) {
        p.combat = Some(crate::combat::CombatState {
            target_id: "ghost".into(),
            last_attack: std::time::Instant::now(),
        });
    }

    fn tick(p: &mut Player, ms: u64) {
        apply_regen(p, ms);
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // 7+8: Level-1-Basiswerte je Klasse (normale Regeneration, 100 %).
    #[test]
    fn class_base_values_level1() {
        assert_eq!(
            (
                effective_regen_per_sec("Kämpfer", 1, 0.0, false, false),
                effective_mana_regen_per_sec("Kämpfer", 1, 0.0, false, false)
            ),
            (6.0, 3.0)
        );
        assert_eq!(
            (
                effective_regen_per_sec("Magier", 1, 0.0, false, false),
                effective_mana_regen_per_sec("Magier", 1, 0.0, false, false)
            ),
            (3.0, 7.0)
        );
        assert_eq!(
            (
                effective_regen_per_sec("Priester", 1, 0.0, false, false),
                effective_mana_regen_per_sec("Priester", 1, 0.0, false, false)
            ),
            (4.0, 6.0)
        );
        assert_eq!(
            (
                effective_regen_per_sec("Kundschafter", 1, 0.0, false, false),
                effective_mana_regen_per_sec("Kundschafter", 1, 0.0, false, false)
            ),
            (5.0, 4.0)
        );
        // legacy-Namen (Warrior/Mage) liefern dieselben Werte.
        assert_eq!(
            effective_regen_per_sec("Warrior", 1, 0.0, false, false),
            ClassRegen::FIGHTER.hp
        );
        assert_eq!(
            effective_mana_regen_per_sec("Mage", 1, 0.0, false, false),
            ClassRegen::MAGE.mana
        );
    }

    // 8: Levelwachstum (Level 50 = +49 × 0,2 = +9,8; Level 1 exakt Basiswert).
    #[test]
    fn level_scaling() {
        // Kämpfer Level 50: (6 + 9,8) = 15,8 HP/s; Mana 3 + 9,8 = 12,8 Mana/s.
        assert!(approx(
            effective_regen_per_sec("Kämpfer", 50, 0.0, false, false),
            15.8
        ));
        assert!(approx(
            effective_mana_regen_per_sec("Kämpfer", 50, 0.0, false, false),
            12.8
        ));
        // Level 1: kein Zuwachs.
        assert_eq!(
            effective_regen_per_sec("Kämpfer", 1, 0.0, false, false),
            ClassRegen::FIGHTER.hp
        );
        // Level 11: +10 × 0,2 = +2,0.
        assert!(approx(
            effective_regen_per_sec("Kämpfer", 11, 0.0, false, false),
            8.0
        ));
    }

    // 5: im Kampf 15 % (sitzend oder stehend — identisch).
    #[test]
    fn combat_multiplier_15_pct() {
        assert!(approx(
            effective_regen_per_sec("Kämpfer", 1, 0.0, true, false),
            0.9
        ));
        assert!(approx(
            effective_mana_regen_per_sec("Kämpfer", 1, 0.0, true, false),
            0.45
        ));
        assert!(approx(
            effective_regen_per_sec("Kämpfer", 1, 0.0, true, true),
            0.9
        ));
    }

    // 3: sitzend außerhalb des Kampfes 125 %.
    #[test]
    fn sitting_outside_combat_125_pct() {
        assert!(approx(
            effective_regen_per_sec("Kämpfer", 1, 0.0, false, true),
            7.5
        ));
        assert!(approx(
            effective_mana_regen_per_sec("Kämpfer", 1, 0.0, false, true),
            3.75
        ));
    }

    // 4: im Kampf sitzend bleibt bei 15 % (kein Sitzbonus).
    #[test]
    fn sitting_in_combat_stays_15_pct() {
        assert!(approx(
            effective_regen_per_sec("Kämpfer", 1, 0.0, true, true),
            0.9
        ));
        assert!(approx(
            effective_mana_regen_per_sec("Kämpfer", 1, 0.0, true, true),
            0.45
        ));
    }

    // 6: additive Boni werden VOR dem Multiplikator addiert
    // (Beispiel aus §7: Basis 12 Mana/s + 4 Mana/s).
    #[test]
    fn additive_bonus_before_multiplier() {
        // Kundschafter Level 41: Basis-Mana 4,0 + 40 × 0,2 = 12,0 Mana/s.
        let base = effective_mana_regen_per_sec("Kundschafter", 41, 0.0, false, false);
        assert!(approx(base, 12.0));
        // Mit +4,0 Mana/s: 16,0 normal / 20,0 sitzend / 2,4 im Kampf.
        assert!(approx(
            effective_mana_regen_per_sec("Kundschafter", 41, 4.0, false, false),
            16.0
        ));
        assert!(approx(
            effective_mana_regen_per_sec("Kundschafter", 41, 4.0, false, true),
            20.0
        ));
        assert!(approx(
            effective_mana_regen_per_sec("Kundschafter", 41, 4.0, true, false),
            2.4
        ));
        // Algebra direkt: (12,0 + 4,0) × Zustandsmultiplikator.
        let r_normal = (12.0 + 4.0) * REGEN_MULT_NORMAL;
        let r_sitting = (12.0 + 4.0) * REGEN_MULT_SITTING;
        let r_combat = (12.0 + 4.0) * REGEN_MULT_COMBAT;
        assert!(approx(r_normal, 16.0) && approx(r_sitting, 20.0) && approx(r_combat, 2.4));
    }

    // 1: HP-Regeneration außerhalb des Kampfes läuft mit 100 %.
    // 2: Mana-Regeneration außerhalb des Kampfes läuft mit 100 %.
    #[test]
    fn regen_outside_combat_full_rate() {
        // Magier Level 1: HP 3/s, Mana 7/s. 100 ms → 0,3 / 0,7.
        let mut m = p("Magier", 1, 50, 100, 50, 100);
        tick(&mut m, 100);
        assert!(approx(m.hp_regen_carry, 0.3));
        assert!(approx(m.mana_regen_carry, 0.7));
        // Nach 1000 ms: ganze 3 HP bzw. 7 Mana.
        for _ in 0..9 {
            tick(&mut m, 100);
        }
        assert_eq!(m.hp, 53);
        assert_eq!(m.mana, 57);
        assert!(m.hp_regen_carry.abs() < 1e-9);
        assert!(m.mana_regen_carry.abs() < 1e-9);
    }

    // 9: HP wird bei Max-HP gedeckelt (Carry wird nicht aufgebraucht).
    // 10: Mana wird bei Max-Mana gedeckelt.
    #[test]
    fn caps_at_max_hp_and_max_mana() {
        let mut m = p("Kämpfer", 1, 99, 100, 49, 100);
        tick(&mut m, 100); // 0,6 HP → bleibt Carry
        assert_eq!(m.hp, 99);
        tick(&mut m, 100); // 1,2 HP → +1
        assert_eq!(m.hp, 100);
        tick(&mut m, 100); // voller Pool → kein Zuwachs, kein Carry
        assert_eq!(m.hp, 100);
        assert!(m.hp_regen_carry.abs() < 1e-9);

        let mut n = p("Kämpfer", 1, 100, 100, 99, 100);
        tick(&mut n, 100); // 0,3 Mana → Carry, Mana unverändert
        assert_eq!(n.mana, 99);
        tick(&mut n, 100); // 0,6 Mana → Carry, Mana unverändert
        assert_eq!(n.mana, 99);
        tick(&mut n, 100); // 0,9 Mana → Carry, Mana unverändert
        assert_eq!(n.mana, 99);
        tick(&mut n, 100); // 1,2 Mana → +1 → 100 (Max-Mana, Carry 0)
        assert_eq!(n.mana, 100);
        assert!(n.mana_regen_carry.abs() < 1e-9);
    }

    // 11: tote Charaktere werden nicht durch HP-Regeneration reaktiviert.
    #[test]
    fn dead_character_not_resurrected() {
        let mut d = p("Kämpfer", 1, 0, 100, 50, 100);
        tick(&mut d, 1000);
        assert_eq!(d.hp, 0);
        tick(&mut d, 10000);
        assert_eq!(d.hp, 0);
    }

    // 12: Delta-/Tick-Raten sind unabhängig (2×250 ms == 1×500 ms).
    #[test]
    fn tick_rate_independent() {
        let mut a = p("Kämpfer", 1, 90, 100, 40, 100);
        for _ in 0..2 {
            tick(&mut a, 250);
        }
        let mut b = p("Kämpfer", 1, 90, 100, 40, 100);
        tick(&mut b, 500);
        assert_eq!(a.hp, b.hp);
        assert_eq!(a.mana, b.mana);
        assert!(approx(a.hp_regen_carry, b.hp_regen_carry));
        assert!(approx(a.mana_regen_carry, b.mana_regen_carry));
    }

    // 13: dezimale Raten ohne vorgezogenes Runden über viele Ticks.
    /// `P-17`: Mana-Regeneration markiert `Resources` und schreibt die
    /// Generation fort — ueber den **echten** Regenerationspfad
    /// (`apply_regen`), nicht ueber ein nachgebautes `mark_dirty`.
    #[test]
    fn mana_regen_marks_resources_dirty_and_raises_generation() {
        let mut pl = p("Adventurer", 5, 50, 100, 10, 100);
        assert!(!pl.dirty.any(), "Ausgangszustand: sauber");
        assert_eq!(pl.persist_generation, 0);
        let mana_vorher = pl.mana;

        tick(&mut pl, 5_000);

        assert!(
            pl.mana > mana_vorher,
            "Mana muss regeneriert sein, war {mana_vorher}, jetzt {}",
            pl.mana
        );
        assert!(
            pl.dirty.is_dirty(crate::persist::PersistComponent::Resources),
            "Mana-Regeneration muss Resources dirty markieren"
        );
        assert!(
            pl.persist_generation > 0,
            "mark_dirty muss die Generation fortschreiben"
        );
    }

    /// `P-17`: Der regenerierte Mana-Wert erreicht den Snapshot. Der Test
    /// laeuft ueber den echten Persistenzlauf (`persist_dirty_into`) und
    /// prueft den Snapshot-Inhalt.
    #[tokio::test]
    async fn mana_regen_makes_the_player_reachable_for_the_persist_run() {
        let shared = crate::world::new_shared();
        let mut pl = p("Adventurer", 5, 50, 100, 10, 100);
        let mana_vorher = pl.mana;
        tick(&mut pl, 5_000);
        let mana_nachher = pl.mana;
        assert!(mana_nachher > mana_vorher, "Voraussetzung: Mana regeneriert");
        {
            let mut w = shared.lock().await;
            w.players.insert(pl.id.clone(), pl);
        }

        let erfasst = Arc::new(Mutex::new(None));
        let erfasst2 = erfasst.clone();
        let res = crate::persist::persist_dirty_into(&shared, "t", false, move |snapshot| {
            let erfasst2 = erfasst2.clone();
            async move {
                *erfasst2.lock().unwrap() = Some(snapshot.mana);
                Ok(())
            }
        })
        .await;
        assert!(res.is_ok(), "Persistenzlauf muss den Spieler aufnehmen");
        let gespeichert = erfasst.lock().unwrap().expect("Snapshot muss erfasst sein");
        assert_eq!(
            gespeichert, mana_nachher,
            "Snapshot muss den regenerierten Mana-Wert enthalten"
        );
    }

    /// `P-17`: Keine Aenderung -> keine Dirty-Markierung, keine
    /// Generationserhoehung. Zwei Faelle ueber den echten Pfad `apply_regen`:
    /// volle Pools (die Regenerationszweige werden gar nicht betreten) und
    /// ein reiner Carry-Bruchteil ohne volle Einheit (`whole == 0`).
    #[test]
    fn unchanged_mana_marks_nothing_dirty() {
        // Volle HP- und Mana-Pools: `hp < max_hp` und `mana < max_mana` sind
        // beide nicht erfuellt, es wird nichts veraendert und nichts markiert.
        let mut pl = p("Adventurer", 5, 100, 100, 100, 100);
        assert!(!pl.dirty.any());
        tick(&mut pl, 5_000);
        assert_eq!(pl.mana, 100, "voller Pool darf nicht veraendert werden");
        assert_eq!(pl.hp, 100);
        assert!(!pl.dirty.any(), "unveraenderte Poolwerte markieren nichts");
        assert_eq!(pl.persist_generation, 0, "keine Generationserhoehung");

        // Carry-Bruchteil: `mana_regen_carry` sammelt, aber `whole == 0`, damit
        // bleibt `mana` unveraendert und `mark_dirty` wird nicht erreicht.
        let mut pl2 = p("Adventurer", 5, 100, 100, 10, 100);
        tick(&mut pl2, 1);
        assert_eq!(pl2.mana, 10, "ein Bruchteil regeneriert nicht ganzzahlig");
        assert!(
            !pl2.dirty.is_dirty(crate::persist::PersistComponent::Resources),
            "ohne Aenderung keine Resources-Markierung"
        );
        assert_eq!(
            pl2.persist_generation, 0,
            "ohne Aenderung keine Generationserhoehung"
        );
    }

    /// `P-17`: Bei gleichzeitiger HP- UND Mana-Aenderung wird `Resources`
    /// zweimal markiert. Das ist mit der bestehenden Semantik vereinbar: das
    /// Bit bleibt gesetzt, die Generation steigt zweimal. Der Test haelt die
    /// gegenwaertige, nicht optimierte Semantik fest.
    #[test]
    fn simultaneous_hp_and_mana_regen_mark_resources_twice() {
        let mut pl = p("Adventurer", 5, 50, 100, 10, 100);
        tick(&mut pl, 5_000);
        assert!(pl.hp > 50 && pl.mana > 10, "beide Pool regeneriert");
        assert!(pl.dirty.is_dirty(crate::persist::PersistComponent::Resources));
        // HP-Pfad und Mana-Pfad markieren jeweils ueber `mark_dirty`.
        assert_eq!(
            pl.persist_generation, 2,
            "zwei Markierungen -> Generationsstand 2"
        );
        // Das Bit ist unveraendert gesetzt.
        assert!(pl.dirty.is_dirty(crate::persist::PersistComponent::Resources));
    }

    #[test]
    fn decimal_precision_no_premature_rounding() {
        // Magier Level 1: 3,0 HP/s → 0,3 HP pro 100 ms. Über 7 Ticks müssten
        // bei Ganzzahl-Aufsummierung (0,3 → 0 je Tick) keine HP entstehen;
        // mit Carry entstehen exakt 2 HP (3 × 0,3 = 0,9 → 2 nach 7× 0,3 = 2,1).
        let mut m = p("Magier", 1, 50, 100, 50, 100);
        for _ in 0..7 {
            tick(&mut m, 100);
        }
        assert_eq!(m.hp, 52, "Carry darf keinen Dezimal-Schmelzverlust");
        // Mana 7/s → 0,7 pro 100 ms: 7 Ticks = 4,9 → 4 Mana.
        assert_eq!(m.mana, 54);
    }
}
