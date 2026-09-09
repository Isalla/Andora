// effects — Aktive Effekte: Buffs, Debuffs, DoT/HoT, Control (Combat V3).
//
// Realm-autoritative Effektverwaltung (Ability-System.md §§6–7).
// Effektgruppen: gleiche Gruppe ersetzt, unterschiedliche gleichzeitig.
// Waffen-Effekte: unterschiedliche Quellen gleichzeitig, gleiche Quelle
// nicht stapeln/verlängern.
// Beim Tod: alle aktiven Buffs/Debuffs entfernt.
use std::time::{Duration, Instant};

/// Quelle eines Effekts (Ability-System.md §7: Waffen-Effekte).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    Ability,
    Weapon,
}

/// Aktiver Effekt auf einer Entität.
#[derive(Debug, Clone)]
pub struct Effect {
    /// Eindeutige Instanz-ID (z. B. "p1:fire_bolt:e1").
    pub id: String,
    /// Content-Effekt-ID (z. B. "fire_bolt", "soul_rend").
    pub effect_id: String,
    /// Effektgruppe (Austausch-Regel: gleiche Gruppe ersetzt).
    pub group: String,
    /// Quell-Entität (Wer hat den Effekt ausgelöst).
    pub source_entity: String,
    /// Ability oder Weapon?
    pub source_kind: SourceKind,
    /// Ziel-Entität.
    #[allow(dead_code)] // Modellfeld (Ability-System.md §7); Auswertung folgt
    pub target_entity: String,
    /// Art des Effekts.
    pub kind: EffectKind,
    /// Startzeit.
    pub started_at: Instant,
    /// Gesamtdauer in Millisekunden.
    pub duration_ms: u64,
    /// Tick-Intervall in Millisekunden (0 = kein Tick → Sofortwirkung).
    pub tick_ms: u64,
    /// Nächster geplanter Tick-Zeitpunkt (DoT/HoT); None = kein Tick.
    pub next_tick_at: Option<Instant>,
    /// Schaden / Heilung pro Tick.
    pub value: f64,
    /// Wird durch Schaden unterbrochen (Root, Sleep)?
    pub interrupts_on_damage: bool,
}

/// Effekt-Art (Ability-System.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    Buff,
    Debuff,
    Dot,
    Hot,
    Stun,
    Silence,
    Root,
    Slow,
}

impl EffectKind {
    pub fn key(&self) -> &'static str {
        match self {
            EffectKind::Buff => "buff",
            EffectKind::Debuff => "debuff",
            EffectKind::Dot => "dot",
            EffectKind::Hot => "hot",
            EffectKind::Stun => "stun",
            EffectKind::Silence => "silence",
            EffectKind::Root => "root",
            EffectKind::Slow => "slow",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "buff" => EffectKind::Buff,
            "debuff" => EffectKind::Debuff,
            "dot" => EffectKind::Dot,
            "hot" => EffectKind::Hot,
            "stun" => EffectKind::Stun,
            "silence" => EffectKind::Silence,
            "root" => EffectKind::Root,
            "slow" => EffectKind::Slow,
            _ => EffectKind::Debuff,
        }
    }

    /// Tickt dieser Effekt periodisch (DoT/HoT)?
    pub fn tickable(self) -> bool {
        matches!(self, EffectKind::Dot | EffectKind::Hot)
    }
}

/// Parst den `effect_kind`-String aus der DB-Schicht in EffectKind.
pub fn parse_effect_kind(s: &str) -> EffectKind {
    EffectKind::from_str(s)
}

/// Prüft, ob ein Effekt abgelaufen ist.
pub fn is_expired(effect: &Effect, now: Instant) -> bool {
    now.duration_since(effect.started_at) >= Duration::from_millis(effect.duration_ms)
}

