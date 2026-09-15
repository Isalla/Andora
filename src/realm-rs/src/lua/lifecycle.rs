// lifecycle — Worker-Lebenszyklus, DedicatedContext und Pool-Konfiguration
// (docs/Lua-Scripting-System.md §9–§11).
//
// Stufe 3 liefert:
//   - DedicatedContext: eindeutige Repräsentation eines Dedicated Contexts
//     (§10: context_type + context_id; Raid-Instanzen über raid_instance_id)
//   - WorkerLifecycle: dokumentierte Zustandsmaschine (§11)
//   - PoolConfig: Konfiguration für Basis-/Reservierungs-/Maximalworker (§9)
//   - ContextMismatch: Fehler bei Context-Zuordnungsabweichung (§10)
//   - LifecycleError: ungültige Zustandsübergänge
//
// Keine Emergency-Worker, keine Scaling-Policy, kein Telemetrie-Integration.

use std::fmt;

/// Art eines Dedicated Contexts (docs §10, §13).
///
/// Raid-Instanzen werden über ihre raid_instance_id eindeutig repräsentiert.
/// Keine neue Gameplay-Kategorie wird erfunden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DedicatedContextKind {
    /// Raid-Instanz (§13): eindeutig über raid_instance_id.
    Raid,
}

impl fmt::Display for DedicatedContextKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Raid => write!(f, "raid"),
        }
    }
}

/// Eindeutige Repräsentation eines Dedicated Contexts (§10).
///
/// Jeder Dedicated Worker bedient während seiner Bindung genau EINEN
/// Dedicated Context (1:1). Der Context ist über `kind` und `id`
/// eindeutig identifizierbar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedicatedContext {
    /// Art des Contexts (z. B. Raid).
    pub kind: DedicatedContextKind,
    /// Eindeutige Context-ID innerhalb der Art.
    ///
    /// Für Raids entspricht dies der raid_instance_id.
    pub id: u64,
}

impl DedicatedContext {
    /// Neuer Dedicated Context.
    pub fn new(kind: DedicatedContextKind, id: u64) -> Self {
        Self { kind, id }
    }

    /// Raid-Instanz-Context.
    pub fn raid(raid_instance_id: u64) -> Self {
        Self::new(DedicatedContextKind::Raid, raid_instance_id)
    }

    /// Eindeutige Kurzbezeichnung für Log-/Fehlermeldungen.
    pub fn label(&self) -> String {
        format!("{}-{}", self.kind, self.id)
    }
}

impl fmt::Display for DedicatedContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.kind, self.id)
    }
}

/// Dokumentierter Worker-Lebenszyklus (§11).
///
/// ```text
/// IDLE --> RESERVED --> ACTIVE --> DRAINING --> [DESTROYING | RESETTING]
///   ^                                               |
///   +------- RESETTING -----------------------------+
/// ```
///
/// Zu FAILED: Der Zustand darf vorhanden sein, wird aber in Stufe 3
/// nicht neu getriggert (§14, Watchdog/Failover gehört zu Stufe 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkerLifecycle {
    /// Worker ist im Pool verfügbar, kein Context gebunden (§11).
    Idle,
    /// Worker ist einem Dedicated Context exklusiv zugeordnet;
    /// Context-Bindung besteht. Worker verarbeitet noch keine Events (§11).
    Reserved,
    /// Worker verarbeitet ausschließlich Arbeit seines gebundenen
    /// Dedicated Contexts (§11).
    Active,
    /// Abwicklungsphase: keine neuen Lua-Gameplay-Events mehr annehmen;
    /// bereits akzeptierte Arbeit abarbeiten (§11).
    Draining,
    /// VM-/Runtime-Zustand wird vollständig verworfen; frische VM (§11).
    Resetting,
    /// Temporärer Worker wird nach Context-Ende entfernt (§11).
    Destroying,
    /// Technischer Fehlerzustand (§11, §14).
    /// Wird in Stufe 4 durch Watchdog/Failover getriggert.
    Failed,
}

impl WorkerLifecycle {
    /// Versucht einen Zustandsübergang; gibt den neuen Zustand zurück
    /// oder einen Fehler bei ungültigem Übergang.
    pub fn try_transition(self, target: WorkerLifecycle) -> Result<Self, LifecycleError> {
        let valid = match (self, target) {
            (Self::Idle, Self::Reserved) => true,
            (Self::Reserved, Self::Active) => true,
            (Self::Active, Self::Draining) => true,
            (Self::Draining, Self::Resetting) => true,
            (Self::Draining, Self::Destroying) => true,
            (Self::Resetting, Self::Idle) => true,
            (Self::Failed, _) => false, // Kein Ausgang aus FAILED in Stufe 3
            _ => false,
        };
        if valid {
            Ok(target)
        } else {
            Err(LifecycleError {
                kind: LifecycleErrorKind::InvalidTransition {
                    from: self,
                    to: target,
                },
            })
        }
    }

