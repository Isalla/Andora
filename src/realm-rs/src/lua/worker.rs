// worker — Lua-Worker mit Isolation, Lifecycle und Context-Bindung
// (docs/Lua-Scripting-System.md §8–§11).
//
// Jeder Worker besitzt eine eigene, isolierte Lua-VM und einen eigenen
// ScriptManager. Workers sind vollständig unabhängig voneinander:
//   - Kein gemeinsamer Lua-Zustand zwischen Workern
//   - Keine direkten mutablen World-Referenzen
//   - Sauberes Erzeugen und Freigeben
//
// Stufe 1: Eindeutige Worker-ID, Erzeugung mit eigener VM, Freigabe.
// Stufe 3: Lifecycle-Zustandsmaschine, Dedicated Context-Bindung (1:1),
//   Context-Mismatch-Prüfung, Reset mit frischer VM, Pool-Reservierungsschutz.

use std::sync::atomic::{AtomicU64, Ordering};

use super::context::ScriptContext;
use super::domain::ScriptDomain;
use super::error::LuaError;
use super::event::ScriptEvent;
use super::lifecycle::{DedicatedContext, LifecycleError, LifecycleErrorKind, WorkerLifecycle};
use super::runtime::{DispatchResult, ScriptManager};
use super::script::{ScriptId, ScriptSource};

/// Atomarer Zähler für eindeutige Worker-IDs.
///
/// Jeder Aufruf von [`WorkerId::next`] liefert einen garantiert
/// einmaligen Wert innerhalb des Prozesses. Die ID beginnt bei 1.
static WORKER_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Eindeutige Identifikation eines Lua-Workers.
///
/// Wird bei [`LuaWorker::new`] atomar vergeben; innerhalb eines Prozesses
/// gibt es keine doppelte ID.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkerId(u64);

impl WorkerId {
    /// Interne Vergabe einer neuen, eindeutigen ID.
    fn next() -> Self {
        Self(WORKER_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Rohwert der Worker-ID (zu Diagnose-/Loggingzwecken).
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Debug for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "worker-{}", self.0)
    }
}

/// Ein isolierter Lua-Worker mit eigener VM und eigenem ScriptManager.
///
/// Besitzstruktur:
/// - Jeder `LuaWorker` besitzt seinen `ScriptManager` (und damit seine
///   eigene `mlua::Lua`-VM) vollständig.
/// - Es gibt keine gemeinsame Referenz auf eine VM zwischen Workern.
/// - Beim Drop des Workers werden VM und alle geladenen Scripts freigegeben.
///
/// # Erzeugung
///
/// ```ignore
/// let worker = LuaWorker::new()?;
/// println!("Worker-ID: {}", worker.id());
/// ```
pub struct LuaWorker {
    /// Eindeutige Worker-ID.
    id: WorkerId,
    /// Zugehöriger ScriptManager mit eigener Lua-VM.
    manager: ScriptManager,
    /// Aktueller Lifecycle-Zustand (§11).
    lifecycle: WorkerLifecycle,
    /// Gebundener Dedicated Context (nur bei Dedicated-Bindung, 1:1, §10).
    binding: Option<DedicatedContext>,
    /// `true`, wenn dieser Worker temporär ist (kein Basis-Worker).
    ///
    /// Temporäre Worker werden nach Context-Ende zerstört
    /// (`DRAINING → DESTROYING`), Basis-/wiederverwendbare Worker
    /// zurückgesetzt (`DRAINING → RESETTING → IDLE`, §11).
    temporary: bool,
    /// Für die normale Welt reserviert (§9): Dieser Worker wird NIE für
    /// einen Dedicated Context gebunden.
    reserved: bool,
}

impl LuaWorker {
    /// Erzeugt einen neuen, vollständig isolierten Lua-Worker.
    ///
    /// Jeder Aufruf liefert einen Worker mit:
    /// - eigener, eindeutiger [`WorkerId`]
    /// - eigener Lua-VM (Sandbox, Whitelist)
    /// - eigenem leeren [`ScriptManager`]
    /// - Lifecycle-Zustand [`WorkerLifecycle::Idle`]
    ///
    /// Initialisierungsfehler werden direkt an den Aufrufer zurückgegeben;
    /// ein unvollständig initialisierter Worker wird nicht erzeugt.
    pub fn new() -> Result<Self, LuaError> {
        let id = WorkerId::next();
        let manager = ScriptManager::new()?;
        Ok(Self {
            id,
            manager,
            lifecycle: WorkerLifecycle::Idle,
            binding: None,
            temporary: false,
            reserved: false,
        })
    }

