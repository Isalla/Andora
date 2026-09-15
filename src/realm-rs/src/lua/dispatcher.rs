// dispatcher — Lua-Dispatcher/Scheduler (Stufe 2 + Stufe 3)
// (docs/Lua-Scripting-System.md §8, §9, §10, §11, §12).
//
// Der Dispatcher nimmt Arbeit aus der (bounded) Event-Queue entgegen und
// verteilt sie auf Worker des Pools:
//
// Normale World-Arbeit (§12):
//   - Jeder normale Worker besitzt hier eine eigene Inbox (bounded), in
//     die der Dispatcher Arbeit einstellt. Lua wird vom Dispatcher NICHT
//     ausgeführt; die Ausführung bleibt Sache des Workers (Stufe 1).
//   - Auswahlstrategie: einfaches, deterministisches Round-Robin ab einem
//     Positionszeiger (kein komplexes Load-Balancing).
//   - Überlauf (§12): Ist die Inbox des zunächst gewählten normalen Workers
//     voll, wird Arbeit auf einen anderen zulässigen, verfügbaren normalen
//     Worker verteilt. Stehen keine verfügbaren Worker bereit, gilt die
//     QoS-Politik: Droppable wird verworfen; Reliable/Coalescable wird
//     NICHT verworfen, sondern zurückgegeben (NoWorkerAvailable).
//   - Dedicated Worker (gebundener Worker) nehmen NIE normale World-Arbeit
//     an (§11) — sie sind von der normalen Verteilung ausgeschlossen.
//
// Dedicated Arbeit (§10, §11):
//   - Zustellung ausschließlich an den Worker, der an genau diesen Context
//     gebunden ist (1:1). Keine Round-Robin-Verteilung eines Dedicated
//     Contexts auf andere Worker.
//   - Die Context-Zuordnung wird VOR der Inbox-Zustellung geprüft
//     (Context-Mismatch-Regel, §10): Abweichender Context wird nicht
//     eingereiht, nicht ausgeführt und als eindeutiger ContextMismatch
//     abgelehnt.
//   - Nur ein ACTIVE-Worker nimmt Dedicated-Arbeit an (RESERVED verarbeitet
//     noch nichts; DRAINING nimmt keine neuen Lua-Gameplay-Events an).
//
// Reservierter Normalschutz (§9):
//   - n Reservierte Workers stehen ausschließlich der normalen Welt zur
//     Verfügung und werden NIE für Dedicated Contexts gebunden.
//
// Offen (bewusst NICHT gelöst, §12): Verhalten bei voller Inbox eines
// Dedicated Workers. Es wird nur eine neutrale technische Rückmeldung
// (InboxFull) zurückgegeben. Es werden keine Reliable Events verworfen,
// kein Realm blockiert, kein zweiter Worker zugewiesen, kein Emergency
// Worker gestartet, kein Reset und kein Raid-Abbruch vorgenommen.

use super::error::LuaError;
use super::lifecycle::{
    DedicatedContext, LifecycleError, LifecycleErrorKind, PoolConfig, WorkerLifecycle,
};
use super::queue::{BoundedQueue, LuaWork, QueueClass, QueueFull};
use super::worker::{LuaWorker, WorkerId};

/// Ergebnis einer Zustellung durch den Dispatcher (normale Welt).
#[derive(Debug, Clone)]
pub enum DispatchOutcome {
    /// Arbeit wurde in die Inbox des angegebenen normalen Workers eingestellt.
    Accepted { worker: WorkerId },
    /// Droppable-Arbeit konnte keinem verfügbaren Worker zugestellt werden
    /// und wurde bewusst verworfen (erlaubt, docs §12).
    Dropped,
    /// Reliable/Coalescable-Arbeit konnte keinem verfügbaren normalen Worker
    /// zugestellt werden und wurde NICHT verworfen: Rückgabe an den Aufrufer
    /// (geregelte Weiterbehandlung; der Realm wartet nicht synchron).
    NoWorkerAvailable { work: LuaWork },
}

/// Ergebnis einer Dedicated-Zustellung durch den Dispatcher (§10, §11).
#[derive(Debug, Clone)]
pub enum DedicatedDispatchResult {
    /// Arbeit wurde in die Inbox des gebundenen Workers eingestellt.
    Accepted { worker: WorkerId },
    /// Zustellung wurde abgelehnt.
    Rejected(DedicatedDispatchError),
}

/// Gründe, aus denen eine Dedicated-Zustellung abgelehnt wird.
#[derive(Debug, Clone)]
pub enum DedicatedDispatchError {
    /// Arbeit besitzt keine Dedicated-Context-Zuordnung (normale Arbeit
    /// darf nicht an einen Dedicated Worker gehen, §11).
    MissingContext { worker: Option<WorkerId> },
    /// Kein Worker ist an diesen Dedicated Context gebunden (§10).
    NoWorkerBound { ctx: DedicatedContext },
    /// Context-Zuordnung der Arbeit passt nicht zum gebundenen Worker:
    /// Geprüft VOR der Inbox-Zustellung; die Arbeit wird nicht eingereiht
    /// und nicht ausgeführt (§10). Enthält worker_id, erwarteten und
    /// empfangenen Context (minimales Rust-Logging).
    ContextMismatch(LifecycleError),
    /// Gebundener Worker ist nicht im ACTIVE-Zustand (z. B. RESERVED
    /// während der Vorbereitung, DRAINING nach Ende der Verarbeitung).
    WorkerNotActive {
        ctx: DedicatedContext,
        worker: WorkerId,
        lifecycle: WorkerLifecycle,
    },
    /// Gebundener Worker-Inbox ist voll. Bewusst offener und nicht
    /// entschiedener Fall (§12): Nur neutrale technische Rückmeldung,
    /// KEINE erfundene Policy. Die Arbeit wird NICHT verworfen, sondern
    /// dem Aufrufer zurückgegeben.
    InboxFull {
        ctx: DedicatedContext,
        worker: WorkerId,
        work: LuaWork,
    },
}

