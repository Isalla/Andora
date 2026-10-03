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
use std::sync::atomic::{AtomicBool, Ordering};
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

impl PersistStatus {
    /// `P-23`: Stabile, dokumentierte Statuswerte für die Beobachtung über
    /// `/status` (`persistence_status`, docs/Player_Persistenz.md §27/§28).
    ///
    /// Die Abbildung ist **additiv**: Die Benennung des Enums bleibt
    /// unverändert (`Recovering`/`Ready`/`Degraded`); nur die Ausgabe verwendet
    /// die drei festgelegten kleingeschriebenen Werte. Der Wert ist reine
    /// Beobachtung und steuert **nichts** — weder Spielfreigabe noch Status.
    pub fn as_str(self) -> &'static str {
        match self {
            PersistStatus::Recovering => "recovering",
            PersistStatus::Ready => "ready",
            PersistStatus::Degraded => "degraded",
        }
    }
}

/// Gemeinsame Batch-Datei **eines Persistenzlaufs** (docs §35, `P-12`).
///
/// Enthält ausschließlich die dirty Player-Snapshots dieses Laufs. Jeder
/// Eintrag ist ein vollständiger persistenter Einzelsnapshot im unveränderten
/// V1-Einzelformat und bleibt unabhängig verarbeitbar (§36).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpoolBatch {
    pub format_version: u16,
    /// Laufkennung, unabhängig von `persist_revision` (§29/§35).
    pub batch_id: String,
    pub entries: Vec<SpoolEntry>,
}

/// Fassung des Batch-Umschlags. Der Eintrag selbst trägt weiterhin seine
/// eigene `format_version` (`SpoolEntry`).
pub const BATCH_FORMAT_VERSION: u16 = 1;

/// Eine (deterministisch beschreibbare) Spool-Datei: einzelner Player-Snapshot.
///
/// `PartialEq` vergleicht **alle** Felder einschließlich des vollständigen
/// `PersistSnapshot` und ist damit die Grundlage des Inhaltsvergleichs
/// `batch_content_eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// `P-22`: Ergebnis der Startup-Recovery: der bisherige Drain-Bericht **plus**
/// die tatsächlich verbliebene relevante Recovery-Arbeit.
///
/// `batches_remaining` zählt ausschließlich die offenen Batch-Dateien in
/// `<base>/spool/` (siehe `Spool::count_batches`). Quarantäne, `superseded/`,
/// Archive und temporäre Dateien sind **keine** offene Recovery und werden
/// hier nicht mitgezählt; sie bleiben getrennt bewertet (`P-14`, `P-30`).
/// `batches_remaining == 0` bedeutet: die Start-Recovery ist tatsächlich
/// abgeschlossen und `READY` ist zulässig.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecoveryOutcome {
    pub report: DrainReport,
    pub batches_remaining: usize,
}

/// `P-22`: Notventil der Startup-Recovery. **Schutzgrenze, kein Statusmittel:**
/// Der Wert bleibt unverändert 10.000 Drain-Aufrufe (eine Iteration = eine
/// Batch-Datei in `<base>/spool/`). Er entscheidet **nicht** über `READY` oder
/// `DEGRADED`; das entscheidet allein die tatsächlich verbliebene Restarbeit
/// (`RecoveryOutcome::batches_remaining`).
pub const RECOVERY_MAX_DRAIN_CALLS: u32 = 10_000;

/// `P-22`: Ergebnis der Statusentscheidung nach der Startup-Recovery
/// (`PersistRuntime::apply_startup_recovery`) beziehungsweise nach einem
/// erfolgreichen Tick des periodischen Drainers
/// (`PersistRuntime::apply_drain_tick`). Der Aufrufer protokolliert
/// ausschließlich den Zähler `remaining`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStatusUpdate {
    /// Die Start-Recovery ist bestätigt abgeschlossen bzw. es liegt nur
    /// normale Spool-Arbeit vor → Status steht auf `READY`.
    Ready,
    /// Die Start-Recovery ist weiterhin offen und Restarbeit verbleibt →
    /// Status bleibt `DEGRADED`, `remaining` ist der reine Restzähler.
    RecoveryStillOpen { remaining: usize },
}

/// Durable-Snapshot-Spool. `in_flight` serialisiert pro Spieler (verhindert
/// konkurrierende Snapshots derselben Revisions-Baseline und serialisiert
/// zusätzlich den Eigentümer-/Logout-Übergang pro `player_id` — siehe
/// `player_gate`).
///
/// `P-29`: Die Map hält **nicht** mehr nur das Gate, sondern einen
/// `GateEntry` mit einem Halter-Zähler. Sobald der letzte Halter eines Gates
/// seinen `PlayerGate` bzw. `PlayerGateGuard` droppt, wird der Map-Eintrag
/// entfernt — die Map wächst also nicht mehr über die Prozesslaufzeit.
#[derive(Clone)]
pub struct Spool {
    pub base_dir: PathBuf,
    in_flight: Arc<std::sync::Mutex<HashMap<String, Arc<GateEntry>>>>,
}

/// Ein Spieler-Gate mit Halter-Zähler.
///
/// `holders` zählt **zugegebene `PlayerGate`- und `PlayerGateGuard`-Werte**,
/// nicht `Arc`-Referenzen. Das ist der entscheidende Unterschied zu einem
/// Cleanup über `Arc::strong_count`: der Zähler wird ausschließlich von
/// `Spool::player_gate` (Erwerb) und vom `Drop` der Lease (Freigabe)
/// verändert, und beide Erhöhungen sowie die Entfernung passieren unter
/// derselben `in_flight`-Sperre. Ein bereits ausgegebener, noch nicht
/// gelockter Handle hält den Zähler daher zu Recht auf mindestens 1.
struct GateEntry {
    gate: Arc<tokio::sync::Mutex<()>>,
    holders: std::sync::atomic::AtomicUsize,
}

impl GateEntry {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            gate: Arc::new(tokio::sync::Mutex::new(())),
            holders: std::sync::atomic::AtomicUsize::new(0),
        })
    }
}

/// Sperrt `in_flight` **ohne** bei Poisoning zu panicken.
///
/// `Drop` der Lease darf nach einem Panic niemals einen zweiten Panic
/// auslösen (das würde den Prozess beenden). Ein vergifteter Mutex wird daher
/// übernommen statt behandelt.
fn lock_in_flight(
    map: &std::sync::Mutex<HashMap<String, Arc<GateEntry>>>,
) -> std::sync::MutexGuard<'_, HashMap<String, Arc<GateEntry>>> {
    map.lock().unwrap_or_else(|e| e.into_inner())
}

/// Zugriffsschutz auf das Gate einer `player_id` (`P-29`).
///
/// Erwerb und Freigabe eines Gates sind an **dasselbe** Gate gebunden:
/// Beide Operationen laufen unter der `in_flight`-Sperre, und die Freigabe
/// prüft zusätzlich per `Arc::ptr_eq`, dass die Map wirklich noch dieses
/// Gate hält. Damit kann für dieselbe `player_id` **kein** zweites
/// unabhängiges Gate entstehen, solange ein Handle oder Guard existiert.
pub struct PlayerGate {
    map: Arc<std::sync::Mutex<HashMap<String, Arc<GateEntry>>>>,
    player_id: String,
    entry: Arc<GateEntry>,
}

impl PlayerGate {
    /// Sperre auf **eigenem** Arc; der Guard trägt die Lease weiter.
    ///
    /// Das ist der von Produktionspfaden zu verwendende Aufruf: Er hält den
    /// Halter-Zähler über die gesamte Sperrzeit auf mindestens 1, sodass der
    /// Map-Eintrag unter keinen Umständen entfernt wird, während die Sperre
    /// gehalten wird.
    pub async fn lock_owned(self) -> PlayerGateGuard {
        let guard = self.entry.gate.clone().lock_owned().await;
        PlayerGateGuard {
            _guard: guard,
            _lease: self,
        }
    }

    /// Identität des zugrunde liegenden Gates.
    ///
    /// Nur für Tests: `Arc::ptr_eq` über `PlayerGate` ist nicht möglich, weil
    /// die Lease nicht denselben Typ wie der Map-Wert trägt.
    #[cfg(test)]
    pub(crate) fn same_gate(&self, other: &PlayerGate) -> bool {
        Arc::ptr_eq(&self.entry, &other.entry)
    }

    /// Identität gegen einen **gehaltenen** Guard.
    ///
    /// Nur für Tests: der Guard hält die Lease, nicht das Gate, deshalb ist
    /// ein Vergleich über `PlayerGate` nicht moeglich.
    #[cfg(test)]
    pub(crate) fn same_gate_during(&self, guard: &PlayerGateGuard) -> bool {
        Arc::ptr_eq(&self.entry, &guard._lease.entry)
    }

    /// Nicht blockierender Erwerb; `None`, wenn das Gate gerade gehalten wird.
    ///
    /// Nur für Tests (P-30-Doppel-Apply-Nachweis). Die Lease wird bei `None`
    /// mit dem `PlayerGate` verworfen und gibt ihren Zähler frei — der
    /// Map-Eintrag bleibt bestehen, solange ein echter Halter existiert.
    #[cfg(test)]
    pub(crate) async fn try_lock_owned(self) -> Option<PlayerGateGuard> {
        match self.entry.gate.clone().try_lock_owned() {
            Ok(guard) => Some(PlayerGateGuard {
                _guard: guard,
                _lease: self,
            }),
            Err(_) => None,
        }
    }
}

/// Hält Sperre **und** Lease.
///
/// Die Lease wird bewusst mitgeführt: Bei
/// `player_gate(..).lock_owned()` existiert das `PlayerGate` nur als
/// temporäres Handle. Würde die Map den Eintrag schon beim Rückkehr aus
/// `player_gate` entfernen, könnte ein zweiter Aufrufer ein zweites Gate
/// anlegen, während die Sperre noch gehalten wird — genau die Spaltung, die
/// P-30 verhindert.
pub struct PlayerGateGuard {
    _guard: tokio::sync::OwnedMutexGuard<()>,
    _lease: PlayerGate,
}

impl Drop for PlayerGate {
    fn drop(&mut self) {
        let mut map = lock_in_flight(&self.map);
        let Some(entry) = map.get(&self.player_id) else {
            return; // bereits entfernt: nichts zu tun
        };
        // Nur freigeben, wenn die Map wirklich noch **dieses** Gate hält.
        if !Arc::ptr_eq(entry, &self.entry) {
            return;
        }
        let previous = entry.holders.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        if previous <= 1 {
            // Letzter Halter: Eintrag jetzt entfernen. Der Zähler- und
            // Entfernungsschritt liegen beide unter der Map-Sperre, daher
            // kann sich kein neuer Halter dazwischen einfinden.
            if entry.holders.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                map.remove(&self.player_id);
            }
        }
    }
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
    ///
    /// `P-29`: Der Rückgabewert ist ein `PlayerGate` mit RAII-Lebensdauer. Der
    /// Halter-Zähler wird **hier** unter der `in_flight`-Sperre erhöht; fällt
    /// der Aufrufer frühzeitig weg oder tritt ein Panic auf, wird der Eintrag
    /// beim `Drop` der Lease wieder entfernt. Für den gehaltenen Sperr-Guard
    /// ist `PlayerGate::lock_owned` zu verwenden — es erhält die Lease und
    /// verhindert damit eine Gate-Spaltung.
    ///
    /// Die Signatur bleibt bewusst `async`, obwohl der Zugriff synchron ist:
    /// die `in_flight`-Map enthält nur kurze Kopieroperationen, und die
    /// Aufruferkette (Login-Gating, Logout, periodischer Flush, Drain) bleibt
    /// dadurch unverändert. Es entsteht **kein** `await` innerhalb der
    /// Map-Sperre.
    pub async fn player_gate(&self, player_id: &str) -> PlayerGate {
        let mut map = lock_in_flight(&self.in_flight);
        let entry = map
            .entry(player_id.to_string())
            .or_insert_with(GateEntry::new)
            .clone();
        entry.holders.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        PlayerGate {
            map: self.in_flight.clone(),
            player_id: player_id.to_string(),
            entry,
        }
    }

    /// `P-29`: Anzahl der aktuell registrierten Gate-Einträge. Für Tests und
    /// die Beobachtung des Wachstums; die Produktion loggt diesen Wert nicht.
    #[cfg(test)]
    pub(crate) fn in_flight_len(&self) -> usize {
        lock_in_flight(&self.in_flight).len()
    }
}

/// Laufzeit-Objekt der Stufe B: Spool + Status + Drian-Zugriff.
pub struct PersistRuntime {
    spool: Spool,
    weapon_skill_id: String,
    status: Arc<Mutex<PersistStatus>>,
    /// `P-22`: Die Start-Recovery ist noch **offen** (Limit erreicht oder
    /// Abbruch ohne Fortschritt, jeweils mit verbliebener Restarbeit). Nur so
    /// unterscheidet der Hintergrund-Drainer „erster Erfolg bei noch offener
    /// Start-Recovery" von „normale neue Spool-Arbeit im Regelbetrieb".
    recovery_open: Arc<AtomicBool>,
}