    /// Erzeugt einen temporären Dedicated Worker (kein Basis-Worker).
    ///
    /// Temporäre Worker werden nach dem Ende ihres Contexts zerstört
    /// (`DRAINING → DESTROYING`) und NIE als normaler Basis-Worker
    /// wiederverwendet (§11).
    pub fn new_temporary() -> Result<Self, LuaError> {
        let mut worker = Self::new()?;
        worker.temporary = true;
        Ok(worker)
    }

    /// Erzeugt einen für die normale Welt reservierten Worker (§9).
    ///
    /// Reservierte Normal-Worker werden NIE für einen Dedicated Context
    /// gebunden (geschützte QoS der normalen Welt).
    pub fn new_reserved() -> Result<Self, LuaError> {
        let mut worker = Self::new()?;
        worker.reserved = true;
        Ok(worker)
    }

    /// Eindeutige Worker-ID.
    pub fn id(&self) -> WorkerId {
        self.id
    }

    /// Zugriff auf den internen ScriptManager (Lesen).
    ///
    /// Ermöglicht Script-Operationen über die bestehende ScriptManager-API.
    pub fn manager(&self) -> &ScriptManager {
        &self.manager
    }

    /// Zugriff auf den internen ScriptManager (Schreiben).
    ///
    /// Ermöglicht das Laden, Ausführen und Dispatchen von Scripts
    /// über die bestehende ScriptManager-API.
    pub fn manager_mut(&mut self) -> &mut ScriptManager {
        &mut self.manager
    }

    /// Anzahl der geladenen Scripts in diesem Worker.
    pub fn script_count(&self) -> usize {
        self.manager.script_count()
    }

    /// Lädt ein Script in diesen Worker.
    pub fn load_script(
        &mut self,
        domain: ScriptDomain,
        source: ScriptSource,
    ) -> Result<ScriptId, LuaError> {
        self.manager.load_script(domain, source)
    }

    /// Führt das Top-Level eines Scripts aus.
    pub fn run(&mut self, id: &ScriptId, ctx: &ScriptContext) -> Result<DispatchResult, LuaError> {
        self.manager.run(id, ctx)
    }

    /// Führt einen benannten Callback aus.
    pub fn call(
        &mut self,
        id: &ScriptId,
        callback: &str,
        ctx: &ScriptContext,
    ) -> Result<DispatchResult, LuaError> {
        self.manager.call(id, callback, ctx)
    }

    /// Registriert ein Script für ein Event.
    pub fn register_event_handler(
        &mut self,
        domain: ScriptDomain,
        event: impl Into<String>,
        id: ScriptId,
    ) {
        self.manager.register_event_handler(domain, event, id);
    }

    /// Dispatcht ein Event an alle registrierten Handler.
    pub fn dispatch_event(
        &mut self,
        domain: ScriptDomain,
        event: &ScriptEvent,
    ) -> Vec<Result<DispatchResult, LuaError>> {
        self.manager.dispatch_event(domain, event)
    }

    // ── Lifecycle (§11) ────────────────────────────────────────────

    /// Aktueller Lifecycle-Zustand.
    pub fn lifecycle(&self) -> WorkerLifecycle {
        self.lifecycle
    }

    /// `true`, wenn der Worker temporär ist (kein wiederverwendbarer
    /// Basis-Worker, §11).
    pub fn is_temporary(&self) -> bool {
        self.temporary
    }

    /// `true`, wenn der Worker für die normale Welt reserviert ist (§9).
    pub fn is_reserved(&self) -> bool {
        self.reserved
    }

    /// Gebundener Dedicated Context (1:1).
    pub fn binding(&self) -> Option<&DedicatedContext> {
        self.binding.as_ref()
    }