    /// `true`, wenn der Worker gerade Arbeit verarbeitet (Active).
    pub fn is_processing(self) -> bool {
        self == Self::Active
    }

    /// `true`, wenn der Worker für neue Dedizierte-Arbeit gesperrt ist
    /// (DRAINING, RESETTING, DESTROYING, FAILED).
    pub fn is_drained_or_later(self) -> bool {
        matches!(
            self,
            Self::Draining | Self::Resetting | Self::Destroying | Self::Failed
        )
    }

    /// Humane Kurzbezeichnung für Log-/Fehlermeldungen.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Reserved => "RESERVED",
            Self::Active => "ACTIVE",
            Self::Draining => "DRAINING",
            Self::Resetting => "RESETTING",
            Self::Destroying => "DESTROYING",
            Self::Failed => "FAILED",
        }
    }
}

impl fmt::Display for WorkerLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Fehler bei einem ungültigen Lifecycle-Zustandsübergang.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleError {
    /// Art des Fehlers.
    pub kind: LifecycleErrorKind,
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            LifecycleErrorKind::InvalidTransition { from, to } => {
                write!(f, "Ungültiger Lifecycle-Übergang: {from} -> {to}")
            }
            LifecycleErrorKind::ContextMismatch {
                worker_id,
                expected,
                received,
            } => {
                write!(
                    f,
                    "Context-Mismatch: worker {worker_id} erwartet {expected}, \
                     empfangen {received}"
                )
            }
            LifecycleErrorKind::WorkerAlreadyBound {
                worker_id,
                existing,
            } => {
                write!(f, "Worker {worker_id} ist bereits an {existing} gebunden")
            }
            LifecycleErrorKind::CannotBindReserved => {
                write!(
                    f,
                    "Reservierte Normal-Worker können nicht für Dedicated Contexts gebunden werden"
                )
            }
            LifecycleErrorKind::CannotBindDedicatedAsNormal => {
                write!(
                    f,
                    "Dedicated Worker kann nicht als normaler Overflow-Worker verwendet werden"
                )
            }
            LifecycleErrorKind::NotActive => {
                write!(f, "Worker ist nicht im ACTIVE-Zustand")
            }
            LifecycleErrorKind::NotReady => {
                write!(f, "Worker ist nicht bereit für die Aktivierung (Vorbereitung nicht abgeschlossen)")
            }
        }
    }
}

impl std::error::Error for LifecycleError {}

/// Art eines Lifecycle-Fehlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleErrorKind {
    /// Ungültiger Zustandsübergang.
    InvalidTransition {
        /// Ursprünglicher Zustand.
        from: WorkerLifecycle,
        /// Gewünschter Zielzustand.
        to: WorkerLifecycle,
    },
    /// Context-Mismatch: Event gehört nicht zum gebundenen Context (§10).
    ContextMismatch {
        /// Betroffene Worker-ID.
        worker_id: u64,
        /// Erwarteter Context.
        expected: String,
        /// Empfangener Context.
        received: String,
    },
    /// Worker ist bereits an einen anderen Context gebunden (§10, 1:1).
    WorkerAlreadyBound {
        /// Betroffene Worker-ID.
        worker_id: u64,
        /// Bestehender Context.
        existing: String,
    },
    /// Reservierte Normal-Worker können nicht für Dedicated gebunden werden (§9).
    CannotBindReserved,
    /// Dedicated Worker kann nicht als normaler Overflow-Worker verwendet werden (§11).
    CannotBindDedicatedAsNormal,
    /// Worker ist nicht im ACTIVE-Zustand.
    NotActive,
    /// Vorbereitung für ACTIVE nicht abgeschlossen.
    NotReady,
}

/// Konfiguration des Worker-Pools (§9).
///
/// Verbindliche Beziehung:
/// ```text
/// normal_reserved <= base_count <= max_count
/// ```
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Basis-Worker, die dauerhaft vorgehalten werden (Default: 10).
    pub base_count: usize,
    /// davon für normale Welt geschützte Worker (Default: 4).
    pub normal_reserved: usize,
    /// Obergrenze inkl. temporärer Worker (Default: 15, kein hartes Limit).
    pub max_count: usize,
    /// Inbox-Kapazität pro Worker (kein erfundener Produktionswert).
    pub inbox_capacity: usize,
}