impl PersistRuntime {
    /// Legt die Spool-Verzeichnisse an (fehlerfrei = bereit).
    pub fn new(base_dir: &Path, weapon_skill_id: &str) -> Result<Self, String> {
        let spool = Spool {
            base_dir: base_dir.to_path_buf(),
            in_flight: Arc::new(std::sync::Mutex::new(HashMap::new())),
        };
        spool.ensure_dirs()?;
        Ok(Self {
            spool,
            weapon_skill_id: weapon_skill_id.to_string(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn spool(&self) -> &Spool {
        &self.spool
    }

    /// Per-player-Serialisierung (Gate → World, nie umgekehrt): durable
    /// Schreibvorgänge und Eigentümer-/Logout-Übergänge derselben
    /// `player_id`. Siehe `Spool::player_gate`.
    pub async fn player_gate(&self, player_id: &str) -> PlayerGate {
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

    /// `P-22`: Ist die Start-Recovery noch offen? Nur unmittelbar nach dem
    /// Start relevant; danach ist der Zustand nur noch „Start-Recovery
    /// abgeschlossen" (`false`).
    pub fn recovery_open(&self) -> bool {
        self.recovery_open.load(Ordering::SeqCst)
    }

    /// `P-22`: Markiert die Start-Recovery als offen/abgeschlossen. Wird nur
    /// vom Startpfad und vom bestätigten Abschluss gesetzt — nicht von den
    /// normalen Save- und Drainpfaden.
    pub fn set_recovery_open(&self, open: bool) {
        self.recovery_open.store(open, Ordering::SeqCst);
    }

    /// `P-22`: Statusentscheidung des **Startpfads** nach der Startup-Recovery
    /// (aufgerufen mit dem echten `RecoveryOutcome` aus `recover`).
    ///
    /// - `batches_remaining == 0`: die Recovery ist **bestätigt abgeschlossen**
    ///   → `READY`, auch dann, wenn das Limit erreicht wurde.
    /// - `batches_remaining > 0`: Limitabbruch **oder** Abbruch ohne
    ///   Fortschritt mit relevanter Restarbeit → **kein** `READY`, sondern
    ///   `DEGRADED`; die Restarbeit wird als „offen" markiert, damit der
    ///   Hintergrund-Drainer `READY` erst nach dem Abarbeiten zulässt.
    pub fn apply_startup_recovery(&self, outcome: &RecoveryOutcome) -> RecoveryStatusUpdate {
        if outcome.batches_remaining == 0 {
            self.set_recovery_open(false);
            self.set_status(PersistStatus::Ready);
            RecoveryStatusUpdate::Ready
        } else {
            self.set_recovery_open(true);
            self.set_status(PersistStatus::Degraded);
            RecoveryStatusUpdate::RecoveryStillOpen {
                remaining: outcome.batches_remaining,
            }
        }
    }

    /// `P-22`: Statusentscheidung des **periodischen Drainers** nach einem
    /// erfolgreichen Tick (`Ok(Some(..))` wie `Ok(None)`).
    ///
    /// - Spool leer: die Start-Recovery ist damit **bestätigt abgeschlossen**
    ///   → `READY` (der Abschluss überschreibt bewusst einen zwischenzeitlich
    ///   gesetzten `DEGRADED`, weil der Recovery-Pfad selbst fehlerfrei blieb).
    /// - Spool nicht leer **und** Start-Recovery noch offen: der Tick hat nur
    ///   eine Batch abgearbeitet → **kein** `READY`; `DEGRADED` bleibt erhalten
    ///   und wird datensparsam protokolliert.
    /// - Spool nicht leer, Start-Recovery abgeschlossen: **normale neue
    ///   Spool-Arbeit** im Regelbetrieb → unverändert `READY`, sie erzwingt
    ///   nicht pauschal `DEGRADED`.
    ///
    /// Der Restcheck misst ausschließlich `<base>/spool/`; ein Fehler des
    /// Restchecks wird **weitergereicht** und löst hier keinen Statuswechsel
    /// aus. Der Fehlerpfad des Drains (`Err`) bleibt beim Aufrufer und setzt
    /// dort wie bisher `DEGRADED`.
    pub fn apply_drain_tick(&self) -> Result<RecoveryStatusUpdate, String> {
        match self.spool.count_batches()? {
            0 => {
                self.set_recovery_open(false);
                self.set_status(PersistStatus::Ready);
                Ok(RecoveryStatusUpdate::Ready)
            }
            remaining => {
                if self.recovery_open() {
                    Ok(RecoveryStatusUpdate::RecoveryStillOpen { remaining })
                } else {
                    self.set_status(PersistStatus::Ready);
                    Ok(RecoveryStatusUpdate::Ready)
                }
            }
        }
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

    /// `P-12`/§35: ein Persistenzlauf über mehrere Spieler erzeugt **eine**
    /// gemeinsame Batch-Datei. Der aufrufende Lauf gibt den reservierten
    /// Snapshot-Speicher nach der dauerhaften Veröffentlichung frei, ohne auf
    /// die DB-Verarbeitung zu warten.
    ///
    /// Rückgabe: Anzahl der in die Batch-Datei übernommenen Snapshots; `0`
    /// bedeutet: leere Dirty-Menge, es wurde **keine** Datei erzeugt.
    pub async fn persist_dirty_run(
        &self,
        shared: &crate::world::Shared,
        player_ids: &[String],
    ) -> Result<u32, String> {
        let spool = self.spool.clone();
        match crate::persist::persist_dirty_run(shared, player_ids, |snapshots| async move {
            spool.write_batch_run(snapshots).map(|_| ())
        })
        .await
        {
            Ok(n) => Ok(n),
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
    ///
    /// `P-22`: Der Rückgabewert unterscheidet den **bestätigten Abschluss**
    /// (`batches_remaining == 0`) von einem **Limitabbruch oder Abbruch ohne
    /// Fortschritt mit verbleibender Restarbeit** (`batches_remaining > 0`).
    /// Das Limit selbst bleibt `RECOVERY_MAX_DRAIN_CALLS` und entscheidet
    /// **nicht** über den Status.
    pub async fn recover(&self, pool: &Pool<MySql>) -> Result<RecoveryOutcome, String> {
        self.recover_with_limit(&PoolDrainDb { pool }, RECOVERY_MAX_DRAIN_CALLS)
            .await
    }

    /// Wie `recover`, aber mit internem Limit für die Tests. Die Produktion
    /// ruft ausschließlich `recover` mit `RECOVERY_MAX_DRAIN_CALLS` auf; die
    /// Schleife selbst ist identisch.
    async fn recover_with_limit<D: DrainDb>(
        &self,
        db: &D,
        max_drain_calls: u32,
    ) -> Result<RecoveryOutcome, String> {
        let mut total = DrainReport::default();
        let mut guard = 0u32;
        while self.spool.count_batches()? > 0 && guard < max_drain_calls {
            guard += 1;
            match self.spool.drain_one_with(db, &self.weapon_skill_id).await? {
                Some(r) => total = merge_report(total, r),
                None => break,
            }
        }
        // Restarbeit **nach** jedem regulären Schleifenende messen: Limit,
        // Abbruch ohne Fortschritt und vollständiger Durchlauf werden dadurch
        // am Ergebnis unterscheidbar. Fehler des Restchecks werden
        // weitergereicht.
        let batches_remaining = self.spool.count_batches()?;
        Ok(RecoveryOutcome {
            report: total,
            batches_remaining,
        })
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
            // `P-12`: gemeinsames Batch **und** alte Einzeldatei berücksichtigen.
            // Andernfalls wäre eine im neuen Format gesicherte Revision für die
            // Fail-closed-Vorabprüfung unsichtbar — die P-30-Garantie wäre
            // ausgehebelt.
            let Some(entries) = entries_in_file(&raw) else {
                continue;
            };
            for entry in entries {
                if entry.player_id == player_id && max.is_none_or(|m| entry.persist_revision > m)
                {
                    max = Some(entry.persist_revision);
                }
            }
        }
        Ok(max)
    }

    /// `P-12`/§35: **ein** Persistenzlauf erzeugt **eine** gemeinsame
    /// Batch-Datei mit **ausschließlich** den dirty Player-Snapshots dieses
    /// Laufs. Eine leere Snapshot-Menge erzeugt **keine** Datei.
    ///
    /// Rückgabe: `Some(pfad)` wenn eine Batch-Datei veröffentlicht wurde,
    /// `None` wenn die Dirty-Menge leer war. Der aufrufende Lauf gibt den
    /// reservierten Snapshot-Speicher unmittelbar danach frei — ohne auf die
    /// DB-Verarbeitung zu warten.
    pub fn write_batch_run(
        &self,
        snapshots: Vec<PersistSnapshot>,
    ) -> Result<Option<PathBuf>, String> {
        if snapshots.is_empty() {
            return Ok(None);
        }
        // Kanonische Reihenfolge VOR Namensbildung und Serialisierung: der
        // Name und der serialisierte Inhalt hängen dann nicht von der
        // Eingabereihenfolge ab.
        let entries: Vec<SpoolEntry> = canonicalize_entries(
            snapshots
                .into_iter()
                .map(|snapshot| SpoolEntry {
                    format_version: FORMAT_VERSION,
                    snapshot,
                })
                .collect(),
        )?;
        let captured_at_ms = entries
            .iter()
            .map(|e| e.snapshot.captured_at_ms)
            .min()
            .unwrap_or(0);
        let file_name = batch_file_name(captured_at_ms, batch_digest(&entries));
        let batch = SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: file_name.trim_end_matches(".json").to_string(),
            entries,
        };
        let body = serde_json::to_string(&batch)
            .map_err(|e| format!("Spool-Batch serialisieren: {e}"))?;
        let target = self.spool_dir().join(&file_name);

        // `target.exists()` allein bestaetigt **keinen** erfolgreichen Write.
        // Der Wiederholungsfall laeuft ueber `confirm_existing_publication`:
        // das bestaetigt den **Inhalt** der vorhandenen Datei und wiederholt
        // den Verzeichnis-Sync, bevor der Zustand als dauerhaft gilt. Das ist
        // genau der Fall "Sync-Fehler, Datei lag bereits vor" (§40).
        if target.exists() {
            return confirm_existing_publication(&target, &batch).map(|()| Some(target));
        }

        // Atomar veroeffentlichen OHNE Ueberschreiben: Inhalt vollstaendig
        // unter dem temporaeren Namen, dann `hard_link` auf den finalen Namen
        // (`publish_new_file`). Ein Renennen-Rennen scheitert dort, statt eine
        // fremde Datei zu verdraengen.
        let tmp = self.spool_dir().join(format!(".tmp-{file_name}"));
        match write_atomic_if_absent(&tmp, &target, body.as_bytes()) {
            Ok(true) => Ok(Some(target)),
            Ok(false) => {
                // Konkurrierende Veroeffentlichung: derselbe echte
                // Bestaetigungspfad — Inhalt **und** Dauerhaftigkeit pruefen.
                confirm_existing_publication(&target, &batch).map(|()| Some(target))
            }
            Err(e) => Err(e),
        }
    }

    /// Durable-Write eines vollständigen Player-Snapshots als **Ein-Eintrag-
    /// Batch** (ein Persistenzlauf mit genau einem dirty Spieler, §35).
    pub fn write_batch(&self, snapshot: &PersistSnapshot) -> Result<(), String> {
        self.write_batch_run(vec![snapshot.clone()]).map(|_| ())
    }

    /// Alte, nicht mehr erzeugte Schreibweise: erzeugt **eine** Datei mit
    /// **einem** Snapshot im V1-Einzelformat. Nur für Kompatibilitätsprüfungen
    /// bestehender veröffentlichter Dateien.
    #[cfg(test)]
    pub fn write_legacy_single(&self, snapshot: &PersistSnapshot) -> Result<(), String> {
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
            return Ok(());
        }
        let tmp = self.spool_dir().join(format!(".tmp-{file_name}"));
        if publish_new_file(&tmp, &target, body.as_bytes())? != PublishOutcome::Published {
            return Err(format!("Spool Zieldatei bereits belegt: {:?}", target));
        }
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

        // `P-12`: Das gemeinsame Batch-Format wird am **Inhalt** erkannt
        // (`entries`-Feld), nicht am Dateinamen. Damit bleiben bereits
        // veröffentlichte Einzeldateien (V1) unverändert lesbar und der
        // bestehende `P-30`-Pfad darunter unangetastet.
        let is_shared_batch = raw
            .as_deref()
            .and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
            .map(|v| v.get("entries").is_some())
            .unwrap_or(false);
        if is_shared_batch {
            return self
                .drain_shared_batch(db, weapon_skill_id, &batch_path, &file_name, raw)
                .await;
        }

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
        let _gate_guard: Option<PlayerGateGuard> = match &gate_player {
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

    /// `P-12`: schreibt **einen** Batch-Eintrag dauerhaft als eigene
    /// Ein-Eintrag-Batch-Datei in `dir`. Wird für die **eintragsweise**
    /// Quarantäne und für superseded-Einträge eines gemeinsamen Batches
    /// verwendet, damit die übrigen Einträge weiterverarbeitet werden können.
    ///
    /// Der Dateiname folgt dem kanonischen Quarantäneschema
    /// `<ts:013>-<player_id>-r<rev>.json--<reason>.json`, damit die bestehende
    /// P-30-Zuordnung (`parse_quarantine_file_name`) greift.
    fn write_entry_to_dir(
        &self,
        entry: &SpoolEntry,
        dir: &Path,
        key: &str,
    ) -> Result<(), String> {
        let snap = &entry.snapshot;
        let stem = format!(
            "{:013}-{}-r{}.json",
            snap.captured_at_ms, snap.player_id, snap.persist_revision
        );
        let name = format!("{stem}--{key}.json");
        self.write_entry_to_dir_as(entry, dir, &name)
    }

    /// Gemeinsame Sicherung eines einzelnen Eintrags als Ein-Eintrag-Batch.
    /// `stem_suffix` ist der Namensrest; die Datei ist genau eine Batch-Datei
    /// und damit jederzeit wieder lesbar.
    fn write_entry_to_dir_as(
        &self,
        entry: &SpoolEntry,
        dir: &Path,
        file_name: &str,
    ) -> Result<(), String> {
        let file_name = file_name.to_string();
        let target = dir.join(&file_name);
        if target.exists() {
            return Ok(()); // bereits durable (idempotent)
        }
        let body = serde_json::to_string(&SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: file_name.trim_end_matches(".json").to_string(),
            entries: vec![entry.clone()],
        })
        .map_err(|e| format!("Spool-Eintrag serialisieren: {e}"))?;
        let tmp = dir.join(format!(".tmp-{file_name}"));
        if publish_new_file(&tmp, &target, body.as_bytes())? != PublishOutcome::Published {
            return Err(format!("Spool Zieldatei bereits belegt: {:?}", target));
        }
        Ok(())
    }

    /// `P-12`: quarantänisiert **einen** eindeutig zuordenbaren Eintrag eines
    /// gemeinsamen Batches dauerhaft. Die restlichen Einträge bleiben
    /// bearbeitbar. Schlägt die Sicherung fehl, gilt der Eintrag als **nicht**
    /// erledigt (der Aufrufer lässt die Datei liegen).
    fn quarantine_entry(
        &self,
        entry: &SpoolEntry,
        reason: QuarantineReason,
    ) -> Result<(), String> {
        self.write_entry_to_dir(entry, &self.quarantine_open_dir(), &reason.key())
    }

    /// `P-12`/§35: verarbeitet das **gemeinsame Batch-Format** eines
    /// Persistenzlaufs eintragsweise.
    ///
    /// Kernregeln: die DB-Bestätigung gilt **je Eintrag**; ein problematischer,
    /// eindeutig zuordenbarer Eintrag wird zuerst dauerhaft quarantänisiert und
    /// die übrigen Einträge laufen weiter; die Datei wird **erst** entfernt,
    /// wenn jeder Eintrag DB-bestätigt oder dauerhaft quarantänisiert ist.
    /// Schlägt eine Quarantänesicherung fehl, bleibt die Datei liegen und der
    /// Eintrag gilt als nicht erledigt.
    ///
    /// `P-30` bleibt wirksam: je Eintrag wird das **Charakter-Gate** geholt
    /// (Gate → DB, nie World), und die Datei wird nach dem Lesen reverifiziert,
    /// sodass ein zwischenzeitlich veränderter Batch nicht verarbeitet wird.
    async fn drain_shared_batch<D: DrainDb>(
        &self,
        db: &D,
        weapon_skill_id: &str,
        batch_path: &Path,
        file_name: &str,
        raw: Option<String>,
    ) -> Result<Option<DrainReport>, String> {
        let mut report = DrainReport::default();
        let text = match raw {
            Some(t) => t,
            None => {
                self.quarantine(batch_path, QuarantineReason::Unreadable, "unreadable")?;
                report.batches_quarantined = 1;
                return Ok(Some(report));
            }
        };
        let batch: SpoolBatch = match serde_json::from_str(&text) {
            Ok(b) => b,
            Err(_e) => {
                self.quarantine(batch_path, QuarantineReason::Malformed, "malformed")?;
                report.batches_quarantined = 1;
                return Ok(Some(report));
            }
        };
        if batch.format_version != BATCH_FORMAT_VERSION {
            self.quarantine(
                batch_path,
                QuarantineReason::UnknownFormat(batch.format_version),
                "batch format_version != aktuelle",
            )?;
            report.batches_quarantined = 1;
            return Ok(Some(report));
        }
        if batch.entries.is_empty() {
            // Leerer Batch: nichts zu verarbeiten, kein Artefakt nötig.
            remove_file(batch_path)?;
            return Ok(None);
        }
        // Reverify: nur die seit dem Lesen unveränderte Datei abarbeiten.
        match std::fs::read_to_string(batch_path) {
            Ok(now) if now == text => {}
            _ => return Ok(None),
        }

        // Fail-safe, **vor** dem ersten DB-Zugriff: Ein nicht eindeutig
        // zuordenbarer oder unbekannter Eintrag darf keinen Teilbatch erzeugen.
        // Deshalb wird die gesamte Datei vorab geprüft — sonst könnten die
        // vorherigen Einträge bereits geschrieben sein, bevor die Prüfung den
        // problematischen Eintrag sieht. Es wird keine Zuordnung erfunden; die
        // Datei geht als nicht zuordenbarer Analysefall in die Quarantäne.
        if batch
            .entries
            .iter()
            .any(|e| {
                e.format_version != FORMAT_VERSION
                    || canonical_player_id(&e.snapshot.player_id).is_none()
            })
        {
            self.quarantine(
                batch_path,
                QuarantineReason::Malformed,
                "batch entry nicht zuordenbar",
            )?;
            report.batches_quarantined = 1;
            return Ok(Some(report));
        }

        for entry in &batch.entries {
            let player_id = entry.snapshot.player_id.clone();
            // P-30: Charakter-Gate je Eintrag, vor jeder Mutation.
            let _entry_gate = self.player_gate(&player_id).await.lock_owned().await;
            // Reverify **nach** dem Gate und **vor** jedem DB-Zugriff: nur die
            // seit dem Lesen unveränderte Datei abarbeiten. Damit erkennt ein
            // zweiter Aufrufer die zwischenzeitliche Mutation und beendet sich
            // endlich, ohne einen zweiten Apply oder DB-Lesevorgang zu erzeugen.
            match std::fs::read_to_string(batch_path) {
                Ok(now) if now == text => {}
                _ => return Ok(None),
            }
            let db_rev = match db.load_persist_revision(&player_id).await {
                Ok(Some(rev)) => rev,
                Ok(None) => {
                    // Kein Charakter-Datensatz: nur dieser Eintrag wird
                    // dauerhaft quarantänisiert, die übrigen laufen weiter.
                    self.quarantine_entry(entry, QuarantineReason::UnknownCharacter)?;
                    report.batches_quarantined += 1;
                    continue;
                }
                Err(e) => return Err(format!("Drain {file_name}: {e}")),
            };
            match db_rev.cmp(&entry.snapshot.persist_revision) {
                std::cmp::Ordering::Equal => {
                    // Bereits committet: idempotent, kein zweiter Apply (§38).
                    report.entries_skipped += 1;
                }
                std::cmp::Ordering::Greater => {
                    // Übertroffen: nur dieser Eintrag wird superseded abgelegt.
                    // Namensschema wie bisher: `{player_id}-r{revision}.json`.
                    let stem = format!(
                        "{}-r{}",
                        entry.snapshot.player_id, entry.snapshot.persist_revision
                    );
                    let name = format!("{stem}.json");
                    self.write_entry_to_dir_as(entry, &self.superseded_dir(), &name)?;
                    report.entries_superseded += 1;
                }
                std::cmp::Ordering::Less => {
                    db.apply_snapshot(&entry.snapshot, weapon_skill_id).await?;
                    report.entries_applied += 1;
                    let _ = self.archive_resolved_quarantine_cases_inner(
                        &player_id,
                        entry.snapshot.persist_revision,
                    );
                }
            }
        }
        // Erst jetzt ist jeder Eintrag erledigt: die Datei darf entfernt werden.
        remove_file(batch_path)?;
        report.batches_processed = 1;
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
///
/// Batch-Dateiname eines Persistenzlaufs: `<captured_at_ms:013>-b<digest:012>.json`.
/// Der Name enthält bewusst **keine** Charakter-ID; die Zuordnung erfolgt
/// ausschließlich über die Einträge.
/// `P-12`: Batch-Dateiname `<ts:013>-b<digest:012>.json`.
///
/// **Was den Dateinamen bestimmt:** ausschließlich
/// 1. `min(captured_at_ms)` aller Einträge (Zeit-Präfix) und
/// 2. `batch_digest` über die **kanonisch geordneten** Einträge.
///
/// Der Digest ist ein **Namensteil, kein Inhaltsnachweis**. Er ist über
/// `player_id` + `persist_revision` je Eintrag gebildet und damit kein
/// kollisionsfreier Nachweis der Snapshot-Inhalte. Die inhaltliche
/// Gleichheit zweier Batches wird deshalb **immer zusätzlich** über einen
/// vollständigen Byte- oder Strukturvergleich geprüft
/// (`batch_content_eq`), nie allein über den Digest.
///
/// Das Zeit-Präfix erhält die lexikografische Reihenfolge = chronologische
/// Drain-Reihenfolge (§36).
fn batch_file_name(captured_at_ms: i64, digest: u64) -> String {
    format!("{captured_at_ms:013}-b{digest:012}.json")
}

/// FNV-1a (64 Bit) über die Inhaltskennung der **kanonisch geordneten**
/// Einträge (`player_id` + `persist_revision`). Bewusst ohne `DefaultHasher`:
/// die Zuordnung muss über Prozess- und Compilerläufe hinweg stabil bleiben.
///
/// Der Wert dient der Namensbildung. Er ist **kein** Nachweis, dass zwei
/// Batches inhaltlich gleich sind — siehe `batch_content_eq`.
fn batch_digest(entries: &[SpoolEntry]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for e in entries {
        mix(e.snapshot.player_id.as_bytes());
        mix(&e.snapshot.persist_revision.to_le_bytes());
    }
    h
}

/// Vollständiger Inhaltsvergleich zweier Batches.
///
/// `Ok(true)` nur, wenn **beide** Dateien als `SpoolBatch` lesbar sind und
/// **jeder** persistentierte Feldwert übereinstimmt (`SpoolEntry` leitet
/// `PartialEq` aus allen Feldern ab, einschließlich der vollständigen
/// `PersistSnapshot`). Der Vergleich ist bewusst strukturell und nicht
/// namensbasiert: er soll einen echten Inhaltsunterschied finden können,
/// auch wenn der Dateiname identisch ist.
///
/// Lesefehler oder fremde Formate gelten **nicht** als Gleichheit.
fn batch_content_eq(path: &Path, batch: &SpoolBatch) -> bool {
    let Ok(raw) = std::fs::read(path) else {
        return false;
    };
    let Ok(existing) = serde_json::from_slice::<SpoolBatch>(&raw) else {
        return false;
    };
    existing == *batch
}

/// `P-12`/§35: bringt die Einträge eines Batches in die **kanonische**
/// Reihenfolge, bevor Name und serialisierter Inhalt gebildet werden.
///
/// Sortiert wird nach `player_id`; die Identität ist damit die stabile
/// Reihenfolgegrundlage. Der Name eines Batches und sein serialisierter Inhalt
/// hängen dadurch **nicht** mehr von der Eingabereihenfolge ab: derselbe
/// vollständige Batch erzeugt bei vertauschter Eingabe denselben Namen und
/// dieselbe Datei.
///
/// Doppelte `player_id` werden **nicht** stillschweigend verdrängt: `build_snapshot`
/// vergibt je Lauf eine eigene Revision, zwei Einträge derselben Identität in einem
/// Batch sind daher ein Fehlerbild und kein Normalfall.
fn canonicalize_entries(mut entries: Vec<SpoolEntry>) -> Result<Vec<SpoolEntry>, String> {
    entries.sort_by(|a, b| a.snapshot.player_id.cmp(&b.snapshot.player_id));
    for w in entries.windows(2) {
        if w[0].snapshot.player_id == w[1].snapshot.player_id {
            return Err(format!(
                "Spool-Batch mehrfach dieselbe Spieleridentität im selben Batch: {}",
                w[0].snapshot.player_id
            ));
        }
    }
    Ok(entries)
}

/// `P-12`/§35: `sync`-Fehler an der Dauerhaftigkeitsgrenze **wiederholen**.
///
/// Nach einem fehlgeschlagenen Verzeichnis-Sync kann bereits eine finale Datei
/// vorliegen. Ein erneuter Versuch darf sie weder blind überschreiben noch
/// ihre Dauerhaftigkeit ungeprüft annehmen: Der Inhalt wird vollständig
/// verglichen **und** der Verzeichnis-Sync erneut ausgeführt. Erst danach gilt
/// der Zustand als dauerhaft bestätigt.
///
/// Produktionspfad: `write_batch_run` ruft diese Funktion in **beiden**
/// Wiederholungsfaellen — vorhandene Datei vor dem Schreibversuch und
/// Konkurrenz während der Veröffentlichung.
fn confirm_existing_publication(target: &Path, batch: &SpoolBatch) -> Result<(), String> {
    if !target.exists() {
        return Err("Spool Bestätigung: Zieldatei fehlt".to_string());
    }
    if !batch_content_eq(target, batch) {
        return Err(format!(
            "Spool Batch-Konflikt: {} existiert bereits mit abweichendem Inhalt",
            target
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<unbekannt>".into())
        ));
    }
    // Inhalt stimmt; die Dauerhaftigkeit wird dennoch erneut bestätigt.
    sync_dir(target)
}

/// `P-12`: liefert die Snapshots **einer** Spool-Datei unabhängig vom Format.
/// Unterstützt das gemeinsame Batch-Format (`entries`) und die alte
/// Einzeldatei (V1). `None` = nicht lesbar bzw. kein bekanntes Format.
fn entries_in_file(raw: &str) -> Option<Vec<PersistSnapshot>> {
    if serde_json::from_str::<serde_json::Value>(raw)
        .map(|v| v.get("entries").is_some())
        .unwrap_or(false)
    {
        return serde_json::from_str::<SpoolBatch>(raw)
            .ok()
            .map(|b| b.entries.into_iter().map(|e| e.snapshot).collect());
    }
    serde_json::from_str::<SpoolEntry>(raw)
        .ok()
        .map(|e| vec![e.snapshot])
}

/// Parst einen Batch-Dateinamen streng kanonisch. `None` = kein Batchname.
/// Der Batch-Name trägt bewusst **keine** Charakter-ID (§35); die Zuordnung
/// erfolgt ausschließlich über die Einträge.
#[cfg_attr(not(test), allow(dead_code))]
fn parse_batch_file_name(name: &str) -> Option<i64> {
    let stem = name.strip_suffix(".json")?;
    let (ts, seq) = stem.rsplit_once("-b")?;
    if seq.is_empty() || !seq.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    canonical_ts(ts)
}

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

/// Dauerhaftes **atomares** Veröffentlichen einer neuen Datei `target`.
///
/// Reihenfolge und Bedeutung (`P-12`/§35, Safe-Write §25):
/// 1. Der **vollständige** Inhalt wird ausschließlich unter dem temporären
///    Namen `tmp` geschrieben und `fsync`iert. Unter dem finalen Namen wird
///    zu diesem Zeitpunkt **nichts** angelegt oder geschrieben — er ist für
///    einen Drain also noch nicht sichtbar.
/// 2. `hard_link(tmp, target)` macht den **fertigen** Inhalt unter dem
///    finalen Namen atomar sichtbar. Ein Hardlink ist hier die passende
///    Lösung, weil er den Zielnamen **nie** ersetzt (er scheitert mit
///    `AlreadyExists`, wenn der Name belegt ist — kein Rename-Rennen, kein
///    Überschreiben) und den Inhalt **sofort vollständig** sichtbar macht,
///    sodass unter dem finalen Namen nie eine unvollständige Datei entsteht.
///    Eignung: `tmp` und `target` liegen im selben Spool-Verzeichnis und damit
///    auf demselben Dateisystem; der unterstützte Pfad ist Linux/ext4.
/// 3. `tmp` wird entfernt; der finale Name bleibt als reguläre Datei
///    (gelinkter Inhalt) erhalten.
/// 4. Das **Elternverzeichnis** wird `fsync`iert. Erst danach gilt die
///    Veröffentlichung als dauerhaft. Der Fehler wird **weitergegeben**, weil
///    der Verzeichniseintrag sonst einen Absturz nicht überlebt.
///
/// Datei-Sync und Verzeichnis-Sync sind verschieden: `sync_all` auf `tmp`
/// sichert den Inhalt, der Verzeichnis-Sync sichert die Sichtbarkeit unter
/// `target`.
fn publish_new_file(
    tmp: &Path,
    target: &Path,
    bytes: &[u8],
) -> Result<PublishOutcome, String> {
    // 1) Vollständig unter dem temporären Namen schreiben und sichern.
    {
        let mut f =
            std::fs::File::create(tmp).map_err(|e| format!("Spool Temp {tmp:?}: {e}"))?;
        if let Err(e) = std::io::Write::write_all(&mut f, bytes) {
            let _ = std::fs::remove_file(tmp);
            return Err(format!("Spool schreiben: {e}"));
        }
        if let Err(e) = f.sync_all() {
            let _ = std::fs::remove_file(tmp);
            return Err(format!("Spool Datei-fsync {tmp:?}: {e}"));
        }
    }

    // 2) Atomar unter dem finalen Namen sichtbar machen, ohne zu ersetzen.
    if let Err(e) = std::fs::hard_link(tmp, target) {
        // Temporärdatei sicher entfernen; am Ziel wurde nichts verändert.
        let _ = std::fs::remove_file(tmp);
        // `AlreadyExists` ist der erwartete Konkurrenzfall und wird **über den
        // Fehlertyp** unterschieden, nicht über eine Textsuche in der
        // Fehlermeldung (die plattformabhängig "File exists" lautet).
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            return Ok(PublishOutcome::TargetExists);
        }
        return Err(format!("Spool veröffentlichen {target:?}: {e}"));
    }

    // 3) Temporärdatei entfernen (Bestand bleibt unter `target` erhalten).
    let _ = std::fs::remove_file(tmp);

    // 4) Dauerhaftigkeitsgrenze: ohne bestätigten Verzeichnis-Sync gilt die
    // Veröffentlichung NICHT als abgeschlossen.
    sync_dir(target)?;
    Ok(PublishOutcome::Published)
}

/// Ergebnis eines Veröffentlichungsversuchs (`publish_new_file`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishOutcome {
    /// Der finale Name war frei; der vollständige Inhalt ist jetzt sichtbar
    /// und das Verzeichnis ist gesichert.
    Published,
    /// Der finale Name war bereits belegt. Es wurde **nichts** verändert; der
    /// Inhalt entscheidet über die weitere Behandlung.
    TargetExists,
}

/// `fsync` des Elternverzeichnisses. Fehler werden weitergereicht: die
/// unterstuetzte Zielplattform (Linux) liefert hier einen echten Fehler, wenn
/// das Verzeichnis nicht gesichert werden kann.
fn sync_dir(path: &Path) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("Spool Verzeichnis zu {path:?} unbekannt"))?;
    let d = std::fs::File::open(dir).map_err(|e| format!("Spool Verzeichnis oeffnen {dir:?}: {e}"))?;
    d.sync_all()
        .map_err(|e| format!("Spool Verzeichnis-fsync {dir:?}: {e}"))
}

/// Alte, weiterhin benutzte Schreibweise: legt `target` an, ohne eine
/// vorhandene Datei zu ueberschreiben. Der Inhalt wird nur geschrieben, wenn
/// die Datei neu angelegt wurde; eine bereits vorhandene Datei bleibt
/// unberuehrt und wird als `false` gemeldet.
fn write_atomic_if_absent(tmp: &Path, target: &Path, bytes: &[u8]) -> Result<bool, String> {
    Ok(publish_new_file(tmp, target, bytes)? == PublishOutcome::Published)
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
    use futures_util::FutureExt;
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
            in_flight: Arc::new(std::sync::Mutex::new(HashMap::new())),
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
        // `P-12`: Idempotenz bedeutet weiterhin **eine** Datei für zwei
        // identische Schreibvorgänge. Der Name ist jetzt inhaltsbasiert, also
        // wird kein fester Legacy-Name mehr erwartet, sondern genau eine
        // lesbare Batch-Datei mit dem erwarteten Eintrag.
        assert_eq!(s.count_batches().unwrap(), 1);
        let path = base.join("spool").join(only_batch_name(&base));
        assert!(path.exists(), "Batch-Datei existiert durable");
        let raw = std::fs::read_to_string(&path).unwrap();
        let batch: SpoolBatch = serde_json::from_str(&raw).expect("Batch lesbar");
        assert_eq!(batch.format_version, BATCH_FORMAT_VERSION);
        assert_eq!(batch.entries.len(), 1);
        assert_eq!(batch.entries[0].snapshot.player_id, "p1");
        assert_eq!(batch.entries[0].snapshot.persist_revision, 3);
        assert!(!base.join("spool").read_dir().unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-")
        }));
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Name der einzigen Batch-Datei im Spool-Verzeichnis.
    fn only_batch_name(base: &std::path::Path) -> String {
        let dir = base.join("spool");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| !n.starts_with(".tmp-"))
            .collect();
        assert_eq!(names.len(), 1, "genau eine Batch-Datei erwartet: {names:?}");
        names.into_iter().next().unwrap()
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
        // `P-12`: Der gemeinsame Batch-Name ist inhaltsbasiert. Die
        // garantierte Eigenschaft ist die **chronologische** Drain-Reihenfolge
        // (§36), also wird das Zeitstempel-Präfix geprüft, nicht ein Name.
        let first = s.next_batch_path().unwrap().unwrap();
        let first_name = first.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            first_name.starts_with("0000000000100-"),
            "älteste Datei zuerst, war: {first_name}"
        );
        assert_eq!(parse_batch_file_name(&first_name), Some(100));
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
        // `P-29`: Die Identitätsprüfung erfolgt über `same_gate`, weil der
        // Rückgabewert jetzt eine Lease mit eigenem Zähler ist und damit
        // nicht mehr der Map-`Arc` selbst ist.
        assert!(a1.same_gate(&a2), "gleiche player_id => gleiches Gate");
        assert!(!a1.same_gate(&b), "andere player_id => eigenes Gate");
        drop(a1);
        drop(a2);
        drop(b);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Ein nicht reentrantes Gate: der zweite Erwerb desselben Gates wartet,
    /// bis der erste Guard freigegeben ist. Nachweis ohne Sleep über eine
    /// kontrollierte Reihenfolge.
    ///
    /// `P-29`: Jeder Aufrufer holt seine **eigene** Lease — ein `clone` des
    /// Handles existiert bewusst nicht, weil nur `Spool::player_gate` den
    /// Halter-Zähler erhöhen darf.
    #[tokio::test]
    async fn player_gate_is_not_reentrant() {
        let base = temp_dir("p30d2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let held = tokio::spawn({
            let s = s.clone();
            async move {
                let _g = s.player_gate("42").await.lock_owned().await;
                tokio::task::yield_now().await;
            }
        });
        let waiter = tokio::spawn({
            let s = s.clone();
            async move {
                let _g = s.player_gate("42").await.lock_owned().await;
                "durch"
            }
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
        /// `P-12`: DB-Revision je Charakter-ID. `Some(map)` überschreibt `db_rev`;
        /// ein `None` **im Map** bedeutet: kein Charakterdatensatz.
        per_player: Option<HashMap<String, Option<i64>>>,
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
                per_player: None,
            }
        }
        /// `P-12`: DB mit Revision **pro** Charakter. `(id, None)` modelliert
        /// „kein Charakterdatensatz".
        fn multi(revs: &[(&str, Option<i64>)]) -> Self {
            Self {
                db_rev: None,
                loads: std::sync::atomic::AtomicUsize::new(0),
                applies: std::sync::atomic::AtomicUsize::new(0),
                applied: Mutex::new(Vec::new()),
                gate: None,
                per_player: Some(
                    revs.iter()
                        .map(|(k, v)| (k.to_string(), *v))
                        .collect::<HashMap<_, _>>(),
                ),
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
            char_id: &'a str,
        ) -> BoxFuture<'a, Result<Option<i64>, String>> {
            self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // `P-12`: gemeinsame Batches brauchen je Charakter eine eigene
            // DB-Revision.
            if let Some(map) = &self.per_player {
                let rev = map.get(char_id).copied().flatten();
                return Box::pin(async move { Ok(rev) });
            }
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

    // ===== P-22: Recovery-Abschluss, Limitabbruch und Statusübergänge =====
    //
    // Diese Tests rufen die **echten** Produktionsfunktionen `recover_with_limit`
    // (identische Schleife wie `recover`, nur mit internem Limit), `drain_one_with`
    // und die Statusentscheidungen `apply_startup_recovery`/`apply_drain_tick` auf.
    // Sie bauen die Entscheidung nicht im Test nach. Das Limit wird klein
    // gewählt, damit der Limitabbruch ohne zehntausend Dateien erreichbar ist;
    // der Produktionswert `RECOVERY_MAX_DRAIN_CALLS` bleibt davon unberührt.

    /// `PersistRuntime` über einem echten Spool, Startstatus `Recovering`.
    fn p22_runtime(s: &Spool) -> PersistRuntime {
        PersistRuntime {
            spool: s.clone(),
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
        }
    }

    /// `n` getrennte Batch-Dateien (je ein Spieler), DB-Revision je Spieler
    /// niedriger als die Snapshot-Revision ⇒ jeder Drain wendet an.
    fn p22_write_n_batches(s: &Spool, n: usize) -> FakeDb {
        const IDS: [&str; 5] = ["100", "101", "102", "103", "104"];
        for id in IDS.iter().take(n) {
            s.write_batch(&snapshot(id, 5, 1.0)).unwrap();
        }
        let revs: Vec<(&str, Option<i64>)> = IDS.iter().take(n).map(|id| (*id, Some(1))).collect();
        FakeDb::multi(&revs)
    }

    /// Vollständige Recovery ⇒ keine Restarbeit ⇒ Startentscheidung `READY`.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_complete_recovery_reports_no_remaining_and_allows_ready() {
        let base = temp_dir("p22ok");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let db = p22_write_n_batches(&s, 3);
        let rt = p22_runtime(&s);

        let outcome = rt.recover_with_limit(&db, 10).await.unwrap();
        assert_eq!(outcome.report.batches_processed, 3);
        assert_eq!(outcome.report.entries_applied, 3);
        assert_eq!(
            outcome.batches_remaining, 0,
            "vollständig abgearbeitete Recovery darf keine Rest melden"
        );
        assert_eq!(rt.status(), PersistStatus::Recovering);

        assert_eq!(
            rt.apply_startup_recovery(&outcome),
            RecoveryStatusUpdate::Ready
        );
        assert_eq!(rt.status(), PersistStatus::Ready);
        assert!(!rt.recovery_open());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Fünf Dateien bei Limit drei ⇒ genau zwei verbleiben ⇒ `DEGRADED`.
    /// Der Limitwert selbst ist unerheblich: entscheidend ist die Restarbeit.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_limit_with_five_batches_leaves_two_and_degrades() {
        let base = temp_dir("p22lim");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let db = p22_write_n_batches(&s, 5);
        let rt = p22_runtime(&s);
        assert_eq!(s.count_batches().unwrap(), 5);

        let outcome = rt.recover_with_limit(&db, 3).await.unwrap();
        assert_eq!(outcome.report.batches_processed, 3);
        assert_eq!(outcome.batches_remaining, 2);
        // Die Restarbeit liegt unverändert im Spool (nicht etwa in Quarantäne).
        assert_eq!(s.count_batches().unwrap(), 2);

        assert_eq!(
            rt.apply_startup_recovery(&outcome),
            RecoveryStatusUpdate::RecoveryStillOpen { remaining: 2 }
        );
        assert_eq!(rt.status(), PersistStatus::Degraded);
        assert!(
            rt.recovery_open(),
            "Start-Recovery bleibt als offen markiert"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Genau am Limit vollständig fertig ⇒ `READY` bleibt möglich. Das Limit
    /// ist ein Schutz und darf einen vollständigen Abschluss nicht verhindern.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_exactly_at_limit_is_a_complete_recovery() {
        let base = temp_dir("p22edge");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let db = p22_write_n_batches(&s, 3);
        let rt = p22_runtime(&s);

        let outcome = rt.recover_with_limit(&db, 3).await.unwrap();
        assert_eq!(outcome.report.batches_processed, 3);
        assert_eq!(outcome.batches_remaining, 0);
        assert_eq!(s.count_batches().unwrap(), 0);
        assert_eq!(
            rt.apply_startup_recovery(&outcome),
            RecoveryStatusUpdate::Ready
        );
        assert_eq!(rt.status(), PersistStatus::Ready);
        assert!(!rt.recovery_open());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Abbruch **ohne Fortschritt** mit Restarbeit ⇒ ebenfalls `DEGRADED`.
    /// Modelliert wird der `Ok(None)`-Pfad (Attributionskonflikt `P-30`), der
    /// die Schleife beendet, während die Datei liegen bleibt.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_abort_without_progress_with_remaining_is_degraded() {
        let base = temp_dir("p22stall");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let path = write_conflicting_batch(&s);
        let db = FakeDb::multi(&[]);
        let rt = p22_runtime(&s);

        let outcome = rt.recover_with_limit(&db, 10).await.unwrap();
        assert_eq!(
            outcome.batches_remaining, 1,
            "blockierte Datei bleibt relevante Restarbeit"
        );
        assert!(path.exists(), "blockierte Datei bleibt erhalten");
        assert_eq!(db.applies(), 0, "kein Apply ohne Fortschritt");

        assert_eq!(
            rt.apply_startup_recovery(&outcome),
            RecoveryStatusUpdate::RecoveryStillOpen { remaining: 1 }
        );
        assert_eq!(rt.status(), PersistStatus::Degraded);
        assert!(rt.recovery_open());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Quarantäne ist **keine** offene Recovery: eine nicht parsebare Datei
    /// wird überführt, der Spool ist leer ⇒ `READY` bleibt zulässig.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_quarantine_is_not_counted_as_remaining_recovery() {
        let base = temp_dir("p22quar");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        write_unparseable_batch(&s);
        let db = FakeDb::new(Some(1));
        let rt = p22_runtime(&s);

        let outcome = rt.recover_with_limit(&db, 10).await.unwrap();
        assert_eq!(outcome.report.batches_quarantined, 1);
        assert_eq!(
            outcome.batches_remaining, 0,
            "Quarantäne darf nicht als Restarbeit zählen"
        );
        assert!(!list_json_files(&s.quarantine_open_dir())
            .unwrap()
            .is_empty());
        assert_eq!(
            rt.apply_startup_recovery(&outcome),
            RecoveryStatusUpdate::Ready
        );
        assert_eq!(rt.status(), PersistStatus::Ready);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Erster Hintergrund-Erfolg bei **noch offener** Start-Recovery hebt
    /// `DEGRADED` nicht vorzeitig auf: nach einem echten `drain_one` bleibt
    /// Restarbeit, also kein `READY`.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_first_drain_success_keeps_degraded_while_recovery_open() {
        let base = temp_dir("p22tick1");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let db = p22_write_n_batches(&s, 3);
        let rt = p22_runtime(&s);
        // Start mit offener Recovery und Restarbeit.
        let outcome = rt.recover_with_limit(&db, 1).await.unwrap();
        assert_eq!(outcome.batches_remaining, 2);
        rt.apply_startup_recovery(&outcome);
        assert_eq!(rt.status(), PersistStatus::Degraded);
        assert!(rt.recovery_open());

        // Ein erfolgreicher Drainer-Tick: eine Batch abgearbeitet, eine bleibt.
        let report = s.drain_one_with(&db, "ws").await.unwrap();
        assert!(report.is_some(), "der Tick verarbeitet eine Batch");
        assert_eq!(s.count_batches().unwrap(), 1);
        assert_eq!(
            rt.apply_drain_tick().unwrap(),
            RecoveryStatusUpdate::RecoveryStillOpen { remaining: 1 },
            "einzelner Erfolg darf offene Recovery nicht als fertig melden"
        );
        assert_eq!(
            rt.status(),
            PersistStatus::Degraded,
            "READY ist vor bestätigtem Abschluss unzulässig"
        );
        assert!(rt.recovery_open());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Bestätigter Abschluss (Spool leer) ⇒ `READY` wird zulässig und die
    /// offene Recovery-Markierung entfällt.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_confirmed_completion_allows_ready_again() {
        let base = temp_dir("p22tick2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let db = p22_write_n_batches(&s, 1);
        let rt = p22_runtime(&s);
        rt.set_recovery_open(true);
        rt.set_status(PersistStatus::Degraded);

        let report = s.drain_one_with(&db, "ws").await.unwrap();
        assert!(report.is_some());
        assert_eq!(s.count_batches().unwrap(), 0);
        assert_eq!(rt.apply_drain_tick().unwrap(), RecoveryStatusUpdate::Ready);
        assert_eq!(rt.status(), PersistStatus::Ready);
        assert!(!rt.recovery_open(), "Abschluss ist bestätigt");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Normale neue Spool-Arbeit nach abgeschlossener Recovery: Der Spool ist
    /// nicht leer, die Start-Recovery ist aber abgeschlossen ⇒ weiterhin
    /// `READY`, **kein** pauschales `DEGRADED`.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_normal_new_batch_after_completed_recovery_stays_ready() {
        let base = temp_dir("p22tick3");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let rt = p22_runtime(&s);
        rt.set_recovery_open(false);
        rt.set_status(PersistStatus::Ready);

        // Regulärer Save erzeugt eine neue Batch.
        s.write_batch(&snapshot("900", 3, 1.0)).unwrap();
        assert_eq!(s.count_batches().unwrap(), 1);
        assert_eq!(
            rt.apply_drain_tick().unwrap(),
            RecoveryStatusUpdate::Ready,
            "normale neue Spool-Arbeit ist keine offene Start-Recovery"
        );
        assert_eq!(rt.status(), PersistStatus::Ready);
        assert!(!rt.recovery_open());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Ein Fehlerpfad des Drains (`Err`) bleibt unverändert beim Aufrufer: er
    /// setzt `DEGRADED`, und der Drainer-Tick hebt das erst nach einem
    /// bestätigten Abschluss auf.
    #[tokio::test(flavor = "current_thread")]
    async fn p22_drain_error_keeps_degraded_until_confirmed_completion() {
        struct FailingDb;
        impl DrainDb for FailingDb {
            fn load_persist_revision<'a>(
                &'a self,
                _char_id: &'a str,
            ) -> BoxFuture<'a, Result<Option<i64>, String>> {
                Box::pin(async move { Err("db down".into()) })
            }
            fn apply_snapshot<'a>(
                &'a self,
                _snapshot: &'a PersistSnapshot,
                _weapon_skill_id: &'a str,
            ) -> BoxFuture<'a, Result<(), String>> {
                Box::pin(async move { Err("db down".into()) })
            }
        }

        let base = temp_dir("p22tick4");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        s.write_batch(&snapshot("901", 3, 1.0)).unwrap();
        let rt = p22_runtime(&s);
        rt.set_recovery_open(true);

        // Fehlerpfad: der Aufrufer setzt DEGRADED (unveränderte Semantik).
        assert!(s.drain_one_with(&FailingDb, "ws").await.is_err());
        rt.set_status(PersistStatus::Degraded);

        // Der Restcheck darf daraus kein READY machen, solange Restarbeit da ist.
        assert_eq!(
            rt.apply_drain_tick().unwrap(),
            RecoveryStatusUpdate::RecoveryStillOpen { remaining: 1 }
        );
        assert_eq!(rt.status(), PersistStatus::Degraded);

        // Erst nach bestätigtem Abschluss (Spool leer) ist READY zulässig: der
        // nächste erfolgreiche Drain verarbeitet die letzte Batch.
        let ok_db = FakeDb::multi(&[("901", Some(1))]);
        assert!(s.drain_one_with(&ok_db, "ws").await.unwrap().is_some());
        assert_eq!(s.count_batches().unwrap(), 0);
        assert_eq!(rt.apply_drain_tick().unwrap(), RecoveryStatusUpdate::Ready);
        assert_eq!(rt.status(), PersistStatus::Ready);
        std::fs::remove_dir_all(&base).unwrap();
    }

