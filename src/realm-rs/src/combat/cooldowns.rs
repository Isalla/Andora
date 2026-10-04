// cooldowns — Fähigkeits-Cooldowns pro Entität ( Spieler + NPC ).
//
// Cooldowns werden als ready-at (SystemTime / wall clock) gespeichert.
// Kurze Kampf-Cooldowns werden bei Tod/Evade/Return zurückgesetzt;
// persistente Cooldowns bleiben erhalten (Ability-System.md §2, §6).
//
// `P-18`: Spieler-Ability-Cooldowns werden als **absolute Ablaufzeitpunkte**
// persistiert (Epoch-Millisekunden, siehe `to_epoch_ms`/`from_epoch_ms`) und
// bleiben über Logout/Disconnect/Reconnect erhalten. Die bestehende
// Wall-Clock-Semantik bleibt Grundlage; ein Uhrsprung kann die verbleibende
// Dauer verändern. NPC-Cooldowns bleiben RAM-Zustand.
use std::collections::BTreeMap;
use std::time::SystemTime;

/// Cooldown-State einer Entität: Ability-ID → ready_at (SystemTime).
/// Für Spieler persistiert der Snapshot dieselben Zeitpunkte in
/// Epoch-Millisekunden (`P-18`).
pub type CooldownMap = BTreeMap<String, SystemTime>;

/// Wandelt einen Ablaufzeitpunkt in die persistierte Zeiteinheit um:
/// Millisekunden seit dem Unix-Epoch.
///
/// **Aufrundung:** `Duration::as_millis()` schneidet Submillisekunden **ab**.
/// Ein Abschneiden würde den Ablaufzeitpunkt um bis zu 999.999 µs **vorziehen**
/// und damit einen laufenden Cooldown nach dem Laden zu früh freigeben. Deshalb
/// wird bei vorhandenem Submillisekunden-Rest auf die **nächste** Millisekunde
/// **aufgerundet**; ein exakter Millisekundenwert bleibt unverändert. Die
/// Aufrundung kann den Ablauf damit minimal verlängern, aber nie verkürzen.
///
/// Grenzwertbehandlung: `as_millis()` liefert `u128`; das Aufrunden läuft über
/// `saturating_add` und die Rückgabe über `i64::try_from`, sodass weder ein
/// `u128`-noch ein `i64`-Overflow entsteht. Ein nicht darstellbarer Wert wird
/// auf `i64::MAX` begrenzt.
///
/// Liegt der Zeitpunkt **vor** dem Epoch, ist er sicher in der Vergangenheit;
/// die Konvertierung liefert dann `0`, was beim Laden als „bereits abgelaufen"
/// behandelt wird.
pub fn to_epoch_ms(ready_at: SystemTime) -> i64 {
    match ready_at.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => {
            let millis = d.as_millis();
            // `as_millis()` enthält den Millisekundenanteil bereits; der
            // abzuschneidende Rest sind die Nanosekunden **unterhalb** einer
            // Millisekunde (`subsec_nanos() % 1_000_000`).
            let has_sub_milli_remainder = d.subsec_nanos() % 1_000_000 != 0;
            let rounded = if has_sub_milli_remainder {
                millis.saturating_add(1)
            } else {
                millis
            };
            i64::try_from(rounded).unwrap_or(i64::MAX)
        }
        Err(_) => 0,
    }
}

/// Gegenstück zu [`to_epoch_ms`]: Epoch-Millisekunden → Ablaufzeitpunkt.
///
/// `0` und negative Werte ergeben `UNIX_EPOCH` und damit „bereits abgelaufen".
/// Ein Zeitpunkt, dessen Addition auf `SystemTime` überlaufen würde, wird
/// ebenfalls als abgelaufen behandelt, damit **keine** Fähigkeit dauerhaft
/// gesperrt bleiben kann.
pub fn from_epoch_ms(ms: i64) -> SystemTime {
    if ms <= 0 {
        return SystemTime::UNIX_EPOCH;
    }
    match SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_millis(ms as u64)) {
        Some(t) => t,
        None => SystemTime::UNIX_EPOCH,
    }
}

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
///
/// Rückgabe: `true`, wenn sich der gespeicherte Zustand **tatsächlich**
/// geändert hat (neuer oder anderer Ablaufzeitpunkt, bzw. ein entfernter
/// Eintrag). Nur eine tatsächliche Änderung ist eine Zustandsänderung und
/// damit dirty-relevant (`P-18`); ein unveränderter Eintrag ist es nicht.
pub fn start(
    cooldowns: &mut CooldownMap,
    ability_id: String,
    cooldown_ms: u64,
    wall_now: SystemTime,
) -> bool {
    if cooldown_ms == 0 {
        return cooldowns.remove(&ability_id).is_some();
    }
    let ready = wall_now
        .checked_add(std::time::Duration::from_millis(cooldown_ms))
        .unwrap_or(wall_now);
    if cooldowns.get(&ability_id) == Some(&ready) {
        return false;
    }
    cooldowns.insert(ability_id, ready);
    true
}

