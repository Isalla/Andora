// class — Zyklische Klassenbasis (Realm-autoritativ).
//
// Umgesetzt nach docs/Klassensystem.md:
// - Jeder neue Charakter beginnt als Abenteurer (Adventurer).
// - Tutorial-Phasen Level 1–8: eine temporäre Grundklasse wird erlebt;
//   der Charakter bleibt technisch `Adventurer`:
//     L1–2 Kämpfer (Fighter), L3–4 Kundschafter (Scout),
//     L5–6 Priester (Priest), L7–8 Magier (Mage).
//   Es gibt keine echten Klassenwechsel während des Tutorials.
// - Ab Level 9 klassische Klassenwahl bei einem Klassentrainer:
//   permanent, Adventurer wird durch die gewählte Grundklasse ersetzt,
//   Tutorial-Phasen werden beendet.
// - Hauptattribute sind Klassenmetadaten und werden im Inhalt später
//   verteilt (kein erfundenes Start-Attributssystem).
// - Level 10 ist das Maximum im neutralen Startgebiet; darüber erst nach
//   Fraktionswahl und Übergang in ein Fraktionsgebiet. Das Fraktions-
//   System folgt technisch später; der Hook ist hier vorbereitet.
//
// Spätere Unterklassen (Krieger, Paladin, Hexer, Mentalist, Druide,
// Templer, Räuber, Barde) sind bewusst NICHT Teil dieser Basis und
// werden später als `SubClass` direkt angeschlossen (keine Magic-Strings
// am Charakter, keine falschen Frühfreischaltungen).
//
// Wachstums-Modul: Teil der API sind Hooks für spätere Systeme
// (Trainer-Flow, Fraktions-/Zonensystem, SubClass-Spezialisierung);
// daher kein dead_code-Flag für gewollte Hooks.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};

/// Der kanonische Klassenstatus eines Charakters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClassStatus {
    /// Gemeinsame Startklasse: Abenteurer.
    Adventurer,
    /// Permanent gewählte Grundklasse.
    Fighter,
    Mage,
    Priest,
    Scout,
}

impl ClassStatus {
    pub fn from_db(value: i32) -> ClassStatus {
        match value {
            1 => ClassStatus::Adventurer,
            2 => ClassStatus::Fighter,
            3 => ClassStatus::Mage,
            4 => ClassStatus::Priest,
            5 => ClassStatus::Scout,
            _ => ClassStatus::Adventurer,
        }
    }

    pub fn as_db(self) -> i32 {
        match self {
            ClassStatus::Adventurer => 1,
            ClassStatus::Fighter => 2,
            ClassStatus::Mage => 3,
            ClassStatus::Priest => 4,
            ClassStatus::Scout => 5,
        }
    }
}

/// Temporäre Klassen-Erprobung im Tutorial (technisch immer `Adventurer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TutorialPhase {
    Fighter,
    Scout,
    Priest,
    Mage,
}

/// Maximales Level im neutralen Startgebiet (docs/Klassensystem.md:
/// Level 10, darüber erst nach Fraktionswahl).
pub const NEUTRAL_MAX_LEVEL: u32 = 10;

/// Level, ab dem die permanente Grundklassenwahl möglich ist.
pub const CLASS_CHOICE_LEVEL: u32 = 9;