    // ===== P-23: Monitoring während der Start-Recovery =====

    /// Echter HTTP-GET gegen den laufenden Health-Server. Nur Lesezugriff,
    /// keine Wartezeit, keine Schleife: der Server beantwortet und schließt die
    /// Verbindung (`Connection: close`).
    async fn p23_http_get_json(addr: std::net::SocketAddr, path: &str) -> serde_json::Value {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut sock = tokio::net::TcpStream::connect(addr)
            .await
            .expect("Verbindung zum Monitoring");
        sock.write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .expect("Request schreiben");
        let mut raw = Vec::new();
        sock.read_to_end(&mut raw).await.expect("Antwort lesen");
        let text = String::from_utf8_lossy(&raw).to_string();
        let (_head, body) = text.split_once("\r\n\r\n").expect("Kopf und Body");
        serde_json::from_str(body).expect("JSON-Antwort")
    }

    /// `P-23`: Das bestehende Monitoring ist während der Startup-Recovery
    /// erreichbar und beobachtet den tatsächlichen Persistenzstatus.
    ///
    /// **Produktionspfad:** Das Monitoring wird über dieselbe Startstelle gestartet
    /// wie im Startpfad (`health::spawn_monitor` → `serve` → `serve_bound` →
    /// `accept_loop`); der Test nutzt `health::spawn_monitor_on`, das denselben
    /// `serve_bound`/`accept_loop` auf einem bereits gebundenen Listener
    /// ausführt. Die Recovery ist die Produktionsschleife `recover_with_limit`
    /// (identisch zu `recover`), die Statusentscheidung ist die Produktions-
    /// entscheidung `apply_startup_recovery`.
    ///
    /// **Ereignisgesteuert, ohne Sleeps und ohne Datenbank:** Der freie Port
    /// entsteht durch eine **echte Bindung** auf `127.0.0.1:0` — kein
    /// Portraten und keine Probe-Bindung mit anschließendem Freigeben. Die
    /// erfolgreiche Bindung ist zugleich der Bereitschaftsnachweis, weil der
    /// Kernel Verbindungen bereits annimmt. Das Anhalten der Recovery erfolgt im
    /// DB-Schritt der vorhandenen `FakeDb::gated`-Barriere (Kanal + `Notify`),
    /// nicht über eine Wartezeit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn p23_monitoring_is_reachable_during_recovery_and_reports_the_real_status() {
        let base = temp_dir("p23mon");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        // Startstatus der Runtime: RECOVERING (Produktionszustand vor der
        // Startup-Recovery).
        let rt = Arc::new(p22_runtime(&s));
        assert_eq!(rt.status(), PersistStatus::Recovering);