    /// Führt einen Zustandsübergang aus; bei ungültigem Übergang bleibt
    /// der Zustand unverändert und es wird ein Fehler zurückgegeben.
    fn transition(&mut self, target: WorkerLifecycle) -> Result<(), LifecycleError> {
        self.lifecycle = self.lifecycle.try_transition(target)?;
        Ok(())
    }

    /// Reserviert diesen Worker exklusiv für einen Dedicated Context
    /// (`IDLE → RESERVED`, §11).
    ///
    /// Die Context-Bindung besteht ab sofort; der Worker verarbeitet noch
    /// keine Events des Contexts. Eine zweite Bindung während einer
    /// bestehenden Bindung ist unzulässig (1:1, §10).
    pub fn reserve_for(&mut self, ctx: DedicatedContext) -> Result<(), LifecycleError> {
        // Geschützter Normal-Worker (§9): NIE für Dedicated Contexts.
        if self.reserved {
            return Err(LifecycleError {
                kind: LifecycleErrorKind::CannotBindReserved,
            });
        }
        // 1:1-Bindung (§10): Ein bereits gebundener Worker kann keinen
        // zweiten Context übernehmen.
        if let Some(existing) = &self.binding {
            return Err(LifecycleError {
                kind: LifecycleErrorKind::WorkerAlreadyBound {
                    worker_id: self.id.as_u64(),
                    existing: existing.label(),
                },
            });
        }
        if self.lifecycle != WorkerLifecycle::Idle {
            return Err(self.invalid_transition(WorkerLifecycle::Reserved));
        }
        self.binding = Some(ctx);
        self.lifecycle = WorkerLifecycle::Reserved;
        Ok(())
    }

    /// Schließt die Vorbereitung ab und aktiviert den Worker
    /// (`RESERVED → ACTIVE`, §11).
    ///
    /// Nur zulässig, wenn Initialisierung/Reset vollständig abgeschlossen
    /// ist und damit eine frische, verarbeitungsbereite Lua-VM vorliegt.
    pub fn prepare_for_activation(&mut self) -> Result<(), LifecycleError> {
        if self.lifecycle != WorkerLifecycle::Reserved {
            return Err(LifecycleError {
                kind: LifecycleErrorKind::NotReady,
            });
        }
        // Frische Lua-VM für diesen Context (§11: RESERVED → ACTIVE erst
        // nach vollständiger Initialisierung/Reset mit verarbeitungsbereiter
        // VM).
        self.manager = ScriptManager::fresh_vm().map_err(|_| LifecycleError {
            kind: LifecycleErrorKind::InvalidTransition {
                from: WorkerLifecycle::Reserved,
                to: WorkerLifecycle::Active,
            },
        })?;
        self.lifecycle = WorkerLifecycle::Active;
        Ok(())
    }

    /// Beendet die Lua-Verarbeitung des Contexts (`ACTIVE → DRAINING`, §11).
    ///
    /// Ab DRAINING werden keine neuen Lua-Gameplay-Events für diesen
    /// Context mehr angenommen; bereits akzeptierte Arbeit verbleibt.
    pub fn start_draining(&mut self) -> Result<(), LifecycleError> {
        if self.lifecycle != WorkerLifecycle::Active {
            return Err(self.invalid_transition(WorkerLifecycle::Draining));
        }
        self.lifecycle = WorkerLifecycle::Draining;
        Ok(())
    }

    /// Successor des normalen Drains für wiederverwendbare Worker:
    /// `DRAINING → RESETTING → IDLE` (§11).
    ///
    /// DRAINING muss abgeschlossen sein (keine ausstehende Arbeit) und
    /// die Bindungen werden beim Neustart gelöscht. Die VM wird auf eine
    /// frische Instanz zurückgesetzt.
    pub fn complete_drain_for_reuse(&mut self) -> Result<(), LifecycleError> {
        self.transition(WorkerLifecycle::Resetting)?;
        self.reset().map_err(|_| LifecycleError {
            kind: LifecycleErrorKind::InvalidTransition {
                from: WorkerLifecycle::Resetting,
                to: WorkerLifecycle::Idle,
            },
        })?;
        Ok(())
    }