impl ClassStatus {
    /// Kanonische Basis-/Kampfklassen-Bezeichnung für das DB-Feld
    /// `char_class` (kompakt und stabil; deutsche Anzeigen folgen im
    /// Client-/i18n-Layer, Combat-Logik versteht beide Schreibweisen).
    pub fn canonical_db_name(self) -> &'static str {
        match self {
            ClassStatus::Adventurer => "Adventurer",
            ClassStatus::Fighter => "Fighter",
            ClassStatus::Mage => "Mage",
            ClassStatus::Priest => "Priest",
            ClassStatus::Scout => "Scout",
        }
    }

    /// Tolerante DB-Lesbarkeit: übernimmt existierende Werte aus dem
    /// Übergangsstand (deutsche Kernklassen, legacy `Warrior`/`Mage`,
    /// Unterklassen-Bezeichnungen) ohne Schema-Kompromisse.
    pub fn from_db_name(raw: &str) -> ClassStatus {
        let c = raw.trim().to_lowercase();
        match c.as_str() {
            "adventurer" | "abenteurer" => ClassStatus::Adventurer,
            "fighter" | "kämpfer" | "kampfer" | "krieger" | "paladin" | "warrior" => {
                ClassStatus::Fighter
            }
            "mage" | "magier" | "hexer" | "mentalist" => ClassStatus::Mage,
            "priest" | "priester" | "druide" | "templer" => ClassStatus::Priest,
            "scout" | "kundschafter" | "räuber" | "raeber" | "barde" => ClassStatus::Scout,
            _ => ClassStatus::Adventurer,
        }
    }

    /// Ist der Charakter dauerhaft eine Grundklasse (nicht Abenteurer)?
    pub fn is_base_class(&self) -> bool {
        !matches!(self, ClassStatus::Adventurer)
    }

    /// Tutorial-Phase für das aktuelle Level (nur im neutralen Startgebiet
    /// bzw. vor Klassenwahl relevant; Level 1–8 wie in
    /// docs/Klassensystem.md; ab L9 ist die Auswahl möglich).
    pub fn tutorial_phase(&self, level: u32) -> Option<TutorialPhase> {
        if !matches!(self, ClassStatus::Adventurer) || level > 8 {
            return None;
        }
        match level {
            1..=2 => Some(TutorialPhase::Fighter),
            3..=4 => Some(TutorialPhase::Scout),
            5..=6 => Some(TutorialPhase::Priest),
            7..=8 => Some(TutorialPhase::Mage),
            _ => None,
        }
    }

    /// Hauptattribute der Grundklasse als reine Metadaten (docs/
    /// Klassensystem.md; Konkrete Verteilung folgt im Inhalt — kein
    /// erfundenes Attributssystem beim Start).
    pub fn primary_attributes(&self) -> (&'static str, &'static str) {
        match self {
            ClassStatus::Fighter => ("Strength", "Constitution"),
            ClassStatus::Scout => ("Dexterity", "Luck"),
            ClassStatus::Mage => ("Intelligence", "Wisdom"),
            ClassStatus::Priest => ("Intelligence", "Wisdom"),
            ClassStatus::Adventurer => ("", ""),
        }
    }

    /// L10-Progressions-Status im neutralen Startgebiet:
    /// - Abenteurer dürfen in den neutralen Startzonen weiter leveln
    ///   (Level 10 ist erst für dauerhaft klassierte Charaktere das
    ///   Maximum im neutralen Startgebiet).
    /// - Gewählte Grundklasse bei Level 10: kein weiteres Leveln, bis ein
    ///   Fraktions-Übergang vorliegt (`faction_transition`).
    ///
    /// Das Fraktions-Zonensystem selbst folgt technisch später;
    /// `faction_transition` ist der dafür vorgesehene Hook.
    pub fn progression_in_neutral_zone(
        &self,
        level: u32,
        faction_transition: bool,
    ) -> (bool, bool) {
        let is_at_cap = self.is_base_class() && level >= NEUTRAL_MAX_LEVEL;
        let can_level_up = if is_at_cap { faction_transition } else { true };
        let requires_faction = !can_level_up;
        (can_level_up, requires_faction)
    }
}

impl TutorialPhase {
    /// Die temporäre Grundklasse der Phase (Klassenname im DB-Feld).
    pub fn base_class_name(&self) -> &'static str {
        match self {
            TutorialPhase::Fighter => "Fighter",
            TutorialPhase::Scout => "Scout",
            TutorialPhase::Priest => "Priest",
            TutorialPhase::Mage => "Mage",
        }
    }

    /// Entsprechende permanente Grundklasse (L9-Wahl).
    pub fn to_status(self) -> ClassStatus {
        match self {
            TutorialPhase::Fighter => ClassStatus::Fighter,
            TutorialPhase::Scout => ClassStatus::Scout,
            TutorialPhase::Priest => ClassStatus::Priest,
            TutorialPhase::Mage => ClassStatus::Mage,
        }
    }
}


/// Ergebnis einer Klassenwahl (L9-Trainer-Flow; auch als Validierung
/// für Tests und spätere Trainer-Dialoge verwendbar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassChoiceResult {
    Ok,
    ErrBelowLevel,
    ErrAlreadyChosen,
    ErrInvalidClass,
}