        // Ein Batch, dessen Revision über der DB-Revision liegt ⇒ ein Apply
        // würde im DB-Schritt stattfinden und die Barriere auslösen.
        s.write_batch(&snapshot("777", 5, 1.0)).unwrap();

        // Freien Port durch echte Bindung ermitteln.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Listener binden");
        let addr = listener.local_addr().unwrap();

        // Monitoring starten — vor der Recovery, wie im Startpfad.
        let health_task = crate::health::spawn_monitor_on(
            listener,
            Arc::new(crate::health::test_config()),
            crate::world::new_shared(),
            rt.clone(),
        );

        // Recovery starten; die Barriere hält sie im DB-Schritt an.
        let (entered_tx, mut entered_rx) = mpsc::unbounded_channel::<()>();
        let release = Arc::new(tokio::sync::Notify::new());
        let db = FakeDb::gated(Some(1), Some((entered_tx, release.clone())));
        let recovery = {
            let rt = rt.clone();
            tokio::spawn(async move { rt.recover_with_limit(&db, 10).await })
        };

        // Ereignisgesteuert warten, bis die Recovery im DB-Schritt steht.
        tokio::time::timeout(Duration::from_secs(10), entered_rx.recv())
            .await
            .expect("Recovery erreicht den DB-Schritt nicht")
            .expect("Kanal geschlossen");
        assert_eq!(
            rt.status(),
            PersistStatus::Recovering,
            "während der Recovery ist der Status RECOVERING"
        );