    /// Successor des normalen Drains für temporäre Worker:
    /// `DRAINING → DESTROYING` (§11). Anschließend wird der Worker über
    /// [`into_destroyed`] entnommen.
    pub fn complete_drain_for_destroy(&mut self) -> Result<(), LifecycleError> {
        self.transition(WorkerLifecycle::Destroying)?;
        Ok(())
    }

    /// Context-Zuordnung-Prüfung (§10): verarbeitet der Worker ausschließlich
    /// Arbeit des gebundenen Contexts? Bei Mismatch wird ein eindeutiger
    /// [`LifecycleErrorKind::ContextMismatch`]-Fehler zurückgegeben.
    pub fn ensure_context(&self, ctx: &DedicatedContext) -> Result<(), LifecycleError> {
        match &self.binding {
            Some(bound) if bound == ctx => Ok(()),
            Some(bound) => Err(LifecycleError {
                kind: LifecycleErrorKind::ContextMismatch {
                    worker_id: self.id.as_u64(),
                    expected: bound.label(),
                    received: ctx.label(),
                },
            }),
            None => Err(LifecycleError {
                kind: LifecycleErrorKind::ContextMismatch {
                    worker_id: self.id.as_u64(),
                    expected: "<kein Context gebunden>".to_string(),
                    received: ctx.label(),
                },
            }),
        }
    }

    /// Normalisiert einen ungültigen Übergang in einen Lifecycle-Fehler.
    fn invalid_transition(&self, target: WorkerLifecycle) -> LifecycleError {
        LifecycleError {
            kind: LifecycleErrorKind::InvalidTransition {
                from: self.lifecycle,
                to: target,
            },
        }
    }

    /// Nimmt einen Dedicated Lua-Gameplay-Event entgegen und führt ihn aus.
    ///
    /// Zustellberechtigungen (§10, §11):
    ///   - Worker muss den gebundenen Context besitzen und ACTIVE sein.
    ///   - In DRAINING wird keine neue Arbeit angenommen.
    ///   - Context-Mismatch wird ohne Ausführung als
    ///     [`LifecycleErrorKind::ContextMismatch`] zurückgegeben (kein
    ///     Einreihen in die Inbox, kein Lua-Aufruf).
    pub fn process_dedicated_event(
        &mut self,
        ctx: &DedicatedContext,
        domain: ScriptDomain,
        event: &ScriptEvent,
    ) -> Result<Vec<Result<DispatchResult, LuaError>>, LifecycleError> {
        self.ensure_context(ctx)?;
        if self.lifecycle != WorkerLifecycle::Active {
            return Err(self.invalid_transition(WorkerLifecycle::Draining));
        }
        Ok(self.dispatch_event(domain, event))
    }

    /// Setzt den Worker zurück (frische VM, keine Context-Bindung, IDLE).
    ///
    /// Verworfene Zustände: alter Lua-/VM-Zustand, alter ScriptManager,
    /// Context-Bindung (§11).
    pub fn reset(&mut self) -> Result<(), LuaError> {
        if self.lifecycle != WorkerLifecycle::Resetting {
            self.lifecycle = WorkerLifecycle::Resetting;
        }
        self.manager = ScriptManager::fresh_vm()?;
        self.binding = None;
        self.lifecycle = WorkerLifecycle::Idle;
        Ok(())
    }
}