/// Wendet einen neuen Effekt auf eine Entität an.
/// - Gleiche Gruppe (nicht-Weapon): ersetzt alle vorherigen Effekte dieser Gruppe.
/// - Gleiche Weapon-Quelle: kein Stapel, keine Erneuerung solange aktiv.
/// - Unterschiedliche Weapon-Quellen: gleichzeitig.
/// - Unterschiedliche Gruppen: gleichzeitig.
/// Gibt Liste der entfernten alten Effekte zurück (für EFFECT_REMOVED-Events).
pub fn apply_effect(
    effects: &mut Vec<Effect>,
    new_effect: Effect,
) -> Vec<Effect> {
    let mut removed = Vec::new();

    if new_effect.source_kind == SourceKind::Weapon {
        // Weapon-Regel: gleiche Quelle nicht stapeln/verlängern
        let dominated = effects.iter().any(|e| {
            e.source_kind == SourceKind::Weapon
                && e.source_entity == new_effect.source_entity
                && e.effect_id == new_effect.effect_id
        });
        if dominated {
            return removed; // Effekt bereits aktiv, nicht anwenden
        }
        effects.push(new_effect);
        return removed;
    }

    // Ability-Regel: gleiche Gruppe ersetzt
    if !new_effect.group.is_empty() {
        let old: Vec<_> = effects
            .iter()
            .enumerate()
            .filter(|(_, e)| e.group == new_effect.group)
            .map(|(i, _)| i)
            .rev()
            .collect();
        for i in old {
            let removed_effect = effects.remove(i);
            removed.push(removed_effect);
        }
    }

    effects.push(new_effect);
    removed
}

/// Entfernt alle abgelaufenen Effekte und gibt sie zurück.
pub fn remove_expired(effects: &mut Vec<Effect>, now: Instant) -> Vec<Effect> {
    let mut removed = Vec::new();
    let mut i = effects.len();
    while i > 0 {
        i -= 1;
        if is_expired(&effects[i], now) {
            removed.push(effects.remove(i));
        }
    }
    removed
}

/// Entfernt alle Effekte (Tod: Ability-System.md §6).
pub fn clear_all(effects: &mut Vec<Effect>) -> Vec<Effect> {
    effects.drain(..).collect()
}

/// DoT/HoT-Tick: gibt Beschädigungen/Heilungen zurück.
/// Wird einmal pro Tick-Intervall aufgerufen.
pub struct TickResult {
    pub kind: EffectKind,
    pub value: i32,
}

/// Verarbeitet DoT/HoT-Ticks für einen Effekt. Gibt `Some(TickResult)` zurück,
/// wenn seit dem letzten Tick genügend Zeit vergangen ist (`next_tick_at`),
/// und plant den Folge-Tick (`next_tick_at = now + tick_ms`). Es wird maximal
/// ein Tick pro Aufruf ausgeführt — verspätete Aufrufe (Server-Lag) laufen
/// nicht auf, sondern verschieben den Rhythmus nach vorne.
pub fn process_tick(effect: &mut Effect, now: Instant) -> Option<TickResult> {
    if effect.tick_ms == 0 {
        return None; // Kein Tick
    }
    if effect.kind != EffectKind::Dot && effect.kind != EffectKind::Hot {
        return None; // Nur DoT/HoT
    }
    match effect.next_tick_at {
        Some(next) if now >= next => {
            effect.next_tick_at = Some(
                now.checked_add(Duration::from_millis(effect.tick_ms))
                    .unwrap_or(now),
            );
            Some(TickResult {
                kind: effect.kind,
                value: effect.value.round() as i32,
            })
        }
        _ => None,
    }
}

/// Prüft, ob eine Entität betäubt ist (kein Handeln möglich).
pub fn is_stunned(effects: &[Effect]) -> bool {
    effects.iter().any(|e| e.kind == EffectKind::Stun)
}

/// Prüft, ob eine Entität schwieg (Silence: Cast-Fähigkeiten verhindert).
pub fn is_silenced(effects: &[Effect]) -> bool {
    effects.iter().any(|e| e.kind == EffectKind::Silence)
}

/// Prüft, ob eine Entität bewegungsunfähig ist (Root).
pub fn is_rooted(effects: &[Effect]) -> bool {
    effects.iter().any(|e| e.kind == EffectKind::Root)
}