        // Echter HTTP-Request **während** die Recovery läuft.
        let body = p23_http_get_json(addr, "/status").await;
        assert!(
            !recovery.is_finished(),
            "die Recovery läuft noch — Monitoring hat also währenddessen geantwortet"
        );
        assert_eq!(
            body["persistence_status"], "recovering",
            "RECOVERING muss über /status beobachtbar sein"
        );
        assert_eq!(
            rt.status(),
            PersistStatus::Recovering,
            "der Health-Request mutiert keinen Zustand"
        );
        // Bestehende Felder bleiben erhalten.
        assert_eq!(body["ok"], true);
        assert_eq!(body["server_up"], true);
        assert_eq!(body["players"], 0);

        // Recovery freigeben und abwarten.
        release.notify_one();
        let joined = tokio::time::timeout(Duration::from_secs(10), recovery)
            .await
            .expect("Recovery terminiert nicht")
            .expect("kein Panic");
        let outcome = joined.expect("Recovery ohne Fehler");
        assert_eq!(outcome.report.batches_processed, 1);
        assert_eq!(outcome.batches_remaining, 0);

        // Produktions-Statusentscheidung des Startpfads anwenden.
        assert_eq!(
            rt.apply_startup_recovery(&outcome),
            RecoveryStatusUpdate::Ready
        );

        // Korrekter Folgestatus über HTTP beobachten.
        let body = p23_http_get_json(addr, "/status").await;
        assert_eq!(body["persistence_status"], "ready");

        // Monitoring ist keine Spielfreigabe: /health bleibt Liveness, und
        // /players bleibt unberührt.
        let live = p23_http_get_json(addr, "/health").await;
        assert_eq!(live["ok"], true);
        assert!(live.get("persistence_status").is_none());

        // Tasks und Listener zuverlässig beenden.
        health_task.abort();
        let _ = health_task.await;
        std::fs::remove_dir_all(&base).unwrap();
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
        let held = s.player_gate("42").await.lock_owned().await;
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
        assert!(
            s.player_gate("42").await.try_lock_owned().await.is_none(),
            "Gate muss über den DB-Schritt gehalten sein"
        );
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

    // ---- P-29: Lebensdauer der in_flight-Gates ----

