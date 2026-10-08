// item_lifecycle — Item-Instanz-Lifecycle für den Single-Process-Realm
// (docs/inventory_system.md §16–§18, docs/Handelssystem.md §4).
//
// Trennung (verbindlich):
// - Runtime-History (`SellHistory`): ausschließlich Runtime-State der
//   laufenden Player-Session (max. 20 Einträge, FIFO). Sie bleibt NICHT
//   persistent: kein Snapshot-Feld, kein DB-Write, Verwerfen am Session-Ende.
// - Dauerhafte Lifecycle-Metadaten (`ItemLifecycle`): ermöglichen Zuordnung
//   und kontrollierte Finalisierung abgekoppelter UUIDs über den
//   revisionierten Snapshot-/Drain-Pfad (Migration 021,
//   `item_instance_finalizations`).
//
// Betriebsvertrag: genau ein Realm-Serverprozess je RealmDB
// (docs/Login_Realm_Architektur.md, Abschnitt „Realm-Server“;
// docs/Datenbank_Architektur.md §17). Das ist ein Betriebsvertrag, kein
// bereits implementierter technischer Doppelstartschutz — gegen
// vertragswidrige parallele Starts wird keine Sicherheit behauptet.
//
// Hinweis zur Modulabdeckung: `try_take_instance`/`try_insert_instance` und
// `retired_uuid` (docs/inventory_system.md §17) sind die Anschlussstellen;
// die Verrechnung (`reconcile_*`) und die History-Mutationen werden erst vom
// späteren Händler-Spiellayer aufgerufen (keine Händlerhandler, Preise oder
// Angebote in diesem Auftrag). Bis dahin decken die Tests diese Pfade ab.
#![allow(dead_code)]
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

/// Maximale Einträge der Sell-/Buyback-History (docs/Handelssystem.md §3).
pub const SELL_HISTORY_LIMIT: usize = 20;

/// Eigene Runtime-Kennung dieses Realm-Prozesslaufs. Sie stempelt
/// Lifecycle-Metadaten zur Zuordnung (welcher Lauf hat abgekoppelt) und wird
/// im Snapshot mitgeführt. Das Format ist eine reine Zuordnungshilfe
/// (`rt-<pid>-<nanos>`) ohne Sicherheits- oder Einmaligkeitsgarantie über
/// Prozessneustarts mit gleicher PID/Nanos-Kollision hinweg — für die
/// Korrektheit der Finalisierung ist allein die Revisions- und
/// Referenzprüfung maßgeblich, nicht diese Kennung.
pub fn new_runtime_id() -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("rt-{pid}-{nanos:x}")
}

/// Aktuelle Zeit in Millisekunden seit dem Unix-Epoch (Erfassungszeitpunkt
/// von Abkopplungen; dieselbe Einheit wie `PersistSnapshot.captured_at_ms`).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Ein Verkaufseintrag der nicht-persistenten Runtime-History
/// (docs/Handelssystem.md §3): verkaufte Item-/Stack-Identität, tatsächlich
/// verkaufte Menge und erhaltener Verkaufswert (Rückkaufpreis-Anschluss für
/// den späteren Händler-Spiellayer; Preise/Angebote sind nicht Teil dieses
/// Auftrags).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SellHistoryEntry {
    pub item_id: String,
    pub item_uuid: String,
    pub count: i64,
    pub sell_gold_value: i64,
}

/// Sell-/Buyback-History einer laufenden Player-Session: spielergebunden,
/// maximal 20 Einträge, FIFO. Bewusst NICHT serialisierbar (kein Snapshot-,
/// kein DB-Pfad führt hierher); am Session-Ende wird sie verworfen, indem
/// der Spieler samt World-Einträgen entfernt wird.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SellHistory {
    entries: VecDeque<SellHistoryEntry>,
}

impl SellHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Zeichnet einen Verkauf auf; der älteste Eintrag wird bei Überlauf
    /// verdrängt (FIFO, docs/Handelssystem.md §3).
    pub fn record(&mut self, entry: SellHistoryEntry) {
        if self.entries.len() >= SELL_HISTORY_LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Verbraucht den Eintrag einer UUID für den Rückkauf (atomar: kein
    /// Teilrückkauf, docs/Handelssystem.md §7/§8). `None` = nicht enthalten.
    pub fn take_for_buyback(&mut self, item_uuid: &str) -> Option<SellHistoryEntry> {
        let pos = self.entries.iter().position(|e| e.item_uuid == item_uuid)?;
        self.entries.remove(pos)
    }

    /// Verwirft die gesamte History (Session-Ende, docs/Handelssystem.md §2).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &SellHistoryEntry> {
        self.entries.iter()
    }
}

/// Grund einer UUID-Abkopplung (Zuordnungshilfe für Operator/Diagnose; die
/// Finalisierungsentscheidung hängt nicht vom Grund ab).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetachReason {
    /// Vollständige Entnahme aus dem Inventar (späterer Verkaufspfad).
    Sold,
    /// Aufgegebene UUID bei Vollverschmelzung (`retired_uuid`).
    Merged,
    /// Verworfenes Puffer-Item am Session-Ende (docs/inventory_system.md §11).
    Discarded,
}