impl DedicatedDispatchResult {
    /// `true`, wenn die Arbeit der Inbox zugestellt wurde.
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted { .. })
    }

    /// `true`, wenn die Zustellung als Context-Mismatch abgelehnt wurde.
    pub fn is_context_mismatch(&self) -> bool {
        matches!(
            self,
            Self::Rejected(DedicatedDispatchError::ContextMismatch(_))
        )
    }
}

/// Ein Pool-Slot: Worker samt eigener bounded Inbox.
///
/// Nur der Dispatcher stellt Arbeit in die Inbox ein; der Worker behält
/// seine Stufe-1-API unverändert (eigene VM, eigener ScriptManager).
#[derive(Debug)]
pub struct WorkerSlot {
    worker: LuaWorker,
    inbox: BoundedQueue<LuaWork>,
}

impl WorkerSlot {
    fn new(worker: LuaWorker, inbox_capacity: usize) -> Self {
        Self {
            worker,
            inbox: BoundedQueue::with_capacity(inbox_capacity),
        }
    }

    /// Zugriff auf den Worker (Lesen).
    pub fn worker(&self) -> &LuaWorker {
        &self.worker
    }

    /// Zugriff auf den Worker (Schreiben).
    pub fn worker_mut(&mut self) -> &mut LuaWorker {
        &mut self.worker
    }

    /// `true`, wenn der Worker für die normale Welt reserviert ist (§9).
    pub fn is_reserved(&self) -> bool {
        self.worker.is_reserved()
    }

    /// Anzahl der in der Inbox eingestellten Arbeiten.
    pub fn inbox_len(&self) -> usize {
        self.inbox.len()
    }

    /// `true`, wenn die Inbox keine Arbeit enthält.
    pub fn inbox_is_empty(&self) -> bool {
        self.inbox.is_empty()
    }

    /// `true`, wenn die Inbox voll ist.
    pub fn inbox_is_full(&self) -> bool {
        self.inbox.is_full()
    }

    /// Inbox-Kapazität dieses Workers.
    pub fn inbox_capacity(&self) -> usize {
        self.inbox.capacity()
    }

    /// Entnimmt die älteste eingestellte Arbeit (FIFO).
    pub fn inbox_pop(&mut self) -> Option<LuaWork> {
        self.inbox.pop()
    }

    fn try_deliver(&mut self, work: LuaWork) -> Result<(), QueueFull<LuaWork>> {
        self.inbox.try_push(work)
    }
}

/// Verteilt Lua-Arbeit deterministisch auf normale Worker und leitet
/// Dedicated Arbeit an ihren gebundenen Worker.
///
/// Besitzt die Worker-Slots (LuaWorker + Inbox). Der Realm füllt den
/// Dispatcher über `dispatch`/`dispatch_dedicated`; Lua-Ausführung findet
/// hier nie statt.
#[derive(Debug)]
pub struct LuaDispatcher {
    slots: Vec<WorkerSlot>,
    /// Positionszeiger: Startplatz für die nächste Suche (Round-Robin).
    cursor: usize,
}