impl PoolConfig {
    /// Standardkonfiguration aus der Dokumentation (§9).
    pub fn default_config() -> Self {
        Self {
            base_count: 10,
            normal_reserved: 4,
            max_count: 15,
            inbox_capacity: 64,
        }
    }

    /// Anzahl der für Spezial-/Dedicated-Contexts verfügbaren Basis-Worker.
    pub fn base_dedicated_slots(&self) -> usize {
        self.base_count.saturating_sub(self.normal_reserved)
    }

    /// Maximal mögliche temporäre Worker.
    pub fn max_temporary_workers(&self) -> usize {
        self.max_count.saturating_sub(self.base_count)
    }

    /// Prüft die Invariante: normal_reserved <= base_count <= max_count.
    pub fn validate(&self) -> Result<(), PoolConfigError> {
        if self.normal_reserved > self.base_count {
            return Err(PoolConfigError::ReservedExceedsBase {
                reserved: self.normal_reserved,
                base: self.base_count,
            });
        }
        if self.base_count > self.max_count {
            return Err(PoolConfigError::BaseExceedsMax {
                base: self.base_count,
                max: self.max_count,
            });
        }
        if self.inbox_capacity == 0 {
            return Err(PoolConfigError::ZeroInboxCapacity);
        }
        Ok(())
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

/// Fehler bei ungültiger Pool-Konfiguration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolConfigError {
    /// normal_reserved > base_count.
    ReservedExceedsBase { reserved: usize, base: usize },
    /// base_count > max_count.
    BaseExceedsMax { base: usize, max: usize },
    /// Inbox-Kapazität ist 0.
    ZeroInboxCapacity,
}

impl fmt::Display for PoolConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReservedExceedsBase { reserved, base } => {
                write!(
                    f,
                    "normal_reserved ({reserved}) darf base_count ({base}) nicht überschreiten"
                )
            }
            Self::BaseExceedsMax { base, max } => {
                write!(
                    f,
                    "base_count ({base}) darf max_count ({max}) nicht überschreiten"
                )
            }
            Self::ZeroInboxCapacity => {
                write!(f, "Inbox-Kapazität muss größer als 0 sein")
            }
        }
    }
}

