// progression — Charakter-EXP- und Level-Progressionssystem V1.
// Verbindliche Spezifikation: docs/Erfahrung_und_Progressionssystem.md.
// Nur reine Berechnungslogik — keine DB-/Netz-/Tick-Abhängigkeiten. Diese
// Datei ist die zentrale EINZIGE Anwendung von EXP-Kurve, Level-Cap,
// Levelaufstieg, Attributpunkten, Leveldifferenz und Rested-EXP (keine
// Duplikation in Handlern).
//
// Interpretationsentscheidung: `exp_needed(level)` bezieht sich auf das
// AKTUELLE Charakterlevel (Aufwand, um level → level+1 zu erreichen);
// die Doku (§12.3) nennt die Anforderung "für sein aktuelles Level".
// Die Kurvenwerte base/factor sind Balancing (Doku §3: "wird erst durch
// Tests festgelegt") — Defaults provisorisch, per Config übersteuerbar.
#![allow(dead_code)]

use crate::class::ClassStatus;

/// Leveldifferenz-Tabelle (docs/Erfahrung_und_Progressionssystem.md §7):
/// Promille-Anteile der Monster-EXP je Level-Differenz (Gegner − Charakter).
/// Die Prozentwerte sind Balancing und per Config übersteuerbar; die
/// Zonengrenzen (5+, 2–4, ±1, −2…−3, −4…−5, −6…−9, −10+) sind fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelDiffCfg {
    /// Gegner ist 5 oder mehr Level höher als der Charakter (125 %).
    pub plus_5_permille: u32,
    /// Gegner 2–4 Level höher (110 %).
    pub plus_2_to_4_permille: u32,
    /// Leveldifferenz ±1 (100 %).
    pub same_zone_permille: u32,
    /// Gegner 2–3 Level niedriger (75 %).
    pub minus_2_to_3_permille: u32,
    /// Gegner 4–5 Level niedriger (50 %).
    pub minus_4_to_5_permille: u32,
    /// Gegner 6–9 Level niedriger (25 %).
    pub minus_6_to_9_permille: u32,
    /// Gegner 10 oder mehr Level niedriger (0 %).
    pub minus_10_permille: u32,
}

impl Default for LevelDiffCfg {
    fn default() -> Self {
        LevelDiffCfg {
            plus_5_permille: 1250,
            plus_2_to_4_permille: 1100,
            same_zone_permille: 1000,
            minus_2_to_3_permille: 750,
            minus_4_to_5_permille: 500,
            minus_6_to_9_permille: 250,
            minus_10_permille: 0,
        }
    }
}

/// Konfiguration des Progressionssystems (docs/Erfahrung_und_Progressions
/// system.md §§3/4/7). Defaults sind provisorische Balancingwerte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressionCfg {
    /// Basiswert der EXP-Kurve exp_needed(level) = base + factor · level².
    pub exp_base: u64,
    /// Faktor der EXP-Kurve (level² gewichtet).
    pub exp_factor: u64,
    /// Aktives Level-Cap (§4): Core 40, Erweiterung 50 — per Config.
    pub level_cap: u32,
    pub level_diff: LevelDiffCfg,
}

impl Default for ProgressionCfg {
    fn default() -> Self {
        ProgressionCfg {
            exp_base: 100,
            exp_factor: 10,
            level_cap: 40,
            level_diff: LevelDiffCfg::default(),
        }
    }
}

/// EXP-Anforderung für das AKTUELLE Level (kurve: base + factor · level²).
/// level 0 existiert nicht; die Anforderung für das Cap-Level wird nie
/// abgefragt (Aufstieg endet am Cap).
pub fn exp_needed(cfg: &ProgressionCfg, level: u32) -> i64 {
    let l = i64::from(level);
    let square = l.saturating_mul(l);
    (cfg.exp_base as i64)
        .saturating_add((cfg.exp_factor as i64).saturating_mul(square))
}

/// Attributpunkte beim Erreichen eines Levels (docs/Erfahrung_und_
/// Progressionssystem.md §5): L1 gibt keine, normale Level 5, jedes
/// zehnte Level (10/20/30/…) stattdessen 10.
pub fn attr_points_for_reaching_level(level: u32) -> u32 {
    if level <= 1 {
        0
    } else if level % 10 == 0 {
        10
    } else {
        5
    }
}