/// Setzt alle nicht-persistenten Cooldowns einer Entität zurück.
/// Wird bei Tod aufgerufen; `persistent_ids` stammt aus der tatsächlichen
/// Ability-Registry (`P-18`).
/// `persistent_ids`: Menge der Ability-IDs mit `cooldown_persistent = 1`.
///
/// Rückgabe: `true`, wenn Einträge entfernt wurden (dirty-relevant).
pub fn reset_non_persistent(
    cooldowns: &mut CooldownMap,
    persistent_ids: &std::collections::HashSet<String>,
) -> bool {
    let before = cooldowns.len();
    cooldowns.retain(|id, _| persistent_ids.contains(id));
    cooldowns.len() != before
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

    // ── P-18: Änderungserkennung und Zeiteinheit ──────────────────────────

    #[test]
    fn start_reports_only_real_changes() {
        let mut c = CooldownMap::new();
        let now = wall();
        // Neuer Eintrag: Zustand hat sich geändert.
        assert!(start(&mut c, "fire_bolt".into(), 2000, now));
        // Gleicher Ablaufzeitpunkt (gleiche Fähigkeit, gleiche Wall-Zeit,
        // gleiche Dauer): keine Änderung, also kein Dirty.
        assert!(!start(&mut c, "fire_bolt".into(), 2000, now));
        // Späterer Ablaufzeitpunkt: geändert (dirty-relevant).
        assert!(start(
            &mut c,
            "fire_bolt".into(),
            2000,
            now + std::time::Duration::from_millis(500)
        ));
    }

    #[test]
    fn zero_cooldown_reports_removal_as_change() {
        let mut c = CooldownMap::new();
        // Nichts vorhanden: Entfernen ändert nichts.
        assert!(!start(&mut c, "fire_bolt".into(), 0, wall()));
        start(&mut c, "fire_bolt".into(), 2000, wall());
        // Vorhandener Eintrag wird entfernt: das ist eine Änderung.
        assert!(start(&mut c, "fire_bolt".into(), 0, wall()));
        assert!(is_ready(&c, "fire_bolt", wall()));
    }

    #[test]
    fn reset_non_persistent_reports_removal_as_change() {
        let mut c = CooldownMap::new();
        let mut persistent = std::collections::HashSet::new();
        persistent.insert("ultimate".into());

        // Nur ein **markierter** Eintrag: es gibt nichts zu entfernen, der
        // gespeicherte Zustand bleibt unangetastet.
        start(&mut c, "ultimate".into(), 60000, wall());
        assert!(!reset_non_persistent(&mut c, &persistent));
        assert!(!is_ready(&c, "ultimate", wall()));

        // Zusätzlich ein **nicht** markierter Eintrag: das Entfernen ist eine
        // Änderung; die markierte Fähigkeit bleibt gesperrt.
        start(&mut c, "fire_bolt".into(), 2000, wall());
        assert!(reset_non_persistent(&mut c, &persistent));
        // Entfernt ⇒ kein Eintrag ⇒ sofort wieder bereit.
        assert!(is_ready(&c, "fire_bolt", wall()));
        // Markiert ⇒ bleibt bestehen und läuft normal ab.
        assert!(!is_ready(&c, "ultimate", wall()));
    }

    #[test]
    fn epoch_ms_roundtrip_preserves_ready_at() {
        // (1) Exakter Millisekundenwert bleibt **unverändert**.
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_123_456);
        assert_eq!(to_epoch_ms(now), 1_700_000_123_456);
        assert_eq!(from_epoch_ms(1_700_000_123_456), now);

        // (2) Submillisekunden-Rest wird **aufgerundet**, damit der Ablauf
        // nicht vorgezogen wird. `as_millis()` allein würde hier 1_500 liefern
        // und den Cooldown 700 µs zu früh freigeben.
        let sub = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_millis(1_500)
            + std::time::Duration::from_micros(700);
        assert_eq!(
            to_epoch_ms(sub),
            1_501,
            "Submillisekunden-Rest muss aufrunden, nicht abschneiden"
        );
        assert_eq!(
            to_epoch_ms(SystemTime::UNIX_EPOCH + std::time::Duration::from_micros(1)),
            1,
            "Minimaler positiver Rest wird auf 1 ms aufgerundet"
        );
        assert_eq!(
            to_epoch_ms(SystemTime::UNIX_EPOCH),
            0,
            "Exakt der Epoch ist exakt 0 und wird nicht auf 1 aufgerundet"
        );

        // (3) Die Aufrundung darf den Ablauf nur **verlängern**, nie verkürzen:
        //     der geladene Zeitpunkt liegt nie vor dem ursprünglichen.
        let restored = from_epoch_ms(to_epoch_ms(sub));
        assert!(
            restored >= sub,
            "Ablauf darf durch die Konvertierung nicht vorgezogen werden"
        );
        let delta = restored
            .duration_since(sub)
            .expect("restored liegt nicht vor sub");
        assert!(
            delta < std::time::Duration::from_millis(1),
            "Verlängerung bleibt unter einer Millisekunde, war: {delta:?}"
        );
    }

    #[test]
    fn epoch_ms_conversion_is_explicit_and_bounded() {
        // Vor dem Unix-Epoch: als „bereits abgelaufen" (0), nicht negativ.
        let before_epoch = SystemTime::UNIX_EPOCH - std::time::Duration::from_millis(5_000);
        assert_eq!(to_epoch_ms(before_epoch), 0);
        assert_eq!(from_epoch_ms(0), SystemTime::UNIX_EPOCH);
        assert_eq!(from_epoch_ms(-1), SystemTime::UNIX_EPOCH);

        // Grenzwert: der größte darstellbare Wert bleibt sichergestellt, und
        // `from_epoch_ms` läuft ohne Panic/Overflow.
        assert_eq!(to_epoch_ms(from_epoch_ms(i64::MAX)), i64::MAX);
        let big = 4_000_000_000_000i64; // ca. 2096
        assert_eq!(to_epoch_ms(from_epoch_ms(big)), big);

        // Grenzwert mit Submillisekunden-Rest direkt an der i64-Kante: das
        // Aufrunden darf nicht überlaufen, sondern wird begrenzt.
        let at_i64_max = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_millis(i64::MAX as u64)
            + std::time::Duration::from_micros(999_999);
        let converted = to_epoch_ms(at_i64_max);
        assert_eq!(
            converted,
            i64::MAX,
            "Aufrunden an der i64-Grenze wird begrenzt, kein Overflow"
        );
        // Roundtrip bleibt darstellbar und ohne Panic.
        let _ = from_epoch_ms(converted);

        // Sehr weit entfernte Zeitpunkte werden nicht unbeabsichtigt auf
        // „bereits abgelaufen" geklemmt (Wall-Clock-Semantik, §23).
        assert!(to_epoch_ms(from_epoch_ms(i64::MAX - 1)) > i64::MAX - 2);
    }

    #[test]
    fn expired_ready_at_frees_the_ability_after_offline_time() {
        // Cooldown 10 s, Ablauf 5 s später: nach 20 s Offline-Zeit ist die
        // Fähigkeit bereit — die Offline-Zeit zählt normal mit.
        let login = wall();
        let ready_at = login + std::time::Duration::from_millis(5_000);
        let mut c = CooldownMap::new();
        c.insert("choke".into(), ready_at);

        let stored = to_epoch_ms(*c.get("choke").unwrap());
        let restored = from_epoch_ms(stored);
        c.insert("choke".into(), restored);

        // Vor dem Ablauf: noch nicht bereit.
        assert!(!is_ready(
            &c,
            "choke",
            login + std::time::Duration::from_millis(4_999)
        ));
        // Nach dem Ablauf (Offline-Zeit wirkt normal): bereit.
        assert!(is_ready(
            &c,
            "choke",
            login + std::time::Duration::from_millis(20_000)
        ));
    }
}
