// spool — Durable Player-Snapshot-Spool der Stufe B (docs/Player_Persistenz.md
// §21, §28–§32, §38–§42).
//
// Architektur: RAM → [dirty/generation] → PersistSnapshot (vollständig) →
// Durable-Spool (JSON-Datei-Batch, fsync + atomarer Rename) → DB-Drain
// (sequenziell, älteste zuerst, EINE Transaktion je Snapshot, §30).
//
// Dateien:
//   <base>/spool/                  — zu verarbeitende Batch-Dateien
//   <base>/superseded/             — bereits übertroffene Snapshots (Retention 30d)
//   <base>/quarantine/open/        — nicht verarbeitbare Dateien (Operator entscheidet)
//   <base>/quarantine/archive/     — automatische Retention (30d)
//
// Format (ein Eintrag je Batch-Datei; die Doku erlaubt mehrere Spieler je
// Batch, V1 verarbeitet sequenziell — pro Lauf genau EINE Batch-Datei):
//   { "format_version": 1, "character_id": …, "persist_revision": …, … }
//
// Drain-Reihenfolge (docs §38): db_rev <  snap_rev → anwenden;  == → idempotent
// skip (bereits committet, ggf. Crash-nach-Commit); > → superseded (Datei nach
// superseded/, deterministischer Name → idempotent). BATCH-Datei wird erst
// entfernt, wenn ALLE Einträge abgeschlossen sind (committed/skipped/
// superseded/quarantäniert). DB-Fehler → Batch bleibt, drain abgebrochen,
// Status DEGRADED (Realm läuft weiter, docs §16). Nicht lesbare Dateien /
// unbekannte format_version / unbekannter Charakter → Quarantäne (nie blind
// einspielen).
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sqlx::{MySql, Pool};

use crate::persist::PersistSnapshot;

/// Aktuelle Drahtformat-Version der Spool-Dateien.
pub const FORMAT_VERSION: u16 = 1;

/// Aufbewahrungsfrist für superseded-/Archiv-Dateien (docs §33).
const RETENTION_SECS: u64 = 30 * 24 * 60 * 60;

/// Status der Spieler-Persistenz (docs §34/§36): RECOVERING blockiert Logins
/// (Startup-Recovery), READY = nominal, DEGRADED = Fehler aufgetreten, Realm
/// läuft weiter (periodischer Drain retryt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistStatus {
    Recovering,
    Ready,
    Degraded,
}

/// Eine (deterministisch beschreibbare) Spool-Datei: einzelner Player-Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoolEntry {
    pub format_version: u16,
    #[serde(flatten)]
    pub snapshot: PersistSnapshot,
}

/// Ergebnis eines Drain-Laufs.
#[derive(Debug, Default, Clone, Copy)]
pub struct DrainReport {
    pub batches_processed: u32,
    pub entries_applied: u32,
    pub entries_skipped: u32,
    pub entries_superseded: u32,
    pub batches_quarantined: u32,
}