impl DetachReason {
    /// Kanonischer DB-Wert (`item_instance_finalizations.reason`).
    pub fn as_db(self) -> &'static str {
        match self {
            DetachReason::Sold => "sold",
            DetachReason::Merged => "merged",
            DetachReason::Discarded => "discarded",
        }
    }

    /// Parst einen DB-Wert; `None` = unbekannt (solche Zeilen werden bei der
    /// Startup-Finalisierung bewusst nicht angefasst).
    pub fn from_db(s: &str) -> Option<DetachReason> {
        match s.trim() {
            "sold" => Some(DetachReason::Sold),
            "merged" => Some(DetachReason::Merged),
            "discarded" => Some(DetachReason::Discarded),
            _ => None,
        }
    }
}

/// RAM-seitige, noch unbestätigte Abkopplung einer Item-UUID aus den
/// persistenten Inventarplatzierungen. Sie bleibt in neueren Snapshots
/// erhalten, bis der DB-Commit die Finalisierung bestätigt hat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDetachment {
    pub item_uuid: String,
    pub reason: DetachReason,
    pub runtime_id: String,
    pub recorded_at_ms: i64,
}

/// Dauerhafte Lifecycle-Metadaten eines Spielers (RAM-Abbild; persistent über
/// den Snapshot-/Drain-Pfad, Migration 021). Enthält ausschließlich
/// abgekoppelte UUIDs zur Zuordnung und kontrollierten Finalisierung.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemLifecycle {
    pending: BTreeMap<String, PendingDetachment>,
}

impl ItemLifecycle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Erfasst eine Abkopplung. Leere/whitespace-only UUIDs werden bewusst
    /// nicht erfasst (sie können keiner Instanz zugeordnet werden).
    pub fn record_detachment(&mut self, detachment: PendingDetachment) {
        if detachment.item_uuid.trim().is_empty() {
            return;
        }
        self.pending
            .insert(detachment.item_uuid.clone(), detachment);
    }

    /// Hebt eine Abkopplung auf (die UUID lebt wieder im Inventar, z. B.
    /// Buyback vor dem Verkaufssnapshot). Neue Änderungen dürfen bei der
    /// späteren Bestätigung nicht versehentlich freigegeben werden — genau
    /// dafür wird die ausstehende Entfernung hier zurückgenommen.
    pub fn cancel(&mut self, item_uuid: &str) {
        self.pending.remove(item_uuid);
    }

    pub fn contains(&self, item_uuid: &str) -> bool {
        self.pending.contains_key(item_uuid)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Abgekoppelte UUIDs (sortiert, deterministisch).
    pub fn pending_uuids(&self) -> BTreeSet<String> {
        self.pending.keys().cloned().collect()
    }

    /// Snapshot-Sicht: stempelt die Snapshot-Revision auf jede ausstehende
    /// Abkopplung (Zuordnung, welcher Snapshot sie trägt). Die Reihenfolge ist
    /// nach UUID sortiert und damit deterministisch (Spool-Bytevergleich).
    pub fn snapshot_view(
        &self,
        snapshot_revision: i64,
        snapshot_runtime_id: &str,
    ) -> ItemLifecycleSnapshot {
        ItemLifecycleSnapshot {
            runtime_id: snapshot_runtime_id.to_string(),
            pending: self
                .pending
                .values()
                .map(|p| DetachedInstance {
                    item_uuid: p.item_uuid.clone(),
                    reason: p.reason,
                    runtime_id: p.runtime_id.clone(),
                    recorded_at_ms: p.recorded_at_ms,
                    detached_at_revision: snapshot_revision,
                })
                .collect(),
        }
    }
}

/// Abgekoppelte Instanz im Snapshot-Drahtformat (Teil von `PersistSnapshot`,
/// Migration 021).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetachedInstance {
    pub item_uuid: String,
    pub reason: DetachReason,
    pub runtime_id: String,
    pub recorded_at_ms: i64,
    /// Snapshot-Revision, die diese Abkopplung trägt (Zuordnung).
    pub detached_at_revision: i64,
}

/// Lifecycle-Teil des Snapshots. `None` im Snapshot = Altformat ohne Feld:
/// Der Drain lässt Metadaten und Instanzen dann unberührt (kein Ersetzen,
/// keine Löschung). `Some` (auch leer) = neues Format: Der Drain schreibt die
/// Metadaten vollständig neu und finalisiert zulässige Instanzen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemLifecycleSnapshot {
    /// Runtime-Kennung des erfassenden Prozesslaufs.
    pub runtime_id: String,
    /// Ausstehende Abkopplungen, sortiert nach UUID.
    pub pending: Vec<DetachedInstance>,
}

