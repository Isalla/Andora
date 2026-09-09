// cooldowns — Fähigkeits-Cooldowns pro Entität ( Spieler + NPC ).
//
// Cooldowns werden als ready-at (SystemTime / wall clock) gespeichert.
// Kurze Kampf-Cooldowns werden bei Tod/Evade/Return zurückgesetzt;
// persistente Cooldowns bleiben erhalten (Ability-System.md §2, §6).
// Persistente DB-Speicherung wird später über spezifische Funktions-
// signaturen angebunden (hier: In-Memory-Struktur + Speicher-Interface).
use std::collections::BTreeMap;
use std::time::SystemTime;

/// Cooldown-State einer Entität: Ability-ID → ready_at (SystemTime).
/// SystemTime wird für spätere DB-Persistenz über Tod/Logout verwendet.
pub type CooldownMap = BTreeMap<String, SystemTime>;

/// Gibt zurück, ob ein Ability-Cooldown für diesen Zeitpunkt abgelaufen ist.
/// Kein Eintrag = abgelaufen / nie gesetzt.
pub fn is_ready(cooldowns: &CooldownMap, ability_id: &str, wall_now: SystemTime) -> bool {
    match cooldowns.get(ability_id) {
        Some(ready_at) => wall_now >= *ready_at,
        None => true,
    }
}

/// Startet einen Cooldown nach erfolgreicher Ausführung (Ability-System.md §2).
/// `cooldown_ms = 0` → kein Cooldown gesetzt (sofort abgelaufen).
pub fn start(cooldowns: &mut CooldownMap, ability_id: String, cooldown_ms: u64, wall_now: SystemTime) {
    if cooldown_ms == 0 {
        cooldowns.remove(&ability_id);
        return;
    }
    let ready = wall_now
        .checked_add(std::time::Duration::from_millis(cooldown_ms))
        .unwrap_or(wall_now);
    cooldowns.insert(ability_id, ready);
}

/// Setzt alle nicht-persistenten Cooldowns einer Entität zurück.
/// Wird bei Tod, Logout, Evade/Return aufgerufen.
/// `persistent_ids`: Menge der Ability-IDs mit persistant_cooldown = true.
pub fn reset_non_persistent(cooldowns: &mut CooldownMap, persistent_ids: &std::collections::HashSet<String>) {
    cooldowns.retain(|id, _| persistent_ids.contains(id));
}

/// Setzt ALLE Cooldowns einer Entität zurück (NPC Evade/Return, §20).
pub fn reset_all(cooldowns: &mut CooldownMap) {
    cooldowns.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall() -> SystemTime {
        SystemTime::UNIX_EPOCH
    }

    #[test]
    fn no_cooldown_means_ready() {
        let c = CooldownMap::new();
        assert!(is_ready(&c, "fire_bolt", wall()));
    }

    #[test]
    fn start_sets_ready_at() {
        let mut c = CooldownMap::new();
        let now = wall();
        start(&mut c, "fire_bolt".into(), 2000, now);
        assert!(!is_ready(&c, "fire_bolt", now));
        let later = now + std::time::Duration::from_millis(2000);
        assert!(is_ready(&c, "fire_bolt", later));
    }

    #[test]
    fn zero_cooldown_removes_entry() {
        let mut c = CooldownMap::new();
        start(&mut c, "fire_bolt".into(), 2000, wall());
        start(&mut c, "fire_bolt".into(), 0, wall());
        assert!(is_ready(&c, "fire_bolt", wall()));
    }

    #[test]
    fn reset_all_clears_everything() {
        let mut c = CooldownMap::new();
        start(&mut c, "fire_bolt".into(), 5000, wall());
        start(&mut c, "heal".into(), 5000, wall());
        reset_all(&mut c);
        assert!(is_ready(&c, "fire_bolt", wall()));
    }

    #[test]
    fn reset_non_persistent_keeps_persistent() {
        let mut c = CooldownMap::new();
        start(&mut c, "fire_bolt".into(), 5000, wall());
        start(&mut c, "ultimate".into(), 60000, wall());
        let mut persistent = std::collections::HashSet::new();
        persistent.insert("ultimate".into());
        reset_non_persistent(&mut c, &persistent);
        assert!(is_ready(&c, "fire_bolt", wall()));
        assert!(!is_ready(&c, "ultimate", wall()));
    }
}