impl LuaDispatcher {
    /// Neuer, leerer Dispatcher (kein Worker-Pool vorhanden).
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            cursor: 0,
        }
    }

    /// Neuer Dispatcher mit Standard-Pool-Konfiguration (§9).
    ///
    /// Erzeugt `base_count` Worker; davon sind `normal_reserved` für die
    /// normale Welt geschützt. Die Inbox-Kapazität wird je Worker gesetzt.
    pub fn with_pool(config: &PoolConfig) -> Result<Self, LuaError> {
        config
            .validate()
            .map_err(|e| LuaError::internal(e.to_string()))?;
        let mut dispatcher = Self::new();
        for i in 0..config.base_count {
            if i < config.normal_reserved {
                let worker = LuaWorker::new_reserved()?;
                dispatcher.add_reserved_normal_worker(worker, config.inbox_capacity);
            } else {
                let worker = LuaWorker::new()?;
                dispatcher.add_normal_worker(worker, config.inbox_capacity);
            }
        }
        Ok(dispatcher)
    }

    /// Fügt einen normalen World-/Regular-Worker mit eigener Inbox hinzu
    /// (nicht reserviert, für Spezial-/Dedicated-Nutzung verfügbar).
    ///
    /// Die Inbox-Kapazität wird hier je Worker konfiguriert (keine
    /// erfundenen Produktionswerte). Liefert die Worker-ID zurück.
    pub fn add_normal_worker(&mut self, worker: LuaWorker, inbox_capacity: usize) -> WorkerId {
        let id = worker.id();
        self.slots.push(WorkerSlot::new(worker, inbox_capacity));
        id
    }

    /// Fügt einen für die normale Welt reservierten Worker hinzu (§9).
    ///
    /// Reservierte Worker werden NIE für Dedicated Contexts gebunden.
    pub fn add_reserved_normal_worker(
        &mut self,
        worker: LuaWorker,
        inbox_capacity: usize,
    ) -> WorkerId {
        let id = worker.id();
        debug_assert!(worker.is_reserved(), "reservierter Worker erwartet");
        self.slots.push(WorkerSlot::new(worker, inbox_capacity));
        id
    }

    /// Fügt einen temporären Dedicated Worker hinzu (§11).
    ///
    /// Nach dem Ende seines Contexts wird der temporäre Worker zerstört
    /// (`DRAINING → DESTROYING`) und NIE als normaler Basis-Worker
    /// wiederverwendet.
    pub fn add_temporary_worker(&mut self, inbox_capacity: usize) -> Result<WorkerId, LuaError> {
        let worker = LuaWorker::new_temporary()?;
        let id = worker.id();
        self.slots.push(WorkerSlot::new(worker, inbox_capacity));
        Ok(id)
    }

    /// Anzahl der verwalteten Worker.
    pub fn worker_count(&self) -> usize {
        self.slots.len()
    }

    /// Anzahl der für die normale Welt reservierten Worker.
    pub fn reserved_worker_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_reserved()).count()
    }

    /// Anzahl der in Dedicated Contexts gebundenen Worker.
    pub fn bound_worker_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.worker.binding().is_some())
            .count()
    }

    /// `true`, wenn der Dispatcher keine Worker verwaltet.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Worker-Slot per ID.
    pub fn slot(&self, id: WorkerId) -> Option<&WorkerSlot> {
        self.slots.iter().find(|s| s.worker.id() == id)
    }

    /// Worker-Slot per ID (Schreiben).
    pub fn slot_mut(&mut self, id: WorkerId) -> Option<&mut WorkerSlot> {
        self.slots.iter_mut().find(|s| s.worker.id() == id)
    }

    /// Worker per ID (Lesen).
    pub fn worker(&self, id: WorkerId) -> Option<&LuaWorker> {
        self.slot(id).map(|s| s.worker())
    }

    /// Worker per ID (Schreiben).
    pub fn worker_mut(&mut self, id: WorkerId) -> Option<&mut LuaWorker> {
        self.slot_mut(id).map(|s| s.worker_mut())
    }

    /// Einstellungsgröße der Inbox eines Workers.
    pub fn inbox_len(&self, id: WorkerId) -> Option<usize> {
        self.slot(id).map(|s| s.inbox_len())
    }

    /// `true`, wenn die Inbox eines Workers leer ist.
    pub fn inbox_is_empty(&self, id: WorkerId) -> Option<bool> {
        self.slot(id).map(|s| s.inbox_is_empty())
    }

    /// `true`, wenn die Inbox eines Workers voll ist.
    pub fn inbox_is_full(&self, id: WorkerId) -> Option<bool> {
        self.slot(id).map(|s| s.inbox_is_full())
    }

    /// Entnimmt die älteste Arbeit aus der Inbox eines Workers (FIFO).
    pub fn inbox_pop(&mut self, id: WorkerId) -> Option<LuaWork> {
        self.slot_mut(id).and_then(|s| s.inbox_pop())
    }

    /// Reserviert einen verfügbaren Worker exklusiv für einen Dedicated
    /// Context (`IDLE → RESERVED`, §10/§11).
    ///
    /// Auswahl: erster nicht-reservierter, nicht-gebundener Worker im
    /// IDLE-Zustand. Reservierte Normal-Worker (§9) werden dabei NIE
    /// gewählt (`CannotBindReserved`).
    pub fn reserve_for_dedicated(
        &mut self,
        ctx: DedicatedContext,
    ) -> Result<WorkerId, LifecycleError> {
        let idx = self.slots.iter().position(|s| {
            !s.worker.is_reserved()
                && s.worker.lifecycle() == WorkerLifecycle::Idle
                && s.worker.binding().is_none()
        });
        let idx = match idx {
            Some(i) => i,
            None => {
                return Err(LifecycleError {
                    kind: LifecycleErrorKind::CannotBindReserved,
                })
            }
        };
        let worker_id = self.slots[idx].worker.id();
        self.slots[idx].worker.reserve_for(ctx)?;
        Ok(worker_id)
    }

    /// Findet den Slot, der exakt an diesen Dedicated Context gebunden ist.
    fn bound_slot(&self, ctx: &DedicatedContext) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.worker.binding() == Some(ctx))
    }

    /// Verteilt Dedicated Arbeit an den gebundenen Worker (§10, §11).
    ///
    /// Vor der Inbox-Zustellung wird geprüft:
    ///   1. Es existiert ein an `ctx` gebundener Worker.
    ///   2. `work.dedicated` passt zu `ctx` (Context-Mismatch-Regel).
    ///   3. Der Worker ist ACTIVE (RESERVED verarbeitet nicht,
    ///      DRAINING nimmt nichts Neues an).
    ///
    /// Erst danach wird in die Inbox eingestellt. Bei voller Dedicated
    /// Inbox bleibt der Fall offen (§12): Es wird nur neutral `InboxFull`
    /// zurückgegeben — keine erfundene Policy.
    pub fn dispatch_dedicated(
        &mut self,
        ctx: &DedicatedContext,
        work: LuaWork,
    ) -> DedicatedDispatchResult {
        let idx = match self.bound_slot(ctx) {
            Some(i) => i,
            None => {
                return DedicatedDispatchResult::Rejected(DedicatedDispatchError::NoWorkerBound {
                    ctx: ctx.clone(),
                })
            }
        };

        let expected = ctx.label();
        let received = match &work.dedicated {
            Some(d) => d.label(),
            None => {
                return DedicatedDispatchResult::Rejected(DedicatedDispatchError::MissingContext {
                    worker: Some(self.slots[idx].worker.id()),
                })
            }
        };
        if received != expected {
            let mismatch = LifecycleError {
                kind: LifecycleErrorKind::ContextMismatch {
                    worker_id: self.slots[idx].worker.id().as_u64(),
                    expected,
                    received,
                },
            };
            return DedicatedDispatchResult::Rejected(DedicatedDispatchError::ContextMismatch(
                mismatch,
            ));
        }

        let lifecycle = self.slots[idx].worker.lifecycle();
        if lifecycle != WorkerLifecycle::Active {
            return DedicatedDispatchResult::Rejected(DedicatedDispatchError::WorkerNotActive {
                ctx: ctx.clone(),
                worker: self.slots[idx].worker.id(),
                lifecycle,
            });
        }

        match self.slots[idx].try_deliver(work) {
            Ok(()) => DedicatedDispatchResult::Accepted {
                worker: self.slots[idx].worker.id(),
            },
            Err(full) => DedicatedDispatchResult::Rejected(DedicatedDispatchError::InboxFull {
                ctx: ctx.clone(),
                worker: self.slots[idx].worker.id(),
                work: full.into_item(),
            }),
        }
    }

    /// Verteilt Arbeit auf einen geeigneten normalen Worker.
    ///
    /// Nur nicht-gebundene (normale) Worker im IDLE-Zustand sind Kandidaten.
    /// Deterministische Suche ab dem Positionszeiger (Round-Robin):
    /// Es wird der erste verfügbare Worker (Inbox nicht voll) gewählt.
    /// Sind alle Inboxes voll, greift die QoS-Politik (§12):
    /// Droppable wird verworfen, Reliable/Coalescable zurückgegeben.
    pub fn dispatch(&mut self, work: LuaWork) -> DispatchOutcome {
        if self.slots.is_empty() {
            return DispatchOutcome::NoWorkerAvailable { work };
        }

        let n = self.slots.len();
        let start = self.cursor % n;
        for offset in 0..n {
            let idx = (start + offset) % n;
            let slot = &self.slots[idx];
            // Dedicated Worker (gebunden bzw. nicht IDLE) nehmen keine
            // normale World-Arbeit an (§11).
            if slot.worker.lifecycle() != WorkerLifecycle::Idle || slot.worker.binding().is_some() {
                continue;
            }
            if !slot.inbox_is_full() {
                self.slots[idx]
                    .try_deliver(work)
                    .expect("inbox vorher nicht voll geprüft: Zustellung muss gelingen");
                self.cursor = (idx + 1) % n;
                return DispatchOutcome::Accepted {
                    worker: self.slots[idx].worker.id(),
                };
            }
        }

        self.cursor = (start + 1) % n;
        match work.class {
            QueueClass::Droppable => DispatchOutcome::Dropped,
            QueueClass::Reliable | QueueClass::Coalescable => {
                DispatchOutcome::NoWorkerAvailable { work }
            }
        }
    }
}