impl ItemLifecycleSnapshot {
    pub fn pending_uuids(&self) -> BTreeSet<String> {
        self.pending.iter().map(|d| d.item_uuid.clone()).collect()
    }
}

/// Entscheidet, welche abgekoppelten UUIDs im Drain finalisiert (aus
/// `item_instances` gelöscht) werden dürfen. Reine Funktion über dem
/// Snapshot — ohne DB-Zugriff, ohne RAM-Zugriff.
///
/// - In den Snapshot-Platzierungen enthaltene UUIDs sind wieder lebendig
///   (widersprüchliche Zuordnung) und werden NICHT gelöscht.
/// - In der DB referenzierte UUIDs (Platzierungs-/Pufferzeilen, auch fremder
///   Charaktere) werden NICHT gelöscht — vorhandene Referenzorte bleiben
///   erhalten.
/// - Das Ergebnis ist sortiert und dedupliziert (idempotente Wiederholung).
pub fn deletable_candidates(
    pending: &[DetachedInstance],
    snapshot_inventory_uuids: &BTreeSet<String>,
    db_referenced_uuids: &BTreeSet<String>,
) -> Vec<String> {
    pending
        .iter()
        .map(|d| d.item_uuid.clone())
        .filter(|u| !snapshot_inventory_uuids.contains(u))
        .filter(|u| !db_referenced_uuids.contains(u))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Führt gespeicherte DB-Metadaten mit der Snapshot-Sicht zusammen (reine
/// Funktion; produktiv aufgerufen aus `db::apply_item_lifecycle` in derselben
/// Transaktion wie der Metadaten-Vollschreib).
///
/// - Eine Wiedereinsetzung (Buyback/erneute Platzierung) hebt nur die jeweils
///   passende Pflicht auf; alle übrigen gespeicherten Pflichten bleiben
///   erhalten — auch bei `Some(empty)` nach Neustart mit leerem RAM.
/// - Bei UUID-Gleichstand gewinnt der Snapshot (frischer Revisionsstempel);
///   Herkunft (`runtime_id`, Erfassungszeit) bleibt sonst erhalten.
/// - Fremde UUIDs entstehen hier nicht: Gespeichertes ist je Charakter
///   gelesen, Snapshot-Pending stammt aus dem eigenen RAM; die Löschung bleibt
///   zusätzlich an die Referenzprüfung gebunden.
/// - Leere UUIDs entfallen; das Ergebnis ist nach UUID sortiert.
pub fn merge_pending(
    stored: &[DetachedInstance],
    snapshot_pending: &[DetachedInstance],
    snapshot_inventory_uuids: &BTreeSet<String>,
) -> Vec<DetachedInstance> {
    let mut merged = BTreeMap::new();
    for d in stored.iter().chain(snapshot_pending.iter()) {
        if d.item_uuid.trim().is_empty() {
            continue;
        }
        if snapshot_inventory_uuids.contains(&d.item_uuid) {
            merged.remove(&d.item_uuid);
            continue;
        }
        merged.insert(d.item_uuid.clone(), d.clone());
    }
    merged.into_values().collect()
}

/// Anschlussstelle Entnahme (`try_take_instance`, docs/inventory_system.md
/// §17): Die entnommene UUID ist genau dann abgekoppelt, wenn sie in keiner
/// persistenten Platzierung mehr vorkommt. Ein Teilstack-Rest behält seine
/// UUID und bleibt dadurch geschützt; die entnommene Teilmenge trägt eine
/// neue UUID und wird unter dieser erfasst.
pub fn reconcile_after_take(
    lifecycle: &mut ItemLifecycle,
    inventory_uuids: &BTreeSet<String>,
    taken_uuid: &str,
    reason: DetachReason,
    runtime_id: &str,
    now_ms: i64,
) {
    if taken_uuid.trim().is_empty() || inventory_uuids.contains(taken_uuid) {
        return;
    }
    lifecycle.record_detachment(PendingDetachment {
        item_uuid: taken_uuid.to_string(),
        reason,
        runtime_id: runtime_id.to_string(),
        recorded_at_ms: now_ms,
    });
}

/// Anschlussstelle Wiedereinsetzen (`try_insert_instance`, Buyback-Pfad):
/// - `retired_uuid` (Vollverschmelzung): die aufgegebene UUID ist
///   abgekoppelt und wird erfasst.
/// - Sonst: lebt die eingehende UUID im Inventar, wird eine ausstehende
///   Abkopplung aufgehoben (Buyback vor dem Verkaufssnapshot).
pub fn reconcile_after_insert(
    lifecycle: &mut ItemLifecycle,
    inventory_uuids: &BTreeSet<String>,
    incoming_uuid: &str,
    retired_uuid: Option<&str>,
    runtime_id: &str,
    now_ms: i64,
) {
    if let Some(retired) = retired_uuid {
        if !retired.trim().is_empty() && !inventory_uuids.contains(retired) {
            lifecycle.record_detachment(PendingDetachment {
                item_uuid: retired.to_string(),
                reason: DetachReason::Merged,
                runtime_id: runtime_id.to_string(),
                recorded_at_ms: now_ms,
            });
        }
        return;
    }
    if inventory_uuids.contains(incoming_uuid) {
        lifecycle.cancel(incoming_uuid);
    }
}

/// Startup-Finalisierung alter Runtime-Metadaten: Läuft ausschließlich bei
/// **vollständig abgeschlossener** Recovery. Bei unvollständiger Recovery
/// (Fehler oder Restarbeit) findet keine widersprüchliche Bereinigung und
/// keine vorzeitige Freigabe statt — der Aufrufer (Startpfad) lässt die
/// Finalisierung dann vollständig aus.
pub fn startup_finalization_allowed(recovery_failed: bool, batches_remaining: usize) -> bool {
    !recovery_failed && batches_remaining == 0
}

/// Metadatenzeile aus `item_instance_finalizations` (Startup-Lesung).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizationRow {
    pub char_id: i64,
    pub item_uuid: String,
    pub reason: String,
}

/// Entscheidung der Startup-Finalisierung über gelesene Metadatenzeilen
/// (reine Funktion; die SQL-Schicht in `db.rs` bildet sie Zeile für Zeile
/// ab — FakeDb-Harness prüft die Logik, kein SQL/FK/Rollback):
///
///   - referenzierte UUID: weder löschen noch Metadaten anfassen,
///   - fehlende Instanzzeile: nur Metadatenzeile bereinigen,
///   - sonst: Instanzzeile löschen und Metadatenzeile bereinigen.
///
/// Ausgaben sind sortiert und damit deterministisch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupFinalizeDecision {
    pub delete_instances: Vec<String>,
    pub drop_metadata: Vec<(i64, String)>,
}

