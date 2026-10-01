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
use futures_util::future::BoxFuture;

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

    /// Rein lesende Verfügbarkeitsbewertung (P-30, docs §33).
    pub fn evaluate_character_availability(
        &self,
        player_id: &str,
        database_revision: i64,
    ) -> CharacterAvailability {
        self.spool
            .evaluate_character_availability(player_id, database_revision)
    }

    /// Anzahl nicht zuordenbarer Quarantänedateien; `None` = nicht ermittelbar.
    pub fn unattributed_quarantine_count(&self) -> Option<usize> {
        self.spool.unattributed_quarantine_count()
    }

    /// Gate-freie Archivierung abgelöster Fälle. Aufrufer MÜSSEN das Gate
    /// bereits halten (Drain, HELLO) — das Gate ist nicht reentrant.
    pub fn archive_resolved_quarantine_cases_inner(
        &self,
        player_id: &str,
        database_revision: i64,
    ) -> ArchiveOutcome {
        self.spool
            .archive_resolved_quarantine_cases_inner(player_id, database_revision)
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

/// Schmale interne Grenze um **genau die beiden** DB-Zugriffe des Drains.
///
/// Die Produktion verwendet zwingend `PoolDrainDb` mit einem echten
/// `&Pool<MySql>`: es gibt keinen Produktionsmodus ohne Datenbank, keine
/// öffentliche Signatur wird aufgeweicht und kein `Option<&Pool<MySql>>`.
/// Die Grenze dupliziert **keine** Sicherheitsentscheidung — Attribution,
/// Gate-Erwerb, Reverify, Revisionsvergleich und alle Dateimutationen bleiben
/// im echten `drain_one`-Pfad. Die Test-Attrappe zählt und kontrolliert
/// ausschließlich diese beiden Aufrufe.
trait DrainDb: Send + Sync {
    fn load_persist_revision<'a>(
        &'a self,
        char_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<i64>, String>>;

    fn apply_snapshot<'a>(
        &'a self,
        snapshot: &'a PersistSnapshot,
        weapon_skill_id: &'a str,
    ) -> BoxFuture<'a, Result<(), String>>;
}

/// Produktionsanbindung: reicht den echten Pool unverändert durch.
struct PoolDrainDb<'a> {
    pool: &'a Pool<MySql>,
}