/// Entfernt Effekte, die durch erlittenen Schaden unterbrochen werden
/// (Root, Sleep — Ability-System.md §6).
pub fn on_damage_taken(effects: &mut Vec<Effect>, damage: i32) -> Vec<Effect> {
    if damage <= 0 {
        return Vec::new();
    }
    let mut removed = Vec::new();
    let mut i = effects.len();
    while i > 0 {
        i -= 1;
        if effects[i].interrupts_on_damage {
            removed.push(effects.remove(i));
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_effect(group: &str, source: &str, source_kind: SourceKind) -> Effect {
        Effect {
            id: format!("{}:{}", source, group),
            effect_id: "test".into(),
            group: group.into(),
            source_entity: source.into(),
            source_kind,
            target_entity: "target".into(),
            kind: EffectKind::Debuff,
            started_at: Instant::now(),
            duration_ms: 10000,
            tick_ms: 0,
            next_tick_at: None,
            value: 5.0,
            interrupts_on_damage: false,
        }
    }

    #[test]
    fn same_group_replaces() {
        let mut effects = Vec::new();
        let e1 = test_effect("doom", "caster1", SourceKind::Ability);
        let e2 = test_effect("doom", "caster2", SourceKind::Ability);
        apply_effect(&mut effects, e1);
        assert_eq!(effects.len(), 1);
        let removed = apply_effect(&mut effects, e2);
        assert_eq!(effects.len(), 1);
        assert_eq!(removed.len(), 1); // alter Effekt entfernt
        assert_eq!(effects[0].source_entity, "caster2");
    }

    #[test]
    fn different_groups_coexist() {
        let mut effects = Vec::new();
        let mut e1 = test_effect("doom", "caster1", SourceKind::Ability);
        let mut e2 = test_effect("shout", "caster2", SourceKind::Ability);
        e1.effect_id = "soul_rend".into();
        e2.effect_id = "battle_shout".into();
        apply_effect(&mut effects, e1);
        apply_effect(&mut effects, e2);
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn weapon_same_source_does_not_stack() {
        let mut effects = Vec::new();
        let e1 = Effect {
            id: "weapon1".into(),
            effect_id: "poison".into(),
            group: "poison".into(),
            source_entity: "rogue".into(),
            source_kind: SourceKind::Weapon,
            target_entity: "target".into(),
            kind: EffectKind::Dot,
            started_at: Instant::now(),
            duration_ms: 5000,
            tick_ms: 1000,
            next_tick_at: None,
            value: 3.0,
            interrupts_on_damage: false,
        };
        let e2 = e1.clone();
        apply_effect(&mut effects, e1);
        apply_effect(&mut effects, e2);
        assert_eq!(effects.len(), 1); // kein Stapel
    }

    #[test]
    fn weapon_different_sources_coexist() {
        let mut effects = Vec::new();
        let e1 = Effect {
            id: "weapon1".into(),
            effect_id: "poison".into(),
            group: "poison".into(),
            source_entity: "rogue1".into(),
            source_kind: SourceKind::Weapon,
            target_entity: "target".into(),
            kind: EffectKind::Dot,
            started_at: Instant::now(),
            duration_ms: 5000,
            tick_ms: 1000,
            next_tick_at: None,
            value: 3.0,
            interrupts_on_damage: false,
        };
        let mut e2 = e1.clone();
        e2.id = "weapon2".into();
        e2.source_entity = "rogue2".into();
        apply_effect(&mut effects, e1);
        apply_effect(&mut effects, e2);
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn clear_all_removes_everything() {
        let mut effects = Vec::new();
        effects.push(test_effect("a", "x", SourceKind::Ability));
        effects.push(test_effect("b", "y", SourceKind::Ability));
        let removed = clear_all(&mut effects);
        assert!(effects.is_empty());
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn on_damage_removes_interruptible() {
        let mut effects = Vec::new();
        let mut root = test_effect("root", "caster", SourceKind::Ability);
        root.kind = EffectKind::Root;
        root.interrupts_on_damage = true;
        let mut shout = test_effect("shout", "caster", SourceKind::Ability);
        shout.kind = EffectKind::Buff;
        shout.interrupts_on_damage = false;
        effects.push(root);
        effects.push(shout);
        let removed = on_damage_taken(&mut effects, 10);
        assert_eq!(removed.len(), 1);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].group, "shout");
    }
}