pub fn startup_finalize_decisions(
    rows: &[FinalizationRow],
    referenced: &BTreeSet<String>,
    existing_instances: &BTreeSet<String>,
) -> StartupFinalizeDecision {
    let mut delete_instances = BTreeSet::new();
    let mut drop_metadata = BTreeSet::new();
    for row in rows {
        if row.item_uuid.trim().is_empty() {
            continue;
        }
        if referenced.contains(&row.item_uuid) {
            // Referenziert oder widersprüchlich zugeordnet → nicht löschen,
            // Metadaten zur erneuten Prüfung erhalten.
            continue;
        }
        if !existing_instances.contains(&row.item_uuid) {
            // Bereits finalisiert (oder nie persistiert): nur Metadaten
            // bereinigen, damit die Tabelle nicht wächst.
            drop_metadata.insert((row.char_id, row.item_uuid.clone()));
            continue;
        }
        delete_instances.insert(row.item_uuid.clone());
        drop_metadata.insert((row.char_id, row.item_uuid.clone()));
    }
    StartupFinalizeDecision {
        delete_instances: delete_instances.into_iter().collect(),
        drop_metadata: drop_metadata.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RT: &str = "rt-test-1";
    const NOW: i64 = 1_700_000_000_000;

    fn entry(uuid: &str) -> SellHistoryEntry {
        SellHistoryEntry {
            item_id: "hp_potion".into(),
            item_uuid: uuid.into(),
            count: 3,
            sell_gold_value: 30,
        }
    }

    fn pending(uuid: &str) -> DetachedInstance {
        DetachedInstance {
            item_uuid: uuid.into(),
            reason: DetachReason::Sold,
            runtime_id: RT.into(),
            recorded_at_ms: NOW,
            detached_at_revision: 7,
        }
    }

    fn set(uuids: &[&str]) -> BTreeSet<String> {
        uuids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn history_fifo_caps_at_20_and_displaces_oldest() {
        let mut h = SellHistory::new();
        for i in 0..25 {
            h.record(entry(&format!("u{i}")));
        }
        assert_eq!(h.len(), 20);
        let uuids: Vec<&str> = h.iter().map(|e| e.item_uuid.as_str()).collect();
        assert!(!uuids.contains(&"u0"), "ältester Eintrag verdrängt");
        assert!(!uuids.contains(&"u4"), "u0..u4 verdrängt");
        assert!(uuids.contains(&"u5"), "u5 bleibt als ältester");
        assert!(uuids.contains(&"u24"), "neuester bleibt");
    }

    #[test]
    fn history_take_for_buyback_consumes_entry_atomically() {
        let mut h = SellHistory::new();
        h.record(entry("u1"));
        h.record(entry("u2"));
        let taken = h.take_for_buyback("u1").expect("Eintrag vorhanden");
        assert_eq!(taken.item_uuid, "u1");
        assert_eq!(taken.count, 3);
        assert_eq!(taken.sell_gold_value, 30);
        assert_eq!(h.len(), 1);
        assert!(h.take_for_buyback("u1").is_none(), "kein Duplikat");
        assert!(h.take_for_buyback("fremd").is_none());
        assert_eq!(h.len(), 1, "fremde UUID verändert nichts");
    }

    #[test]
    fn history_clear_discards_all_at_session_end() {
        let mut h = SellHistory::new();
        h.record(entry("u1"));
        h.record(entry("u2"));
        assert!(!h.is_empty());
        h.clear();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn full_take_detaches_but_partial_take_protects_the_rest() {
        let mut lc = ItemLifecycle::new();
        // Vollentnahme: UUID ist aus den Platzierungen verschwunden.
        reconcile_after_take(&mut lc, &set(&[]), "sold-u", DetachReason::Sold, RT, NOW);
        assert!(lc.contains("sold-u"));
        assert_eq!(lc.len(), 1);
        // Wiederholung derselben Abkopplung bleibt genau ein Eintrag.
        reconcile_after_take(&mut lc, &set(&[]), "sold-u", DetachReason::Sold, RT, NOW);
        assert_eq!(lc.len(), 1);
        // Teilentnahme: Rest behält UUID "rest-u" (geschützt), entnommener
        // Teil trägt neue UUID "part-neu" (abgekoppelt, verkauft).
        reconcile_after_take(
            &mut lc,
            &set(&["rest-u"]),
            "rest-u",
            DetachReason::Sold,
            RT,
            NOW,
        );
        assert!(
            !lc.contains("rest-u"),
            "Rest-UUID ist kein Abkopplungskandidat"
        );
        reconcile_after_take(
            &mut lc,
            &set(&["rest-u"]),
            "part-neu",
            DetachReason::Sold,
            RT,
            NOW,
        );
        assert!(lc.contains("part-neu"));
        assert_eq!(lc.pending_uuids(), set(&["sold-u", "part-neu"]));
    }

    #[test]
    fn empty_uuid_is_never_recorded() {
        let mut lc = ItemLifecycle::new();
        reconcile_after_take(&mut lc, &set(&[]), "   ", DetachReason::Sold, RT, NOW);
        lc.record_detachment(PendingDetachment {
            item_uuid: String::new(),
            reason: DetachReason::Sold,
            runtime_id: RT.into(),
            recorded_at_ms: NOW,
        });
        assert!(lc.is_empty());
    }

    #[test]
    fn full_merge_records_retired_uuid() {
        let mut lc = ItemLifecycle::new();
        // Buyback mit Vollverschmelzung: eingehende UUID "back-u" aufgegeben.
        reconcile_after_insert(
            &mut lc,
            &set(&["ziel-u"]),
            "back-u",
            Some("back-u"),
            RT,
            NOW,
        );
        assert!(lc.contains("back-u"));
        assert_eq!(lc.len(), 1);
    }

    #[test]
    fn buyback_before_sale_snapshot_cancels_the_detachment() {
        let mut lc = ItemLifecycle::new();
        // Verkauf: "u" abgekoppelt, aber Snapshot steht noch aus.
        reconcile_after_take(&mut lc, &set(&[]), "u", DetachReason::Sold, RT, NOW);
        assert!(lc.contains("u"));
        // Buyback VOR dem Verkaufssnapshot: "u" lebt wieder im Inventar.
        reconcile_after_insert(&mut lc, &set(&["u"]), "u", None, RT, NOW);
        assert!(!lc.contains("u"), "neue Änderung hebt die Entfernung auf");
        assert!(lc.is_empty());
    }

    #[test]
    fn snapshot_view_stamps_revision_and_sorts_deterministically() {
        let mut lc = ItemLifecycle::new();
        for u in ["u-b", "u-a", "u-c"] {
            reconcile_after_take(&mut lc, &set(&[]), u, DetachReason::Sold, RT, NOW);
        }
        let view = lc.snapshot_view(9, "rt-neu");
        assert_eq!(view.runtime_id, "rt-neu");
        let order: Vec<&str> = view.pending.iter().map(|d| d.item_uuid.as_str()).collect();
        assert_eq!(order, vec!["u-a", "u-b", "u-c"]);
        for d in &view.pending {
            assert_eq!(d.detached_at_revision, 9);
            assert_eq!(d.runtime_id, RT, "Herkunftslauf bleibt erhalten");
        }
        assert_eq!(view.pending_uuids(), set(&["u-a", "u-b", "u-c"]));
    }

    #[test]
    fn sale_snapshot_marks_exactly_the_detached_uuid_deletable() {
        // Verkaufssnapshot R1: "u" fehlt im Inventar, unreferenziert → löschbar.
        let got = deletable_candidates(&[pending("u")], &set(&[]), &set(&[]));
        assert_eq!(got, vec!["u".to_string()]);
    }

    #[test]
    fn inventory_resident_and_referenced_uuids_are_protected() {
        // UUID wieder im Snapshot-Inventar (widersprüchliche Zuordnung).
        let got = deletable_candidates(&[pending("u")], &set(&["u"]), &set(&[]));
        assert!(got.is_empty(), "lebendige UUID wird nicht gelöscht");
        // UUID in der DB referenziert (fremde Platzierung/Pufferzeile).
        let got = deletable_candidates(&[pending("v")], &set(&[]), &set(&["v"]));
        assert!(got.is_empty(), "referenzierte UUID wird nicht gelöscht");
        // Gemischt: nur die wirklich verwaiste UUID ist Kandidat.
        let both = vec![pending("u"), pending("v"), pending("w")];
        let got = deletable_candidates(&both, &set(&["u"]), &set(&["v"]));
        assert_eq!(got, vec!["w".to_string()]);
    }

    #[test]
    fn candidates_are_deduplicated_and_sorted() {
        let dup = vec![pending("b"), pending("a"), pending("b")];
        assert_eq!(
            deletable_candidates(&dup, &set(&[]), &set(&[])),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    /// Entscheidungs-Harness (FakeDb-Grenze, K4): bildet den Drain-Ablauf als
    /// **Modell** über Tabellenständen ab — **kein** Produktionspfadnachweis.
    /// Nachgebaut (modelliert, nicht produktionsverifiziert) sind der
    /// Revisionsvergleich mit Skip/Supersede-Entscheidung und die
    /// Commit-Atomarität; einzige Produktionsfunktion unter Test ist
    /// `deletable_candidates`. Das produktive Revisions-Gating liegt in
    /// `spool.rs` (lifecycle-agnostische FakeDb-Tests), Transaktionsatomarität
    /// und SQL sind offline prinzipbedingt unbelegt. Geprüft wird
    /// ausschließlich die Entscheidungslogik (welche UUID bei welchem
    /// Revisions-/Referenzstand gelöscht würde).
    struct Harness {
        revision: i64,
        instances: BTreeSet<String>,
        refs: BTreeSet<String>,
        deletes: Vec<String>,
    }

    impl Harness {
        /// Wendet einen Snapshot an (`db_rev < snap_rev`): löscht genau die
        /// zulässigen Kandidaten und schreibt die Revision fort. Gibt an, ob
        /// angewendet wurde (bei `>=` wird nichts gelöscht: skip/superseded).
        fn drain(
            &mut self,
            snap_rev: i64,
            inventory: &BTreeSet<String>,
            pending: &[DetachedInstance],
        ) -> bool {
            if self.revision >= snap_rev {
                return false; // skip (==) oder superseded (>)
            }
            for u in deletable_candidates(pending, inventory, &self.refs) {
                // Idempotent: bereits gelöschte Zeile ist ein No-Op.
                if self.instances.remove(&u) {
                    self.deletes.push(u);
                }
            }
            self.revision = snap_rev;
            true
        }
    }

    fn sale_pending() -> Vec<DetachedInstance> {
        vec![pending("u")]
    }

    #[test]
    fn sale_snapshot_and_later_finalization_apply_order() {
        // Modell (K4, kein Produktionspfad): R1 = Verkaufssnapshot (ohne
        // "u", pending), R2 = späterer Snapshot (weiter ohne "u",
        // Abkopplung unbestätigt mitgeführt).
        let mut h = Harness {
            revision: 5,
            instances: set(&["u", "rest"]),
            refs: set(&[]),
            deletes: vec![],
        };
        assert!(h.drain(6, &set(&["rest"]), &sale_pending()));
        assert_eq!(h.deletes, vec!["u".to_string()]);
        assert!(!h.instances.contains("u"));
        // R2 trägt die unbestätigte Entfernung weiter — idempotent, kein
        // zweiter Löschvorgang, keine Freigabe von "rest".
        assert!(h.drain(7, &set(&["rest"]), &sale_pending()));
        assert_eq!(h.deletes.len(), 1, "kein zweiter Löschvorgang");
        assert!(
            h.instances.contains("rest"),
            "neuer Bestand bleibt erhalten"
        );
        assert_eq!(h.revision, 7);
    }

    #[test]
    fn newer_snapshot_first_supersedes_the_older_removal() {
        // Modell (K4, kein Produktionspfad): R2 wurde zuerst angewendet
        // (DB-Revision 7); der ältere R1 mit derselben Abkopplung wird
        // danach als superseded abgewiesen und löscht nichts — auch wenn
        // "u" dort ebenfalls fehlt.
        let mut h = Harness {
            revision: 5,
            instances: set(&["u"]),
            refs: set(&[]),
            deletes: vec![],
        };
        assert!(h.drain(7, &set(&[]), &sale_pending()));
        assert_eq!(h.deletes, vec!["u".to_string()]);
        // R1 nachträglich angeboten: superseded, kein erneuter Eingriff.
        assert!(!h.drain(6, &set(&[]), &sale_pending()));
        assert_eq!(h.deletes.len(), 1);
    }

    #[test]
    fn superseding_snapshot_without_the_removal_never_deletes() {
        // Modell (K4, kein Produktionspfad): R1 (pending "u") bleibt
        // unbestätigt liegen; R2 OHNE Abkopplung wird angewendet und
        // übertrifft R1. R1 löscht danach nichts mehr.
        let mut h = Harness {
            revision: 5,
            instances: set(&["u"]),
            refs: set(&[]),
            deletes: vec![],
        };
        assert!(h.drain(7, &set(&["u"]), &[]));
        assert!(h.deletes.is_empty());
        assert!(!h.drain(6, &set(&[]), &sale_pending()), "R1 ist superseded");
        assert!(h.deletes.is_empty(), "superseded löscht nie");
        assert!(h.instances.contains("u"));
    }

    #[test]
    fn buyback_after_sale_snapshot_releases_nothing() {
        // Modell (K4, kein Produktionspfad): R1 (Verkauf) angewendet: "u"
        // gelöscht. Buyback danach: "u" kehrt als neue Platzierung zurück
        // (R2 mit "u", ohne pending) — die Bestätigung von R1 hat die neue
        // Änderung nicht freigegeben, R2 löscht "u" nicht erneut.
        let mut h = Harness {
            revision: 5,
            instances: set(&["u"]),
            refs: set(&[]),
            deletes: vec![],
        };
        assert!(h.drain(6, &set(&[]), &sale_pending()));
        assert_eq!(h.deletes.len(), 1);
        // Buyback stellt "u" wieder her (Upsert-Pfad des Inventar-Vollwrites).
        h.instances.insert("u".to_string());
        assert!(h.drain(7, &set(&["u"]), &[]));
        assert_eq!(h.deletes.len(), 1, "kein zweiter Löschvorgang");
        assert!(h.instances.contains("u"), "rückgekauftes Item bleibt");
    }

    #[test]
    fn commit_failure_keeps_pending_and_retry_deletes_exactly_once() {
        // Modell (K4, kein Produktionspfad): Der Drain bricht ab, bevor
        // Revision/Zustand fortgeschrieben werden — die unbestätigte
        // Entfernung bleibt erhalten. Der wiederholte Drain löscht genau
        // einmal; ein dritter (Revision gleich) wird übersprungen. Die
        // Transaktionsatomarität des echten Commits ist hier nachgebaut,
        // nicht produktionsverifiziert.
        let mut h = Harness {
            revision: 5,
            instances: set(&["u"]),
            refs: set(&[]),
            deletes: vec![],
        };
        // Fehlgeschlagener Versuch: kein `drain`-Aufruf mit Wirkung.
        assert_eq!(h.revision, 5);
        assert!(h.instances.contains("u"));
        // Wiederholter Drain wendet an.
        assert!(h.drain(6, &set(&[]), &sale_pending()));
        assert_eq!(h.deletes, vec!["u".to_string()]);
        // Erneuter Drain derselben Revision: skip, kein zweiter Löschvorgang.
        assert!(!h.drain(6, &set(&[]), &sale_pending()));
        assert_eq!(h.deletes.len(), 1);
    }

    #[test]
    fn incomplete_recovery_blocks_startup_finalization() {
        assert!(
            !startup_finalization_allowed(true, 0),
            "Fehler → keine Bereinigung"
        );
        assert!(
            !startup_finalization_allowed(false, 3),
            "Restarbeit → keine vorzeitige Freigabe"
        );
        assert!(
            !startup_finalization_allowed(true, 2),
            "Fehler plus Restarbeit → keine Bereinigung"
        );
        assert!(
            startup_finalization_allowed(false, 0),
            "vollständig abgeschlossene Recovery → Finalisierung zulässig"
        );
    }

    fn row(char_id: i64, uuid: &str) -> FinalizationRow {
        FinalizationRow {
            char_id,
            item_uuid: uuid.into(),
            reason: "sold".into(),
        }
    }

    #[test]
    fn startup_finalize_deletes_only_unreferenced_existing_instances() {
        let rows = vec![
            row(7, "frei"),
            row(7, "belegt"),
            row(8, "weg"),
            row(8, "fremd"),
        ];
        let d = startup_finalize_decisions(
            &rows,
            &set(&["belegt", "fremd"]),
            &set(&["frei", "belegt"]),
        );
        assert_eq!(d.delete_instances, vec!["frei".to_string()]);
        // "weg": Instanzzeile fehlt bereits → nur Metadaten bereinigen.
        // "fremd": referenziert → Metadaten zur erneuten Prüfung erhalten.
        assert_eq!(d.drop_metadata, vec![(7, "frei".into()), (8, "weg".into())]);
    }

    #[test]
    fn already_drained_detachment_after_restart_is_a_metadata_cleanup() {
        // Bereits gedrainte Abkopplung nach Neustart: Instanzzeile weg,
        // Metadatenzeile liegt noch vor → reine Bereinigung, kein Löschen.
        let d = startup_finalize_decisions(&[row(7, "u")], &set(&[]), &set(&[]));
        assert!(d.delete_instances.is_empty());
        assert_eq!(d.drop_metadata, vec![(7, "u".into())]);
    }

    #[test]
    fn startup_finalize_ignores_blank_uuids_and_is_deterministic() {
        let rows = vec![row(9, "b"), row(7, "a"), row(7, "  "), row(9, "b")];
        let d = startup_finalize_decisions(&rows, &set(&[]), &set(&["a", "b"]));
        assert_eq!(d.delete_instances, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(d.drop_metadata, vec![(7, "a".into()), (9, "b".into())]);
    }

    #[test]
    fn detach_reason_db_roundtrip() {
        for (reason, db) in [
            (DetachReason::Sold, "sold"),
            (DetachReason::Merged, "merged"),
            (DetachReason::Discarded, "discarded"),
        ] {
            assert_eq!(reason.as_db(), db);
            assert_eq!(DetachReason::from_db(db), Some(reason));
        }
        assert_eq!(DetachReason::from_db("unbekannt"), None);
        assert_eq!(DetachReason::from_db(""), None);
    }

    #[test]
    fn runtime_id_has_stable_shape() {
        let id = new_runtime_id();
        assert!(id.starts_with("rt-"), "Kennung beginnt mit rt-, war: {id}");
        assert!(id.len() > "rt-1-0".len());
        assert_ne!(id, new_runtime_id(), "zwei Läufe erzeugen zwei Kennungen");
    }

    #[test]
    fn lifecycle_snapshot_wire_format_roundtrip() {
        let mut lc = ItemLifecycle::new();
        reconcile_after_take(&mut lc, &set(&[]), "u", DetachReason::Merged, RT, NOW);
        let view = lc.snapshot_view(4, RT);
        let json = serde_json::to_string(&view).expect("serialisierbar");
        let back: ItemLifecycleSnapshot = serde_json::from_str(&json).expect("lesbar");
        assert_eq!(back, view);
        assert_eq!(back.pending[0].reason, DetachReason::Merged);
    }

    // ── K1: Merge gespeicherter Pflichten (produktiv in db::apply_item_lifecycle) ──

    /// Gespeicherte Metadatenzeile, wie sie der Drain-Loader liefert.
    fn stored(uuid: &str) -> DetachedInstance {
        DetachedInstance {
            item_uuid: uuid.into(),
            reason: DetachReason::Sold,
            runtime_id: "rt-alt".into(),
            recorded_at_ms: NOW - 1,
            detached_at_revision: 6,
        }
    }

    /// Regression zu K1: Die DB enthält eine blockierte Pflicht, der RAM
    /// beginnt leer, der neue Snapshot trägt `Some(empty)` — die Pflicht
    /// bleibt über den Merge erhalten. Erst danach entscheidet die
    /// Referenzprüfung über die Löschung; beide aufgerufenen Funktionen sind
    /// Produktionscode (kein SQL-Modell).
    #[test]
    fn restart_with_empty_snapshot_preserves_blocked_duty() {
        let merged = merge_pending(&[stored("u")], &[], &set(&[]));
        assert_eq!(merged.len(), 1, "Pflicht darf nicht verschwinden");
        assert_eq!(merged[0].item_uuid, "u");
        assert_eq!(
            merged[0].detached_at_revision, 6,
            "Zuordnung (Revision/Herkunftslauf) bleibt erhalten"
        );
        assert_eq!(merged[0].runtime_id, "rt-alt");
        // Fremde Referenz schützt die erhaltene Pflicht vor Löschung.
        assert!(
            deletable_candidates(&merged, &set(&[]), &set(&["u"])).is_empty(),
            "fremd referenzierte Pflicht wird nicht gelöscht"
        );
        // Ohne Referenz wäre dieselbe Pflicht Kandidat.
        assert_eq!(
            deletable_candidates(&merged, &set(&[]), &set(&[])),
            vec!["u".to_string()]
        );
    }

    /// Passende Wiedereinsetzung hebt nur die passende Pflicht auf; fremde
    /// Pflichten und Snapshot-Neueinträge bleiben bestehen (Gleichstand
    /// gewinnt der Snapshot mit frischem Stempel).
    #[test]
    fn matching_reinsertion_cancels_only_that_duty() {
        let merged = merge_pending(&[stored("u"), stored("v")], &[], &set(&["u"]));
        assert_eq!(
            merged.iter().map(|d| d.item_uuid.as_str()).collect::<Vec<_>>(),
            vec!["v"],
            "nur die wiedereingesetzte Pflicht entfällt"
        );
        let fresh = DetachedInstance {
            detached_at_revision: 9,
            ..stored("v")
        };
        let merged = merge_pending(&[stored("v")], &[fresh], &set(&[]));
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].detached_at_revision, 9,
            "Snapshot-Eintrag gewinnt mit frischem Stempel"
        );
        // Leere UUIDs entfallen beidseitig.
        let blank = DetachedInstance {
            item_uuid: "  ".into(),
            ..stored("v")
        };
        let blank_stored = blank.clone();
        let merged = merge_pending(&[blank_stored], &[blank], &set(&[]));
        assert!(merged.is_empty());
    }
}