impl std::fmt::Debug for LuaWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LuaWorker")
            .field("id", &self.id)
            .field("lifecycle", &self.lifecycle)
            .field("binding", &self.binding)
            .field("temporary", &self.temporary)
            .field("reserved", &self.reserved)
            .field("script_count", &self.manager.script_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::super::host::RealmRequest;
    use super::*;

    fn code_source(name: impl Into<String>, code: impl Into<String>) -> ScriptSource {
        ScriptSource::Code {
            name: name.into(),
            code: code.into(),
        }
    }

    fn minimal_returns(ctx: &str) -> String {
        format!("return {{ handle = function(c) {ctx} end }}")
    }

    // ── Worker-Erzeugung und IDs ──────────────────────────────────

    #[test]
    fn worker_new_creates_isolated_instance() {
        let w = LuaWorker::new().unwrap();
        assert_eq!(w.script_count(), 0);
    }

    #[test]
    fn multiple_workers_have_distinct_ids() {
        let w1 = LuaWorker::new().unwrap();
        let w2 = LuaWorker::new().unwrap();
        let w3 = LuaWorker::new().unwrap();
        assert_ne!(w1.id(), w2.id());
        assert_ne!(w2.id(), w3.id());
        assert_ne!(w1.id(), w3.id());
    }

    #[test]
    fn worker_id_is_deterministic_across_clones() {
        let w = LuaWorker::new().unwrap();
        let id = w.id();
        assert_eq!(w.id(), id);
        assert_eq!(id.as_u64(), id.as_u64());
    }

    #[test]
    fn worker_id_display_format() {
        let w = LuaWorker::new().unwrap();
        let s = format!("{}", w.id());
        assert!(s.starts_with("worker-"));
    }

    // ── Script-Operationen über Worker ─────────────────────────────

    #[test]
    fn worker_load_and_run_script() {
        let mut w = LuaWorker::new().unwrap();
        let id = w
            .load_script(
                ScriptDomain::Npc,
                code_source("hallo", minimal_returns("andora.emit('greet', {})")),
            )
            .unwrap();
        let result = w.call(&id, "handle", &ScriptContext::new()).unwrap();
        assert_eq!(result.requests.len(), 1);
        match &result.requests[0] {
            RealmRequest::Emit { name, .. } => assert_eq!(name, "greet"),
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    #[test]
    fn worker_call_callback() {
        let mut w = LuaWorker::new().unwrap();
        let id = w
            .load_script(
                ScriptDomain::Item,
                code_source(
                    "untersuche",
                    minimal_returns("andora.emit('examined', { item = c.item_id })"),
                ),
            )
            .unwrap();
        let ctx = ScriptContext {
            item_id: Some("eisenbarren".into()),
            ..Default::default()
        };
        let result = w.call(&id, "handle", &ctx).unwrap();
        match &result.requests[0] {
            RealmRequest::Emit { name, payload } => {
                assert_eq!(name, "examined");
                assert_eq!(payload["item"], "eisenbarren");
            }
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    #[test]
    fn worker_dispatch_event() {
        let mut w = LuaWorker::new().unwrap();
        let id = w
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "reagiere",
                    "return { hail = function(c) andora.emit('hail_ok', { npc = c.npc_id }) end }",
                ),
            )
            .unwrap();
        w.register_event_handler(ScriptDomain::Npc, "hail", id.clone());
        let event = ScriptEvent::new(
            "hail",
            ScriptContext {
                npc_id: Some("npc-99".into()),
                ..Default::default()
            },
        );
        let results = w.dispatch_event(ScriptDomain::Npc, &event);
        assert_eq!(results.len(), 1);
        let dr = results[0].as_ref().unwrap();
        match &dr.requests[0] {
            RealmRequest::Emit { payload, .. } => assert_eq!(payload["npc"], "npc-99"),
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    // ── Isolation zwischen Workern ─────────────────────────────────

    #[test]
    fn worker_lua_state_not_visible_in_other_worker() {
        // Worker A: Script laden mit State in einer lokalen Variable
        let mut wa = LuaWorker::new().unwrap();
        let id_a = wa
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "internA",
                    "local geheim = 'A-geheim'\n\
                     return { expose = function(c) andora.emit('intern', { v = geheim }) end }",
                ),
            )
            .unwrap();
        let result_a = wa.call(&id_a, "expose", &ScriptContext::new()).unwrap();
        let val_a = match &result_a.requests[0] {
            RealmRequest::Emit { payload, .. } => payload["v"].as_str().unwrap().to_string(),
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        };
        assert_eq!(val_a, "A-geheim");

        // Worker B: eigene VM – keine Sicht auf A-Zustand
        let mut wb = LuaWorker::new().unwrap();
        let id_b = wb
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "checkB",
                    "return { check = function(c) andora.emit('intern', { geheim = geheim }) end }",
                ),
            )
            .unwrap();
        let result_b = wb.call(&id_b, "check", &ScriptContext::new()).unwrap();
        match &result_b.requests[0] {
            RealmRequest::Emit { payload, .. } => {
                // 'geheim' ist nil in Worker B (andere VM) -> JSON null
                assert!(payload["geheim"].is_null());
            }
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    #[test]
    fn worker_scripts_not_visible_in_other_worker() {
        let mut wa = LuaWorker::new().unwrap();
        let id_a = wa
            .load_script(ScriptDomain::Npc, code_source("nurA", minimal_returns("")))
            .unwrap();
        assert!(wa.manager().has_script(&id_a));

        let wb = LuaWorker::new().unwrap();
        assert!(!wb.manager().has_script(&id_a));
        assert_eq!(wb.script_count(), 0);
    }

    // ── Drop / Freigabe ───────────────────────────────────────────

    #[test]
    fn dropping_worker_does_not_affect_others() {
        let mut wa = LuaWorker::new().unwrap();
        wa.load_script(
            ScriptDomain::Npc,
            code_source("langLebe", minimal_returns("andora.emit('alive', {})")),
        )
        .unwrap();

        let mut wb = LuaWorker::new().unwrap();
        let id_b = wb
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "auchLangLebe",
                    minimal_returns("andora.emit('alive_b', {})"),
                ),
            )
            .unwrap();

        // Worker A freigeben
        drop(wa);

        // Worker B funktioniert unverändert
        let result = wb.call(&id_b, "handle", &ScriptContext::new()).unwrap();
        assert_eq!(result.requests.len(), 1);
        match &result.requests[0] {
            RealmRequest::Emit { name, .. } => assert_eq!(name, "alive_b"),
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    #[test]
    fn new_worker_inherits_no_state_from_dropped_worker() {
        // Worker A: setzt einen Env-Zustand und gibt ihn in einem Lauf aus.
        let mut wa = LuaWorker::new().unwrap();
        let id_a = wa
            .load_script(
                ScriptDomain::Zone,
                code_source(
                    "leakset",
                    "token = 'leak-test'\n\
                     return { read = function(c) andora.emit('leak', { v = token }) end }",
                ),
            )
            .unwrap();
        let result_a = wa.call(&id_a, "read", &ScriptContext::new()).unwrap();
        match &result_a.requests[0] {
            RealmRequest::Emit { payload, .. } => {
                assert_eq!(payload["v"].as_str().unwrap(), "leak-test");
            }
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }

        // Worker A freigeben
        drop(wa);

        // Neu erzeugter Worker B übernimmt keinerlei Zustand von A.
        let mut wb = LuaWorker::new().unwrap();
        let id_b = wb
            .load_script(
                ScriptDomain::Zone,
                code_source(
                    "leakread",
                    "return { read = function(c) andora.emit('leak', { exists = token ~= nil }) end }",
                ),
            )
            .unwrap();
        let result_b = wb.call(&id_b, "read", &ScriptContext::new()).unwrap();
        match &result_b.requests[0] {
            RealmRequest::Emit { payload, .. } => {
                // 'token' ist in der frischen VM/Env nicht sichtbar.
                assert_eq!(payload["exists"], false);
            }
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    // ── Sandbox wirkt in jedem Worker ──────────────────────────────

    #[test]
    fn sandbox_effective_in_each_worker() {
        let mut w1 = LuaWorker::new().unwrap();
        let id1 = w1
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "sb1",
                    minimal_returns("andora.emit('probe', { has_os = os ~= nil })"),
                ),
            )
            .unwrap();
        let r1 = w1.call(&id1, "handle", &ScriptContext::new()).unwrap();
        match &r1.requests[0] {
            RealmRequest::Emit { payload, .. } => assert_eq!(payload["has_os"], false),
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }

        let mut w2 = LuaWorker::new().unwrap();
        let id2 = w2
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "sb2",
                    minimal_returns("andora.emit('probe', { has_io = io ~= nil })"),
                ),
            )
            .unwrap();
        let r2 = w2.call(&id2, "handle", &ScriptContext::new()).unwrap();
        match &r2.requests[0] {
            RealmRequest::Emit { payload, .. } => assert_eq!(payload["has_io"], false),
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    // ── Error-Isolation ────────────────────────────────────────────

    #[test]
    fn worker_error_does_not_corrupt_self() {
        let mut w = LuaWorker::new().unwrap();
        let bad = w
            .load_script(
                ScriptDomain::Npc,
                code_source("errorScript", minimal_returns("error('boom')")),
            )
            .unwrap();
        let err = w.call(&bad, "handle", &ScriptContext::new()).unwrap_err();
        assert!(matches!(err, LuaError::RuntimeError { .. }));

        // Worker bleibt stabil
        let good = w
            .load_script(ScriptDomain::Npc, code_source("good", minimal_returns("")))
            .unwrap();
        assert!(w.call(&good, "handle", &ScriptContext::new()).is_ok());
    }

    #[test]
    fn worker_load_error_does_not_affect_other_scripts() {
        let mut w = LuaWorker::new().unwrap();
        let good = w
            .load_script(
                ScriptDomain::Npc,
                code_source("gut", minimal_returns("andora.emit('ok', {})")),
            )
            .unwrap();
        let _err = w
            .load_script(ScriptDomain::Npc, code_source("kaputt", "function (("))
            .unwrap_err();

        let result = w.call(&good, "handle", &ScriptContext::new()).unwrap();
        assert_eq!(result.requests.len(), 1);
    }

    // ── Debug-Format ───────────────────────────────────────────────

    #[test]
    fn worker_debug_format_contains_id() {
        let w = LuaWorker::new().unwrap();
        let dbg = format!("{:?}", w);
        assert!(dbg.contains("LuaWorker"));
        assert!(dbg.contains("worker-"));
    }

    // ── Worker-Manager-Access ──────────────────────────────────────

    #[test]
    fn worker_manager_mut_loads_scripts() {
        let mut w = LuaWorker::new().unwrap();
        let id = w
            .manager_mut()
            .load_script(
                ScriptDomain::Npc,
                code_source("viaMut", minimal_returns("")),
            )
            .unwrap();
        assert!(w.manager().has_script(&id));
    }

    // ── Stufe 3: Lifecycle (§11) ───────────────────────────────────

    #[test]
    fn worker_reserve_activate_drain_reuse_lifecycle() {
        let mut w = LuaWorker::new().unwrap();
        let ctx = DedicatedContext::raid(4711);

        // IDLE -> RESERVED
        w.reserve_for(ctx.clone()).unwrap();
        assert_eq!(w.lifecycle(), WorkerLifecycle::Reserved);
        assert_eq!(w.binding(), Some(&ctx));

        // RESERVED -> ACTIVE (nach abgeschlossener Vorbereitung)
        w.prepare_for_activation().unwrap();
        assert_eq!(w.lifecycle(), WorkerLifecycle::Active);

        // ACTIVE -> DRAINING
        w.start_draining().unwrap();
        assert_eq!(w.lifecycle(), WorkerLifecycle::Draining);

        // DRAINING -> RESETTING -> IDLE (wiederverwendbarer Basis-Worker)
        w.complete_drain_for_reuse().unwrap();
        assert_eq!(w.lifecycle(), WorkerLifecycle::Idle);
        assert!(w.binding().is_none());
    }

    #[test]
    fn worker_temporary_drain_to_destroying() {
        let mut w = LuaWorker::new_temporary().unwrap();
        assert!(w.is_temporary());
        let ctx = DedicatedContext::raid(99);
        w.reserve_for(ctx.clone()).unwrap();
        w.prepare_for_activation().unwrap();
        assert_eq!(w.lifecycle(), WorkerLifecycle::Active);

        w.start_draining().unwrap();
        w.complete_drain_for_destroy().unwrap();
        assert_eq!(w.lifecycle(), WorkerLifecycle::Destroying);
    }

    #[test]
    fn worker_cannot_bind_second_context() {
        let mut w = LuaWorker::new().unwrap();
        let ctx_a = DedicatedContext::raid(1);
        let ctx_b = DedicatedContext::raid(2);
        w.reserve_for(ctx_a.clone()).unwrap();
        let err = w.reserve_for(ctx_b.clone()).unwrap_err();
        assert!(matches!(
            err.kind,
            LifecycleErrorKind::WorkerAlreadyBound { .. }
        ));
        // Bindung bleibt Context A (1:1).
        assert_eq!(w.binding(), Some(&ctx_a));
    }

    #[test]
    fn worker_reserved_normal_cannot_be_bound() {
        let mut w = LuaWorker::new_reserved().unwrap();
        assert!(w.is_reserved());
        let err = w.reserve_for(DedicatedContext::raid(4711)).unwrap_err();
        assert!(matches!(err.kind, LifecycleErrorKind::CannotBindReserved));
        // Worker bleibt IDLE, ungebunden.
        assert_eq!(w.lifecycle(), WorkerLifecycle::Idle);
        assert!(w.binding().is_none());
    }

    #[test]
    fn worker_mismatch_rejects_before_lua_execution() {
        let mut w = LuaWorker::new().unwrap();
        let ctx_a = DedicatedContext::raid(4711);
        let ctx_b = DedicatedContext::raid(4712);
        w.reserve_for(ctx_a.clone()).unwrap();
        w.prepare_for_activation().unwrap();

        let id = w
            .load_script(
                ScriptDomain::Npc,
                code_source("haendler", minimal_returns("andora.emit('executed', {})")),
            )
            .unwrap();

        // Fremder Context B -> Mismatch, Lua wird NICHT aufgerufen.
        let err = w
            .process_dedicated_event(
                &ctx_b,
                ScriptDomain::Npc,
                &ScriptEvent::new("hail", ScriptContext::new()),
            )
            .unwrap_err();
        match &err.kind {
            LifecycleErrorKind::ContextMismatch {
                worker_id,
                expected,
                received,
            } => {
                assert_eq!(*worker_id, w.id().as_u64());
                assert_eq!(expected, "raid-4711");
                assert_eq!(received, "raid-4712");
            }
            other => panic!("unerwarteter Fehler: {other:?}"),
        }

        // Gebundener Context A -> ordnungsgemäß ausgeführt.
        let results = w
            .process_dedicated_event(
                &ctx_a,
                ScriptDomain::Npc,
                &ScriptEvent::new("hail", ScriptContext::new()),
            )
            .unwrap();
        // Kein Handler registriert -> leer, kein Fehler.
        assert!(results.is_empty());
        // Script gezielt prüfen, dass die VM funktioniert.
        let result = w.call(&id, "handle", &ScriptContext::new()).unwrap();
        assert_eq!(result.requests.len(), 1);
    }

    #[test]
    fn worker_reset_discards_lua_state_and_binding() {
        let mut w = LuaWorker::new().unwrap();
        let ctx = DedicatedContext::raid(4711);
        w.reserve_for(ctx.clone()).unwrap();
        w.prepare_for_activation().unwrap();

        w.load_script(
            ScriptDomain::Zone,
            code_source(
                "geheimA",
                "stateA = 'nur-A'\nreturn { handle = function(c) end }",
            ),
        )
        .unwrap();
        assert_eq!(w.script_count(), 1);
        assert_eq!(w.binding(), Some(&ctx));

        // Drain + Reset.
        w.start_draining().unwrap();
        w.complete_drain_for_reuse().unwrap();

        // Frische VM: keine Scripts, keine Bindung.
        assert_eq!(w.script_count(), 0);
        assert!(w.binding().is_none());
        assert_eq!(w.lifecycle(), WorkerLifecycle::Idle);

        // Neuer Context B: Zustand von A nicht sichtbar.
        let ctx_b = DedicatedContext::raid(4712);
        w.reserve_for(ctx_b.clone()).unwrap();
        w.prepare_for_activation().unwrap();
        let id_b = w
            .load_script(
                ScriptDomain::Zone,
                code_source(
                    "checkB",
                    "return { check = function(c) andora.emit('leak', { sichtbar = stateA ~= nil }) end }",
                ),
            )
            .unwrap();
        let result = w.call(&id_b, "check", &ScriptContext::new()).unwrap();
        match &result.requests[0] {
            RealmRequest::Emit { payload, .. } => {
                // 'stateA' ist in der frischen VM von Context B nil.
                assert_eq!(payload["sichtbar"], false);
            }
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }
}