    /// `P-29`: Zwei Aufrufer derselben `player_id` benutzen **dasselbe** Gate.
    /// Die Identitätsprüfung läuft über `same_gate` (der Map-`Arc` ist hinter
    /// der Lease verborgen).
    #[tokio::test]
    async fn p29_two_callers_of_same_player_share_one_gate() {
        let base = temp_dir("p29same");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let a = s.player_gate("42").await;
        let b = s.player_gate("42").await;
        assert!(a.same_gate(&b), "gleiche player_id => ein Gate");
        assert_eq!(s.in_flight_len(), 1, "genau ein Map-Eintrag");
        drop(a);
        drop(b);
        assert_eq!(s.in_flight_len(), 0, "nach Freigabe entfernt");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// `P-29`: Ein **wartender** Aufrufer (seine Lease existiert bereits, der
    /// Guard noch nicht) überlebt das Cleanup des bisherigen Besitzers sicher.
    /// Der Map-Eintrag darf nicht verschwinden, solange der Wartende zählt.
    #[tokio::test]
    async fn p29_waiting_holder_survives_owner_cleanup() {
        let base = temp_dir("p29wait");
        let s = spool(&base);
        s.ensure_dirs().unwrap();

        // Besitzer hält das Gate; der Wartende holt schon seine Lease.
        let held = s.player_gate("42").await.lock_owned().await;
        let waiter_lease = s.player_gate("42").await;
        assert_eq!(s.in_flight_len(), 1);

        // Der Besitzer gibt frei. Der Wartende muss **dasselbe** Gate sehen,
        // also darf der Map-Eintrag nicht entfernt worden sein.
        drop(held);
        assert_eq!(
            s.in_flight_len(),
            1,
            "wartende Lease haelt den Map-Eintrag am Leben"
        );

        // Der Wartende erhaelt nun das Gate und kann es sperren.
        let acquired = waiter_lease.lock_owned().await;
        // Ein frischer Zugriff landet ebenfalls auf diesem Gate.
        let later = s.player_gate("42").await;
        assert!(later.same_gate_during(&acquired), "kein zweites Gate entstanden");
        drop(later);
        drop(acquired);
        assert_eq!(s.in_flight_len(), 0, "nach letztem Halter entfernt");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// `P-29`: Ein **ausgegebener, aber nicht gelockter** Handle verhindert
    /// die Gate-Spaltung. Genau der Fall, an dem ein Cleanup über
    /// `Arc::strong_count` scheitern würde.
    #[tokio::test]
    async fn p29_unlocked_handle_prevents_gate_split() {
        let base = temp_dir("p29split");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        // Handle ausgeben, aber nicht sperren: der Zaehler steht auf 1.
        let handle = s.player_gate("42").await;
        assert_eq!(s.in_flight_len(), 1);
        // Ein zweiter Aufrufer muss dasselbe Gate vorfinden, kein neues.
        let second = s.player_gate("42").await;
        assert!(handle.same_gate(&second), "kein zweites Gate");
        drop(second);
        // Solange `handle` lebt, bleibt der Eintrag bestehen.
        assert_eq!(s.in_flight_len(), 1, "Handle ohne Lock haelt Eintrag");
        drop(handle);
        assert_eq!(s.in_flight_len(), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// `P-29`: Nach dem letzten Nutzer wird der Map-Eintrag entfernt, und ein
    /// erneuter Zugriff funktioniert wieder.
    #[tokio::test]
    async fn p29_entry_removed_after_last_user_and_reusable_afterwards() {
        let base = temp_dir("p29reuse");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        {
            let _g = s.player_gate("42").await.lock_owned().await;
            assert_eq!(s.in_flight_len(), 1);
        }
        assert_eq!(s.in_flight_len(), 0, "Guard-Drop entfernt den Eintrag");

        // Erneuter Zugriff nach vollstaendiger Freigabe: funktioniert wieder
        // und wird am Ende sauber abgeraeumt.
        {
            let again = s.player_gate("42").await.lock_owned().await;
            assert_eq!(s.in_flight_len(), 1, "erneuter Zugriff erzeugt Eintrag");
            // Solange dieser Aufrufer haelt, sieht ein weiterer das gleiche
            // Gate — genau die Invariante, die der Cleanup schuetzt.
            let third = s.player_gate("42").await;
            assert!(
                third.same_gate_during(&again),
                "waehrend der Haltezeit ein Gate, kein Reuse daneben"
            );
            drop(third);
            assert_eq!(s.in_flight_len(), 1, "Guard haelt den Eintrag");
        }
        assert_eq!(s.in_flight_len(), 0, "auch der zweite Durchlauf raeumt auf");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// `P-29`: Viele verschiedene, jeweils abgeschlossene Spielerzugriffe
    /// lassen die Map **nicht** dauerhaft wachsen.
    #[tokio::test]
    async fn p29_many_finished_players_do_not_grow_the_map() {
        let base = temp_dir("p29many");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        for i in 0..64 {
            let _g = s.player_gate(&format!("p{i}")).await.lock_owned().await;
        }
        assert_eq!(
            s.in_flight_len(),
            0,
            "nach 64 abgeschlossenen Zugriffen kein Rest"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// `P-29`: Fehler- und Panic-Unwinding hinterlaesst kein unbenutztes Gate.
    /// Der Fehlerpfad enthaelt kein `catch_unwind`; das Unwinding laeuft ueber den
    /// normalen Rust-Drop der Lease.
    #[tokio::test]
    async fn p29_error_and_panic_paths_leave_no_unused_gate() {
        let base = temp_dir("p29err");
        let s = spool(&base);
        s.ensure_dirs().unwrap();

        // (a) Fehlerpfad: der Aufrufer bricht per `?` ab.
        let fehler: Result<(), String> = async {
            let _g = s.player_gate("42").await.lock_owned().await;
            Err(" simulierter Fehler".to_string())
        }
        .await;
        assert!(fehler.is_err());
        assert_eq!(s.in_flight_len(), 0, "Fehlerpfad ohne Rest");

        // (b) Panic-Unwinding: der Panic wird gefangen, die Lease wird per
        // Drop freigegeben. `catch_unwind` um einen Future braucht
        // `AssertUnwindSafe`, weil die Map nicht `UnwindSafe` ist.
        let s2 = s.clone();
        let res = std::panic::AssertUnwindSafe(async move {
            let _g = s2.player_gate("42").await.lock_owned().await;
            panic!("simulierter Panic");
        })
        .catch_unwind()
        .await;
        assert!(res.is_err(), "Panic muss den Stack abwickeln");
        assert_eq!(s.in_flight_len(), 0, "Panic-Pfad ohne Rest");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// `P-29`: Unterschiedliche Spieler bleiben unabhaengig — ihre Gates
    /// blockieren sich nicht gegenseitig.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn p29_different_players_stay_independent() {
        let base = temp_dir("p29indep");
        let s = std::sync::Arc::new(spool(&base));
        s.ensure_dirs().unwrap();
        let a = s.player_gate("a").await.lock_owned().await;
        let b = s.player_gate("b").await.lock_owned().await;
        assert_eq!(s.in_flight_len(), 2, "zwei getrennte Eintraege");
        drop(a);
        drop(b);
        assert_eq!(s.in_flight_len(), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    // ---- P-12: atomare Veröffentlichung ohne Klon-Sichtbarkeit (§35) ----

    /// P-12: Während der Erstellung ist **keine** finale Datei sichtbar.
    /// Der vollständige Inhalt entsteht ausschließlich unter dem temporären
    /// Namen; ein finaler Batch erscheint erst durch den atomaren Schritt.
    ///
    /// Der Test beobachtet den Schreibpfad ereignisgesteuert über einen
    /// Leser, der unmittelbar nach dem Anlegen der Temporärdatei läuft.
    /// Erwartet wird: zu diesem Zeitpunkt existiert **keine** finale Datei
    /// und `list_json_files` (die Sicht für den Drain) ist leer.
    #[test]
    fn p12_no_final_file_is_visible_while_the_batch_is_being_written() {
        let base = temp_dir("p12invis");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("42", 5, 1.0);
        let batch = SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: "probe".into(),
            entries: vec![SpoolEntry {
                format_version: FORMAT_VERSION,
                snapshot: snap.clone(),
            }],
        };
        let body = serde_json::to_vec(&batch).unwrap();
        let target = base.join("spool").join("probe.json");

        // Der Veröffentlichungsschritt wird aufgeteilt: erst der Temp-Inhalt
        // (unvollständigster Zwischenstand, den ein Drain niemals liest), dann
        // der atomare Schritt. Dazwischen liegt **keine** finale Datei vor.
        let tmp = base.join("spool").join(".tmp-probe.json");
        let mut f = std::fs::File::create(&tmp).unwrap();
        std::io::Write::write_all(&mut f, &body[..body.len() / 2]).unwrap();
        f.sync_all().unwrap();
        drop(f);

        // Sichtbarkeit des Drains: keine fertige Datei, Temp ist ausgefiltert.
        assert!(
            !target.exists(),
            "vor der Veröffentlichung darf keine finale Datei existieren"
        );
        assert!(
            list_json_files(&s.spool_dir()).unwrap().is_empty(),
            "Drain darf keinen halbfertigen Batch lesen"
        );
        assert!(
            s.next_batch_path().unwrap().is_none(),
            "der Drain sieht keinen halbfertigen Batch"
        );

        // Der atomare Abschluss macht den fertigen Inhalt sichtbar.
        write_atomic_if_absent(&tmp, &target, &body).unwrap();
        assert!(target.exists());
        assert_eq!(std::fs::read(&target).unwrap(), body, "fertiger Inhalt");
        assert_eq!(list_json_files(&s.spool_dir()).unwrap().len(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Ein Drain kann keinen halbfertigen Batch lesen — auch dann nicht,
    /// wenn eine Temporärdatei liegen bleibt. Geprüft über den echten
    /// Drain-Aufruf: er sieht ausschließlich die fertige Datei.
    #[tokio::test]
    async fn p12_drain_never_reads_a_half_written_batch() {
        let base = temp_dir("p12halbdrain");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let batch = SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: "halb".into(),
            entries: vec![SpoolEntry {
                format_version: FORMAT_VERSION,
                snapshot: snapshot("42", 5, 1.0),
            }],
        };
        let body = serde_json::to_vec(&batch).unwrap();
        // Abgebrochene Veröffentlichung: nur die halbe Temporärdatei liegt vor.
        let tmp = base.join("spool").join(".tmp-halbbatch.json");
        let mut f = std::fs::File::create(&tmp).unwrap();
        std::io::Write::write_all(&mut f, &body[..10]).unwrap();
        f.sync_all().unwrap();
        drop(f);

        let db = FakeDb::new(Some(1));
        let out = s.drain_one_with(&db, "1").await.unwrap();
        assert!(
            out.is_none(),
            "Drain darf die halbe Datei nicht als Batch verarbeiten"
        );
        assert_eq!(db.loads(), 0, "kein DB-Zugriff aus einer Temporärdatei");
        assert_eq!(db.applies(), 0);
        assert!(tmp.exists(), "Temporärdatei bleibt unangetastet liegen");
        assert_no_side_artifacts(&s);

        // Erst die vollständige Veröffentlichung macht den Batch sichtbar.
        let target = base.join("spool").join("halb.json");
        write_atomic_if_absent(&tmp, &target, &body).unwrap();
        let out = s.drain_one_with(&db, "1").await.unwrap().expect("jetzt verarbeitbar");
        assert_eq!(out.entries_applied, 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Abbruch **vor** der Veröffentlichung hinterlässt höchstens eine
    /// Temporärdatei und keinen unvollständigen finalen Batch. Der
    /// Schreibfehlerfall wird deterministisch erzwungen, indem das
    /// Spool-Verzeichnis entfernt wird.
    #[tokio::test]
    async fn p12_abort_before_publication_leaves_no_incomplete_final_batch() {
        let base = temp_dir("p12abort");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let shared = crate::world::new_shared();
        let (p, _rx) = dirty_test_player("42");
        shared.lock().await.players.insert(p.id.clone(), p);
        let runtime = PersistRuntime {
            spool: s.clone(),
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
        };

        // Schreibziel unbenutzbar → Abbruch vor jeder Veröffentlichung.
        let spool_dir = s.spool_dir();
        std::fs::remove_dir_all(&spool_dir).unwrap();
        assert!(
            runtime.persist_dirty_run(&shared, &["42".to_string()]).await.is_err(),
            "Abbruch muss als Fehler melden"
        );

        // Es existiert **kein** unvollständiger finaler Batch.
        assert!(!spool_dir.exists());
        s.ensure_dirs().unwrap();
        assert!(
            list_json_files(&s.spool_dir()).unwrap().is_empty(),
            "kein unvollständiger finaler Batch nach Abbruch"
        );
        assert_eq!(s.count_batches().unwrap(), 0);
        // Dirty und Revision bleiben für einen späteren Versuch erhalten.
        let world = shared.lock().await;
        let p = world.players.get("42").unwrap();
        assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Position));
        assert_eq!(p.persist_revision, 5);
        drop(world);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Konkurrierende Veröffentlichungen überschreiben keinen Bestand.
    /// Zwei Aufrufe mit demselben Namen: der erste gewinnt, der zweite
    /// scheitert am atomaren Schritt und entscheidet anschließend über den
    /// vollständigen Inhaltsvergleich, ohne die vorhandene Datei zu ändern.
    #[test]
    fn p12_racing_publications_never_clobber_an_existing_file() {
        let base = temp_dir("p12race2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("42", 5, 1.0);
        let batch = SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: "race".into(),
            entries: vec![SpoolEntry {
                format_version: FORMAT_VERSION,
                snapshot: snap.clone(),
            }],
        };
        let body = serde_json::to_vec(&batch).unwrap();
        let target = base.join("spool").join("race.json");

        // Erster Versuch gewinnt.
        let tmp1 = base.join("spool").join(".tmp-race-1.json");
        assert!(write_atomic_if_absent(&tmp1, &target, &body).unwrap());
        let first = std::fs::read(&target).unwrap();

        // Zweiter Versuch mit abweichendem Inhalt, identischem Zielnamen.
        let mut other = snap.clone();
        other.x = 77.0;
        let conflicting = SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: "race".into(),
            entries: vec![SpoolEntry {
                format_version: FORMAT_VERSION,
                snapshot: other,
            }],
        };
        let body2 = serde_json::to_vec(&conflicting).unwrap();
        let tmp2 = base.join("spool").join(".tmp-race-2.json");
        assert!(
            !write_atomic_if_absent(&tmp2, &target, &body2).unwrap(),
            "zweiter Versuch darf den Namen nicht belegen"
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            first,
            "Bestand darf nicht überschrieben werden"
        );
        // Der echte Produktionspfad entscheidet danach inhaltlich.
        assert!(
            confirm_existing_publication(&target, &conflicting).is_err(),
            "abweichender Inhalt darf nicht bestätigt werden"
        );
        assert!(
            confirm_existing_publication(&target, &batch).is_ok(),
            "identischer Inhalt wird bestätigt"
        );
        assert_eq!(std::fs::read(&target).unwrap(), first);
        assert_eq!(list_json_files(&s.spool_dir()).unwrap().len(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Wiederholung **nach** Fehler an der Dauerhaftigkeitsgrenze nutzt
    /// den echten Produktionspfad (`persist_dirty_run` → `write_batch_run` →
    /// `confirm_existing_publication`) und erhält Dirty/Revision korrekt.
    ///
    /// Szenario: Die Veröffentlichung scheitert, nachdem die Zieldatei bereits
    /// vorlag. Der erneute Lauf muss Inhalt und Dauerhaftigkeit bestätigen,
    /// ohne die Datei neu zu schreiben oder den Zustand zu verlieren.
    #[tokio::test]
    async fn p12_retry_after_durability_failure_uses_the_production_path() {
        let base = temp_dir("p12retrypath");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let shared = crate::world::new_shared();
        let (p, _rx) = dirty_test_player("42");
        shared.lock().await.players.insert(p.id.clone(), p);
        let runtime = PersistRuntime {
            spool: s.clone(),
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
        };

        // Der erste Lauf scheitert an der Dauerhaftigkeitsgrenze
        // (Schreibziel unbenutzbar); es entsteht keine Datei.
        std::fs::remove_dir_all(s.spool_dir()).unwrap();
        assert!(runtime.persist_dirty_run(&shared, &["42".to_string()]).await.is_err());
        assert_eq!(
            runtime.status(),
            PersistStatus::Degraded,
            "erster Fehlschlag setzt DEGRADED"
        );

        // Zustand nach dem Fehlschlag: Dirty und Revision unverändert.
        {
            let world = shared.lock().await;
            let p = world.players.get("42").unwrap();
            assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Position));
            assert_eq!(p.persist_revision, 5, "Revision nicht verbraucht");
        }

        // Der Wiederholungslauf nimmt **denselben** Produktionspfad und
        // veröffentlicht regulär.
        s.ensure_dirs().unwrap();
        assert_eq!(
            runtime.persist_dirty_run(&shared, &["42".to_string()]).await.unwrap(),
            1
        );
        assert_eq!(s.count_batches().unwrap(), 1, "genau eine Datei");
        let name = only_batch_name(&base);
        let published = std::fs::read(base.join("spool").join(&name)).unwrap();
        {
            let world = shared.lock().await;
            assert!(!world.players.get("42").unwrap().dirty.any(), "Dirty bereinigt");
        }

        // Ein **dritter** Lauf erfasst eine **neue Revision** (RAM-Revision
        // wurde fortgeschrieben). Damit ist ein abweichender Inhalt zu
        // erwarten: Der Lauf schreibt eine weitere Datei und lässt den
        // vorhandenen Bestand unangetastet. Nachgewiesen wird deshalb der
        // Bestandsschutz, nicht die Dateianzahl.
        let world = shared.lock().await;
        let rev_vorher = world.players.get("42").unwrap().persist_revision;
        drop(world);
        {
            let mut w = shared.lock().await;
            let pl = w.players.get_mut("42").unwrap();
            pl.mark_dirty(crate::persist::PersistComponent::Position);
            drop(w);
        }
        assert_eq!(
            runtime.persist_dirty_run(&shared, &["42".to_string()]).await.unwrap(),
            1
        );
        assert_eq!(
            std::fs::read(base.join("spool").join(&name)).unwrap(),
            published,
            "Bestand aus dem ersten Lauf bleibt unverändert erhalten"
        );
        let world = shared.lock().await;
        assert!(
            world.players.get("42").unwrap().persist_revision > rev_vorher,
            "Revision wurde fortgeschrieben"
        );
        drop(world);
        std::fs::remove_dir_all(&base).unwrap();
    }

    // ---- P-12: Veröffentlichung, Überschreibschutz, Idempotenz (§35/§38) ----

    /// P-12: Ein Mehrspieler-Batch erzeugt **unabhängig von der
    /// Eingabereihenfolge** denselben Namen und denselben serialisierten
    /// Inhalt. Belegt die kanonische Ordnung vor Namensbildung.
    #[test]
    fn p12_swapped_input_order_yields_identical_content_and_no_second_file() {
        let base = temp_dir("p12order");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let a = snapshot("42", 5, 1.0);
        let b = snapshot("43", 7, 2.0);
        s.write_batch_run(vec![a.clone(), b.clone()]).unwrap();
        let first = only_batch_name(&base);
        let first_body = std::fs::read(base.join("spool").join(&first)).unwrap();
        assert_eq!(s.count_batches().unwrap(), 1);

        // Vertauschte Eingabereihenfolge, gleiche vollständige Snapshots.
        s.write_batch_run(vec![b, a]).unwrap();
        assert_eq!(
            s.count_batches().unwrap(),
            1,
            "vertauschte Reihenfolge darf keine zweite fertige Datei erzeugen"
        );
        assert_eq!(only_batch_name(&base), first, "gleicher Name");
        assert_eq!(
            std::fs::read(base.join("spool").join(&first)).unwrap(),
            first_body,
            "serialisierter Inhalt muss identisch sein"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Ein bereits vorhandener **identischer** Batch gilt als
    /// sicherer idempotenter Erfolg — belegt durch `Ok`, nicht durch einen
    /// Schreibvorgang.
    #[test]
    fn p12_existing_identical_batch_is_a_safe_idempotent_success() {
        let base = temp_dir("p12idem2");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("42", 5, 1.0);
        s.write_batch(&snap).unwrap();
        let name = only_batch_name(&base);
        let before = std::fs::read(base.join("spool").join(&name)).unwrap();
        // Erneuter identischer Versuch: Erfolg, keine neue Datei.
        let out = s.write_batch_run(vec![snap]).unwrap();
        assert_eq!(out.map(|p| p.file_name().unwrap().to_string_lossy().to_string()),
            Some(name.clone()));
        assert_eq!(s.count_batches().unwrap(), 1);
        assert_eq!(std::fs::read(base.join("spool").join(&name)).unwrap(), before);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Erzwungener gleicher Zielname bei **abweichendem** Snapshot-Inhalt.
    /// Erwartung: kein Überschreiben, kein falscher Erfolg.
    ///
    /// Der Konflikt wird deterministisch erzeugt, indem ein Batch geschrieben
    /// und danach unter demselben Namen ein Batch mit abweichendem Inhalt
    /// angeboten wird. Der Digest allein kann den Konflikt nicht erkennen —
    /// deshalb wird der **vorhandene Inhalt** gezielt überschrieben, um genau
    /// den Namenskonflikt mit abweichendem Inhalt nachzustellen.
    #[test]
    fn p12_forced_same_name_with_different_content_never_overwrites_or_succeeds() {
        let base = temp_dir("p12conflict");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let mut snap = snapshot("42", 5, 1.0);
        s.write_batch(&snap).unwrap();
        let name = only_batch_name(&base);
        let target = base.join("spool").join(&name);
        let original = std::fs::read(&target).unwrap();

        // Zweiter, inhaltlich abweichender Batch mit identischer
        // Identität+Revision (x statt 1.0) → kanonisch gleiche Namensbasis.
        snap.x = 99.0;
        let err = s
            .write_batch(&snap)
            .expect_err("abweichender Inhalt unter gleichem Namen darf kein Erfolg sein");
        assert!(
            err.contains("Batch-Konflikt"),
            "klarer Konfliktfehler erwartet, war: {err}"
        );
        // Vorhandene Datei unverändert erhalten.
        assert_eq!(std::fs::read(&target).unwrap(), original, "kein Überschreiben");
        assert_eq!(s.count_batches().unwrap(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Konkurrierende Veröffentlichung unter demselben Namen.
    /// Der Erstschreibende gewinnt, der Zweite erzeugt **keine** zweite Datei
    /// und verliert keine bereits veröffentlichte Datei. Der Inhaltsvergleich
    /// entscheidet über den Erfolg.
    #[test]
    fn p12_concurrent_publication_loses_no_already_published_file() {
        let base = temp_dir("p12concur");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("42", 5, 1.0);
        s.write_batch(&snap).unwrap();
        let name = only_batch_name(&base);
        let target = base.join("spool").join(&name);
        let published = std::fs::read(&target).unwrap();

        // "Konkurrierender" Zweitversuch: derselbe Name, identischer Inhalt.
        // `write_atomic_if_absent` muss am `hard_link` scheitern und darf die
        // vorhandene Datei nicht verdrängen.
        let body = serde_json::to_vec(&SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: name.trim_end_matches(".json").to_string(),
            entries: vec![SpoolEntry {
                format_version: FORMAT_VERSION,
                snapshot: snap.clone(),
            }],
        })
        .unwrap();
        let tmp = base.join("spool").join(".tmp-konkurrenz");
        let created = write_atomic_if_absent(&tmp, &target, &body).unwrap();
        assert!(!created, "hard_link muss einen bestehenden Namen ablehnen");
        assert!(target.exists(), "veröffentlichte Datei darf nicht verschwinden");
        assert_eq!(std::fs::read(&target).unwrap(), published, "Inhalt unverändert");

        // Der reguläre Pfad meldet daraufhin sicheren idempotenten Erfolg.
        assert!(s.write_batch(&snap).is_ok());
        assert_eq!(s.count_batches().unwrap(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12, Dauerhaftigkeitsgrenze: `publish_new_file` darf nur `Ok` liefern,
    /// wenn der **Verzeichnis-Sync** erfolgreich war. Ein erzwungener
    /// Sync-Fehler am Elternverzeichnis muss als Fehler durchschlagen, damit
    /// keine Dirty-Rücknahme erfolgt.
    ///
    /// Der Fehler wird deterministisch erzeugt: Das Zielverzeichnis wird vor
    /// dem Schreiben entfernt. Dann schlägt bereits das Anlegen fehl — das
    /// belegt, dass `write_batch_run` **keinen** erfolgreichen Abschluss
    /// meldet, wenn keine dauerhafte Veröffentlichung stattgefunden hat, und
    /// dass die Dirty-Bits erhalten bleiben.
    #[tokio::test]
    async fn p12_failed_durability_boundary_reports_no_successful_persistence() {
        let base = temp_dir("p12durable");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let shared = crate::world::new_shared();
        let (p, _rx) = dirty_test_player("42");
        shared.lock().await.players.insert(p.id.clone(), p);
        let runtime = PersistRuntime {
            spool: s.clone(),
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
        };

        // Spool-Verzeichnis entfernen: die Veröffentlichung muss scheitern.
        let spool_dir = s.spool_dir();
        std::fs::remove_dir_all(&spool_dir).unwrap();
        let res = runtime.persist_dirty_run(&shared, &["42".to_string()]).await;
        assert!(res.is_err(), "ohne dauerhafte Veröffentlichung kein Erfolg");
        assert_eq!(
            runtime.status(),
            PersistStatus::Degraded,
            "Fehlschlag setzt DEGRADED"
        );
        // Dirty-Zustand bleibt für einen späteren Versuch erhalten (§40).
        let world = shared.lock().await;
        let p = world.players.get("42").unwrap();
        assert!(
            p.dirty.is_dirty(crate::persist::PersistComponent::Position),
            "Dirty bleibt erhalten, es wurde nichts dauerhaft gesichert"
        );
        assert_eq!(p.persist_revision, 5, "Revision nicht verbraucht");
        drop(world);

        // Nach Wiederherstellung des Verzeichnisses gelingt der Versuch.
        s.ensure_dirs().unwrap();
        assert_eq!(runtime.persist_dirty_run(&shared, &["42".to_string()]).await.unwrap(), 1);
        assert_eq!(s.count_batches().unwrap(), 1);
        let world = shared.lock().await;
        assert!(!world.players.get("42").unwrap().dirty.any());
        drop(world);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Bestätigung nach fehlgeschlagenem Sync muss Inhalt **und**
    /// Dauerhaftigkeit prüfen. Ein abweichender Inhalt wird abgelehnt, ein
    /// übereinstimmender Inhalt wird durch erneuten Verzeichnis-Sync bestätigt.
    #[test]
    fn p12_retry_confirmation_checks_content_and_redoes_dir_sync() {
        let base = temp_dir("p12confirm");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("42", 5, 1.0);
        s.write_batch(&snap).unwrap();
        let name = only_batch_name(&base);
        let target = base.join("spool").join(&name);
        let batch = SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: name.trim_end_matches(".json").to_string(),
            entries: vec![SpoolEntry {
                format_version: FORMAT_VERSION,
                snapshot: snap.clone(),
            }],
        };
        // Identischer Inhalt: Bestätigt (inkl. erneutem Verzeichnis-Sync).
        assert!(confirm_existing_publication(&target, &batch).is_ok());
        // Abweichender Inhalt: abgelehnt.
        let mut other = snap.clone();
        other.x = 42.0;
        let abweichend = SpoolBatch {
            format_version: BATCH_FORMAT_VERSION,
            batch_id: batch.batch_id.clone(),
            entries: vec![SpoolEntry {
                format_version: FORMAT_VERSION,
                snapshot: other,
            }],
        };
        assert!(
            confirm_existing_publication(&target, &abweichend).is_err(),
            "abweichender Inhalt darf nicht bestätigt werden"
        );
        // Fehlende Datei: abgelehnt.
        std::fs::remove_file(&target).unwrap();
        assert!(confirm_existing_publication(&target, &batch).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Doppelte Spieleridentität in einem Batch wird eindeutig
    /// abgewiesen, nicht stillschweigend verdrängt.
    #[test]
    fn p12_duplicate_player_identity_in_one_batch_is_rejected() {
        let base = temp_dir("p12dup");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let a = snapshot("42", 5, 1.0);
        let mut b = snapshot("42", 6, 2.0);
        b.captured_at_ms = a.captured_at_ms;
        let err = s
            .write_batch_run(vec![a, b])
            .expect_err("doppelte Identität muss abgewiesen werden");
        assert!(
            err.contains("mehrfach dieselbe Spieleridentität"),
            "eindeutiger Fehler erwartet, war: {err}"
        );
        assert_eq!(
            s.count_batches().unwrap(),
            0,
            "kein Batch bei abgewiesener Identität"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    // ---- P-12: gemeinsamer Batch je Persistenzlauf (§35) ----

    /// P-12, Kernregel 1: ein Lauf über N dirty Spieler erzeugt **eine**
    /// gemeinsame Batch-Datei mit genau diesen N Snapshots.
    #[tokio::test]
    async fn p12_run_writes_exactly_one_batch_containing_all_dirty_players() {
        let base = temp_dir("p12one");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let shared = crate::world::new_shared();
        for id in ["42", "43", "44"] {
            let (p, _rx) = dirty_test_player(id);
            shared.lock().await.players.insert(p.id.clone(), p);
        }
        let ids: Vec<String> = ["42", "43", "44"].iter().map(|s| s.to_string()).collect();
        let runtime = PersistRuntime {
            spool: s.clone(),
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
        };
        assert_eq!(runtime.persist_dirty_run(&shared, &ids).await.unwrap(), 3);

        // Genau eine Datei, und sie ist das gemeinsame Batch.
        let files = list_json_files(&s.spool_dir()).unwrap();
        assert_eq!(files.len(), 1, "ein Lauf = eine Datei, war: {files:?}");
        let raw = std::fs::read_to_string(&files[0]).unwrap();
        let batch: SpoolBatch = serde_json::from_str(&raw).expect("Batch lesbar");
        assert_eq!(batch.format_version, BATCH_FORMAT_VERSION);
        assert_eq!(batch.entries.len(), 3, "alle dirty Spieler in EINER Datei");
        let mut got: Vec<String> = batch
            .entries
            .iter()
            .map(|e| e.snapshot.player_id.clone())
            .collect();
        got.sort();
        assert_eq!(got, vec!["42", "43", "44"]);
        // Jeder Eintrag ist ein vollständiger Snapshot.
        for e in &batch.entries {
            assert_eq!(e.format_version, FORMAT_VERSION);
            assert!(e.snapshot.persist_revision > 0);
        }
        // Und genau eine Datei, keine Reste.
        assert_eq!(s.count_batches().unwrap(), 1);
        assert!(!s.spool_dir().read_dir().unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().starts_with(".tmp-")));
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12, Kernregel 2: leere Dirty-Menge erzeugt **keine** Datei.
    #[tokio::test]
    async fn p12_empty_dirty_set_creates_no_file_at_all() {
        let base = temp_dir("p12empty");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let shared = crate::world::new_shared();
        // Spieler ohne Dirty-State (dirty_test_player markiert Position).
        let (mut p, _rx) = dirty_test_player("42");
        p.dirty = crate::persist::PersistDirty::default();
        shared.lock().await.players.insert(p.id.clone(), p);
        let runtime = PersistRuntime {
            spool: s.clone(),
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
        };
        let out = runtime
            .persist_dirty_run(&shared, &["42".to_string()])
            .await
            .unwrap();
        assert_eq!(out, 0);
        assert!(list_json_files(&s.spool_dir()).unwrap().is_empty(), "keine Datei");
        assert_eq!(s.count_batches().unwrap(), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Dirty-Bits und Revisionen werden je Eintrag nach der
    /// dauerhaften Veröffentlichung bereinigt.
    #[tokio::test]
    async fn p12_run_clears_dirty_and_advances_revision_per_entry() {
        let base = temp_dir("p12dirty");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let shared = crate::world::new_shared();
        for id in ["42", "43"] {
            let (p, _rx) = dirty_test_player(id);
            shared.lock().await.players.insert(p.id.clone(), p);
        }
        let runtime = PersistRuntime {
            spool: s.clone(),
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
        };
        let ids: Vec<String> = ["42", "43"].iter().map(|s| s.to_string()).collect();
        runtime.persist_dirty_run(&shared, &ids).await.unwrap();
        let world = shared.lock().await;
        for id in &ids {
            let p = world.players.get(id).unwrap();
            assert!(!p.dirty.any(), "{id} muss nach dem Write clean sein");
            assert!(p.persist_revision > 5, "{id} Revision muss fortgeschrieben sein");
        }
        drop(world);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12, Kernregel 5: Änderungen **während** des Schreibens bleiben dirty
    /// (§15/§39). Der Write-Callback mutiert den Spieler; die Generation
    /// stimmt danach nicht mehr, also darf das Dirty-Bit nicht bereinigt werden.
    #[tokio::test]
    async fn p12_changes_during_the_write_stay_dirty() {
        let base = temp_dir("p12race");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let shared = crate::world::new_shared();
        let (p, _rx) = dirty_test_player("42");
        shared.lock().await.players.insert(p.id.clone(), p);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // Kernregel 1 als Direktnachweis: der Write wird genau einmal gerufen.
        let inner = s.clone();
        let writer_shared = shared.clone();
        let calls_c = calls.clone();
        let out = crate::persist::persist_dirty_run(
            &shared,
            &["42".to_string()],
            move |snapshots| async move {
                calls_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                assert_eq!(snapshots.len(), 1);
                // Änderung während des Schreibens: neue Komponente dirty.
                let mut w = writer_shared.lock().await;
                if let Some(p) = w.players.get_mut("42") {
                    p.mark_dirty(crate::persist::PersistComponent::Progression);
                }
                drop(w);
                inner.write_batch_run(snapshots).map(|_| ())
            },
        )
        .await
        .unwrap();
        assert_eq!(out, 1);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "genau ein Write");
        let world = shared.lock().await;
        let p = world.players.get("42").unwrap();
        assert!(
            p.dirty.is_dirty(crate::persist::PersistComponent::Progression),
            "Änderung während des Schreibens bleibt dirty"
        );
        drop(world);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: zwei Läufe erzeugen zwei Dateien (kein Vermischen), und die
    /// Reihenfolge bleibt chronologisch.
    #[tokio::test]
    async fn p12_two_runs_create_two_distinct_batches() {
        let base = temp_dir("p12two");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let mut a = snapshot("42", 3, 1.0);
        a.captured_at_ms = 1_700_000_000_000;
        let mut b = snapshot("43", 4, 2.0);
        b.captured_at_ms = 1_700_000_000_500;
        s.write_batch_run(vec![a.clone()]).unwrap();
        s.write_batch_run(vec![b.clone()]).unwrap();
        assert_eq!(s.count_batches().unwrap(), 2, "zwei Läufe = zwei Dateien");
        // Inhaltlich verschieden -> verschiedene Namen (Digest).
        let first = s.next_batch_path().unwrap().unwrap();
        assert_eq!(parse_batch_file_name(&first.file_name().unwrap().to_string_lossy()), Some(a.captured_at_ms));
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: ein gemeinsamer Batch wendet **jeden** Eintrag an; die Datei
    /// verschwindet erst danach.
    #[tokio::test]
    async fn p12_shared_batch_applies_every_entry_once() {
        let base = temp_dir("p12apply");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snaps = vec![snapshot("42", 5, 1.0), snapshot("43", 7, 2.0)];
        s.write_batch_run(snaps).unwrap();
        let db = FakeDb::multi(&[("42", Some(1)), ("43", Some(1))]);
        let out = s.drain_one_with(&db, "1").await.unwrap().expect("Batch verarbeitet");
        assert_eq!(out.batches_processed, 1);
        assert_eq!(out.entries_applied, 2, "beide Einträge angewendet");
        assert_eq!(out.entries_superseded, 0);
        assert_eq!(out.batches_quarantined, 0);
        assert_eq!(db.applies(), 2);
        let mut applied = db.applied();
        applied.sort();
        assert_eq!(applied, vec![("42".to_string(), 5), ("43".to_string(), 7)]);
        assert!(list_json_files(&s.spool_dir()).unwrap().is_empty(), "Datei erst nach allen Einträgen weg");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12, Kernregel 6: ein problematischer, aber eindeutig zuordenbarer
    /// Eintrag wird **einzeln** dauerhaft quarantänisiert; die übrigen
    /// Einträge werden trotzdem angewendet.
    #[tokio::test]
    async fn p12_single_entry_quarantine_does_not_block_the_others() {
        let base = temp_dir("p12quar");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snaps = vec![
            snapshot("42", 5, 1.0),
            snapshot("43", 7, 2.0), // kein Charakterdatensatz in der DB
            snapshot("44", 9, 3.0),
        ];
        s.write_batch_run(snaps).unwrap();
        let db = FakeDb::multi(&[("42", Some(1)), ("43", None), ("44", Some(1))]);
        let out = s.drain_one_with(&db, "1").await.unwrap().expect("Batch verarbeitet");
        assert_eq!(out.entries_applied, 2, "42 und 44 werden angewendet");
        assert_eq!(out.batches_quarantined, 1, "nur 43 wird quarantänisiert");
        assert_eq!(out.entries_skipped, 0);
        let mut applied = db.applied();
        applied.sort();
        assert_eq!(applied, vec![("42".to_string(), 5), ("44".to_string(), 9)]);
        // Die Datei ist erst jetzt entfernt worden: alle drei Einträge waren erledigt.
        assert!(list_json_files(&s.spool_dir()).unwrap().is_empty());
        // Genau ein Quarantäneartefakt, kanonisch 43 zugeordnet.
        let q = list_json_files(&s.quarantine_open_dir()).unwrap();
        assert_eq!(q.len(), 1, "genau ein Quarantäneartefakt");
        let qname = q[0].file_name().unwrap().to_string_lossy().to_string();
        assert!(
            qname.contains("-43-r7.json--unknown-character.json"),
            "kanonisch 43 zuordenbar: {qname}"
        );
        let qcase = parse_quarantine_file_name(&qname).expect("kanonisch zuordenbar");
        assert_eq!(qcase.player_id, "43");
        assert_eq!(qcase.revision, 7);
        assert_eq!(qcase.reason, "unknown-character");
        assert!(list_json_files(&s.superseded_dir()).unwrap().is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12, Kernregel 6 in der Gegenrichtung: superseded wird **je Eintrag**
    /// abgelegt, ohne die übrigen Einträge zu beeinträchtigen.
    #[tokio::test]
    async fn p12_superseded_entry_is_archived_per_entry() {
        let base = temp_dir("p12sup");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snaps = vec![snapshot("42", 5, 1.0), snapshot("43", 7, 2.0)];
        s.write_batch_run(snaps).unwrap();
        // 42 ist in der DB bereits weiter -> superseded.
        let db = FakeDb::multi(&[("42", Some(99)), ("43", Some(1))]);
        let out = s.drain_one_with(&db, "1").await.unwrap().expect("Batch verarbeitet");
        assert_eq!(out.entries_superseded, 1);
        assert_eq!(out.entries_applied, 1, "43 wird unabhängig davon angewendet");
        let sup = list_json_files(&s.superseded_dir()).unwrap();
        assert_eq!(sup.len(), 1, "nur der superseded Eintrag");
        assert!(list_json_files(&s.quarantine_open_dir()).unwrap().is_empty());
        assert!(list_json_files(&s.spool_dir()).unwrap().is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12, Kernregel 7: ein bereits committeter Eintrag wird idempotent
    /// übersprungen — kein zweiter Apply.
    #[tokio::test]
    async fn p12_already_applied_entry_is_skipped_without_second_apply() {
        let base = temp_dir("p12skip");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snaps = vec![snapshot("42", 5, 1.0), snapshot("43", 7, 2.0)];
        s.write_batch_run(snaps).unwrap();
        // 42 exakt auf DB-Revision -> bereits committet.
        let db = FakeDb::multi(&[("42", Some(5)), ("43", Some(1))]);
        let out = s.drain_one_with(&db, "1").await.unwrap().expect("Batch verarbeitet");
        assert_eq!(out.entries_skipped, 1);
        assert_eq!(out.entries_applied, 1);
        assert_eq!(db.applied(), vec![("43".to_string(), 7)]);
        assert!(list_json_files(&s.spool_dir()).unwrap().is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Ein **nicht** eindeutig zuordenbarer Eintrag darf keinen
    /// Teilbatch erzeugen. Es wird keine Zuordnung erfunden: die gesamte Datei
    /// geht in die Quarantäne und die DB wird nicht angefasst.
    #[tokio::test]
    async fn p12_unattributable_entry_quarantines_whole_file_and_never_writes_db() {
        let base = temp_dir("p12unattr");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snaps = vec![snapshot("42", 5, 1.0), snapshot("nicht-kanonisch", 7, 2.0)];
        s.write_batch_run(snaps).unwrap();
        let path = s.next_batch_path().unwrap().unwrap();
        let db = FakeDb::multi(&[("42", Some(1))]);
        let out = s.drain_one_with(&db, "1").await.unwrap().expect("Batch verarbeitet");
        assert_eq!(out.batches_quarantined, 1);
        assert_eq!(out.entries_applied, 0, "kein Teilbatch angewendet");
        assert_eq!(db.loads(), 0, "kein DB-Lesevorgang");
        assert_eq!(db.applies(), 0, "kein DB-Write");
        assert!(!path.exists(), "Quelle wandert in die Quarantäne");
        assert_eq!(list_json_files(&s.quarantine_open_dir()).unwrap().len(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12, Kompatibilität: eine bereits veröffentlichte Einzeldatei im alten
    /// Format bleibt vollständig lesbar und wird normal gedraint.
    #[tokio::test]
    async fn p12_legacy_single_file_is_still_readable() {
        let base = temp_dir("p12legacy");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snap = snapshot("42", 5, 1.0);
        s.write_legacy_single(&snap).unwrap();
        let path = s.next_batch_path().unwrap().unwrap();
        assert!(
            path.file_name().unwrap().to_string_lossy().ends_with("-42-r5.json"),
            "Altformat-Dateiname bleibt erhalten"
        );
        // Fail-closed-Vorabprüfung sieht die Altdatei ebenfalls.
        assert_eq!(s.pending_revision("42"), Ok(Some(5)));
        let db = FakeDb::new(Some(1));
        let out = s.drain_one_with(&db, "1").await.unwrap().expect("Altdatei verarbeitet");
        assert_eq!(out.entries_applied, 1);
        assert_eq!(db.applied(), vec![("42".to_string(), 5)]);
        assert!(list_json_files(&s.spool_dir()).unwrap().is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// P-12: Die Fail-closed-Vorabprüfung (P-30) erkennt die neue gemeinsame
    /// Batch-Datei genauso wie das Altformat. Ohne diesen Nachweis wäre die
    /// P-30-Garantie beim Formatwechsel stillschweigend verloren gegangen.
    #[tokio::test]
    async fn p12_pending_revision_sees_shared_batches_for_p30() {
        let base = temp_dir("p12pend");
        let s = spool(&base);
        s.ensure_dirs().unwrap();
        let snaps = vec![snapshot("42", 5, 1.0), snapshot("43", 7, 2.0)];
        s.write_batch_run(snaps).unwrap();
        assert_eq!(s.pending_revision("42"), Ok(Some(5)));
        assert_eq!(s.pending_revision("43"), Ok(Some(7)));
        assert_eq!(s.pending_revision("99"), Ok(None), "fremder Spieler bleibt unberührt");
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
                in_flight: Arc::new(std::sync::Mutex::new(HashMap::new())),
            },
            weapon_skill_id: "ws".into(),
            status: Arc::new(Mutex::new(PersistStatus::Recovering)),
            recovery_open: Arc::new(AtomicBool::new(false)),
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