/// Validiert und vollzieht eine Klassenwahl auf einem Charakter.
/// - Der Charakter wird dauerhaft eine Grundklasse.
/// - `Adventurer` wird ersetzt (kein Respec, kein weiterer Wechsel).
/// - Die Tutorial-Phase endet automatisch, weil sie nur für Adventurer
///   und Level 1–8 definiert ist (levelabgeleitet, kein Eigenfield).
/// - Spätere Unterklassen sind NICHT dadurch freigeschaltet.
pub fn try_class_choice(
    status: &mut ClassStatus,
    level: u32,
    chosen: ClassStatus,
) -> ClassChoiceResult {
    if !matches!(
        chosen,
        ClassStatus::Fighter | ClassStatus::Mage | ClassStatus::Priest | ClassStatus::Scout
    ) {
        return ClassChoiceResult::ErrInvalidClass;
    }
    if level < CLASS_CHOICE_LEVEL {
        return ClassChoiceResult::ErrBelowLevel;
    }
    if status.is_base_class() {
        return ClassChoiceResult::ErrAlreadyChosen;
    }
    // Adventurer wird dauerhaft durch die gewählte Grundklasse ersetzt.
    *status = chosen;
    ClassChoiceResult::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_and_display_names_roundtrip() {
        for (raw, s) in [
            ("Adventurer", ClassStatus::Adventurer),
            ("Fighter", ClassStatus::Fighter),
            ("Mage", ClassStatus::Mage),
            ("Priest", ClassStatus::Priest),
            ("Scout", ClassStatus::Scout),
            ("Kämpfer", ClassStatus::Fighter),
            ("Magier", ClassStatus::Mage),
            ("Priester", ClassStatus::Priest),
            ("Kundschafter", ClassStatus::Scout),
            ("Warrior", ClassStatus::Fighter),
            ("Räuber", ClassStatus::Scout),
            ("Templer", ClassStatus::Priest),
            ("???", ClassStatus::Adventurer),
        ] {
            assert_eq!(ClassStatus::from_db_name(raw), s);
        }
        for s in [
            ClassStatus::Adventurer,
            ClassStatus::Fighter,
            ClassStatus::Mage,
            ClassStatus::Priest,
            ClassStatus::Scout,
        ] {
            assert_eq!(ClassStatus::from_db(s.as_db()), s);
            assert_eq!(ClassStatus::from_db_name(s.canonical_db_name()), s);
        }
    }

    #[test]
    fn tutorial_phase_by_level() {
        let adv = ClassStatus::Adventurer;
        for lv in 1..=8 {
            assert_eq!(adv.tutorial_phase(lv), Some(match lv {
                1 | 2 => TutorialPhase::Fighter,
                3 | 4 => TutorialPhase::Scout,
                5 | 6 => TutorialPhase::Priest,
                7 | 8 => TutorialPhase::Mage,
                _ => unreachable!(),
            }));
        }
        assert_eq!(adv.tutorial_phase(9), None);
        assert_eq!(adv.tutorial_phase(10), None);
        assert_eq!(ClassStatus::Fighter.tutorial_phase(3), None);
        assert_eq!(ClassStatus::Mage.tutorial_phase(1), None);
    }

    #[test]
    fn primary_attributes_per_class() {
        assert_eq!(
            ClassStatus::Fighter.primary_attributes(),
            ("Strength", "Constitution")
        );
        assert_eq!(ClassStatus::Scout.primary_attributes(), ("Dexterity", "Luck"));
        assert_eq!(ClassStatus::Mage.primary_attributes(), ("Intelligence", "Wisdom"));
        assert_eq!(ClassStatus::Priest.primary_attributes(), ("Intelligence", "Wisdom"));
        assert_eq!(ClassStatus::Adventurer.primary_attributes(), ("", ""));
    }

    #[test]
    fn class_choice_validates_and_applies() {
        // Zu früh (L8).
        let mut st = ClassStatus::Adventurer;
        assert_eq!(
            try_class_choice(&mut st, 8, ClassStatus::Mage),
            ClassChoiceResult::ErrBelowLevel
        );
        assert_eq!(st, ClassStatus::Adventurer);

        // Ungültige Klasse (Adventurer darf nicht gewählt werden).
        assert_eq!(
            try_class_choice(&mut st, 9, ClassStatus::Adventurer),
            ClassChoiceResult::ErrInvalidClass
        );
        assert_eq!(st, ClassStatus::Adventurer);

        // Gültig bei L9.
        assert_eq!(
            try_class_choice(&mut st, 9, ClassStatus::Fighter),
            ClassChoiceResult::Ok
        );
        assert_eq!(st, ClassStatus::Fighter);

        // Tutorialphase endet automatisch (nur für Adventurer & L1–8).
        assert_eq!(st.tutorial_phase(9), None);
        assert_eq!(ClassStatus::Fighter.tutorial_phase(7), None);

        // Nochmal (Respec-Verbot).
        assert_eq!(
            try_class_choice(&mut st, 12, ClassStatus::Mage),
            ClassChoiceResult::ErrAlreadyChosen
        );
        assert_eq!(st, ClassStatus::Fighter);
    }

    #[test]
    fn level10_neutral_cap_and_hook() {
        let mut st = ClassStatus::Mage;
        // Abenteurer dürfen im neutralen Startgebiet weiter leveln.
        let (can, req) = ClassStatus::Adventurer.progression_in_neutral_zone(10, false);
        assert!(can && !req);
        // Gewählte Grundklasse: L10 = Cap, Freigabe erst nach Fraktions-
        // Übergang (Hook `faction_transition`).
        let (can, req) = st.progression_in_neutral_zone(10, false);
        assert!(!can && req);
        let (can, req) = st.progression_in_neutral_zone(10, true);
        assert!(can && !req);
        let (can, _) = st.progression_in_neutral_zone(9, false);
        assert!(can);
    }
}