/// Promille-Anteil der Monster-EXP gemäß Leveldifferenz
/// (docs/Erfahrung_und_Progressionssystem.md §7): Gegnerlevel − Charakterlevel.
pub fn level_diff_permille(cfg: &ProgressionCfg, monster_level: u32, char_level: u32) -> u32 {
    let diff = i64::from(monster_level) - i64::from(char_level);
    let t = &cfg.level_diff;
    match diff {
        d if d >= 5 => t.plus_5_permille,
        d if d >= 2 => t.plus_2_to_4_permille,
        d if d >= -1 => t.same_zone_permille,
        d if d >= -3 => t.minus_2_to_3_permille,
        d if d >= -5 => t.minus_4_to_5_permille,
        d if d >= -9 => t.minus_6_to_9_permille,
        _ => t.minus_10_permille,
    }
}

/// Rested-EXP (docs/Erfahrung_und_Progressionssystem.md §12): Offlinezeit
/// baut bis zu 50 % der AKTUELLEN Level-Anforderung auf, 10 % pro 24 h.
/// Deterministisch proportional (i128-Intermedär, kein Float-Rauschen):
///    gain = need · elapsed_secs / 864_000  →  exakt 10 %/24 h und damit
///    volle Füllung nach 5 Tagen (need·5·86400/864000 = need/2 = Maximum).
/// Wird nur beim Login einmalig angewendet; keine periodische
/// Offline-Verarbeitung. `logout_at` = gespeicherter Logout-Epoch-Sekunden.
pub fn apply_offline_rested(
    cfg: &ProgressionCfg,
    level: u32,
    pool: i64,
    logout_at: Option<i64>,
    now_secs: i64,
) -> i64 {
    let pool = pool.max(0);
    let Some(last) = logout_at else {
        return pool;
    };
    if last <= 0 || now_secs <= last {
        return pool;
    }
    let need = exp_needed(cfg, level);
    let max_pool = need / 2;
    let elapsed = now_secs - last;
    let gain = if need > 0 || elapsed > 0 {
        ((need as i128 * elapsed as i128) / 864_000) as i64
    } else {
        0
    };
    pool.saturating_add(gain).min(max_pool.max(0))
}

/// Ergebnis einer EXP-Gutschrift (nur Beobachtung; Zustand steckt im
/// `Progression`-Struct).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantOutcome {
    pub levels_gained: u32,
    pub final_level: u32,
    /// Normal ausgeschüttete EXP (Anteil ohne Rested-Bonus).
    pub plain_exp: i64,
    /// Tatsächlich gewährter Rested-Bonus (= Poolverzehr).
    pub rested_bonus: i64,
    pub free_attr_points_gained: u32,
}

/// Reiner Progressions-Zustand eines Charakters (Mapping von/wird auf
/// world::Player/attributes in world.rs vorgenommen). Enthält alle Felder,
/// die die Progressionsregeln benötigen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progression {
    pub level: u32,
    pub exp: i64,
    pub free_attr_points: u32,
    pub rested_pool: i64,
    pub class: ClassStatus,
    pub faction_transition: bool,
}

impl Progression {
    pub fn new(
        level: u32,
        exp: i64,
        free_attr_points: u32,
        rested_pool: i64,
        class: ClassStatus,
        faction_transition: bool,
    ) -> Self {
        Progression {
            level,
            exp,
            free_attr_points,
            rested_pool,
            class,
            faction_transition,
        }
    }

    /// Effektive Kappe (Level-Cap §4 ODER Klassen-L10-Regel im neutralen
    /// Startgebiet, docs/Klassensystem.md): an der Kappe wird keine EXP
    /// angenommen und nichts "vorgespart" (§4). Die Klassen-L10-Regel wird
    /// über den bestehenden Hook class::progression_in_neutral_zone
    /// eingebunden und erzeugt hier eine effektive Kappe, an der genauso
    /// verworfen wird (kein versteckter EXP-Speicher).
    fn is_effectively_capped(&self, cfg: &ProgressionCfg) -> bool {
        if self.level >= cfg.level_cap {
            return true;
        }
        let (can_level_up, _) = self
            .class
            .progression_in_neutral_zone(self.level, self.faction_transition);
        !can_level_up
    }