impl std::error::Error for PoolConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DedicatedContext ───────────────────────────────────────────

    #[test]
    fn dedicated_context_raid_eindeutig() {
        let c = DedicatedContext::raid(4711);
        assert_eq!(c.kind, DedicatedContextKind::Raid);
        assert_eq!(c.id, 4711);
        assert_eq!(c.label(), "raid-4711");
    }

    #[test]
    fn dedicated_context_display() {
        let c = DedicatedContext::raid(99);
        assert_eq!(format!("{c}"), "raid-99");
    }

    #[test]
    fn dedicated_context_eq() {
        let a = DedicatedContext::raid(1);
        let b = DedicatedContext::raid(1);
        let c = DedicatedContext::raid(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn dedicated_context_kind_display() {
        assert_eq!(format!("{}", DedicatedContextKind::Raid), "raid");
    }

    // ── WorkerLifecycle Übergänge ─────────────────────────────────

    #[test]
    fn idle_to_reserved() {
        assert_eq!(
            WorkerLifecycle::Idle.try_transition(WorkerLifecycle::Reserved),
            Ok(WorkerLifecycle::Reserved)
        );
    }

    #[test]
    fn reserved_to_active() {
        assert_eq!(
            WorkerLifecycle::Reserved.try_transition(WorkerLifecycle::Active),
            Ok(WorkerLifecycle::Active)
        );
    }

    #[test]
    fn active_to_draining() {
        assert_eq!(
            WorkerLifecycle::Active.try_transition(WorkerLifecycle::Draining),
            Ok(WorkerLifecycle::Draining)
        );
    }

    #[test]
    fn draining_to_resetting() {
        assert_eq!(
            WorkerLifecycle::Draining.try_transition(WorkerLifecycle::Resetting),
            Ok(WorkerLifecycle::Resetting)
        );
    }

    #[test]
    fn resetting_to_idle() {
        assert_eq!(
            WorkerLifecycle::Resetting.try_transition(WorkerLifecycle::Idle),
            Ok(WorkerLifecycle::Idle)
        );
    }

    #[test]
    fn draining_to_destroying() {
        assert_eq!(
            WorkerLifecycle::Draining.try_transition(WorkerLifecycle::Destroying),
            Ok(WorkerLifecycle::Destroying)
        );
    }

    #[test]
    fn invalid_idle_to_active() {
        let err = WorkerLifecycle::Idle
            .try_transition(WorkerLifecycle::Active)
            .unwrap_err();
        assert_eq!(
            err.kind,
            LifecycleErrorKind::InvalidTransition {
                from: WorkerLifecycle::Idle,
                to: WorkerLifecycle::Active,
            }
        );
    }

    #[test]
    fn invalid_reserved_to_draining() {
        let err = WorkerLifecycle::Reserved
            .try_transition(WorkerLifecycle::Draining)
            .unwrap_err();
        assert!(matches!(
            err.kind,
            LifecycleErrorKind::InvalidTransition { .. }
        ));
    }

    #[test]
    fn failed_has_no_outgoing_transitions() {
        for target in [
            WorkerLifecycle::Idle,
            WorkerLifecycle::Reserved,
            WorkerLifecycle::Active,
            WorkerLifecycle::Draining,
            WorkerLifecycle::Resetting,
            WorkerLifecycle::Destroying,
            WorkerLifecycle::Failed,
        ] {
            let result = WorkerLifecycle::Failed.try_transition(target);
            assert!(result.is_err(), "FAILED -> {target} sollte ungültig sein");
        }
    }

    #[test]
    fn lifecycle_label() {
        assert_eq!(WorkerLifecycle::Idle.label(), "IDLE");
        assert_eq!(WorkerLifecycle::Active.label(), "ACTIVE");
        assert_eq!(WorkerLifecycle::Draining.label(), "DRAINING");
    }

    #[test]
    fn lifecycle_is_processing() {
        assert!(!WorkerLifecycle::Idle.is_processing());
        assert!(WorkerLifecycle::Active.is_processing());
        assert!(!WorkerLifecycle::Draining.is_processing());
    }

    #[test]
    fn lifecycle_is_drained_or_later() {
        assert!(!WorkerLifecycle::Idle.is_drained_or_later());
        assert!(!WorkerLifecycle::Reserved.is_drained_or_later());
        assert!(!WorkerLifecycle::Active.is_drained_or_later());
        assert!(WorkerLifecycle::Draining.is_drained_or_later());
        assert!(WorkerLifecycle::Resetting.is_drained_or_later());
        assert!(WorkerLifecycle::Destroying.is_drained_or_later());
        assert!(WorkerLifecycle::Failed.is_drained_or_later());
    }

    // ── PoolConfig ────────────────────────────────────────────────

    #[test]
    fn pool_config_default_gültig() {
        let cfg = PoolConfig::default_config();
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.base_count, 10);
        assert_eq!(cfg.normal_reserved, 4);
        assert_eq!(cfg.max_count, 15);
    }

    #[test]
    fn pool_config_invariante() {
        let cfg = PoolConfig {
            base_count: 10,
            normal_reserved: 4,
            max_count: 15,
            inbox_capacity: 64,
        };
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.base_dedicated_slots(), 6);
        assert_eq!(cfg.max_temporary_workers(), 5);
    }

    #[test]
    fn pool_config_reserved_exceeds_base() {
        let cfg = PoolConfig {
            base_count: 4,
            normal_reserved: 5,
            max_count: 10,
            inbox_capacity: 64,
        };
        assert!(matches!(
            cfg.validate(),
            Err(PoolConfigError::ReservedExceedsBase { .. })
        ));
    }

    #[test]
    fn pool_config_base_exceeds_max() {
        let cfg = PoolConfig {
            base_count: 20,
            normal_reserved: 4,
            max_count: 15,
            inbox_capacity: 64,
        };
        assert!(matches!(
            cfg.validate(),
            Err(PoolConfigError::BaseExceedsMax { .. })
        ));
    }

    #[test]
    fn pool_config_zero_inbox() {
        let cfg = PoolConfig {
            base_count: 10,
            normal_reserved: 4,
            max_count: 15,
            inbox_capacity: 0,
        };
        assert!(matches!(
            cfg.validate(),
            Err(PoolConfigError::ZeroInboxCapacity)
        ));
    }

    // ── LifecycleError Display ────────────────────────────────────

    #[test]
    fn lifecycle_error_display() {
        let err = LifecycleError {
            kind: LifecycleErrorKind::ContextMismatch {
                worker_id: 42,
                expected: "raid-4711".into(),
                received: "raid-99".into(),
            },
        };
        let msg = format!("{err}");
        assert!(msg.contains("42"));
        assert!(msg.contains("raid-4711"));
        assert!(msg.contains("raid-99"));
    }
}