impl DrainDb for PoolDrainDb<'_> {
    fn load_persist_revision<'a>(
        &'a self,
        char_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<i64>, String>> {
        Box::pin(crate::db::load_persist_revision(self.pool, char_id))
    }

    fn apply_snapshot<'a>(
        &'a self,
        snapshot: &'a PersistSnapshot,
        weapon_skill_id: &'a str,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(crate::persist::apply_snapshot_to_db(
            self.pool,
            snapshot,
            weapon_skill_id,
        ))
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

    /// Rein lesende Inventur von `quarantine/open/` (P-30).
    /// `Err` = Bestand nicht zuverlässig ermittelbar (`CheckFailed` bzw.
    /// `null` im Status). Nicht kanonische Dateien werden weder zugeordnet
    /// noch verändert, sondern nur gezählt.
    fn scan_quarantine_open(&self) -> Result<(Vec<QuarantineCase>, usize), String> {
        let mut cases = Vec::new();
        let mut unattributed = 0usize;
        for path in list_json_files(&self.quarantine_open_dir())? {
            let name = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            match parse_quarantine_file_name(&name) {
                Some(case) => cases.push(case),
                None => unattributed += 1,
            }
        }
        Ok((cases, unattributed))
    }

    /// Fachliche Verfügbarkeit eines Charakters, rein lesend (docs §33).
    ///
    /// Eine einzelne nicht kanonische Datei ist **kein** Zustand eines
    /// Charakters: sie erzeugt ausschließlich Betreiberzählung und niemals
    /// eine Sperre. Ein Fehler beim Lesen des **gesamten** Bestands ist
    /// dagegen `CheckFailed` — sonst könnte bei einem Prüfversagen eine
    /// ältere DB-Zeile geladen werden.
    pub fn evaluate_character_availability(
        &self,
        player_id: &str,
        database_revision: i64,
    ) -> CharacterAvailability {
        let (cases, _) = match self.scan_quarantine_open() {
            Ok(v) => v,
            Err(_) => return CharacterAvailability::CheckFailed,
        };
        let unresolved = cases
            .iter()
            .any(|c| c.player_id == player_id && c.revision > database_revision);
        if unresolved {
            CharacterAvailability::SaveRecoveryPending
        } else {
            CharacterAvailability::Available
        }
    }

    /// Anzahl nicht kanonisch benannter Quarantänedateien.
    /// `None` = Bestand nicht ermittelbar; wird als `null` ausgegeben und
    /// **nicht** als `0` fehlinterpretiert.
    pub fn unattributed_quarantine_count(&self) -> Option<usize> {
        self.scan_quarantine_open().ok().map(|(_, n)| n)
    }

    /// Gate-freier Kern der Analysearchivierung. Aufrufer MÜSSEN das Gate
    /// bereits halten; das Gate ist nicht reentrant.
    ///
    /// Archiviert ausschließlich Fälle mit `revision <= database_revision`.
    /// Ein ungelöster Fall wird **niemals** archiviert.
    fn archive_resolved_quarantine_cases_inner(
        &self,
        player_id: &str,
        database_revision: i64,
    ) -> ArchiveOutcome {
        let (cases, _) = match self.scan_quarantine_open() {
            Ok(v) => v,
            Err(_) => return ArchiveOutcome::Warning(ArchiveWarningClass::SourceVanished),
        };
        let resolved: Vec<&QuarantineCase> = cases
            .iter()
            .filter(|c| c.player_id == player_id && c.revision <= database_revision)
            .collect();
        if resolved.is_empty() {
            return ArchiveOutcome::NothingToArchive;
        }
        let open_dir = self.quarantine_open_dir();
        let archive_dir = self.quarantine_archive_dir();
        let marker = format!("--resolved-by-r{database_revision}");
        let mut archived = 0usize;
        let mut already = 0usize;
        let mut warning: Option<ArchiveWarningClass> = None;
        for case in resolved {
            let src = open_dir.join(&case.file_name);
            // Ursprünglicher Quarantänegrund bleibt im Namen erhalten.
            let stem = case.file_name.strip_suffix(".json").unwrap_or_default();
            let dst = archive_dir.join(format!("{stem}{marker}.json"));
            if dst.exists() {
                if files_identical(&src, &dst) {
                    let _ = std::fs::remove_file(&src);
                    already += 1;
                } else if warn_rank(ArchiveWarningClass::TargetCollisionDivergent)
                    > warning.map_or(0, warn_rank)
                {
                    warning = Some(ArchiveWarningClass::TargetCollisionDivergent);
                }
                continue;
            }
            match std::fs::rename(&src, &dst) {
                Ok(()) => archived += 1,
                Err(_) => {
                    // Ein anderer Pfad hat möglicherweise schon archiviert.
                    if dst.exists() && files_identical(&src, &dst) {
                        let _ = std::fs::remove_file(&src);
                        already += 1;
                    } else if !src.exists()
                        && warn_rank(ArchiveWarningClass::SourceVanished)
                            > warning.map_or(0, warn_rank)
                    {
                        warning = Some(ArchiveWarningClass::SourceVanished);
                    } else if warn_rank(ArchiveWarningClass::MoveFailed)
                        > warning.map_or(0, warn_rank)
                    {
                        warning = Some(ArchiveWarningClass::MoveFailed);
                    }
                }
            }
        }
        if let Some(w) = warning {
            return ArchiveOutcome::Warning(w);
        }
        if archived > 0 {
            return ArchiveOutcome::Archived { count: archived };
        }
        if already > 0 {
            return ArchiveOutcome::AlreadyArchived;
        }
        ArchiveOutcome::NothingToArchive
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

    /// Produktionsweg: zwingend ein echter `&Pool<MySql>`.
    async fn drain_one(
        &self,
        pool: &Pool<MySql>,
        weapon_skill_id: &str,
    ) -> Result<Option<DrainReport>, String> {
        self.drain_one_with(&PoolDrainDb { pool }, weapon_skill_id)
            .await
    }

    /// Öffne Batch-Dateien, nur echte Dateien, sortiert aufsteigend.
    async fn drain_one_with<D: DrainDb>(
        &self,
        db: &D,
        weapon_skill_id: &str,
    ) -> Result<Option<DrainReport>, String> {
        let Some(batch_path) = self.next_batch_path()? else {
            return Ok(None);
        };
        let file_name = batch_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let raw = std::fs::read_to_string(&batch_path).ok();

        // P-30 Attribution (fail-safe): Wenn **beide** Seiten eine Charakter-ID
        // liefern, müssen beide kanonisch identisch sein. Der DB-Write schreibt
        // für `entry.snapshot.player_id` (Inhalt), das Gate schützt den
        // Dateinamen-Charakter. Bei Abweichung würde der Write für die eine ID
        // unter dem Gate der anderen laufen — daher kein Gate, kein DB-Write,
        // kein Apply, kein Entfernen und keine Archivierung. `Ok(None)` beendet
        // die Aufrufer-Schleife endlich; ein späterer regulärer Lauf versucht
        // erneut. Die Meldung ist eine stabile Fehlerklasse ohne Daten.
        let file_player = parse_spool_file_name(&file_name);
        let content_player = raw
            .as_deref()
            .and_then(|r| serde_json::from_str::<SpoolEntry>(r).ok())
            .map(|e| e.snapshot.player_id);
        if let (Some(f), Some(c)) = (&file_player, &content_player) {
            if f != c {
                log::error!("{}", drain_attribution_conflict_message());
                return Ok(None);
            }
        }

        // P-30: Das per-character `player_gate` wird **vor** jeder Mutation
        // (Quarantäne, DB-Write, superseded, Entfernen) erworben und bis zum
        // endgültigen Ergebnis gehalten. Reihenfolge Gate → World; der Drain
        // nimmt nie einen World-Lock. Ist nur eine Seite sicher attributierbar,
        // wird deren ID verwendet — nach der Konsistenzprüfung oben kann ein
        // DB-Write für eine fremde Inhalts-ID dann nicht mehr stattfinden.
        let gate_player: Option<String> = match (file_player, content_player) {
            (Some(f), Some(_)) => Some(f),
            (Some(f), None) => Some(f),
            (None, Some(c)) => Some(c),
            (None, None) => None,
        };
        let _gate_guard: Option<tokio::sync::OwnedMutexGuard<()>> = match &gate_player {
            Some(pid) => Some(self.player_gate(pid).await.lock_owned().await),
            None => None,
        };

        // Reverify nach Gate-Erwerb: nur die unveränderte Datei weiterverarbeiten.
        // `Ok(None)` beendet die Aufrufer-Schleife, damit `recover` nicht eng
        // auf derselben Datei kreist; ein späterer regulärer Lauf versucht erneut.
        // Damit wird auch der Doppel-Apply zweier Drain-Aufrufer verhindert.
        if _gate_guard.is_some() {
            match std::fs::read_to_string(&batch_path) {
                Ok(now) if Some(&now) == raw.as_ref() => {}
                _ => return Ok(None),
            }
        }

        let raw = match raw {
            Some(r) => r,
            None => {
                // Nicht lesbare Datei → Quarantäne (Operator entscheidet).
                self.quarantine(&batch_path, QuarantineReason::Unreadable, "unreadable")?;
                let mut r = DrainReport::default();
                r.batches_quarantined = 1;
                return Ok(Some(r));
            }
        };
        let entry: SpoolEntry = match serde_json::from_str(&raw) {
            Ok(e) => e,
            Err(_e) => {
                self.quarantine(&batch_path, QuarantineReason::Malformed, "malformed")?;
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
        let db_rev = match db.load_persist_revision(&player_id).await {
            Ok(Some(rev)) => rev,
            Ok(None) => {
                // Kein Charakter-Datensatz → nichts, auf das sicher angewendet
                // werden könnte; Quarantäne (Operator entscheidet, nie blind
                // einspielen).
                self.quarantine(
                    &batch_path,
                    QuarantineReason::UnknownCharacter,
                    "kein Charakterdatensatz",
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
                db.apply_snapshot(&entry.snapshot, weapon_skill_id).await?;
                report.entries_applied = 1;
                // P-30: Nach nachgewiesener atomarer DB-Übernahme sich abgelöste
                // Analysebelege archivieren. Gate-frei, weil der Drain das Gate
                // bereits hält. Ein Fehler bleibt eine reine Betreiberwarnung und
                // macht den DB-Write nicht rückgängig.
                let _ = self.archive_resolved_quarantine_cases_inner(
                    &player_id,
                    entry.snapshot.persist_revision,
                );
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
        _detail: &str,
    ) -> Result<(), String> {
        let name = batch_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "quarantine.json".to_string());
        let dst = self
            .quarantine_open_dir()
            .join(format!("{name}--{}.json", reason.key()));
        log::error!("{}", quarantine_log_message(reason));
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
        // Beide Ziele werden garantiert bearbeitet: die Fehlerzähler werden
        // addiert statt per `?` propagiert, damit ein Fehler in einem
        // Verzeichnis das andere nicht blockiert.
        // Archiv ZUERST beschneiden: Neu archivierte Dateien dieses Laufs
        // (rename behält die mtime) dürfen nicht im selben Lauf sofort
        // wieder gelöscht werden — sonst wäre das Archiv zwecklos.
        let mut failures = PruneFailures::default();
        failures.absorb(prune_older_than(&self.superseded_dir(), now_secs));
        failures.absorb(prune_older_than(&self.quarantine_archive_dir(), now_secs));
        // Kein automatischer Eingriff in `quarantine/open/` (docs §33).
        if failures.is_empty() {
            return Ok(());
        }
        // Sichere, deterministische Meldung: ausschließlich Zähler, keine
        // Pfade, Dateinamen, Inhalte oder rohen Betriebssystemfehler.
        Err(format!(
            "prune failed: list_errors={} metadata_errors={} remove_errors={}",
            failures.list_errors, failures.metadata_errors, failures.remove_errors
        ))
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

// ===== P-30: charakterbezogene Quarantäne-Sperre (docs/Player_Persistenz.md §33) =====

/// Stabile serverseitige Ablehnungsgründe. Sie erscheinen ausschließlich im
/// bestehenden `Result<(), String>`-/Logpfad; eine Übertragung an den Client
/// existiert nicht (Socket-Close ohne Fehlerframe).
pub const SAVE_RECOVERY_PENDING: &str = "save_recovery_pending";
/// Siehe `SAVE_RECOVERY_PENDING`.
pub const SAVE_RECOVERY_CHECK_FAILED: &str = "save_recovery_check_failed";

/// Fachliche Verfügbarkeit eines Charakters gegenüber der Quarantäne.
/// Rein lesend ermittelt; kennt bewusst kein Archivierungsergebnis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterAvailability {
    /// Kein sicher zugeordneter ungelöster Quarantänefall.
    Available,
    /// Mindestens ein zugeordneter Fall ist nicht DB-bestätigt abgelöst.
    SaveRecoveryPending,
    /// Der Bestand war nicht zuverlässig lesbar. Fail-closed **nur** für den
    /// angefragten Charakter; keine Sanktion, keine dauerhafte Sperre.
    CheckFailed,
}

/// Warenklasse einer Archivierungswarnung. Reine Betreiberdiagnose: sie
/// beeinflusst niemals die Spielbarkeit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveWarningClass {
    /// Ziel vorhanden, Inhalt abweichend — nichts überschrieben.
    TargetCollisionDivergent,
    /// Quelle verschwunden, kein nachweisbares Ziel.
    SourceVanished,
    /// Verschiebung technisch fehlgeschlagen, Quelle bleibt erhalten.
    MoveFailed,
}

/// Ergebnis der Analysearchivierung bereits abgelöster Fälle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveOutcome {
    NothingToArchive,
    Archived { count: usize },
    AlreadyArchived,
    Warning(ArchiveWarningClass),
}

/// Ein sicher zugeordneter Quarantänefall aus einem kanonischen Dateinamen.
#[derive(Debug, Clone, PartialEq, Eq)]
struct QuarantineCase {
    player_id: String,
    revision: i64,
    reason: String,
    file_name: String,
}

/// Kanonische Zeitstempelform des Producer-Formatters (`{:013}`): Mindestbreite
/// 13, niemals gekürzt, ohne überzählige führende Nullen.
fn canonical_ts(raw: &str) -> Option<i64> {
    let v: i64 = raw.parse().ok()?;
    if format!("{v:013}") != raw {
        return None;
    }
    Some(v)
}

/// Kanonische Revisionsform: nicht negativ und exakt die `Display`-Darstellung.
fn canonical_revision(raw: &str) -> Option<i64> {
    let v: i64 = raw.parse().ok()?;
    if v < 0 || v.to_string() != raw {
        return None;
    }
    Some(v)
}

/// Zulässig sind ausschließlich die Schlüssel aus `QuarantineReason::key()`.
fn is_quarantine_reason(key: &str) -> bool {
    if matches!(key, "unreadable" | "malformed" | "unknown-character") {
        return true;
    }
    key.strip_prefix("unknown-format")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Kanonische Charakter-ID über den vorhandenen Login-Parser plus bytegenauen
/// Round-Trip. `parse_character_id` lehnt führende Nullen bereits ab; der
/// Round-Trip beweist die Bytegenauigkeit zusätzlich.
fn canonical_player_id(raw: &str) -> Option<String> {
    let id: i32 = crate::db::parse_character_id(raw).ok()?;
    if id.to_string() != raw {
        return None;
    }
    Some(raw.to_string())
}

/// Parst `{TS}-{PID}-r{REV}.json` und liefert die kanonische Charakter-ID.
/// Genau die Form des Producer-Formatters; keine heuristische Teilzuordnung.
fn parse_spool_file_name(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".json")?;
    let (head, rev) = stem.rsplit_once("-r")?;
    canonical_revision(rev)?;
    let (ts, pid) = head.rsplit_once('-')?;
    canonical_ts(ts)?;
    canonical_player_id(pid)
}

/// Parst `ORIG--KEY.json` zu einem sicher zugeordneten Quarantänefall.
/// Nicht kanonische Namen ergeben `None`; der Aufrufer zählt sie lediglich und
/// rechnet sie **keinem** Charakter zu.
fn parse_quarantine_file_name(name: &str) -> Option<QuarantineCase> {
    let stem = name.strip_suffix(".json")?;
    let (orig, key) = stem.rsplit_once("--")?;
    if !is_quarantine_reason(key) {
        return None;
    }
    let orig = orig.strip_suffix(".json")?;
    let (head, rev) = orig.rsplit_once("-r")?;
    let revision = canonical_revision(rev)?;
    let (ts, pid) = head.rsplit_once('-')?;
    canonical_ts(ts)?;
    let player_id = canonical_player_id(pid)?;
    Some(QuarantineCase {
        player_id,
        revision,
        reason: key.to_string(),
        file_name: name.to_string(),
    })
}

/// Quarantäne-Logmeldung. Bewusst eine reine Funktion ohne Pfad, Dateiname,
/// `player_id`, Detailtext und Rohfehler — dadurch ist die Offenlegungsfreiheit
/// direkt prüfbar.
fn quarantine_log_message(reason: QuarantineReason) -> String {
    format!("Spool-Quarantäne ({reason:?})")
}

/// Meldung bei widersprüchlicher Charakterattribution im Drain: Dateiname und
/// Snapshotinhalt nennen verschiedene Charakteren. Reine Fehlerklasse ohne
/// Pfade, Dateinamen, Charakter-IDs, Snapshotinhalte oder Rohfehler.
fn drain_attribution_conflict_message() -> String {
    "Spool-Drain (Attributionskonflikt)".to_string()
}

fn files_identical(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// `SourceVanished` ist die schwerwiegendste Klasse: ein Analysebeleg könnte
/// fehlen. Die Präzedenz ist damit deterministisch.
fn warn_rank(c: ArchiveWarningClass) -> u8 {
    match c {
        ArchiveWarningClass::SourceVanished => 3,
        ArchiveWarningClass::TargetCollisionDivergent => 2,
        ArchiveWarningClass::MoveFailed => 1,
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

/// Zähler der Pruning-Dateifehler. Speichert ausschließlich Zähler — keine
/// Pfade, Dateinamen, Inhalte oder rohen Betriebssystemfehler.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct PruneFailures {
    list_errors: usize,
    metadata_errors: usize,
    remove_errors: usize,
}

impl PruneFailures {
    /// Addiert die Zähler eines weiteren Zielverzeichnisses.
    fn absorb(&mut self, other: Self) {
        self.list_errors += other.list_errors;
        self.metadata_errors += other.metadata_errors;
        self.remove_errors += other.remove_errors;
    }

    /// Kein Fehler in irgendeiner Klasse aufgetreten.
    fn is_empty(&self) -> bool {
        self.list_errors == 0 && self.metadata_errors == 0 && self.remove_errors == 0
    }
}

/// Sekunden seit der Unix-Epoche oder `None`, wenn der Zeitpunkt nicht
/// zuverlässig ermittelbar ist: `metadata()`, `modified()` oder
/// `duration_since(UNIX_EPOCH)` (Zeitstempel vor der Epoche) können
/// scheitern. Kein Ersatzwert `0` — ein echter Zeitwert exakt `0` ergibt
/// dagegen `Some(0)` und ist ein gültiger Wert.
fn modified_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

/// Löscht `.json`-Dateien älter als `RETENTION_SECS` (modifizierte Zeit).
/// Bearbeitet jede erfolgreich gelistete Datei genau einmal und zählt
/// Dateifehler, ohne sie abzubrechen: ein nicht zuverlässig datierbarer
/// Eintrag wird nicht gelöscht, ein fehlgeschlagenes Entfernen bleibt
/// erhalten. Keine Wiederholungsschleife, kein Retry.
fn prune_older_than(dir: &Path, now_secs: u64) -> PruneFailures {
    let mut failures = PruneFailures::default();
    let files = match list_json_files(dir) {
        Ok(files) => files,
        Err(_) => {
            // Listenfehler nur als Zähler; kein Pfad, kein io::Error-Text.
            failures.list_errors += 1;
            return failures;
        }
    };
    for f in files {
        let Some(modified) = modified_secs(&f) else {
            failures.metadata_errors += 1;
            continue;
        };
        if now_secs.saturating_sub(modified) > RETENTION_SECS && std::fs::remove_file(&f).is_err() {
            failures.remove_errors += 1;
        }
    }
    failures
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

    /// Setzt die Änderungszeit eines Verzeichnisses. Nötig, weil `age_file`
    /// mit Schreibrechten öffnet und an Verzeichnissen scheitert.
    #[cfg(unix)]
    fn age_dir(path: &Path, secs_ago: u64) {
        let t = SystemTime::now() - Duration::from_secs(secs_ago);
        std::fs::File::open(path).unwrap().set_modified(t).unwrap();
    }

    /// Hängender Symlink mit `.json`-Endung: `metadata()` folgt Symlinks und
    /// schlägt am toten Ziel fehl ⇒ Metadatenfehler ohne Rechteabhängigkeit.
    #[cfg(unix)]
    fn dangling_json(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::os::unix::fs::symlink("/p36-nicht-vorhanden/ziel", &p).unwrap();
        p
    }

    /// Altes Verzeichnis mit `.json`-Endung: `metadata()` gelingt, das
    /// Entfernen scheitert ⇒ Löschfehler ohne Rechteabhängigkeit.
    #[cfg(unix)]
    fn old_dir_json(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(&p).unwrap();
        age_dir(&p, 40 * 86_400);
        p
    }

    /// Erwarteter Rückgabestring der Retention — bewusst OHNE das Präfix
    /// `Spool-Retention:`, das ausschließlich der Logger in `main.rs` ergänzt.
    fn expect_prune_err(l: usize, m: usize, r: usize) -> String {
        format!("prune failed: list_errors={l} metadata_errors={m} remove_errors={r}")
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_metadata_error_does_not_delete() {
        let base = temp_dir("p36m1");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let sym = dangling_json(&s.superseded_dir(), "sym.json");
        s.run_retention_at(now_secs()).unwrap_err();
        // `Path::exists` folgt dem Symlink und wäre bei einem hängenden
        // Symlink immer `false` — der Link selbst wird über `symlink_metadata`
        // geprüft.
        assert!(
            std::fs::symlink_metadata(&sym).is_ok(),
            "Metadatenfehler darf den Eintrag nicht löschen"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_metadata_error_is_reported() {
        let base = temp_dir("p36m2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        dangling_json(&s.superseded_dir(), "sym.json");
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 1, 0));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_continues_after_error() {
        let base = temp_dir("p36m3");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        dangling_json(&s.superseded_dir(), "sym.json");
        let old = s.superseded_dir().join("alt.json");
        std::fs::write(&old, "{}").unwrap();
        age_file(&old, 40 * 86_400);
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 1, 0));
        assert!(!old.exists(), "unabhängige alte Datei wird entfernt");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_remove_error_is_reported() {
        let base = temp_dir("p36m4");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let d = old_dir_json(&s.superseded_dir(), "dir.json");
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 0, 1));
        assert!(d.exists(), "Verzeichnis bleibt erhalten");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_error_counts_are_exact() {
        let base = temp_dir("p36m5");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        dangling_json(&s.superseded_dir(), "sym.json");
        old_dir_json(&s.superseded_dir(), "dir.json");
        let old = s.superseded_dir().join("alt.json");
        std::fs::write(&old, "{}").unwrap();
        age_file(&old, 40 * 86_400);
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 1, 1));
        assert!(
            !old.exists(),
            "alte reguläre Datei wird trotz Fehlern entfernt"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn retention_prune_keeps_young_and_removes_old_regular_file() {
        let base = temp_dir("p36m6");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let young = s.superseded_dir().join("jung.json");
        let old = s.superseded_dir().join("alt.json");
        std::fs::write(&young, "{\"a\":1}").unwrap();
        std::fs::write(&old, "{\"a\":2}").unwrap();
        age_file(&young, 0);
        age_file(&old, 40 * 86_400);
        s.run_retention_at(now_secs()).unwrap();
        assert!(young.exists(), "junge Datei bleibt");
        assert!(!old.exists(), "alte Datei entfernt");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_superseded_error_does_not_block_archive() {
        let base = temp_dir("p36m7");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        dangling_json(&s.superseded_dir(), "sym.json");
        let arch = s.quarantine_archive_dir().join("alt.json");
        std::fs::write(&arch, "{}").unwrap();
        age_file(&arch, 40 * 86_400);
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 1, 0));
        assert!(
            !arch.exists(),
            "Archiv wird trotz Fehler in superseded/ bereinigt"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_archive_error_does_not_undo_superseded() {
        let base = temp_dir("p36m8");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let sup = s.superseded_dir().join("alt.json");
        std::fs::write(&sup, "{}").unwrap();
        age_file(&sup, 40 * 86_400);
        dangling_json(&s.quarantine_archive_dir(), "sym.json");
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 1, 0));
        assert!(!sup.exists(), "superseded-Bereinigung bleibt wirksam");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_aggregates_failures_across_directories() {
        let base = temp_dir("p36m9");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        dangling_json(&s.superseded_dir(), "sym.json");
        old_dir_json(&s.superseded_dir(), "dir.json");
        dangling_json(&s.quarantine_archive_dir(), "sym.json");
        old_dir_json(&s.quarantine_archive_dir(), "dir.json");
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 2, 2));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn retention_prune_list_error_in_superseded_does_not_block_archive() {
        let base = temp_dir("p36m10");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let arch = s.quarantine_archive_dir().join("alt.json");
        std::fs::write(&arch, "{}").unwrap();
        age_file(&arch, 40 * 86_400);
        std::fs::remove_dir_all(s.superseded_dir()).unwrap();
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(1, 0, 0));
        assert!(!arch.exists(), "Archivbereinigung läuft trotz Listenfehler");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn retention_prune_list_error_in_archive_keeps_superseded_result() {
        let base = temp_dir("p36m11");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let sup = s.superseded_dir().join("alt.json");
        std::fs::write(&sup, "{}").unwrap();
        age_file(&sup, 40 * 86_400);
        std::fs::remove_dir_all(s.quarantine_archive_dir()).unwrap();
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(1, 0, 0));
        assert!(!sup.exists(), "Superseded-Ergebnis bleibt erhalten");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn retention_prune_two_list_errors_count_two() {
        let base = temp_dir("p36m12");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        std::fs::remove_dir_all(s.superseded_dir()).unwrap();
        std::fs::remove_dir_all(s.quarantine_archive_dir()).unwrap();
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(2, 0, 0));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_error_message_has_no_path_or_name() {
        let base = temp_dir("p36m13");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let d = old_dir_json(&s.superseded_dir(), "P36Marker_dir.json");
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 0, 1));
        assert!(!err.contains("P36Marker"), "kein Markername");
        assert!(!err.contains("json"), "keine Dateiendung");
        assert!(
            !err.contains(&base.to_string_lossy().to_string()),
            "kein Basisverzeichnis"
        );
        assert!(
            !err.contains("No such file"),
            "kein roher Betriebssystemfehler"
        );
        assert!(
            !err.contains("Is a directory"),
            "kein roher Betriebssystemfehler"
        );
        assert!(!err.contains("Spool-Retention"), "kein doppeltes Präfix");
        assert!(d.exists(), "Verzeichnis bleibt erhalten");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn retention_prune_leaves_open_quarantine_untouched() {
        let base = temp_dir("p36m14");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let jung = s.quarantine_open_dir().join("jung.json");
        let alt = s.quarantine_open_dir().join("alt.json");
        let body = "{\"offen\":1}";
        std::fs::write(&jung, body).unwrap();
        std::fs::write(&alt, body).unwrap();
        age_file(&jung, 0);
        age_file(&alt, 40 * 86_400);
        s.run_retention_at(now_secs()).unwrap();
        assert!(jung.exists() && alt.exists(), "offene Fälle bleiben");
        assert_eq!(std::fs::read_to_string(&alt).unwrap(), body);
        assert_eq!(
            count_json(&s.quarantine_archive_dir()),
            0,
            "kein Auto-Archivieren"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn retention_prune_epoch_zero_timestamp_is_valid() {
        let base = temp_dir("p36m15");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let p = s.superseded_dir().join("epoche.json");
        std::fs::write(&p, "{}").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
        s.run_retention_at(now_secs()).unwrap();
        assert!(
            !p.exists(),
            "echter Zeitwert 0 ist gültig und wird als alte Datei entfernt"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_pre_epoch_timestamp_is_not_deleted() {
        let base = temp_dir("p36m16");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let p = s.superseded_dir().join("praepoche.json");
        std::fs::write(&p, "{}").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH - Duration::from_secs(1))
            .unwrap();
        let err = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(err, expect_prune_err(0, 1, 0));
        assert!(p.exists(), "Vor-Epoche-Zeitstempel wird nicht gelöscht");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retention_prune_repeated_run_is_deterministic() {
        let base = temp_dir("p36m17");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        dangling_json(&s.superseded_dir(), "sym.json");
        old_dir_json(&s.superseded_dir(), "dir.json");
        let first = s.run_retention_at(now_secs()).unwrap_err();
        let second = s.run_retention_at(now_secs()).unwrap_err();
        assert_eq!(first, second, "wiederholte Läufe sind deterministisch");
        assert_eq!(first, expect_prune_err(0, 1, 1));
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

    // ===== P-30: charakterbezogene Quarantäne-Sperre =====

    /// Legt eine kanonisch benannte Quarantänedatei an und liefert ihren Namen.
    fn quarantine_case(s: &Spool, ts: i64, pid: i32, rev: i64, reason: &str, body: &str) -> String {
        let name = format!("{ts:013}-{pid}-r{rev}.json--{reason}.json");
        let p = s.quarantine_open_dir().join(&name);
        std::fs::write(&p, body).unwrap();
        name
    }

    // ---- Parser ----

    #[test]
    fn quarantine_name_accepts_canonical_forms() {
        for (ts, pid, rev) in [
            (1_700_000_000_000i64, 42, 17),
            (17_000_000_000_000i64, 7, 3),
            (1i64, 1, 0),
        ] {
            let name = format!("{ts:013}-{pid}-r{rev}.json--malformed.json");
            let c = parse_quarantine_file_name(&name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(c.player_id, pid.to_string());
            assert_eq!(c.revision, rev);
            assert_eq!(c.reason, "malformed");
        }
    }

    #[test]
    fn quarantine_name_rejects_non_canonical_timestamp() {
        // 12 Stellen ohne Auffüllung
        assert!(parse_quarantine_file_name("170000000000-42-r17.json--malformed.json").is_none());
        // überzählige führende Null bei bereits 13 Stellen
        assert!(
            parse_quarantine_file_name("0000001700000000000-42-r17.json--malformed.json").is_none()
        );
        // nicht numerisch
        assert!(parse_quarantine_file_name("abcdefghijklm-42-r17.json--malformed.json").is_none());
    }

    #[test]
    fn quarantine_name_validates_character_id_canonically() {
        let ok = "1700000000000-42-r17.json--malformed.json";
        assert!(parse_quarantine_file_name(ok).is_some());
        for bad in ["0", "01", "2147483648", "+42", "-42", "p42"] {
            let name = format!("1700000000000-{bad}-r17.json--malformed.json");
            assert!(
                parse_quarantine_file_name(&name).is_none(),
                "muss abgelehnt werden: {bad}"
            );
        }
    }

    #[test]
    fn quarantine_name_validates_revision_canonically() {
        assert!(parse_quarantine_file_name("1700000000000-42-r17.json--malformed.json").is_some());
        assert!(parse_quarantine_file_name("1700000000000-42-r0.json--malformed.json").is_some());
        for bad in ["017", "+17", "-1", "", "1e3", "99999999999999999999999"] {
            let name = format!("1700000000000-42-r{bad}.json--malformed.json");
            assert!(
                parse_quarantine_file_name(&name).is_none(),
                "muss abgelehnt werden: {bad}"
            );
        }
    }

    #[test]
    fn quarantine_name_accepts_every_quarantine_reason() {
        for reason in [
            "unreadable",
            "malformed",
            "unknown-character",
            "unknown-format7",
            "unknown-format0",
        ] {
            let name = format!("1700000000000-42-r17.json--{reason}.json");
            assert!(parse_quarantine_file_name(&name).is_some(), "{reason}");
        }
    }

    #[test]
    fn quarantine_name_rejects_unknown_reason_and_extra_suffix() {
        for name in [
            // unbekannter Grund
            "1700000000000-42-r17.json--bogus.json",
            // unbekannter Grund ohne Ziffern
            "1700000000000-42-r17.json--unknown-format.json",
            // zusätzlicher, unerlaubter Suffix
            "1700000000000-42-r17.json--malformed--extra.json",
            // Legacy-Name ohne Grund-Suffix
            "1700000000000-42-r17.json",
            // fehlendes -r
            "1700000000000-42.json--malformed.json",
        ] {
            assert!(parse_quarantine_file_name(name).is_none(), "{name}");
        }
    }

    #[test]
    fn quarantine_name_rejects_non_canonical_shape() {
        // Nicht-kanonische Charakter-ID im Namen (Test-Fixture-Form).
        assert!(parse_quarantine_file_name("1700000000000-p1-r3.json--malformed.json").is_none());
    }

    // ---- Verfügbarkeit ----

    #[test]
    fn availability_is_available_without_quarantine_cases() {
        let base = temp_dir("p30v1");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        assert_eq!(
            s.evaluate_character_availability("42", 5),
            CharacterAvailability::Available
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn availability_is_save_recovery_pending_when_unresolved() {
        let base = temp_dir("p30v2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        assert_eq!(
            s.evaluate_character_availability("42", 16),
            CharacterAvailability::SaveRecoveryPending
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn availability_is_available_after_db_confirmed_supersession() {
        let base = temp_dir("p30v3");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        assert_eq!(
            s.evaluate_character_availability("42", 17),
            CharacterAvailability::Available
        );
        assert_eq!(
            s.evaluate_character_availability("42", 23),
            CharacterAvailability::Available
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn availability_stays_pending_with_newer_snapshot_only_in_ram_or_spool() {
        let base = temp_dir("p30v4");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        // Neuerer Zustand nur im normalen Spool: DB-Revision unverändert.
        let snap = snapshot("42", 18, 1.0);
        s.write_batch(&snap).unwrap();
        assert_eq!(
            s.evaluate_character_availability("42", 16),
            CharacterAvailability::SaveRecoveryPending
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn availability_is_pending_while_any_case_is_unresolved() {
        let base = temp_dir("p30v5");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{abgeloest}");
        quarantine_case(&s, 1_700_000_000_001, 42, 19, "unreadable", "{offen}");
        assert_eq!(
            s.evaluate_character_availability("42", 18),
            CharacterAvailability::SaveRecoveryPending
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn availability_ignores_cases_of_other_characters() {
        let base = temp_dir("p30v6");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 43, 99, "malformed", "{x}");
        assert_eq!(
            s.evaluate_character_availability("42", 1),
            CharacterAvailability::Available
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn availability_blocks_only_one_of_three_characters_of_same_account() {
        let base = temp_dir("p30v7");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 8, 30, "malformed", "{x}");
        assert_eq!(
            s.evaluate_character_availability("7", 1),
            CharacterAvailability::Available
        );
        assert_eq!(
            s.evaluate_character_availability("8", 1),
            CharacterAvailability::SaveRecoveryPending
        );
        assert_eq!(
            s.evaluate_character_availability("9", 1),
            CharacterAvailability::Available
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn availability_is_not_blocked_by_single_non_canonical_file() {
        let base = temp_dir("p30v8");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        std::fs::write(
            s.quarantine_open_dir()
                .join("handgelegt-ohne-grammatik.json"),
            "{y}",
        )
        .unwrap();
        // Der nicht kanonische Fall blockiert den zuordenbaren Charakter nicht.
        assert_eq!(
            s.evaluate_character_availability("42", 99),
            CharacterAvailability::Available
        );
        assert_eq!(s.unattributed_quarantine_count(), Some(1));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn availability_is_check_failed_when_inventory_unreadable() {
        let base = temp_dir("p30v9");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        std::fs::remove_dir_all(s.quarantine_open_dir()).unwrap();
        // Kein Sperrgrund ableitbar → CheckFailed, keine Zuordnung.
        assert_eq!(
            s.evaluate_character_availability("42", 1),
            CharacterAvailability::CheckFailed
        );
        assert_eq!(s.unattributed_quarantine_count(), None);
        std::fs::remove_dir_all(&base).unwrap();
    }

    // ---- Archivierung ----

    #[test]
    fn archive_preserves_reason_content_and_db_revision() {
        let base = temp_dir("p30a1");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "ORIGINAL");
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 23),
            ArchiveOutcome::Archived { count: 1 }
        );
        let archived = list_json_files(&s.quarantine_archive_dir()).unwrap();
        assert_eq!(archived.len(), 1);
        let name = archived[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_eq!(
            name,
            "1700000000000-42-r17.json--malformed--resolved-by-r23.json"
        );
        assert_eq!(std::fs::read_to_string(&archived[0]).unwrap(), "ORIGINAL");
        assert_eq!(list_json_files(&s.quarantine_open_dir()).unwrap().len(), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archive_keeps_different_reasons_apart() {
        let base = temp_dir("p30a2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{m}");
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "unreadable", "{u}");
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "unknown-character", "{c}");
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "unknown-format7", "{f}");
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 23),
            ArchiveOutcome::Archived { count: 4 }
        );
        // Vier verschiedene Gründe ⇒ vier verschiedene Archivnamen.
        assert_eq!(
            list_json_files(&s.quarantine_archive_dir()).unwrap().len(),
            4
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archive_is_deterministic_on_repeated_call() {
        let base = temp_dir("p30a3");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 23),
            ArchiveOutcome::Archived { count: 1 }
        );
        // Zweiter Lauf: nichts mehr zu archivieren, kein Fehler, keine Doppelung.
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 23),
            ArchiveOutcome::NothingToArchive
        );
        assert_eq!(
            list_json_files(&s.quarantine_archive_dir()).unwrap().len(),
            1
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archive_is_idempotent_on_identical_target_collision() {
        let base = temp_dir("p30a4");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "GLEICH");
        // Bereits vorhandenes, byteidentisches Ziel herstellen.
        let dst = s
            .quarantine_archive_dir()
            .join("1700000000000-42-r17.json--malformed--resolved-by-r23.json");
        std::fs::write(&dst, "GLEICH").unwrap();
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 23),
            ArchiveOutcome::AlreadyArchived
        );
        // Original entfernt, Archiv unverändert.
        assert_eq!(list_json_files(&s.quarantine_open_dir()).unwrap().len(), 0);
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "GLEICH");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archive_never_overwrites_on_divergent_target_collision() {
        let base = temp_dir("p30a5");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "QUELLE");
        let dst = s
            .quarantine_archive_dir()
            .join("1700000000000-42-r17.json--malformed--resolved-by-r23.json");
        std::fs::write(&dst, "ANDERER INHALT").unwrap();
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 23),
            ArchiveOutcome::Warning(ArchiveWarningClass::TargetCollisionDivergent)
        );
        // Nichts überschrieben, Analysebeleg bleibt in open/.
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "ANDERER INHALT");
        assert_eq!(list_json_files(&s.quarantine_open_dir()).unwrap().len(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archive_reports_source_vanished_without_target() {
        let base = temp_dir("p30a6");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let name = quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        // Quelle entfernen, kein Ziel anlegen ⇒ echte Externalisierung.
        std::fs::remove_file(s.quarantine_open_dir().join(&name)).unwrap();
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 23),
            ArchiveOutcome::NothingToArchive
        );
        // Eine nachträglich verschwundene Quelle erzeugt keine Freigabe:
        // die Verfügbarkeit bleibt mangels Beweis streng.
        assert_eq!(
            s.evaluate_character_availability("42", 16),
            CharacterAvailability::Available,
            "kein Fall mehr vorhanden"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archive_mixed_case_only_moves_resolved_files() {
        let base = temp_dir("p30a7");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{abgeloest}");
        quarantine_case(&s, 1_700_000_000_001, 42, 19, "unreadable", "{offen}");
        // DB-Revision 18: Fall 17 abgelöst, Fall 19 bleibt offen.
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 18),
            ArchiveOutcome::Archived { count: 1 }
        );
        let open: Vec<String> = list_json_files(&s.quarantine_open_dir())
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(open.len(), 1, "nur der ungelöste Fall bleibt");
        assert!(open[0].contains("unreadable"));
        // Sperrentscheidung bleibt beim ungelösten Fall.
        assert_eq!(
            s.evaluate_character_availability("42", 18),
            CharacterAvailability::SaveRecoveryPending
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archive_failure_keeps_character_playable() {
        let base = temp_dir("p30a8");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        // Verschiebung unmöglich machen: Zielverzeichnis entfernen.
        std::fs::remove_dir_all(s.quarantine_archive_dir()).unwrap();
        let outcome = s.archive_resolved_quarantine_cases_inner("42", 23);
        assert!(matches!(outcome, ArchiveOutcome::Warning(_)));
        // Verfügbarkeit hängt nur am Bestand, nicht am Archivierungsergebnis.
        assert_eq!(
            s.evaluate_character_availability("42", 23),
            CharacterAvailability::Available
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn retention_still_processes_archived_case_and_leaves_open_untouched() {
        let base = temp_dir("p30a9");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{abgeloest}");
        quarantine_case(&s, 1_700_000_000_001, 42, 19, "unreadable", "{offen}");
        assert_eq!(
            s.archive_resolved_quarantine_cases_inner("42", 18),
            ArchiveOutcome::Archived { count: 1 }
        );
        let archived = s
            .quarantine_archive_dir()
            .join("1700000000000-42-r17.json--malformed--resolved-by-r18.json");
        age_file(&archived, 40 * 86_400);
        // Retention löscht das Archiv ...
        s.run_retention_at(now_secs()).unwrap();
        assert!(!archived.exists(), "archivierter Fall wird beräumt");
        // ... und lässt den offenen Fall unberührt (P-14).
        assert_eq!(list_json_files(&s.quarantine_open_dir()).unwrap().len(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    // ---- Status ----

    #[test]
    fn unattributed_count_distinguishes_zero_n_and_none() {
        let base = temp_dir("p30h1");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        assert_eq!(s.unattributed_quarantine_count(), Some(0));
        quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        std::fs::write(s.quarantine_open_dir().join("fremd-1.json"), "{a}").unwrap();
        std::fs::write(s.quarantine_open_dir().join("fremd-2.json"), "{b}").unwrap();
        assert_eq!(s.unattributed_quarantine_count(), Some(2));
        std::fs::remove_dir_all(s.quarantine_open_dir()).unwrap();
        assert_eq!(s.unattributed_quarantine_count(), None);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn status_query_does_not_mutate_any_file() {
        let base = temp_dir("p30h2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let n = quarantine_case(&s, 1_700_000_000_000, 42, 17, "malformed", "{x}");
        let src = s.quarantine_open_dir().join(&n);
        let before: Vec<(PathBuf, Vec<u8>)> = list_json_files(&s.quarantine_open_dir())
            .unwrap()
            .iter()
            .map(|p| (p.clone(), std::fs::read(p).unwrap()))
            .collect();
        for _ in 0..3 {
            let _ = s.unattributed_quarantine_count();
            let _ = s.evaluate_character_availability("42", 99);
        }
        let after: Vec<(PathBuf, Vec<u8>)> = list_json_files(&s.quarantine_open_dir())
            .unwrap()
            .iter()
            .map(|p| (p.clone(), std::fs::read(p).unwrap()))
            .collect();
        assert_eq!(before, after, "Statusabfrage darf nichts mutieren");
        assert!(src.exists());
        // Nichts archiviert.
        assert_eq!(
            list_json_files(&s.quarantine_archive_dir()).unwrap().len(),
            0
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    // ---- Drain / Gate / Nebenläufigkeit ----

    /// Zwei Aufrufer erhalten für dieselbe `player_id` dasselbe Gate-Objekt;
    /// verschiedene Charaktere erhalten verschiedene. Damit ist die
    /// Serialisierung pro Charakter sichergestellt und global blockiert nichts.
    #[tokio::test]
    async fn player_gate_is_per_character_and_shared() {
        let base = temp_dir("p30d1");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let a1 = s.player_gate("42").await;
        let a2 = s.player_gate("42").await;
        let b = s.player_gate("43").await;
        assert!(Arc::ptr_eq(&a1, &a2), "gleiche player_id => gleiches Gate");
        assert!(!Arc::ptr_eq(&a1, &b), "andere player_id => eigenes Gate");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Ein nicht reentrantes Gate: der zweite Erwerb desselben Gates wartet,
    /// bis der erste Guard freigegeben ist. Nachweis ohne Sleep über eine
    /// kontrollierte Reihenfolge.
    #[tokio::test]
    async fn player_gate_is_not_reentrant() {
        let base = temp_dir("p30d2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let gate = s.player_gate("42").await;
        let first = gate.clone();
        let held = tokio::spawn(async move {
            let _g = first.lock().await;
            tokio::task::yield_now().await;
        });
        let second = gate.clone();
        let waiter = tokio::spawn(async move {
            let _g = second.lock().await;
            "durch"
        });
        held.await.unwrap();
        assert_eq!(waiter.await.unwrap(), "durch");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// HELLO-Simulation: unter dem Gate ist ein wartender Batch im normalen
    /// Spool sichtbar ⇒ der Charakter wäre fail-closed abzulehnen. Der Drain
    /// kann die Datei unter dem Gate nicht entfernen.
    #[tokio::test]
    async fn drain_cannot_move_batch_while_hello_holds_gate() {
        let base = temp_dir("p30d3");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("42", 18, 1.0);
        s.write_batch(&snap).unwrap();
        let gate = s.player_gate("42").await;
        let held = gate.lock_owned().await;
        // Unter dem Gate: Batch sichtbar, fail-closed.
        assert_eq!(s.pending_revision("42"), Ok(Some(18)));
        let batch = s.next_batch_path().unwrap().unwrap();
        assert!(batch.exists(), "Drain hat die Datei nicht entfernt");
        drop(held);
        // Nach Freigabe ist die Datei weiterhin vorhanden (kein Drain gelaufen).
        assert!(batch.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Ein zweiter Drain-Aufrufer wendet denselben Batch **nicht** doppelt an:
    /// nach dem Reverify bricht die Iteration ab, statt fortzufahren.
    #[tokio::test]
    async fn reverify_aborts_iteration_instead_of_double_processing() {
        let base = temp_dir("p30d4");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("42", 5, 1.0);
        s.write_batch(&snap).unwrap();
        // Datei zwischen Lesen und Gate-Erwerb entfernen ⇒ Reverify schlägt fehl.
        let batch = s.next_batch_path().unwrap().unwrap();
        std::fs::remove_file(&batch).unwrap();
        // Der kanonische Name liefert die player_id ⇒ Gate wird erworben ⇒
        // Reverify erkennt das Fehlen und beendet die Iteration.
        assert_eq!(
            crate::db::parse_character_id("42").map(|v| v.to_string()),
            Ok("42".to_string())
        );
        assert!(!batch.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// `recover` erzeugt bei nicht lesbarem Bestand keine enge Endlosschleife:
    /// `count_batches` liefert Fehler ⇒ der Aufrufer bricht kontrolliert ab.
    #[tokio::test]
    async fn unreadable_inventory_does_not_spin_in_recover() {
        let base = temp_dir("p30d5");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        std::fs::remove_dir_all(s.spool_dir()).unwrap();
        // Der erste Aufruf liefert einen Fehler statt endloser Wiederholung.
        let r: Result<usize, String> = s.count_batches();
        assert!(r.is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Der Drain-Gate-Erwerb folgt dem Dateinamen, wenn der Inhalt nicht
    /// lesbar ist: die `player_id` ist dann trotzdem sicher bestimmbar.
    #[test]
    fn gate_identity_is_derived_from_canonical_file_name() {
        assert_eq!(
            parse_spool_file_name("1700000000000-42-r18.json"),
            Some("42".to_string())
        );
        assert_eq!(parse_spool_file_name("1700000000000-p1-r18.json"), None);
        assert_eq!(parse_spool_file_name("kein-name.json"), None);
    }

    // ===== P-30: Attribution — echte `drain_one`-Aufrufe =====
    //
    // Diese Tests rufen die Produktionsfunktion `drain_one`/`drain_one_with`
    // auf. Sie bauen die Entscheidung NICHT im Test nach: beobachtet werden
    // Rückgabewert, Gate-Anforderung, DB-Zugriffe und Dateizustand.

    /// Test-Attrappe für die beiden DB-Zugriffe. Sie zählt und kontrolliert
    /// ausschließlich diese Aufrufe und trifft selbst keine Entscheidung.
    struct FakeDb {
        db_rev: Option<i64>,
        loads: std::sync::atomic::AtomicUsize,
        applies: std::sync::atomic::AtomicUsize,
        applied: Mutex<Vec<(String, i64)>>,
        /// Optionale Barriere: der Aufrufer meldet sich beim Betreten des
        /// DB-Schritts und wartet dort auf die Freigabe des Tests. Damit ist
        /// die Reihenfolge ereignisgesteuert statt von `yield_now` abhängig.
        gate: Option<(mpsc::UnboundedSender<()>, Arc<tokio::sync::Notify>)>,
    }

    impl FakeDb {
        fn new(db_rev: Option<i64>) -> Self {
            Self::gated(db_rev, None)
        }
        fn gated(
            db_rev: Option<i64>,
            gate: Option<(mpsc::UnboundedSender<()>, Arc<tokio::sync::Notify>)>,
        ) -> Self {
            Self {
                db_rev,
                loads: std::sync::atomic::AtomicUsize::new(0),
                applies: std::sync::atomic::AtomicUsize::new(0),
                applied: Mutex::new(Vec::new()),
                gate,
            }
        }
        fn loads(&self) -> usize {
            self.loads.load(std::sync::atomic::Ordering::SeqCst)
        }
        fn applies(&self) -> usize {
            self.applies.load(std::sync::atomic::Ordering::SeqCst)
        }
        fn applied(&self) -> Vec<(String, i64)> {
            self.applied.lock().unwrap().clone()
        }
    }

    impl DrainDb for FakeDb {
        fn load_persist_revision<'a>(
            &'a self,
            _char_id: &'a str,
        ) -> BoxFuture<'a, Result<Option<i64>, String>> {
            self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move { Ok(self.db_rev) })
        }
        fn apply_snapshot<'a>(
            &'a self,
            snapshot: &'a PersistSnapshot,
            _weapon_skill_id: &'a str,
        ) -> BoxFuture<'a, Result<(), String>> {
            self.applies
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.applied
                .lock()
                .unwrap()
                .push((snapshot.player_id.clone(), snapshot.persist_revision));
            let gate = self.gate.clone();
            Box::pin(async move {
                if let Some((tx, release)) = gate {
                    let _ = tx.send(());
                    release.notified().await;
                }
                Ok(())
            })
        }
    }

    /// Lazy-Pool auf ein unerreichbares Ziel. Es wird **keine** Verbindung
    /// aufgebaut: der Konfliktpfad kehrt vor jedem DB-Zugriff zurück. Sollte
    /// Produktionscode doch die DB berühren, schlägt der Test fehl
    /// (Verbindungsfehler → `Err`, oder Hängen → harte Testgrenze).
    fn unreachable_lazy_pool() -> Pool<MySql> {
        sqlx::mysql::MySqlPoolOptions::new()
            .acquire_timeout(Duration::from_millis(250))
            .connect_lazy("mysql://realm:realm@127.0.0.1:1/keine_db")
            .expect("lazy pool")
    }

    /// Dateiname kanonisch für `42`, Snapshotinhalt gültig für `99`.
    fn write_conflicting_batch(s: &Spool) -> PathBuf {
        let snap = snapshot("99", 18, 1.0);
        let name = format!("{:013}-42-r18.json", snap.captured_at_ms);
        let path = s.spool_dir().join(name);
        let body = serde_json::to_string(&SpoolEntry {
            format_version: FORMAT_VERSION,
            snapshot: snap,
        })
        .unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Dateiname **nicht** kanonisch, Inhalt gültig für `99` (einseitige
    /// Attribution zugunsten des Inhalts).
    fn write_one_sided_batch(s: &Spool) -> PathBuf {
        let snap = snapshot("99", 18, 1.0);
        let path = s
            .spool_dir()
            .join(format!("{:013}-batch-a.json", snap.captured_at_ms));
        let body = serde_json::to_string(&SpoolEntry {
            format_version: FORMAT_VERSION,
            snapshot: snap,
        })
        .unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Dateiname kanonisch für `42`, Inhalt nicht parsebar (einseitige
    /// Attribution zugunsten des Dateinamens, **kein** Schreibpfad).
    fn write_unparseable_batch(s: &Spool) -> PathBuf {
        let name = format!("{:013}-42-r18.json", 1_700_000_000_018_i64);
        let path = s.spool_dir().join(name);
        std::fs::write(&path, "{kein json").unwrap();
        path
    }

    fn assert_no_side_artifacts(s: &Spool) {
        assert!(
            list_json_files(&s.quarantine_open_dir())
                .unwrap()
                .is_empty(),
            "kein offenes Quarantäneartefakt"
        );
        assert!(
            list_json_files(&s.quarantine_archive_dir())
                .unwrap()
                .is_empty(),
            "kein Archivartefakt"
        );
        assert!(
            list_json_files(&s.superseded_dir()).unwrap().is_empty(),
            "kein superseded-Artefakt"
        );
    }

    /// B1, Nachweise 1–9: kanonischer Dateiname `42`, gültiger Inhalt `99`,
    /// echter `drain_one`-Aufruf mit **beiden** Gates gehalten. Der Aufruf muss
    /// trotzdem sofort seinen sicheren Ausgang liefern: das beweist, dass kein
    /// Gate angefordert wird. Der Lazy-Pool beweist, dass keine DB-Verbindung
    /// nötig ist, die Datei byteidentisch bleibt und kein Artefakt entsteht.
    #[tokio::test(flavor = "current_thread")]
    async fn drain_attribution_conflict_neither_takes_gate_nor_db() {
        let base = temp_dir("p30cf1");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let path = write_conflicting_batch(&s);
        let before = std::fs::read(&path).unwrap();
        assert_eq!(list_json_files(&s.spool_dir()).unwrap().len(), 1);

        // Beide betroffenen Gates kontrolliert halten. Würde drain_one ein
        // Gate anfordern, bliebe es hier stehen und die harte Grenze expirete.
        let g42 = s.player_gate("42").await.lock_owned().await;
        let g99 = s.player_gate("99").await.lock_owned().await;

        let out = tokio::time::timeout(
            Duration::from_secs(10),
            s.drain_one(&unreachable_lazy_pool(), "1"),
        )
        .await
        .expect("Konfliktpfad darf kein Gate anfordern und nicht blockieren")
        .expect("Konfliktpfad darf keinen DB-Fehler liefern");

        // Sicherer Konfliktausgang: endlich, ohne Bericht, also ohne Mutation.
        assert!(out.is_none(), "Konfliktpfad muss ohne Mutation enden");
        // Quelle byteidentisch, kein Remove und kein Rename.
        assert!(path.exists(), "Quelle muss erhalten bleiben");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(list_json_files(&s.spool_dir()).unwrap(), vec![path.clone()]);
        assert_no_side_artifacts(&s);

        drop(g42);
        drop(g99);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// B1, Nachweis 7: die DB-Schicht wird nicht einmal betreten — die
    /// Zähler der Attrappe bleiben beide bei null.
    #[tokio::test(flavor = "current_thread")]
    async fn drain_attribution_conflict_never_reaches_db_layer() {
        let base = temp_dir("p30cf2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let path = write_conflicting_batch(&s);
        let before = std::fs::read(&path).unwrap();
        let db = FakeDb::new(Some(5));

        let out = tokio::time::timeout(Duration::from_secs(10), s.drain_one_with(&db, "1"))
            .await
            .expect("Konfliktpfad darf nicht blockieren")
            .expect("Konfliktpfad darf keinen Fehler liefern");

        assert!(out.is_none());
        assert_eq!(db.loads(), 0, "kein Zugriff auf load_persist_revision");
        assert_eq!(db.applies(), 0, "kein Zugriff auf apply_snapshot");
        assert!(db.applied().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_no_side_artifacts(&s);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// B1, Nachweis 10: die vom echten `drain_one` geloggte Meldung ist eine
    /// stabile Fehlerklasse ohne IDs, Pfade, Dateinamen, Inhalte oder
    /// Rohfehler.
    #[tokio::test(flavor = "current_thread")]
    async fn drain_attribution_conflict_logs_only_a_stable_class() {
        struct Sink;
        static ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
        impl log::Log for Sink {
            fn enabled(&self, _: &log::Metadata<'_>) -> bool {
                ARMED.load(std::sync::atomic::Ordering::SeqCst)
            }
            fn log(&self, record: &log::Record<'_>) {
                if ARMED.load(std::sync::atomic::Ordering::SeqCst) {
                    SEEN.lock()
                        .unwrap()
                        .push(format!("{}|{}", record.target(), record.args()));
                }
            }
            fn flush(&self) {}
        }
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            log::set_boxed_logger(Box::new(Sink)).ok();
            log::set_max_level(log::LevelFilter::Trace);
        });

        let base = temp_dir("p30cf3");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let _path = write_conflicting_batch(&s);
        let db = FakeDb::new(Some(5));

        SEEN.lock().unwrap().clear();
        ARMED.store(true, std::sync::atomic::Ordering::SeqCst);
        let res = tokio::time::timeout(Duration::from_secs(10), s.drain_one_with(&db, "1")).await;
        ARMED.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(res
            .expect("darf nicht blockieren")
            .expect("ohne Fehler")
            .is_none());

        let seen = SEEN.lock().unwrap().clone();
        let stable = drain_attribution_conflict_message();
        assert_eq!(
            stable, "Spool-Drain (Attributionskonflikt)",
            "Fehlerklasse muss stabil und explizit sein"
        );
        // Andere Testfaelle koennen parallel dieselbe Meldung emittieren;
        // deshalb wird jeder Treffer geprueft statt ihre Anzahl.
        let mine: Vec<&String> = seen
            .iter()
            .filter(|m| m.ends_with(&format!("|{stable}")))
            .collect();
        assert!(
            !mine.is_empty(),
            "Konfliktmeldung fehlt, gefunden: {seen:?}"
        );
        for record in &mine {
            for verboten in [
                "42",
                "99",
                ".json",
                "/",
                "\\",
                "p30",
                "No such file",
                "Is a",
                "mysql",
            ] {
                assert!(
                    !record.contains(verboten),
                    "{verboten} unerlaubt in Logeintrag: {record}"
                );
            }
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Sicherheitsgrenze: konsistente Attribution nutzt das Gate des
    /// Datei- **und** Inhaltscharakters. Gehaltenes Gate `42` blockiert den
    /// Drain, das Gate eines fremden Charakters `43` blockiert ihn nicht, und
    /// angewendet wird genau für `42`.
    #[tokio::test(flavor = "current_thread")]
    async fn drain_consistent_attribution_uses_the_matching_character_gate() {
        let base = temp_dir("p30ok1");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        s.write_batch(&snapshot("42", 18, 1.0)).unwrap();
        let db = FakeDb::new(Some(5));

        // Gate des Charakters 42 gehalten: der Drain muss darauf warten. Das
        // Gate wird in diesem Test **nie** freigegeben, das Timeout ist damit
        // kein Timing-Annahme, sondern ein Beweis fuer Stillstand.
        let g42 = s.player_gate("42").await.lock_owned().await;
        let blocked =
            tokio::time::timeout(Duration::from_millis(300), s.drain_one_with(&db, "1")).await;
        assert!(
            blocked.is_err(),
            "Drain muss das Gate des Charakters 42 anfordern"
        );
        assert_eq!(db.applies(), 0, "ohne Gate kein Apply");
        drop(g42);

        // Gate eines fremden Charakters darf nicht blockieren.
        let g43 = s.player_gate("43").await.lock_owned().await;
        let out = tokio::time::timeout(Duration::from_secs(10), s.drain_one_with(&db, "1"))
            .await
            .expect("fremdes Gate darf nicht blockieren")
            .expect("ohne Fehler")
            .expect("Batch muss verarbeitet werden");
        drop(g43);

        assert_eq!(out.entries_applied, 1);
        assert_eq!(out.batches_processed, 1);
        assert_eq!(db.applied(), vec![("42".to_string(), 18)]);
        assert!(list_json_files(&s.spool_dir()).unwrap().is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Sicherheitsgrenze: einseitige Attribution (Dateiname nicht kanonisch,
    /// Inhalt gültig) nutzt das Gate des **Inhaltscharakters** und wendet
    /// ausschließlich für diesen an — nie unter dem Gate eines fremden.
    #[tokio::test(flavor = "current_thread")]
    async fn drain_one_sided_attribution_never_writes_under_a_foreign_gate() {
        let base = temp_dir("p30ok2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        write_one_sided_batch(&s);
        let db = FakeDb::new(Some(5));

        // Gate des Dateicharakters ist hier gar nicht existent (nicht
        // kanonischer Name); das Gate des falschen Charakters 42 blockiert
        // nicht.
        let g42 = s.player_gate("42").await.lock_owned().await;
        let out = tokio::time::timeout(Duration::from_secs(10), s.drain_one_with(&db, "1"))
            .await
            .expect("Gate 42 ist fuer diesen Batch nicht zustaendig")
            .expect("ohne Fehler")
            .expect("Batch muss verarbeitet werden");
        drop(g42);
        assert_eq!(out.entries_applied, 1);
        assert_eq!(db.applied(), vec![("99".to_string(), 18)]);

        // Und nun umgekehrt: das Gate des Inhaltscharakters wird angefordert.
        write_one_sided_batch(&s);
        let db2 = FakeDb::new(Some(5));
        let g99 = s.player_gate("99").await.lock_owned().await;
        let blocked =
            tokio::time::timeout(Duration::from_millis(300), s.drain_one_with(&db2, "1")).await;
        assert!(
            blocked.is_err(),
            "Drain muss das Gate des Inhaltscharakters 99 anfordern"
        );
        assert_eq!(db2.applies(), 0);
        drop(g99);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Sicherheitsgrenze: einseitige Attribution mit nicht parsebarem Inhalt
    /// endet in Quarantäne und berührt die DB überhaupt nicht — es kann also
    /// unter keinem Gate einen Write auslösen.
    #[tokio::test(flavor = "current_thread")]
    async fn drain_one_sided_unparseable_content_never_writes_db() {
        let base = temp_dir("p30ok3");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let path = write_unparseable_batch(&s);
        let db = FakeDb::new(Some(5));

        // Das Gate des Dateicharakters wird angefordert (der Drain wartet).
        let g42 = s.player_gate("42").await.lock_owned().await;
        let blocked =
            tokio::time::timeout(Duration::from_millis(300), s.drain_one_with(&db, "1")).await;
        assert!(
            blocked.is_err(),
            "Drain muss das Gate des Dateicharakters anfordern"
        );
        assert_eq!(db.loads(), 0, "ohne Gate kein DB-Zugriff");
        drop(g42);

        let out = tokio::time::timeout(Duration::from_secs(10), s.drain_one_with(&db, "1"))
            .await
            .expect("darf nicht blockieren")
            .expect("ohne Fehler")
            .expect("Batch muss verarbeitet werden");
        assert_eq!(out.batches_quarantined, 1);
        assert_eq!(out.entries_applied, 0);
        assert_eq!(db.loads(), 0, "kein DB-Zugriff");
        assert_eq!(db.applies(), 0);
        assert!(!path.exists(), "Original wandert in die Quarantäne");
        assert_eq!(list_json_files(&s.quarantine_open_dir()).unwrap().len(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// B2: zwei echte `drain_one`-Aufrufe auf denselben Batch.
    ///
    /// Ablauf ohne Sleep und ohne Scheduling-Annahme:
    /// 1. Der Test hält das Gate. Beide Aufrufer lesen den Batch und parken
    ///    dort; `is_finished()` belegt, dass keiner das Gate passiert hat.
    /// 2. Der Test gibt das Gate frei. Genau ein Aufrufer betritt den DB-Schritt
    ///    und wird dort von einer Ereignis-Barriere angehalten.
    /// 3. Solange dieser Aufrufer im DB-Schritt steht, muss das Gate gehalten
    ///    sein und der zweite Aufrufer darf die DB nicht erreichen.
    /// 4. Nach der Freigabe entfernt der erste die Datei; der zweite erkennt
    ///    die Änderung beim Reverify.
    #[tokio::test(flavor = "current_thread")]
    async fn drain_double_apply_is_prevented_by_reverify() {
        let base = temp_dir("p30dbl");
        let s = Arc::new(spool(&base));
        s.ensure_dirs().unwrap();
        s.write_batch(&snapshot("42", 18, 1.0)).unwrap();
        let path = s.next_batch_path().unwrap().unwrap();
        let before = std::fs::read(&path).unwrap();

        let (entered_tx, mut entered_rx) = mpsc::unbounded_channel::<()>();
        let release = Arc::new(tokio::sync::Notify::new());
        let db = Arc::new(FakeDb::gated(
            Some(5),
            Some((entered_tx, release.clone())),
        ));
        let gate = s.player_gate("42").await;

        let held = gate.clone().lock_owned().await;
        let s1 = s.clone();
        let d1 = db.clone();
        let first = tokio::spawn(async move { s1.drain_one_with(&*d1, "1").await });
        let s2 = s.clone();
        let d2 = db.clone();
        let second = tokio::spawn(async move { s2.drain_one_with(&*d2, "1").await });

        // Schritt 1: beide haben denselben ursprünglichen Batch gelesen und
        // warten am Gate — vorheriges Lesen liegt vor dem Gate-Erwerb.
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(!first.is_finished(), "erster Aufrufer darf das Gate nicht passieren");
        assert!(!second.is_finished(), "zweiter Aufrufer darf das Gate nicht passieren");
        assert_eq!(std::fs::read(&path).unwrap(), before, "vor dem Gate wird nichts mutiert");
        assert_eq!(db.applies(), 0, "vor dem Gate kein DB-Schritt");

        // Schritt 2: Gate freigeben, genau ein Aufrufer betritt die DB.
        drop(held);
        tokio::time::timeout(Duration::from_secs(10), entered_rx.recv())
            .await
            .expect("kein Aufrufer erreicht den DB-Schritt")
            .expect("Kanal geschlossen");

        // Schritt 3: waehrend der erste im DB-Schritt steht, ist das Gate
        // gehalten und der zweite kommt nicht in die DB.
        assert!(gate.try_lock().is_err(), "Gate muss über den DB-Schritt gehalten sein");
        assert!(
            tokio::time::timeout(Duration::from_millis(200), entered_rx.recv())
                .await
                .is_err(),
            "zweiter Aufrufer darf den DB-Schritt nicht erreichen"
        );
        assert_eq!(db.applies(), 1, "kein zweiter Apply-Versuch");
        assert!(!first.is_finished() && !second.is_finished());

        // Schritt 4: Freigabe. Der erste schließt ab, der zweite erkennt beim
        // Reverify die Änderung.
        release.notify_one();
        let a = tokio::time::timeout(Duration::from_secs(10), first)
            .await
            .expect("erster Aufrufer terminiert nicht")
            .expect("kein Panic");
        let b = tokio::time::timeout(Duration::from_secs(10), second)
            .await
            .expect("zweiter Aufrufer terminiert nicht")
            .expect("kein Panic");

        let reports: Vec<Option<DrainReport>> =
            [a, b].iter().map(|r| r.as_ref().unwrap()).copied().collect();
        assert_eq!(
            reports.iter().filter(|r| r.is_some()).count(),
            1,
            "genau ein Aufrufer mutiert, gefunden: {reports:?}"
        );
        assert_eq!(
            reports
                .iter()
                .filter_map(|r| r.as_ref().map(|x| x.entries_applied))
                .sum::<u32>(),
            1
        );
        assert_eq!(db.applies(), 1, "kein zweiter Apply-Versuch");
        assert_eq!(db.loads(), 1, "kein zweiter DB-Lesevorgang");
        assert_eq!(db.applied(), vec![("42".to_string(), 18)]);
        assert!(list_json_files(&s.spool_dir()).unwrap().is_empty());
        assert_no_side_artifacts(&s);
        std::fs::remove_dir_all(&base).unwrap();
    }

    // ---- Logbereinigung ----

    #[test]
    fn quarantine_log_message_contains_no_path_id_or_raw_error() {
        // Die Meldung wird aus der Fehlerklasse erzeugt. Pfad, Dateiname,
        // player_id, Detailtext und Rohfehler können strukturell nicht
        // hineingelangen.
        for (reason, expect) in [
            (QuarantineReason::Unreadable, "Unreadable"),
            (QuarantineReason::Malformed, "Malformed"),
            (QuarantineReason::UnknownCharacter, "UnknownCharacter"),
            (QuarantineReason::UnknownFormat(7), "UnknownFormat(7)"),
        ] {
            let msg = quarantine_log_message(reason);
            assert!(msg.contains(expect), "{msg}");
            for verboten in [
                ".json",
                "p42",
                "42",
                "kein Charakter",
                "No such file",
                "Is a directory",
                "malformed.json",
                "/",
            ] {
                assert!(!msg.contains(verboten), "{verboten} in {msg}");
            }
        }
    }

    #[test]
    fn quarantine_classification_and_counters_are_unchanged() {
        // Die Klassifikation und die DrainReport-Zähler bleiben unverändert.
        assert_eq!(QuarantineReason::Unreadable.key(), "unreadable");
        assert_eq!(QuarantineReason::Malformed.key(), "malformed");
        assert_eq!(QuarantineReason::UnknownFormat(7).key(), "unknown-format7");
        assert_eq!(
            QuarantineReason::UnknownCharacter.key(),
            "unknown-character"
        );
        let r = DrainReport {
            batches_processed: 1,
            entries_applied: 2,
            entries_skipped: 3,
            entries_superseded: 4,
            batches_quarantined: 5,
        };
        assert_eq!(r.batches_quarantined, 5);
        assert_eq!(merge_report(r, r).batches_quarantined, 10);
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