    /// Normale EXP-Gutschrift (Quest/Entdeckung, §§8–9): kein Rested-Bonus.
    pub fn grant_exp(&mut self, cfg: &ProgressionCfg, amount: i64) -> GrantOutcome {
        self.add_exp_and_level_up(cfg, amount, 0)
    }

    /// Kill-EXP (§7): wendet den Rested-Bonus (max. +50 % des Anteils,
    /// gedeckelt am Pool) an und verarbeitet den Levelaufstieg. Nur diese
    /// Quelle zieht Rested ab (docs/Erfahrung_und_Progressionssystem.md §12).
    pub fn grant_kill_exp(&mut self, cfg: &ProgressionCfg, amount: i64) -> GrantOutcome {
        if self.is_effectively_capped(cfg) || amount <= 0 {
            return GrantOutcome {
                levels_gained: 0,
                final_level: self.level,
                plain_exp: 0,
                rested_bonus: 0,
                free_attr_points_gained: 0,
            };
        }
        let bonus = self.rested_pool.max(0).min(amount / 2);
        self.rested_pool = self.rested_pool.max(0) - bonus;
        self.add_exp_and_level_up(cfg, amount, bonus)
    }

    /// Gemeinsamer Kern: Gutschrift + Levelaufstiege inkl. Cap.
    /// Deterministisch, integer-basiert (kein Runden am Pool).
    fn add_exp_and_level_up(&mut self, cfg: &ProgressionCfg, amount: i64, rested_bonus: i64) -> GrantOutcome {
        if self.is_effectively_capped(cfg) {
            // An der Kappe (§4): keine Annahme, kein Vorsparen, keine Punkte.
            self.exp = 0;
            return GrantOutcome {
                levels_gained: 0,
                final_level: self.level,
                plain_exp: 0,
                rested_bonus: 0,
                free_attr_points_gained: 0,
            };
        }
        self.exp = self.exp.saturating_add(amount.saturating_add(rested_bonus));

        let mut levels_gained = 0u32;
        let mut points_gained = 0u32;
        while self.level < cfg.level_cap {
            let (can_level_up, _) = self
                .class
                .progression_in_neutral_zone(self.level, self.faction_transition);
            if !can_level_up {
                break;
            }
            let needed = exp_needed(cfg, self.level);
            if self.exp < needed {
                break;
            }
            self.exp -= needed;
            self.level += 1;
            levels_gained += 1;
            let pts = attr_points_for_reaching_level(self.level);
            points_gained += pts;
            self.free_attr_points += pts;
            if self.level == cfg.level_cap {
                // §4: Überschuss wird an der Kappe verworfen.
                self.exp = 0;
                break;
            }
        }
        // Am Ende an der Klassen-L10-Kappe gelaufen (Grundklasse ohne
        // Fraktions-Übergang)? Dann verwerfen — kein versteckter EXP-Speicher.
        if self.is_effectively_capped(cfg) {
            self.exp = 0;
        }
        GrantOutcome {
            levels_gained,
            final_level: self.level,
            plain_exp: amount,
            rested_bonus,
            free_attr_points_gained: points_gained,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(base: u64, factor: u64) -> ProgressionCfg {
        ProgressionCfg {
            exp_base: base,
            exp_factor: factor,
            ..ProgressionCfg::default()
        }
    }

    fn adventurer(level: u32, exp: i64) -> Progression {
        Progression::new(level, exp, 0, 0, ClassStatus::Adventurer, false)
    }

    // ── §3: EXP-Kurve ───────────────────────────────────────────────────

    #[test]
    fn curve_matches_formula() {
        let c = cfg(100, 10);
        assert_eq!(exp_needed(&c, 1), 110);
        assert_eq!(exp_needed(&c, 2), 140);
        assert_eq!(exp_needed(&c, 3), 190);
        assert_eq!(exp_needed(&c, 10), 1100);
        assert_eq!(exp_needed(&c, 39), 15310);
    }

    #[test]
    fn curve_uses_config_values() {
        let c = cfg(0, 25);
        assert_eq!(exp_needed(&c, 4), 400);
        assert_eq!(exp_needed(&c, 5), 625);
    }

    #[test]
    fn curve_monotonic_in_level() {
        let c = cfg(100, 10);
        for l in 1..39 {
            assert!(exp_needed(&c, l) < exp_needed(&c, l + 1));
        }
    }

    // ── Levelaufstieg (§4/§6) ───────────────────────────────────────────

    #[test]
    fn single_level_up_with_leniency() {
        let mut p = adventurer(1, 100);
        let c = cfg(100, 10);
        let out = p.grant_exp(&c, 10);
        assert_eq!(out.levels_gained, 1);
        assert_eq!(p.level, 2);
        assert_eq!(p.exp, 0);
        assert_eq!(out.free_attr_points_gained, 5);
        assert_eq!(p.free_attr_points, 5);
    }

    #[test]
    fn multi_level_up() {
        let c = cfg(100, 10);
        let need_1 = exp_needed(&c, 1); // 110
        let need_2 = exp_needed(&c, 2); // 140
        let mut p = adventurer(1, 0);
        let out = p.grant_exp(&c, need_1 + need_2);
        assert_eq!(out.levels_gained, 2);
        assert_eq!(p.level, 3);
        assert_eq!(p.exp, 0);
        assert_eq!(out.free_attr_points_gained, 10); // 5+5
        assert_eq!(p.free_attr_points, 10);
    }

    #[test]
    fn remainder_exp_stays_in_current_level() {
        let c = cfg(100, 10);
        let mut p = adventurer(1, 0);
        let out = p.grant_exp(&c, 130);
        assert_eq!(out.levels_gained, 1);
        assert_eq!(p.level, 2);
        assert_eq!(p.exp, 20); // 130 − 110
    }

    #[test]
    fn level_up_never_exceeds_cap_and_discards_surplus() {
        let c = ProgressionCfg {
            exp_base: 100,
            exp_factor: 10,
            level_cap: 3,
            ..ProgressionCfg::default()
        };
        let mut p = adventurer(1, 0);
        let out = p.grant_exp(&c, 1_000_000);
        assert_eq!(out.levels_gained, 2);
        assert_eq!(p.level, 3);
        assert_eq!(p.exp, 0);
        assert_eq!(out.free_attr_points_gained, 10); // 5 (L2) + 5 (L3)
    }

    #[test]
    fn every_10th_level_grants_10_points_and_multijump_accumulates() {
        let c = cfg(100, 10);
        // L9 → Überspringen des 10. Levels.
        let mut p = adventurer(9, 0);
        let need_9 = exp_needed(&c, 9); // 910
        let need_10 = exp_needed(&c, 10); // 1100
        let out = p.grant_exp(&c, need_9 + need_10);
        assert_eq!(out.levels_gained, 2);
        assert_eq!(p.level, 11);
        assert_eq!(out.free_attr_points_gained, 15); // L10 → 10, L11 → 5
    }

    #[test]
    fn points_table() {
        assert_eq!(attr_points_for_reaching_level(1), 0);
        assert_eq!(attr_points_for_reaching_level(2), 5);
        assert_eq!(attr_points_for_reaching_level(9), 5);
        assert_eq!(attr_points_for_reaching_level(10), 10);
        assert_eq!(attr_points_for_reaching_level(19), 5);
        assert_eq!(attr_points_for_reaching_level(20), 10);
        assert_eq!(attr_points_for_reaching_level(40), 10);
    }

    #[test]
    fn no_exp_when_already_at_cap() {
        let c = ProgressionCfg {
            exp_base: 100,
            exp_factor: 10,
            level_cap: 3,
            ..ProgressionCfg::default()
        };
        let mut p = adventurer(3, 77);
        let out = p.grant_exp(&c, 500);
        assert_eq!(out.levels_gained, 0);
        assert_eq!(out.plain_exp, 0);
        assert_eq!(p.level, 3);
        assert_eq!(p.exp, 0); // verworfen, nicht vorgespart
    }

    // ── Klassen-L10-Regel im neutralen Startgebiet ──────────────────────

    #[test]
    fn base_class_blocked_at_neutral_zone_level_10_without_faction() {
        let c = cfg(100, 10);
        let mut p = Progression::new(10, 0, 0, 0, ClassStatus::Fighter, false);
        let out = p.grant_exp(&c, 1_000_000);
        assert_eq!(out.levels_gained, 0);
        assert_eq!(p.level, 10);
        assert_eq!(p.exp, 0); // verworfen — kein Vorsparen vor Fraktionswahl
        assert_eq!(out.free_attr_points_gained, 0);
    }

    #[test]
    fn base_class_can_reach_level_10_then_no_more() {
        let c = cfg(100, 10);
        let mut p = Progression::new(9, 0, 0, 0, ClassStatus::Fighter, false);
        let need_9 = exp_needed(&c, 9);
        let out = p.grant_exp(&c, need_9 + 100_000);
        assert_eq!(out.levels_gained, 1); // L9 → L10, danach blockiert
        assert_eq!(p.level, 10);
        assert_eq!(p.exp, 0);
        assert_eq!(out.free_attr_points_gained, 10); // L10 = 10 Punkte
    }

    #[test]
    fn base_class_levels_normally_below_neutral_cap() {
        let c = cfg(100, 10);
        let mut p = Progression::new(9, 0, 0, 0, ClassStatus::Fighter, false);
        let out = p.grant_exp(&c, exp_needed(&c, 9));
        assert_eq!(out.levels_gained, 1);
        assert_eq!(p.level, 10);
    }

    #[test]
    fn adventurer_not_blocked_in_neutral_zone() {
        let c = cfg(100, 10);
        let mut p = adventurer(10, 0);
        let out = p.grant_exp(&c, exp_needed(&c, 10));
        assert_eq!(out.levels_gained, 1);
        assert_eq!(p.level, 11);
    }

    #[test]
    fn faction_transition_opens_base_class_above_10() {
        let c = cfg(100, 10);
        let mut p = Progression::new(10, 0, 0, 0, ClassStatus::Fighter, true);
        let out = p.grant_exp(&c, exp_needed(&c, 10));
        assert_eq!(out.levels_gained, 1);
        assert_eq!(p.level, 11);
    }

    // ── §7: Leveldifferenz ──────────────────────────────────────────────

    #[test]
    fn level_diff_all_zones_defaults() {
        let mut dc = LevelDiffCfg::default();
        // Eindeutig unterscheidbare Promille für Zone-Mapping.
        dc.plus_5_permille = 5000;
        dc.plus_2_to_4_permille = 4000;
        dc.same_zone_permille = 3000;
        dc.minus_2_to_3_permille = 2000;
        dc.minus_4_to_5_permille = 1000;
        dc.minus_6_to_9_permille = 500;
        dc.minus_10_permille = 0;
        let c = ProgressionCfg {
            level_diff: dc,
            ..cfg(100, 10)
        };
        assert_eq!(level_diff_permille(&c, 15, 10), 5000); // +5
        assert_eq!(level_diff_permille(&c, 14, 10), 4000); // +4
        assert_eq!(level_diff_permille(&c, 12, 10), 4000); // +2
        assert_eq!(level_diff_permille(&c, 11, 10), 3000); // +1
        assert_eq!(level_diff_permille(&c, 10, 10), 3000); // 0
        assert_eq!(level_diff_permille(&c, 9, 10), 3000); // −1
        assert_eq!(level_diff_permille(&c, 8, 10), 2000); // −2
        assert_eq!(level_diff_permille(&c, 7, 10), 2000); // −3
        assert_eq!(level_diff_permille(&c, 6, 10), 1000); // −4
        assert_eq!(level_diff_permille(&c, 5, 10), 1000); // −5
        assert_eq!(level_diff_permille(&c, 4, 10), 500); // −6
        assert_eq!(level_diff_permille(&c, 1, 10), 500); // −9
        assert_eq!(level_diff_permille(&c, 0, 10), 0); // −10 und darunter
    }

    #[test]
    fn level_diff_default_permilles() {
        let c = ProgressionCfg::default();
        assert_eq!(level_diff_permille(&c, 15, 10), 1250);
        assert_eq!(level_diff_permille(&c, 12, 10), 1100);
        assert_eq!(level_diff_permille(&c, 10, 10), 1000);
        assert_eq!(level_diff_permille(&c, 7, 10), 750);
        assert_eq!(level_diff_permille(&c, 5, 10), 500);
        assert_eq!(level_diff_permille(&c, 2, 10), 250);
        assert_eq!(level_diff_permille(&c, 0, 10), 0);
    }

    #[test]
    fn diff_boundaries_exact() {
        let c = ProgressionCfg::default();
        assert_eq!(level_diff_permille(&c, 15, 10), 1250); // +5 Grenze
        assert_eq!(level_diff_permille(&c, 14, 10), 1100); // +4 Grenze
        assert_eq!(level_diff_permille(&c, 11, 10), 1000); // +1
        assert_eq!(level_diff_permille(&c, 9, 10), 1000); // −1 Grenze
        assert_eq!(level_diff_permille(&c, 7, 10), 750); // −3 Grenze
        assert_eq!(level_diff_permille(&c, 5, 10), 500); // −5 Grenze
        assert_eq!(level_diff_permille(&c, 1, 10), 250); // −9 Grenze
        assert_eq!(level_diff_permille(&c, 5, 15), 0); // Gegner 10 niedriger: −10 → 0 %
    }

    // ── §7: Gruppen-EXP (Multiplikator, Teilung, gemischte Level) ───────

    #[test]
    fn group_total_uses_highest_eligible_level() {
        let c = ProgressionCfg::default();
        // L20+L30 erlegen L25 (Berechnungslevel 30, Gegner 5 niedriger → 50 %).
        let permille = level_diff_permille(&c, 25, 30);
        assert_eq!(permille, 500);
        let total = 100 * i64::from(permille) / 1000;
        assert_eq!(total, 50);
        let amounts = crate::group::split_exp_equally(total, 2);
        assert_eq!(amounts, vec![25, 25]);
    }

    #[test]
    fn group_has_no_bonus() {
        let c = ProgressionCfg::default();
        // Gleiche Level (monster 10, char 10 → 100 %), exp_reward 100.
        let total = 100 * i64::from(level_diff_permille(&c, 10, 10)) / 1000;
        assert_eq!(total, 100);
        let amounts = crate::group::split_exp_equally(total, 4);
        assert_eq!(amounts, vec![25, 25, 25, 25]);
    }

    #[test]
    fn split_remainder_is_deterministic() {
        let c = ProgressionCfg::default();
        let total = 1000 * i64::from(level_diff_permille(&c, 12, 10)) / 1000;
        assert_eq!(total, 1100);
        let amounts = crate::group::split_exp_equally(total, 3);
        assert_eq!(amounts, vec![367, 367, 366]);
    }

    #[test]
    fn solo_gets_full_share() {
        let c = ProgressionCfg::default();
        let total = 25 * i64::from(level_diff_permille(&c, 14, 12)) / 1000; // +2 → 110 %
        assert_eq!(total, 27);
        assert_eq!(crate::group::split_exp_equally(total, 1), vec![27]);
    }

    // ── §12: Rested EXP ─────────────────────────────────────────────────

    /// Anforderung 10000 bei Level 10 (base 0, factor 100 → 0 + 100·100).
    fn rested_cfg() -> ProgressionCfg {
        cfg(0, 100)
    }

    #[test]
    fn rested_max_is_half_of_level_requirement() {
        let c = rested_cfg();
        assert_eq!(exp_needed(&c, 10), 10000);
        let p = adventurer(10, 0);
        // Voll aufgefüllt nach exakt 5 Tagen Offlinezeit.
        let full = apply_offline_rested(&c, p.level, 0, Some(100_000), 100_000 + 5 * 86400);
        assert_eq!(full, 5000);
        assert!(full <= exp_needed(&c, 10) / 2);
    }

    #[test]
    fn rested_builds_10_percent_per_24h() {
        let c = rested_cfg();
        let after_24h = apply_offline_rested(&c, 10, 0, Some(1_000_000), 1_000_000 + 86400);
        assert_eq!(after_24h, 1000);
        let after_12h = apply_offline_rested(&c, 10, 0, Some(1_000_000), 1_000_000 + 43200);
        assert_eq!(after_12h, 500);
        let after_5days = apply_offline_rested(&c, 10, 0, Some(1_000_000), 1_000_000 + 5 * 86400);
        assert_eq!(after_5days, 5000);
    }

    #[test]
    fn rested_caps_at_maximum() {
        let c = rested_cfg();
        // Bereits 4000 im Pool + 5 Tage (Gain 5000) → Deckel 5000.
        let out = apply_offline_rested(&c, 10, 4000, Some(100_000), 100_000 + 5 * 86400);
        assert_eq!(out, 5000);
    }

    #[test]
    fn rested_ignores_missing_or_future_timestamp() {
        let c = rested_cfg();
        assert_eq!(apply_offline_rested(&c, 10, 123, None, 5000), 123);
        assert_eq!(apply_offline_rested(&c, 10, 123, Some(9_999), 5_000), 123);
    }

    #[test]
    fn rested_deterministic() {
        let c = rested_cfg();
        let a = apply_offline_rested(&c, 10, 0, Some(100), 100 + 3 * 86400 + 3600);
        let b = apply_offline_rested(&c, 10, 0, Some(100), 100 + 3 * 86400 + 3600);
        assert_eq!(a, b);
    }

    #[test]
    fn kill_exp_multiplies_by_rested_bonus_and_consumes_pool() {
        let c = rested_cfg();
        let mut p = adventurer(10, 0);
        p.rested_pool = 5000;
        let out = p.grant_kill_exp(&c, 1000);
        assert_eq!(out.rested_bonus, 500); // 50 % des Anteils
        assert_eq!(out.plain_exp, 1000);
        assert_eq!(p.rested_pool, 4500);
        assert_eq!(p.exp, 1500);
    }

    #[test]
    fn rested_bonus_limited_by_pool() {
        let c = rested_cfg();
        let mut p = adventurer(10, 0);
        p.rested_pool = 200; // weniger als 50 % des Anteils
        let out = p.grant_kill_exp(&c, 1000);
        assert_eq!(out.rested_bonus, 200);
        assert_eq!(p.rested_pool, 0);
        assert_eq!(p.exp, 1200);
    }

    #[test]
    fn rested_pool_never_depleted_for_plain_exp() {
        let c = rested_cfg();
        let mut p = adventurer(10, 0);
        p.rested_pool = 5000;
        p.grant_exp(&c, 1000); // Quest/Entdeckung
        assert_eq!(p.rested_pool, 5000);
        assert_eq!(p.exp, 1000);
    }

    #[test]
    fn rested_uses_current_level_requirement_after_level_up() {
        let c = rested_cfg();
        let mut p = adventurer(10, 0);
        p.rested_pool = 5000;
        // Kill schiebt über L10 hinaus: need(10)=10000, need(11)=12100.
        let out = p.grant_kill_exp(&c, 10_000 + 1_000);
        assert_eq!(out.levels_gained, 1);
        assert_eq!(p.level, 11);
        assert_eq!(p.exp, 6_000); // 16 000 gesamt − 10 000 (L10→11)
        // Bonus: min(Pool 5000, Anteil/2 5500) → kompletter Poolverzehr.
        assert_eq!(out.rested_bonus, 5000);
        assert_eq!(p.rested_pool, 0);
        // Neues Maximum nutzt das NEUE Level (kein Umrechnen des Pools).
        assert_eq!(exp_needed(&c, 11) / 2, 6050);
    }

    #[test]
    fn rested_not_consumed_when_capped() {
        let c = ProgressionCfg {
            exp_base: 100,
            exp_factor: 10,
            level_cap: 10,
            ..ProgressionCfg::default()
        };
        let mut p = adventurer(10, 0);
        p.rested_pool = 3000;
        let out = p.grant_kill_exp(&c, 500);
        assert_eq!(out.rested_bonus, 0);
        assert_eq!(out.plain_exp, 0);
        assert_eq!(p.rested_pool, 3000);
        assert_eq!(p.exp, 0);
    }

    #[test]
    fn grant_outcome_reports_attributes() {
        let c = cfg(100, 10);
        let mut p = adventurer(1, 0);
        let out = p.grant_exp(&c, exp_needed(&c, 1) + 10);
        assert_eq!(out.levels_gained, 1);
        assert_eq!(out.final_level, 2);
        assert_eq!(out.free_attr_points_gained, 5);
    }
}