impl Default for LuaDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::ScriptContext;
    use super::super::domain::ScriptDomain;
    use super::super::event::ScriptEvent;
    use super::super::queue::QueueClass;
    use super::super::script::ScriptSource;
    use super::*;

    fn work(class: QueueClass, name: &str) -> LuaWork {
        LuaWork::new(class, ScriptEvent::new(name, ScriptContext::new()))
    }

    fn work_for(class: QueueClass, name: &str, ctx: DedicatedContext) -> LuaWork {
        LuaWork::dedicated(class, ScriptEvent::new(name, ScriptContext::new()), ctx)
    }

    fn dispatcher_with_two(idle_capacity: usize) -> (LuaDispatcher, WorkerId, WorkerId) {
        let mut d = LuaDispatcher::new();
        let a = d.add_normal_worker(LuaWorker::new().unwrap(), idle_capacity);
        let b = d.add_normal_worker(LuaWorker::new().unwrap(), idle_capacity);
        (d, a, b)
    }

    fn assert_accepted(outcome: &DispatchOutcome, expected: WorkerId) {
        match outcome {
            DispatchOutcome::Accepted { worker } => assert_eq!(*worker, expected),
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
    }

    /// Bindet `worker_id` an `ctx` und aktiviert ihn (RESERVED → ACTIVE).
    fn bind_and_activate(d: &mut LuaDispatcher, worker_id: WorkerId, ctx: &DedicatedContext) {
        d.worker_mut(worker_id)
            .unwrap()
            .reserve_for(ctx.clone())
            .unwrap();
        d.worker_mut(worker_id)
            .unwrap()
            .prepare_for_activation()
            .unwrap();
    }

    // ── Zuordnung (Stufe 2, unverändert) ───────────────────────────

    #[test]
    fn dispatcher_assigns_work_to_available_normal_worker() {
        let (mut d, a, _) = dispatcher_with_two(3);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "hail")), a);
        assert_eq!(d.inbox_len(a), Some(1));
        assert!(!d.slot(a).unwrap().inbox_is_empty());
    }

    #[test]
    fn dispatcher_round_robin_is_deterministic() {
        let (mut d, a, b) = dispatcher_with_two(3);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "1")), a);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "2")), b);
        // Zeiger läuft weiter: nächste Zustellung geht wieder an a.
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "3")), a);
        assert_eq!(d.inbox_len(a), Some(2));
        assert_eq!(d.inbox_len(b), Some(1));
    }

    #[test]
    fn dispatcher_without_workers_returns_work() {
        let mut d = LuaDispatcher::new();
        let keep = work(QueueClass::Reliable, "verwaist");
        match d.dispatch(keep.clone()) {
            DispatchOutcome::NoWorkerAvailable { work } => {
                assert_eq!(work.class, QueueClass::Reliable);
                assert_eq!(work.event.name, "verwaist");
            }
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
    }

    // ── Überlauf auf anderen verfügbaren Worker (Stufe 2) ──────────

    #[test]
    fn dispatcher_overflow_goes_to_another_available_worker() {
        let (mut d, a, b) = dispatcher_with_two(1);
        // a vollen (Kapazität 1):
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "erstes")), a);
        // a ist voll -> b übernimmt den Overflow:
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "zweites")), b);
        assert_eq!(d.inbox_len(a), Some(1));
        assert_eq!(d.inbox_len(b), Some(1));
    }

    #[test]
    fn dispatcher_overflow_wraps_around() {
        let (mut d, a, b) = dispatcher_with_two(1);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "1")), a);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "2")), b);
        // Beide voll; nach Entnahme bei a gibt es wieder Kapazität.
        d.inbox_pop(a);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "3")), a);
    }

    // ── QoS bei vollständig überlastetem normalen Pool (Stufe 2) ───

    #[test]
    fn reliable_is_never_dropped_on_full_pool() {
        let (mut d, _, _) = dispatcher_with_two(1);
        // Beide Inboxes füllen (Round-Robin verteilt deterministisch):
        let mut delivered = 0;
        while !d.slots.iter().all(|s| s.inbox_is_full()) {
            match d.dispatch(work(QueueClass::Reliable, "fuell")) {
                DispatchOutcome::Accepted { .. } => delivered += 1,
                other => panic!("unerwartetes Ergebnis: {other:?}"),
            }
        }
        assert_eq!(delivered, 2);

        // Kein Platz mehr: Reliable wird NICHT verworfen.
        let keep = work(QueueClass::Reliable, "wichtig");
        match d.dispatch(keep.clone()) {
            DispatchOutcome::NoWorkerAvailable { work } => {
                assert_eq!(work.class, QueueClass::Reliable);
                assert_eq!(work.event.name, "wichtig");
            }
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
        assert!(d.slots.iter().all(|s| s.inbox_is_full()));
    }

    #[test]
    fn droppable_may_be_dropped_when_pool_is_full() {
        let (mut d, ..) = dispatcher_with_two(1);
        let _ = d.dispatch(work(QueueClass::Droppable, "f1"));
        let _ = d.dispatch(work(QueueClass::Droppable, "f2"));

        match d.dispatch(work(QueueClass::Droppable, "weg")) {
            DispatchOutcome::Dropped => {}
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
        // Nichts eingestellt, nichts verloren gegangen (nur verworfen).
        assert_eq!(d.slots.iter().map(|s| s.inbox_len()).sum::<usize>(), 2);
    }

    #[test]
    fn droppable_with_space_is_still_accepted() {
        let (mut d, a, _) = dispatcher_with_two(2);
        assert_accepted(&d.dispatch(work(QueueClass::Droppable, "pos")), a);
    }

    // ── Isolation zwischen Workern (Stufe 1/2) ─────────────────────

    #[test]
    fn dispatch_keeps_workers_isolated() {
        let (mut d, a, b) = dispatcher_with_two(3);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "nur-a")), a);

        // Worker a: Arbeit eingestellt.
        assert_eq!(d.inbox_len(a), Some(1));
        let delivered = d.slot_mut(a).unwrap().inbox_pop().unwrap();
        assert_eq!(delivered.event.name, "nur-a");
        assert_eq!(delivered.class, QueueClass::Reliable);

        // Worker b: leere Inbox, eigener Lua-Zustand unverändert.
        assert_eq!(d.inbox_len(b), Some(0));
        assert_eq!(d.worker(b).unwrap().script_count(), 0);
    }

    #[test]
    fn dispatch_does_not_change_other_workers_state() {
        let (mut d, a, b) = dispatcher_with_two(3);

        // Worker b lädt eigenständig ein Script (Stufe-1-API).
        let id_b = {
            let worker = d.worker_mut(b).unwrap();
            worker
                .load_script(
                    ScriptDomain::Npc,
                    ScriptSource::Code {
                        name: "b-script".into(),
                        code: "return { handle = function(c) end }".into(),
                    },
                )
                .unwrap()
        };
        assert_eq!(d.worker(b).unwrap().script_count(), 1);

        // Zustellung an a verändert b weder (Scripts noch Inbox).
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "an-a")), a);
        assert_eq!(d.worker(b).unwrap().script_count(), 1);
        assert!(d.worker(b).unwrap().manager().has_script(&id_b));
        assert_eq!(d.inbox_len(b), Some(0));
        assert_eq!(d.inbox_len(a), Some(1));
    }

    #[test]
    fn multiple_workers_receive_only_their_own_work() {
        let (mut d, a, b) = dispatcher_with_two(3);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "a-1")), a);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "b-1")), b);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "a-2")), a);

        let from_a: Vec<String> = {
            let slot = d.slot_mut(a).unwrap();
            let mut names = Vec::new();
            while let Some(w) = slot.inbox_pop() {
                names.push(w.event.name);
            }
            names
        };
        assert_eq!(from_a, vec!["a-1".to_string(), "a-2".to_string()]);

        let from_b: Vec<String> = {
            let slot = d.slot_mut(b).unwrap();
            let mut names = Vec::new();
            while let Some(w) = slot.inbox_pop() {
                names.push(w.event.name);
            }
            names
        };
        assert_eq!(from_b, vec!["b-1".to_string()]);
    }

    // ── Stufe 3: Dedicated Contexts / 1:1-Bindung ─────────────────

    #[test]
    fn dedicated_binder_exclusive_1to1_context() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx_a = DedicatedContext::raid(4711);
        // Bounding an Context A gelingt.
        d.worker_mut(a).unwrap().reserve_for(ctx_a.clone()).unwrap();
        assert_eq!(d.worker(a).unwrap().binding(), Some(&ctx_a));

        // Zweite Bindung an Context B wird abgelehnt (1:1, §10).
        let ctx_b = DedicatedContext::raid(4712);
        let err = d
            .worker_mut(a)
            .unwrap()
            .reserve_for(ctx_b.clone())
            .unwrap_err();
        assert!(matches!(
            err.kind,
            LifecycleErrorKind::WorkerAlreadyBound { .. }
        ));
        // Bindung bleibt Context A.
        assert_eq!(d.worker(a).unwrap().binding(), Some(&ctx_a));
    }

    #[test]
    fn reserved_state_accepts_no_dedicated_events() {
        let (mut d, a, _) = dispatcher_with_two(16);
        let ctx = DedicatedContext::raid(1);
        d.worker_mut(a).unwrap().reserve_for(ctx.clone()).unwrap();
        assert_eq!(d.worker(a).unwrap().lifecycle(), WorkerLifecycle::Reserved);

        // RESERVED verarbeitet noch keine Dedicated Events (§11).
        let outcome = d.dispatch_dedicated(&ctx, work_for(QueueClass::Reliable, "e", ctx.clone()));
        match outcome {
            DedicatedDispatchResult::Rejected(DedicatedDispatchError::WorkerNotActive {
                lifecycle,
                ..
            }) => assert_eq!(lifecycle, WorkerLifecycle::Reserved),
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
        assert_eq!(d.inbox_len(a), Some(0));
    }

    #[test]
    fn reserved_to_active_after_preparation() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(7);
        d.worker_mut(a).unwrap().reserve_for(ctx.clone()).unwrap();
        // Vorerst RESERVED.
        assert_eq!(d.worker(a).unwrap().lifecycle(), WorkerLifecycle::Reserved);
        // Vorbereitung abschließen -> ACTIVE.
        d.worker_mut(a).unwrap().prepare_for_activation().unwrap();
        assert_eq!(d.worker(a).unwrap().lifecycle(), WorkerLifecycle::Active);
    }

    #[test]
    fn active_accepts_events_of_bound_context() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(99);
        bind_and_activate(&mut d, a, &ctx);

        let outcome =
            d.dispatch_dedicated(&ctx, work_for(QueueClass::Reliable, "raid-e", ctx.clone()));
        match outcome {
            DedicatedDispatchResult::Accepted { worker } => assert_eq!(worker, a),
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
        assert_eq!(d.inbox_len(a), Some(1));
    }

    #[test]
    fn active_rejects_foreign_context_before_inbox_as_mismatch() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx_a = DedicatedContext::raid(4711);
        let ctx_b = DedicatedContext::raid(4712);
        bind_and_activate(&mut d, a, &ctx_a);

        // Arbeit für Context B an den an A gebundenen Worker:
        // Mismatch VOR der Inbox-Zustellung (§10).
        let outcome = d.dispatch_dedicated(&ctx_a, work_for(QueueClass::Reliable, "fremd", ctx_b));
        match outcome {
            DedicatedDispatchResult::Rejected(DedicatedDispatchError::ContextMismatch(e)) => {
                match &e.kind {
                    LifecycleErrorKind::ContextMismatch {
                        worker_id,
                        expected,
                        received,
                    } => {
                        assert_eq!(*worker_id, a.as_u64());
                        assert_eq!(expected, "raid-4711");
                        assert_eq!(received, "raid-4712");
                    }
                    other => panic!("unerwarteter Fehler: {other:?}"),
                }
            }
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
    }

    #[test]
    fn mismatch_never_enters_inbox() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx_a = DedicatedContext::raid(1);
        let ctx_b = DedicatedContext::raid(2);
        bind_and_activate(&mut d, a, &ctx_a);

        let outcome = d.dispatch_dedicated(&ctx_a, work_for(QueueClass::Reliable, "fremd", ctx_b));
        assert!(outcome.is_context_mismatch());
        // Nichts wurde eingereiht.
        assert_eq!(d.inbox_len(a), Some(0));
    }

    #[test]
    fn dedicated_worker_receives_no_normal_world_work() {
        let (mut d, a, b) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(4711);
        bind_and_activate(&mut d, a, &ctx);

        // Normale Weltarbeit geht ausschließlich an den normalen Worker b,
        // NIE an den gebundenen Dedicated Worker a (§11).
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "welt")), b);
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "welt2")), b);
        assert_eq!(d.inbox_len(a), Some(0));
        assert_eq!(d.inbox_len(b), Some(2));
    }

    // ── Stufe 3: Schutz der reservierten Normal-Worker (§9) ────────

    #[test]
    fn reserved_normal_workers_cannot_be_bound_for_dedicated() {
        let mut d = LuaDispatcher::new();
        let r1 = d.add_reserved_normal_worker(LuaWorker::new_reserved().unwrap(), 3);
        d.add_normal_worker(LuaWorker::new().unwrap(), 3);

        // Schützmechanismus: Der reservierte Worker r1 (geschützter
        // Normal-Worker) kann NIE für einen Dedicated Context gebunden
        // werden — auch nicht durch direkten Worker-Zugriff (§9).
        let ctx = DedicatedContext::raid(5);
        let err = d
            .slot_mut(r1)
            .unwrap()
            .worker_mut()
            .reserve_for(ctx.clone())
            .unwrap_err();
        assert!(matches!(err.kind, LifecycleErrorKind::CannotBindReserved));

        // Der freie, nicht-reservierte Worker übernimmt die Bindung.
        let bound = d.reserve_for_dedicated(ctx.clone()).unwrap();
        assert_ne!(bound, r1);
        assert_eq!(d.worker(bound).unwrap().binding(), Some(&ctx));
        // Der geschützte Worker blieb IDLE und ungebunden.
        assert_eq!(d.worker(r1).unwrap().lifecycle(), WorkerLifecycle::Idle);
        assert!(d.worker(r1).unwrap().binding().is_none());
    }

    #[test]
    fn four_protected_normal_workers_stay_protected_by_default() {
        let cfg = PoolConfig::default_config();
        let mut d = LuaDispatcher::with_pool(&cfg).unwrap();
        assert_eq!(d.worker_count(), cfg.base_count);
        assert_eq!(d.reserved_worker_count(), cfg.normal_reserved);

        // Versuche, alle Worker für Dedicated Contexts zu binden.
        // Es dürfen nur die nicht-reservierten Basis-Slots die Bindung
        // annehmen; die vier geschützten bleiben ungebunden.
        let available = d.slots.iter().filter(|s| !s.is_reserved()).count();
        for i in 0..available {
            let ctx = DedicatedContext::raid(1000 + i as u64);
            d.reserve_for_dedicated(ctx.clone()).unwrap();
        }
        // Alle weiteren Reservierungen schlagen fehl (Schutz bleibt).
        let extra = d.reserve_for_dedicated(DedicatedContext::raid(9000));
        assert!(extra.is_err());

        // Genau die vier geschützten Worker sind ohne Bindung / IDLE.
        let unbound = d
            .slots
            .iter()
            .filter(|s| s.worker.binding().is_none())
            .count();
        assert_eq!(unbound, cfg.normal_reserved);
        assert!(d
            .slots
            .iter()
            .all(|s| !s.is_reserved() || s.worker.binding().is_none()));
    }

    // ── Stufe 3: DRAINING / Drain-Abschluss (§11) ──────────────────

    #[test]
    fn active_to_draining_accepts_no_new_gameplay_events() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(4711);
        bind_and_activate(&mut d, a, &ctx);

        // DRAINING einleiten: keine neuen Lua-Gameplay-Events mehr (§11).
        d.worker_mut(a).unwrap().start_draining().unwrap();
        assert_eq!(d.worker(a).unwrap().lifecycle(), WorkerLifecycle::Draining);

        let outcome =
            d.dispatch_dedicated(&ctx, work_for(QueueClass::Reliable, "neu", ctx.clone()));
        match outcome {
            DedicatedDispatchResult::Rejected(DedicatedDispatchError::WorkerNotActive {
                lifecycle,
                ..
            }) => assert_eq!(lifecycle, WorkerLifecycle::Draining),
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
        assert_eq!(d.inbox_len(a), Some(0));
    }

    #[test]
    fn accepted_work_stays_processing_during_draining() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(4711);
        bind_and_activate(&mut d, a, &ctx);

        // Bereits akzeptierte Arbeit wird während DRAINING noch
        // abgearbeitet (FIFO-Entnahme aus der Inbox ist weiter möglich).
        let first = work_for(QueueClass::Reliable, "bereits-akzeptiert", ctx.clone());
        assert!(d.dispatch_dedicated(&ctx, first).is_accepted());
        assert_eq!(d.inbox_len(a), Some(1));

        d.worker_mut(a).unwrap().start_draining().unwrap();

        let delivered = d.inbox_pop(a).unwrap();
        assert_eq!(delivered.event.name, "bereits-akzeptiert");
        assert_eq!(d.inbox_len(a), Some(0));
    }

    #[test]
    fn drain_completes_when_no_pending_work_left() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(4711);
        bind_and_activate(&mut d, a, &ctx);

        // Eine angenommene Arbeit abarbeiten.
        assert!(d
            .dispatch_dedicated(&ctx, work_for(QueueClass::Reliable, "alt", ctx.clone()))
            .is_accepted());
        d.worker_mut(a).unwrap().start_draining().unwrap();
        d.inbox_pop(a).unwrap();

        // Keine ausstehende Arbeit mehr -> normaler Drain abgeschlossen.
        assert_eq!(d.inbox_is_empty(a), Some(true));
        d.worker_mut(a).unwrap().complete_drain_for_reuse().unwrap();
        assert_eq!(d.worker(a).unwrap().lifecycle(), WorkerLifecycle::Idle);
    }

    // ── Stufe 3: RESETTING / frische VM (§11) ──────────────────────

    #[test]
    fn base_worker_draining_resetting_idle() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(77);
        bind_and_activate(&mut d, a, &ctx);

        d.worker_mut(a).unwrap().start_draining().unwrap();
        d.worker_mut(a).unwrap().complete_drain_for_reuse().unwrap();
        assert_eq!(d.worker(a).unwrap().lifecycle(), WorkerLifecycle::Idle);
        assert!(d.worker(a).unwrap().binding().is_none());
    }

    #[test]
    fn reset_removes_context_binding() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(4711);
        bind_and_activate(&mut d, a, &ctx);
        assert_eq!(d.worker(a).unwrap().binding(), Some(&ctx));

        d.worker_mut(a).unwrap().start_draining().unwrap();
        d.worker_mut(a).unwrap().complete_drain_for_reuse().unwrap();
        assert!(d.worker(a).unwrap().binding().is_none());
    }

    #[test]
    fn reset_uses_fresh_vm() {
        let (mut d, a, _) = dispatcher_with_two(3);
        let ctx = DedicatedContext::raid(4711);
        bind_and_activate(&mut d, a, &ctx);

        // Im Context laden wir ein Script (Zustand A).
        d.worker_mut(a)
            .unwrap()
            .load_script(
                ScriptDomain::Npc,
                ScriptSource::Code {
                    name: "nurA".into(),
                    code: "return { handle = function(c) end }".into(),
                },
            )
            .unwrap();
        assert_eq!(d.worker(a).unwrap().script_count(), 1);

        // Drain + Reset -> frische VM; geladene Scripts sind weg.
        d.worker_mut(a).unwrap().start_draining().unwrap();
        d.worker_mut(a).unwrap().complete_drain_for_reuse().unwrap();
        assert_eq!(d.worker(a).unwrap().lifecycle(), WorkerLifecycle::Idle);
        assert_eq!(d.worker(a).unwrap().script_count(), 0);
    }

    #[test]
    fn lua_state_from_context_a_not_visible_in_context_b() {
        let (mut d, a, b) = dispatcher_with_two(3);
        let ctx_a = DedicatedContext::raid(4711);
        let ctx_b = DedicatedContext::raid(4712);

        // Worker a: Context A aktivieren, Lua-Zustand setzen.
        bind_and_activate(&mut d, a, &ctx_a);
        let id_a = d
            .worker_mut(a)
            .unwrap()
            .load_script(
                ScriptDomain::Npc,
                ScriptSource::Code {
                    name: "internA".into(),
                    code: "token = 'A-geheim'\n\
                           return { handle = function(c) andora.emit('leak', { v = token }) end }"
                        .into(),
                },
            )
            .unwrap();
        let dr = d
            .worker_mut(a)
            .unwrap()
            .call(&id_a, "handle", &ScriptContext::new())
            .unwrap();
        assert_eq!(
            dr.requests[0],
            super::super::host::RealmRequest::Emit {
                name: "leak".into(),
                payload: serde_json::json!({ "v": "A-geheim" }),
            }
        );

        // Drain + Reset (frische VM, Script weg, Bindung entfernt).
        d.worker_mut(a).unwrap().start_draining().unwrap();
        d.worker_mut(a).unwrap().complete_drain_for_reuse().unwrap();

        // Worker a neu an Context B binden.
        bind_and_activate(&mut d, a, &ctx_b);
        let id_b = d
            .worker_mut(a)
            .unwrap()
            .load_script(
                ScriptDomain::Npc,
                ScriptSource::Code {
                    name: "internB".into(),
                    code: "return { check = function(c) \
                               andora.emit('leak', { exists = token ~= nil }) end }"
                        .into(),
                },
            )
            .unwrap();
        let drb = d
            .worker_mut(a)
            .unwrap()
            .call(&id_b, "check", &ScriptContext::new())
            .unwrap();
        // 'token' ist in der frischen VM von Context B nicht sichtbar.
        assert_eq!(
            drb.requests[0],
            super::super::host::RealmRequest::Emit {
                name: "leak".into(),
                payload: serde_json::json!({ "exists": false }),
            }
        );

        // Worker b blieb unberührt (Leer, IDLE).
        assert_eq!(d.worker(b).unwrap().script_count(), 0);
        assert_eq!(d.worker(b).unwrap().lifecycle(), WorkerLifecycle::Idle);
    }

    // ── Stufe 3: Temporäre Worker (§11) ────────────────────────────

    #[test]
    fn temporary_worker_draining_to_destroying() {
        let mut d = LuaDispatcher::new();
        let tmp = d.add_temporary_worker(3).unwrap();
        assert!(d.worker(tmp).unwrap().is_temporary());

        let ctx = DedicatedContext::raid(4711);
        d.worker_mut(tmp).unwrap().reserve_for(ctx.clone()).unwrap();
        d.worker_mut(tmp).unwrap().prepare_for_activation().unwrap();
        assert_eq!(d.worker(tmp).unwrap().lifecycle(), WorkerLifecycle::Active);

        d.worker_mut(tmp).unwrap().start_draining().unwrap();
        d.worker_mut(tmp)
            .unwrap()
            .complete_drain_for_destroy()
            .unwrap();
        assert_eq!(
            d.worker(tmp).unwrap().lifecycle(),
            WorkerLifecycle::Destroying
        );
    }

    #[test]
    fn temporary_worker_is_not_reused_as_base_worker() {
        let mut d = LuaDispatcher::new();
        d.add_normal_worker(LuaWorker::new().unwrap(), 3);
        let tmp = d.add_temporary_worker(3).unwrap();
        let ctx = DedicatedContext::raid(4711);
        d.worker_mut(tmp).unwrap().reserve_for(ctx.clone()).unwrap();
        d.worker_mut(tmp).unwrap().prepare_for_activation().unwrap();
        d.worker_mut(tmp).unwrap().start_draining().unwrap();
        d.worker_mut(tmp)
            .unwrap()
            .complete_drain_for_destroy()
            .unwrap();

        // Der temporäre Worker wird NIE als normaler Basis-Worker in die
        // normale Verteilung aufgenommen: Er bleibt temporär und DESTROYING.
        assert_eq!(
            d.worker(tmp).unwrap().lifecycle(),
            WorkerLifecycle::Destroying
        );
        assert!(d.worker(tmp).unwrap().is_temporary());

        // Normale Arbeit geht an den normalen Worker, nicht an tmp.
        let normal_id = d.slots[0].worker.id();
        assert_accepted(&d.dispatch(work(QueueClass::Reliable, "welt")), normal_id);
        assert_eq!(d.inbox_len(tmp), Some(0));
    }

    // ── Stufe 3: Dedicated-Overflow bleibt offen (§12) ─────────────

    #[test]
    fn dedicated_overflow_gets_no_invented_policy() {
        let (mut d, a, _) = dispatcher_with_two(1);
        let ctx = DedicatedContext::raid(4711);
        bind_and_activate(&mut d, a, &ctx);

        // Inbox voll (Kapazität 1).
        assert!(d
            .dispatch_dedicated(&ctx, work_for(QueueClass::Reliable, "f1", ctx.clone()))
            .is_accepted());
        assert_eq!(d.inbox_is_full(a), Some(true));

        // Weitere Dedicated-Arbeit: KEINE erfundene Policy. Nur neutrale
        // technische Rückmeldung (InboxFull) samt Rückgabe der Arbeit.
        // Es wird NICHT verworfen, kein zweiter Worker zugewiesen,
        // kein Reset/Abruch vorgenommen.
        let next = work_for(QueueClass::Reliable, "f2", ctx.clone());
        match d.dispatch_dedicated(&ctx, next.clone()) {
            DedicatedDispatchResult::Rejected(DedicatedDispatchError::InboxFull {
                work, ..
            }) => {
                // Die Arbeit wurde NICHT stillschweigend verworfen.
                assert_eq!(work.event.name, "f2");
                assert_eq!(work.dedicated, Some(ctx.clone()));
            }
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
        // Der Worker bleibt gebunden und ACTIVE (kein Reset, kein Failover).
        assert_eq!(d.worker(a).unwrap().binding(), Some(&ctx));
        assert_eq!(d.worker(a).unwrap().lifecycle(), WorkerLifecycle::Active);
        assert_eq!(d.bound_worker_count(), 1);
    }

    // ── Stufe 3: PoolConfig-Integration ────────────────────────────

    #[test]
    fn pool_config_produces_valid_working_pool() {
        let cfg = PoolConfig {
            base_count: 6,
            normal_reserved: 2,
            max_count: 8,
            inbox_capacity: 4,
        };
        let mut d = LuaDispatcher::with_pool(&cfg).unwrap();
        assert_eq!(d.worker_count(), 6);
        assert_eq!(d.reserved_worker_count(), 2);

        // Normale Round-Robin funktioniert auf dem Pool.
        let outcome = d.dispatch(work(QueueClass::Reliable, "welt"));
        assert!(matches!(outcome, DispatchOutcome::Accepted { .. }));
    }
}