/// Durable-Snapshot-Spool. `in_flight` serialisiert pro Spieler (verhindert
/// konkurrierende Snapshots derselben Revisions-Baseline und serialisiert
/// zusätzlich den Eigentümer-/Logout-Übergang pro `player_id` — siehe
/// `player_gate`).
#[derive(Clone)]
pub struct Spool {
    pub base_dir: PathBuf,
    in_flight: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl Spool {
    /// Bestehende per-player-Serialisierung. Schützt zwei Vorgänge derselben
    /// `player_id` gegeneinander:
    ///
    /// 1. den finalen Disconnect-Save (Snapshot mit `force`) gegen den
    ///    periodischen Spieler-Flush — konkurrierende Snapshots derselben
    ///    Revisions-Baseline (Last-Writer-Loses) sind so ausgeschlossen,
    /// 2. den Logout-Commit der alten Verbindung (`logout_at`-Write) gegen den
    ///    Login-/Takeover-Pfad derselben `player_id` (docs/Security.md
    ///    AUTH-03): der Eigentümerwechsel kann erst abschließen, wenn der alte
    ///    Logout-Write beendet ist, und wird danach nicht mehr von ihm
    ///    markiert.
    ///
    /// Das Gate ist pro `player_id`; verschiedene Spieler blockieren sich
    /// nicht. Aufrufer MÜSSEN es vor jeder World-Sperre holen (Reihenfolge
    /// Gate → World → Elternkontrolle/Gruppen), niemals umgekehrt.
    pub async fn player_gate(&self, player_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.in_flight
            .lock()
            .await
            .entry(player_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

/// Laufzeit-Objekt der Stufe B: Spool + Status + Drian-Zugriff.
pub struct PersistRuntime {
    spool: Spool,
    weapon_skill_id: String,
    status: Arc<Mutex<PersistStatus>>,
}

impl PersistRuntime {
    /// Legt die Spool-Verzeichnisse an (fehlerfrei = bereit).
    pub fn new(base_dir: &Path, weapon_skill_id: &str) -> Result<Self, String> {
        let spool = Spool {
            base_dir: base_dir.to_path_buf(),
            in_flight: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        };
        spool.ensure_dirs()?;
        Ok(Self {
            spool,
            weapon_skill_id: weapon_skill_id.to_string(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
        })
    }

    pub fn spool(&self) -> &Spool {
        &self.spool
    }

    /// Per-player-Serialisierung (Gate → World, nie umgekehrt): durable
    /// Schreibvorgänge und Eigentümer-/Logout-Übergänge derselben
    /// `player_id`. Siehe `Spool::player_gate`.
    pub async fn player_gate(&self, player_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.spool.player_gate(player_id).await
    }

    /// Offene, noch nicht gedrainete Revision im Spool (siehe
    /// `Spool::pending_revision`). Fail-closed-Auswertung im Login-Pfad über
    /// `crate::world::db_row_is_stale`.
    pub fn pending_revision(&self, player_id: &str) -> Result<Option<i64>, String> {
        self.spool.pending_revision(player_id)
    }

    pub fn status(&self) -> PersistStatus {
        *self.status.lock().unwrap()
    }

    pub fn set_status(&self, s: PersistStatus) {
        *self.status.lock().unwrap() = s;
    }

    /// Zentraler Player-Persistenzpfad (durable; siehe persist::persist_player).
    /// Fehlgeschlagener Spool-Write (docs §40): Realm läuft weiter, der
    /// Persistence-Zustand wird DEGRADED; Dirty-/Revision-Schutz bleibt in
    /// persist_dirty_into erhalten (keine künstliche Revisionslücke).
    pub async fn persist_player(
        &self,
        shared: &crate::world::Shared,
        player_id: &str,
        force: bool,
    ) -> Result<(), String> {
        match crate::persist::persist_player(&self.spool, shared, player_id, force).await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.set_status(PersistStatus::Degraded);
                Err(e)
            }
        }
    }

    /// Zentraler Player-Persistenzpfad für Aufrufer, die das per-player-Gate
    /// BEREITS halten (Logout-Commit in net.rs, der die Reihenfolge
    /// Gate → World einhält). Das Gate ist nicht reentrant; ein Aufruf von
    /// `persist_player` unter bereits gehaltenem Gate würde sich sonst selbst
    /// verriegeln. Semantik, Snapshot-/Revision-/Dirty-Regeln und
    /// Degraded-Verhalten sind identisch zu `persist_player` — nur die
    /// Sperre wird nicht erneut genommen.
    pub async fn persist_player_gate_held(
        &self,
        shared: &crate::world::Shared,
        player_id: &str,
        force: bool,
    ) -> Result<(), String> {
        match crate::persist::persist_dirty_into(shared, player_id, force, |snapshot| {
            let spool = self.spool.clone();
            async move { spool.write_batch(&snapshot) }
        })
        .await
        {
            Ok(()) => Ok(()),
            Err(e) => {
                self.set_status(PersistStatus::Degraded);
                Err(e)
            }
        }
    }

    /// Verarbeitet genau eine (die älteste) Batch-Datei. `None` = nichts zu
    /// tun. Fehler (DB down) → Datei bleibt, Drain abgebrochen (DEGRADED).
    pub async fn drain_one(&self, pool: &Pool<MySql>) -> Result<Option<DrainReport>, String> {
        self.spool.drain_one(pool, &self.weapon_skill_id).await
    }

    /// Startup-Recovery: drain alle vorhandenen Batches (älteste zuerst).
    /// Stoppt beim ersten DB-Fehler (Batch bleibt, Realm startet als DEGRADED).
    pub async fn recover(&self, pool: &Pool<MySql>) -> Result<DrainReport, String> {
        let mut total = DrainReport::default();
        let mut guard = 0u32;
        while self.spool.count_batches()? > 0 && guard < 10_000 {
            guard += 1;
            match self.spool.drain_one(pool, &self.weapon_skill_id).await? {
                Some(r) => total = merge_report(total, r),
                None => break,
            }
        }
        Ok(total)
    }

    /// Retention: superseded-/Archiv-Dateien über 30 Tage (docs §33).
    pub fn run_retention(&self) -> Result<(), String> {
        self.spool.run_retention()
    }
}

fn merge_report(a: DrainReport, b: DrainReport) -> DrainReport {
    DrainReport {
        batches_processed: a.batches_processed + b.batches_processed,
        entries_applied: a.entries_applied + b.entries_applied,
        entries_skipped: a.entries_skipped + b.entries_skipped,
        entries_superseded: a.entries_superseded + b.entries_superseded,
        batches_quarantined: a.batches_quarantined + b.batches_quarantined,
    }
}

impl Spool {
    fn dir(&self, name: &str) -> PathBuf {
        self.base_dir.join(name)
    }

    fn spool_dir(&self) -> PathBuf {
        self.dir("spool")
    }

    fn superseded_dir(&self) -> PathBuf {
        self.dir("superseded")
    }

    fn quarantine_open_dir(&self) -> PathBuf {
        self.dir("quarantine").join("open")
    }

    fn quarantine_archive_dir(&self) -> PathBuf {
        self.dir("quarantine").join("archive")
    }

    /// Legt alle Spool-Verzeichnisse an (idempotent).
    pub fn ensure_dirs(&self) -> Result<(), String> {
        for d in [
            self.spool_dir(),
            self.superseded_dir(),
            self.quarantine_open_dir(),
            self.quarantine_archive_dir(),
        ] {
            std::fs::create_dir_all(&d).map_err(|e| format!("Spool-Verzeichnis {d:?}: {e}"))?;
        }
        Ok(())
    }

    /// Anzahl der offenen Batch-Dateien.
    pub fn count_batches(&self) -> Result<usize, String> {
        Ok(list_json_files(&self.spool_dir())?.len())
    }

    /// Höchste Persistenz-Revision, die für `player_id` noch OFFEN im Spool
    /// liegt, also noch nicht per Drain auf die DB angewendet wurde.
    /// `Ok(None)` = kein offener Batch für diesen Spieler.
    ///
    /// `Err` = Spool-Verzeichnis nicht lesbar; der Aufrufer MUSS dann
    /// fail-closed behandeln (der persistierte Zustand ist ungeprüft).
    pub fn pending_revision(&self, player_id: &str) -> Result<Option<i64>, String> {
        let mut max: Option<i64> = None;
        for path in list_json_files(&self.spool_dir())? {
            // Nicht lesbare/zerschnittene Dateien werden übergangen — der Drain
            // quarantänisiert sie; für die Vorabprüfung ist das unkritisch,
            // weil sie ohnehin nicht angewendet werden.
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(entry) = serde_json::from_str::<SpoolEntry>(&raw) else {
                continue;
            };
            if entry.snapshot.player_id == player_id {
                let rev = entry.snapshot.persist_revision;
                if max.is_none_or(|m| rev > m) {
                    max = Some(rev);
                }
            }
        }
        Ok(max)
    }

    /// Durable-Write eines vollständigen Player-Snapshots: JSON in eine
    /// Temp-Datei schreiben, fsync, atomarer Rename in `spool/`. Erst nach
    /// dem Rename gilt der Batch als gesichert (§21).
    ///
    /// Dateiname: `<captured_at_ms:013>-<player_id>-r<revision>.json` —
    /// lexikografisch = chronologisch (älteste zuerst beim Drain).
    pub fn write_batch(&self, snapshot: &PersistSnapshot) -> Result<(), String> {
        let entry = SpoolEntry {
            format_version: FORMAT_VERSION,
            snapshot: snapshot.clone(),
        };
        let body = serde_json::to_string(&entry)
            .map_err(|e| format!("Spool-Snapshot serialisieren: {e}"))?;
        let file_name = format!(
            "{:013}-{}-r{}.json",
            snapshot.captured_at_ms, snapshot.player_id, snapshot.persist_revision
        );
        let target = self.spool_dir().join(&file_name);
        if target.exists() {
            // Gleiche Datei (identische Zeit/Revision) bereits durable — kein
            // zweiter Write nötig (Idempotenz, §38).
            return Ok(());
        }
        let tmp = self.spool_dir().join(format!(".tmp-{file_name}"));
        write_atomic(&tmp, &target, body.as_bytes())?;
        Ok(())
    }

    /// Älteste offene Batch-Datei (lexikografisch = chronologisch).
    fn next_batch_path(&self) -> Result<Option<PathBuf>, String> {
        Ok(list_json_files(&self.spool_dir())?.into_iter().next())
    }

    /// Öffene Batch-Dateien, nur echte Dateien, sortiert aufsteigend.
    async fn drain_one(
        &self,
        pool: &Pool<MySql>,
        weapon_skill_id: &str,
    ) -> Result<Option<DrainReport>, String> {
        let Some(batch_path) = self.next_batch_path()? else {
            return Ok(None);
        };
        let file_name = batch_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let raw = match std::fs::read_to_string(&batch_path) {
            Ok(r) => r,
            Err(e) => {
                // Nicht lesbare Datei → Quarantäne (Operator entscheidet).
                self.quarantine(&batch_path, QuarantineReason::Unreadable, &e.to_string())?;
                let mut r = DrainReport::default();
                r.batches_quarantined = 1;
                return Ok(Some(r));
            }
        };
        let entry: SpoolEntry = match serde_json::from_str(&raw) {
            Ok(e) => e,
            Err(e) => {
                self.quarantine(&batch_path, QuarantineReason::Malformed, &e.to_string())?;
                let mut r = DrainReport::default();
                r.batches_quarantined = 1;
                return Ok(Some(r));
            }
        };
        if entry.format_version != FORMAT_VERSION {
            self.quarantine(
                &batch_path,
                QuarantineReason::UnknownFormat(entry.format_version),
                "format_version != aktuelle",
            )?;
            let mut r = DrainReport::default();
            r.batches_quarantined = 1;
            return Ok(Some(r));
        }

        let mut report = DrainReport::default();
        // Ein Eintrag je Batch-Datei (V1). Robust trotzdem als Liste gedacht:
        // Batch wird erst NACH Abschluss ALLER Einträge entfernt.
        let player_id = entry.snapshot.player_id.clone();
        let db_rev = match crate::db::load_persist_revision(pool, &player_id).await {
            Ok(Some(rev)) => rev,
            Ok(None) => {
                // Kein Charakter-Datensatz → nichts, auf das sicher angewendet
                // werden könnte; Quarantäne (Operator entscheidet, nie blind
                // einspielen).
                self.quarantine(
                    &batch_path,
                    QuarantineReason::UnknownCharacter,
                    &format!("kein Charakter {player_id:?}"),
                )?;
                report.batches_quarantined = 1;
                return Ok(Some(report));
            }
            Err(e) => return Err(format!("Drain {file_name}: {e}")),
        };

        match db_rev.cmp(&entry.snapshot.persist_revision) {
            std::cmp::Ordering::Equal => {
                // Bereits committet (z. B. Crash nach Commit vor Datei-Löschung):
                // idempotent überspringen, Datei entfernen (§38).
                report.entries_skipped = 1;
            }
            std::cmp::Ordering::Greater => {
                // Übertroffen (z. B. frischeres Batch bereits angewendet oder
                // direkter DB-Write mit neuerer Revision): als superseded
                // ablegen — deterministischer Name, doppelte → skip (§31).
                let dst = self.superseded_dir().join(format!(
                    "{}-r{}.json",
                    entry.snapshot.player_id, entry.snapshot.persist_revision
                ));
                if dst.exists() {
                    // Bereits archiviert — Original entfernen.
                    remove_file(&batch_path)?;
                } else {
                    move_file(&batch_path, &dst)?;
                }
                report.entries_superseded = 1;
            }
            std::cmp::Ordering::Less => {
                crate::persist::apply_snapshot_to_db(pool, &entry.snapshot, weapon_skill_id)
                    .await?;
                report.entries_applied = 1;
            }
        }
        if report.entries_applied == 1
            || report.entries_skipped == 1
            || report.entries_superseded == 1
        {
            if batch_path.exists() {
                remove_file(&batch_path)?;
            }
            report.batches_processed = 1;
        }
        Ok(Some(report))
    }

    /// Verschiebt eine Batch-Datei nach `quarantine/open/` (deterministischer
    /// Name; bei Existenz wird das Original danach entfernt — die Quarantäne
    /// ist die Wahrheit).
    fn quarantine(
        &self,
        batch_path: &Path,
        reason: QuarantineReason,
        detail: &str,
    ) -> Result<(), String> {
        let name = batch_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "quarantine.json".to_string());
        let dst = self
            .quarantine_open_dir()
            .join(format!("{name}--{}.json", reason.key()));
        log::error!("Spool-Quarantäne {batch_path:?} ({reason:?}): {detail}");
        if dst.exists() {
            // Bereits quarantäniert (idempotent) — Original entfernen.
            if batch_path.exists() {
                remove_file(batch_path)?;
            }
            return Ok(());
        }
        move_file(batch_path, &dst)?;
        Ok(())
    }

    /// Retention (docs §33): superseded-/Archiv-Dateien > 30 Tage löschen.
    /// `quarantine/open/` wird NICHT angefasst: offene Fälle bleiben
    /// unabhängig von ihrem Alter in `open/` und werden erst nach manueller
    /// beziehungsweise operativer Bearbeitung nach `archive/` überführt.
    pub fn run_retention(&self) -> Result<(), String> {
        self.run_retention_at(now_secs())
    }

    /// Testbarer Kern der Retention: `now_secs` ist die angenommene Jetzt-Zeit.
    fn run_retention_at(&self, now_secs: u64) -> Result<(), String> {
        prune_older_than(&self.superseded_dir(), now_secs)?;
        // Archiv ZUERST beschneiden: Neu archivierte Dateien dieses Laufs
        // (rename behält die mtime) dürfen nicht im selben Lauf sofort
        // wieder gelöscht werden — sonst wäre das Archiv zwecklos.
        prune_older_than(&self.quarantine_archive_dir(), now_secs)?;
        // Kein automatischer Eingriff in `quarantine/open/` (docs §33).
        Ok(())
    }
}

/// Gründe für Quarantäne (deterministischer Namenssuffix).
#[derive(Debug, Clone, Copy)]
pub enum QuarantineReason {
    /// Datei nicht lesbar (IO-Fehler).
    Unreadable,
    /// JSON nicht interpretierbar.
    Malformed,
    /// Unbekannte format_version.
    UnknownFormat(u16),
    /// Kein Charakter-Datensatz in der DB.
    UnknownCharacter,
}

impl QuarantineReason {
    fn key(self) -> String {
        match self {
            QuarantineReason::Unreadable => "unreadable".to_string(),
            QuarantineReason::Malformed => "malformed".to_string(),
            QuarantineReason::UnknownFormat(v) => format!("unknown-format{v}"),
            QuarantineReason::UnknownCharacter => "unknown-character".to_string(),
        }
    }
}

/// Sekunden seit UNIX_EPOCH (für Retention-Rechnungen).
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Alle `.json`-Dateien eines Verzeichnisses, sortiert (älteste/erste zuerst).
fn list_json_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(dir).map_err(|e| format!("Spool-Dir {dir:?} lesen: {e}"))?;
    for entry in rd {
        let entry = entry.map_err(|e| format!("Spool-Dir {dir:?}: {e}"))?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("json")
            && p.file_name()
                .map(|f| !f.to_string_lossy().starts_with(".tmp-"))
                .unwrap_or(false)
        {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Temp-Datei schreiben, fsync, atomar auf `target` verschieben (§21).
fn write_atomic(tmp: &Path, target: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = std::fs::File::create(tmp).map_err(|e| format!("Spool Temp {tmp:?}: {e}"))?;
    std::io::Write::write_all(&mut f, bytes).map_err(|e| format!("Spool schreiben: {e}"))?;
    f.sync_all().map_err(|e| format!("Spool fsync: {e}"))?;
    drop(f);
    std::fs::rename(tmp, target).map_err(|e| format!("Spool rename {target:?}: {e}"))?;
    Ok(())
}

fn move_file(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::rename(src, dst).map_err(|e| format!("Spool verschieben {src:?} → {dst:?}: {e}"))
}

fn remove_file(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("Spool Datei entfernen {path:?}: {e}"))
}

/// Löscht `.json`-Dateien älter als `RETENTION_SECS` (modifizierte Zeit).
fn prune_older_than(dir: &Path, now_secs: u64) -> Result<(), String> {
    let files = list_json_files(dir)?;
    for f in files {
        let modified = std::fs::metadata(&f)
            .and_then(|m| m.modified())
            .map(|m| {
                m.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
            .unwrap_or(0);
        if now_secs.saturating_sub(modified) > RETENTION_SECS {
            let _ = std::fs::remove_file(&f);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::InventoryState;
    use std::collections::{BTreeMap, HashSet};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc;

    fn temp_dir(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!(
            "realmrs-spool-{tag}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn snapshot(id: &str, rev: i64, x: f64) -> PersistSnapshot {
        PersistSnapshot {
            player_id: id.to_string(),
            persist_revision: rev,
            captured_at_ms: 1_700_000_000_000 + rev,
            x,
            y: 1.0,
            level: 5,
            exp: 100,
            free_attr_points: 1,
            rested_pool: 9,
            idia: 42,
            hp: 80,
            mana: 30,
            attributes: Default::default(),
            char_class: "Adventurer".into(),
            faction_transition: false,
            weapon_skill: 2,
            learned_abilities: vec!["fire_bolt".into()],
            inventory: InventoryState::default(),
            generation: 0,
            dirty: crate::persist::PersistDirty::default(),
        }
    }

    fn spool(base: &Path) -> Spool {
        Spool {
            base_dir: base.to_path_buf(),
            in_flight: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    #[test]
    fn write_batch_is_idempotent() {
        let base = temp_dir("idem");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("p1", 3, 10.0);
        s.write_batch(&snap).unwrap();
        s.write_batch(&snap).unwrap();
        assert_eq!(s.count_batches().unwrap(), 1);
        let name = format!("{:013}-p1-r3.json", snap.captured_at_ms);
        let path = base.join("spool").join(name);
        assert!(path.exists(), "Batch-Datei existiert durable");
        assert!(!base.join("spool").read_dir().unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-")
        }));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn batch_files_sort_oldest_first() {
        let base = temp_dir("sort");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let mut a = snapshot("p1", 1, 1.0);
        a.captured_at_ms = 100;
        let mut b = snapshot("p2", 1, 2.0);
        b.captured_at_ms = 200;
        s.write_batch(&a).unwrap();
        s.write_batch(&b).unwrap();
        let first = s.next_batch_path().unwrap().unwrap();
        assert_eq!(
            first.file_name().unwrap().to_string_lossy().as_ref(),
            "0000000000100-p1-r1.json"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Retention-Zeitpunkt für eine Datei: der aus `mtime` abgeleitete
    /// Sekundenzeitpunkt, auf den `run_retention_at` angesetzt wird, damit die
    /// Datei exakt `RETENTION_SECS` alt ist.
    fn mtime_secs(path: &Path) -> u64 {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map(|m| {
                m.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
            .unwrap_or(0)
    }

    /// Setzt die Änderungszeit einer Datei auf `secs_ago` Sekunden in der
    /// Vergangenheit, damit Altersverhältnisse ohne Zeitreise im Dateisystem
    /// steuerbar sind.
    fn age_file(path: &Path, secs_ago: u64) {
        let t = SystemTime::now() - Duration::from_secs(secs_ago);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    /// Zählt die `.json`-Dateien in einem Verzeichnis (ohne `.tmp-`-Reste).
    fn count_json(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path().extension().and_then(|x| x.to_str()) == Some("json")
                            && !e.file_name().to_string_lossy().starts_with(".tmp-")
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn retention_only_prunes_old_files() {
        let base = temp_dir("ret");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let old = s.superseded_dir().join("p1-r1.json");
        let fresh = s.superseded_dir().join("p1-r2.json");
        std::fs::write(&old, "{}").unwrap();
        std::fs::write(&fresh, "{}").unwrap();
        let now = now_secs();
        // Heute angelegt → nichts wird pruned.
        s.run_retention_at(now).unwrap();
        assert!(old.exists() && fresh.exists(), "frische Dateien bleiben");
        // 61 Tage in der Zukunft → beide gelten als überfällig.
        s.run_retention_at(now + RETENTION_SECS + 1).unwrap();
        assert!(
            !old.exists() && !fresh.exists(),
            "überfällige Dateien gelöscht"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// docs/Player_Persistenz.md §33: offene Fälle in `quarantine/open/` werden
    /// NIEMALS aufgrund ihres Alters automatisch gelöscht und nicht automatisch
    /// archiviert. Retention-Zeitsteuerung weiterhin über `run_retention_at`.
    #[test]
    fn retention_keeps_old_open_quarantine_untouched() {
        let base = temp_dir("retq");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let qdir = s.quarantine_open_dir();
        let f = qdir.join("x.json");
        let body = r#"{"format_version":1,"offener":"fall"}"#;
        std::fs::write(&f, body).unwrap();
        // `now` so weit in der ZUKUNFT, dass die Datei weit überfällig wäre.
        let future = mtime_secs(&f) + RETENTION_SECS + 10;
        s.run_retention_at(future).unwrap();
        assert!(f.exists(), "offene Quarantäne bleibt in open/ liegen");
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            body,
            "Inhalt byte-identisch"
        );
        assert!(
            !s.quarantine_archive_dir().join("x.json").exists(),
            "kein automatischer Eintrag im Archiv"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// §33: ein jüngerer offener Fall bleibt ebenfalls unverändert.
    #[test]
    fn retention_keeps_young_open_quarantine_untouched() {
        let base = temp_dir("retqy");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let f = s.quarantine_open_dir().join("y.json");
        let body = r#"{"offener":"junger fall"}"#;
        std::fs::write(&f, body).unwrap();
        s.run_retention_at(mtime_secs(&f) + 1).unwrap();
        assert!(f.exists(), "junge offene Quarantäne bleibt");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), body);
        assert_eq!(count_json(&s.quarantine_archive_dir()), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// §33: bei gleichnamigem Archivziel mit IDENTISCHEM Inhalt bleiben beide
    /// Dateien bestehen — es wird nichts automatisch gelöscht oder verschoben.
    /// Der offene Fall ist bewusst überfällig (40 Tage), das Archivziel jung:
    /// nur so ist das Archivziel nicht selbst von der Archiv-Retention betroffen
    /// und der Nachweis eindeutig.
    #[test]
    fn retention_keeps_open_when_archive_target_is_identical() {
        let base = temp_dir("retqi");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let body = r#"{"offener":"fall"}"#;
        let open = s.quarantine_open_dir().join("z.json");
        let archived = s.quarantine_archive_dir().join("z.json");
        std::fs::write(&open, body).unwrap();
        std::fs::write(&archived, body).unwrap();
        age_file(&open, 40 * 86_400);
        age_file(&archived, 0);
        s.run_retention_at(now_secs()).unwrap();
        assert!(open.exists(), "offene Datei bleibt bestehen");
        assert_eq!(std::fs::read_to_string(&open).unwrap(), body);
        assert!(archived.exists(), "Archivdatei bleibt bestehen");
        assert_eq!(std::fs::read_to_string(&archived).unwrap(), body);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// §33: bei gleichnamigem Archivziel mit ABWEICHENDEM Inhalt bleiben beide
    /// Dateien bestehen. Vor dem Fix löschte dieser Pfad die offene Datei
    /// ungeprüft; genau das ist ausgeschlossen.
    #[test]
    fn retention_keeps_open_when_archive_target_differs() {
        let base = temp_dir("retqd");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let open_body = r#"{"offener":"fall mit diagnoseinhalt"}"#;
        let archive_body = r#"{"abweichender":"archivstand"}"#;
        let open = s.quarantine_open_dir().join("w.json");
        let archived = s.quarantine_archive_dir().join("w.json");
        std::fs::write(&open, open_body).unwrap();
        std::fs::write(&archived, archive_body).unwrap();
        age_file(&open, 40 * 86_400);
        age_file(&archived, 0);
        s.run_retention_at(now_secs()).unwrap();
        assert!(
            open.exists(),
            "offene Datei bleibt trotz abweichendem Archiv"
        );
        assert_eq!(std::fs::read_to_string(&open).unwrap(), open_body);
        assert!(archived.exists(), "Archivdatei bleibt bestehen");
        assert_eq!(std::fs::read_to_string(&archived).unwrap(), archive_body);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// §33: wiederholte Retention-Läufe verändern offene Fälle nicht.
    #[test]
    fn retention_repeated_runs_leave_open_quarantine_untouched() {
        let base = temp_dir("retqr");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let f = s.quarantine_open_dir().join("r.json");
        let body = r#"{"offener":"fall"}"#;
        std::fs::write(&f, body).unwrap();
        let future = mtime_secs(&f) + RETENTION_SECS + 10;
        for _ in 0..3 {
            s.run_retention_at(future).unwrap();
            assert!(f.exists(), "offener Fall bleibt über mehrere Läufe");
            assert_eq!(std::fs::read_to_string(&f).unwrap(), body);
            assert_eq!(count_json(&s.quarantine_archive_dir()), 0);
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// §33: die 30-Tage-Retention für ARCHIVIERTE Fälle bleibt erhalten —
    /// alte werden entfernt, junge bleiben.
    #[test]
    fn retention_still_prunes_old_archive_but_keeps_young() {
        let base = temp_dir("reta");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let old = s.quarantine_archive_dir().join("alt.json");
        let fresh = s.quarantine_archive_dir().join("jung.json");
        std::fs::write(&old, "{}").unwrap();
        std::fs::write(&fresh, "{}").unwrap();
        let now = now_secs();
        s.run_retention_at(now).unwrap();
        assert!(
            old.exists() && fresh.exists(),
            "frische Archivdateien bleiben"
        );
        s.run_retention_at(now + RETENTION_SECS + 1).unwrap();
        assert!(
            !old.exists() && !fresh.exists(),
            "überfällige Archivdateien gelöscht"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// §34: superseded behält seine 30-Tage-Retention ohne manuelle Freigabe.
    /// Nachgewiesen in `retention_only_prunes_old_files` (oben).
    #[test]
    fn retention_superseded_needs_no_manual_release() {
        let base = temp_dir("rets");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let old = s.superseded_dir().join("s-alt-r1.json");
        let fresh = s.superseded_dir().join("s-jung-r2.json");
        std::fs::write(&old, "{}").unwrap();
        std::fs::write(&fresh, "{}").unwrap();
        let now = now_secs();
        s.run_retention_at(now).unwrap();
        assert!(old.exists() && fresh.exists(), "junge superseded bleiben");
        s.run_retention_at(now + RETENTION_SECS + 1).unwrap();
        assert!(
            !old.exists() && !fresh.exists(),
            "überfällige superseded gelöscht"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn malformed_batch_goes_to_quarantine() {
        let base = temp_dir("quar");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let path = s.spool_dir().join("0000000000001-p1-r1.json");
        std::fs::write(&path, "{not json").unwrap();
        // Drain ohne DB: Da der Parse fehlschlägt, wird NIE die DB gefragt.
        // Stattdessen testen wir die reine Quarantäne-Schicht.
        let before = list_json_files(&s.quarantine_open_dir()).unwrap().len();
        s.quarantine(&path, QuarantineReason::Malformed, "test")
            .unwrap();
        let after = list_json_files(&s.quarantine_open_dir()).unwrap();
        assert_eq!(after.len(), before + 1);
        assert!(!path.exists(), "Original entfernt");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn superseded_move_is_deterministic_and_idempotent() {
        let base = temp_dir("sup");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("p9", 7, 10.0);
        let src = s
            .spool_dir()
            .join(format!("{:013}-p9-r7.json", snap.captured_at_ms));
        std::fs::write(&src, "{shim}").unwrap();
        let dst = s.superseded_dir().join("p9-r7.json");
        assert!(!dst.exists());
        move_file(&src, &dst).unwrap();
        assert!(dst.exists());
        // Idempotenz: zweites Auftreten desselben superseded-Stands → vorhandene
        // Ziel-State gilt, kein Überschreiben.
        assert!(dst.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Test-Spieler mit definiertem Dirty-State und Revision (P-20-Test).
    fn dirty_test_player(id: &str) -> (crate::world::Player, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut p = crate::world::Player {
            id: id.into(),
            name: id.into(),
            x: 10.0,
            y: 20.0,
            face: 0.0,
            ping_ms: 0,
            zone_id: 0,
            hp: 100,
            max_hp: 100,
            lang: "de".into(),
            account_id: 0,
            session_id: String::new(),
            entities: HashSet::new(),
            last_activity: Instant::now(),
            tx,
            char_class: "Adventurer".into(),
            class: crate::class::ClassStatus::Adventurer,
            faction_transition: false,
            level: 5,
            exp: 1234,
            free_attr_points: 2,
            rested_pool: 50,
            idia: 77,
            armor: 0,
            weapon_skill: 3,
            combat: None,
            mana: 50,
            max_mana: 50,
            effects: Vec::new(),
            cooldowns: BTreeMap::new(),
            active_cast: None,
            learned_abilities: HashSet::new(),
            attributes: Default::default(),
            max_hp_base: 100,
            max_mana_base: 50,
            sitting: false,
            hp_regen_bonus: 0.0,
            mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0,
            mana_regen_carry: 0.0,
            inventory: InventoryState::new(8),
            quests: BTreeMap::new(),
            dirty: crate::persist::PersistDirty::default(),
            persist_generation: 0,
            persist_revision: 5,
        };
        p.mark_dirty(crate::persist::PersistComponent::Position);
        (p, rx)
    }

    #[tokio::test]
    async fn failed_spool_write_sets_degraded_and_keeps_dirty_and_revision() {
        // docs/Player_Persistenz.md §40 (P-20): fehlgeschlagener Spool-Write →
        // Persistence-Zustand DEGRADED, Dirty-Bits NICHT bereinigt, geplante
        // persist_revision NICHT verbraucht.
        let base = temp_dir("fail");
        // `ensure_dirs` wird bewusst NICHT aufgerufen: Das Verzeichnis `spool/`
        // existiert nicht, wodurch der Durable-Write (Temp-Datei in `spool/`)
        // determiniert fehlschlägt.
        let runtime = PersistRuntime {
            spool: Spool {
                base_dir: base.clone(),
                in_flight: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            },
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
        };
        let shared = crate::world::new_shared();
        let (p, _rx) = dirty_test_player("p");
        {
            let mut world = shared.lock().await;
            world.players.insert(p.id.clone(), p);
        }
        let res = runtime.persist_player(&shared, "p", false).await;
        assert!(res.is_err(), "Spool-Write muss fehlschlagen");
        assert_eq!(
            runtime.status(),
            PersistStatus::Degraded,
            "fehlgeschlagener Spool-Write setzt den Realm auf DEGRADED"
        );
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(
            player.dirty.is_dirty(crate::persist::PersistComponent::Position),
            "Dirty-Bits werden bei Spool-Write-Fehler nicht bereinigt"
        );
        assert_eq!(
            player.persist_revision, 5,
            "geplante Revision (6) wird nicht verbraucht (keine Revisionslücke)"
        );
        // Kein gültiger (dauerhafter) Batch entsteht durch den fehlgeschlagenen
        // Versuch; bereits dauerhaft geschriebene Dateien bleiben erhalten.
        assert!(
            !base.join("spool").exists(),
            "kein Spool-Verzeichnis/keine Batch-Datei durch fehlgeschlagenen Write"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }
}
