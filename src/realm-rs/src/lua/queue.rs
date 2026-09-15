// queue — bounded Lua-Event-Queues mit QoS-Klassen
// (docs/Lua-Scripting-System.md §8, §12).
//
// Stufe 2 liefert die technische Grundlage der Kaskade:
//   - QueueClass: Reliable / Coalescable / Droppable (§12)
//   - BoundedQueue<T>: generische, FIFO-begrenzte Queue ohne Default
//     (die Kapazität wird pro Instanz gesetzt; es werden bewusst keine
//     Produktions-Größen erfunden)
//   - LuaWork: Event + zugehörige QoS-Klasse
//   - EventQueue: kapselt BoundedQueue<LuaWork> und die QoS-Politik:
//         Droppable   -> darf bei voller Queue verworfen werden (§12)
//     Reliable/Coalescable -> wird bei voller Queue NICHT verworfen,
//     sondern zurückgegeben (Überlauf zur Weiterverteilung/geregelten
//     Behandlung durch den Aufrufer)
//   - RequestQueue: minimaler bounded Grundbaustein für den
//     Worker -> Realm-Fluss (Rust bleibt autoritativ; keine vollständige
//     Realm-Request-Verarbeitung).
//
// Coalescing (Zusammenfassen gleichartiger Coalescable-Events) wird in
// dieser Stufe bewusst NICHT umgesetzt (keine erfundenen Merge-Regeln).

use std::collections::VecDeque;

use super::event::ScriptEvent;
use super::host::RealmRequest;
use super::lifecycle::DedicatedContext;

/// QoS-Klasse eines Lua-Events (docs/Lua-Scripting-System.md §12).
///
/// Die Klassenzuordnung wird vom Rust-Eventproduzenten vorgenommen, nicht
/// von Lua.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueClass {
    /// Darf nicht verloren gehen.
    Reliable,
    /// Darf mit gleichartigen Events zusammengefasst werden.
    Coalescable,
    /// Darf unter Last verworfen werden.
    Droppable,
}

impl QueueClass {
    /// Kurzbeschreibung für Log-/Fehlermeldungen.
    pub fn label(self) -> &'static str {
        match self {
            Self::Reliable => "reliable",
            Self::Coalescable => "coalescable",
            Self::Droppable => "droppable",
        }
    }
}

impl std::fmt::Display for QueueClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Ein in die Kaskade eingestelltes Stück Arbeit: Event samt QoS-Klasse.
#[derive(Debug, Clone)]
pub struct LuaWork {
    /// QoS-Klasse dieses Events (vom Rust-Produzenten vergeben).
    pub class: QueueClass,
    /// Das eigentliche Server-Event (Name + Kontext).
    pub event: ScriptEvent,
    /// Dedicated Context-Zuordnung (None = normale Weltarbeit, §8/§12).
    ///
    /// Dedicated Arbeit trägt ihren Context mit sich; der Dispatcher prüft
    /// die Zuordnung vor der Inbox-Zustellung (§10).
    pub dedicated: Option<DedicatedContext>,
}

impl LuaWork {
    /// Neue Arbeit mit Klasse und Event (normale Weltarbeit).
    pub fn new(class: QueueClass, event: ScriptEvent) -> Self {
        Self {
            class,
            event,
            dedicated: None,
        }
    }

    /// Neue Dedicated-Arbeit: Event samt festem Dedicated Context (§10).
    pub fn dedicated(class: QueueClass, event: ScriptEvent, ctx: DedicatedContext) -> Self {
        Self {
            class,
            event,
            dedicated: Some(ctx),
        }
    }
}

/// Die Queue ist voll; der nicht aufgenommene Wert wird zurückgegeben.
#[derive(Debug)]
pub struct QueueFull<T> {
    /// Die Kapazität der betroffenen Queue.
    pub capacity: usize,
    /// Das abgewiesene Element.
    pub item: T,
}

impl<T> QueueFull<T> {
    /// Gibt das abgewiesene Element zurück.
    pub fn into_item(self) -> T {
        self.item
    }
}

/// Generische, begrenzte FIFO-Queue.
///
/// Besitzt keine Default-Kapazität: Die Kapazität wird je Instanz gesetzt
/// (konfigurierbar; es werden in dieser Stufe keine Produktionswerte
/// erfunden). `try_push` lehnt ab, sobald die Kapazität erreicht ist –
/// eine Queue wächst niemals über ihre Kapazität hinaus.
#[derive(Debug, Clone)]
pub struct BoundedQueue<T> {
    capacity: usize,
    items: VecDeque<T>,
}

impl<T> BoundedQueue<T> {
    /// Neue leere Queue mit fester Kapazität.
    ///
    /// # Panics
    /// Eine begrenzte Queue benötigt eine positive Kapazität.
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "bounded queue: Kapazität muss größer als 0 sein"
        );
        Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
        }
    }

    /// Maximale Anzahl gleichzeitig aufgenommener Elemente.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Anzahl der aktuell aufgenommenen Elemente.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true`, wenn keine Elemente aufgenommen sind.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `true`, wenn die Kapazität erreicht ist.
    pub fn is_full(&self) -> bool {
        self.items.len() == self.capacity
    }

    /// Fügt ein Element FIFO ein, sofern Kapazität vorhanden ist.
    ///
    /// Bei voller Queue wird das Element in [`QueueFull`] zurückgegeben;
    /// die Queue wächst dadurch niemals über ihre Kapazität hinaus.
    pub fn try_push(&mut self, item: T) -> Result<(), QueueFull<T>> {
        if self.is_full() {
            return Err(QueueFull {
                capacity: self.capacity,
                item,
            });
        }
        self.items.push_back(item);
        Ok(())
    }

    /// Entnimmt das älteste Element (FIFO).
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    /// Entnimmt alle Elemente (FIFO) und leert die Queue.
    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.items.drain(..)
    }
}

/// Ergebnis eines Einfügens in die Event-Queue (QoS-Politik, §12).
#[derive(Debug)]
pub enum EnqueueOutcome {
    /// Arbeit wurde in die Queue aufgenommen.
    Accepted,
    /// Droppable-Arbeit wurde bei voller Queue bewusst verworfen (erlaubt).
    Dropped,
    /// Arbeit wurde NICHT verworfen, konnte aber nicht aufgenommen werden
    /// (Reliable/Coalescable bei voller Queue): Rückgabe zur geregelten
    /// Weiterverteilung/Behandlung durch den Aufrufer.
    Rejected(LuaWork),
}

/// Begrenzte Event-Queue mit QoS-Politik (docs/Lua-Scripting-System.md §12).
#[derive(Debug, Clone)]
pub struct EventQueue {
    queue: BoundedQueue<LuaWork>,
}

impl EventQueue {
    /// Neue leere Event-Queue mit fester Kapazität.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: BoundedQueue::with_capacity(capacity),
        }
    }

    /// Maximale Anzahl gleichzeitig aufgenommener Events.
    pub fn capacity(&self) -> usize {
        self.queue.capacity()
    }

    /// Anzahl der aktuell aufgenommenen Events.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// `true`, wenn keine Events aufgenommen sind.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// `true`, wenn die Kapazität erreicht ist.
    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }

    /// Fügt Arbeit eingedenk der QoS-Klasse ein.
    ///
    /// * Kapazität vorhanden: immer [`EnqueueOutcome::Accepted`].
    /// * Volle Queue + [`QueueClass::Droppable`]: bewusst verworfen.
    /// * Volle Queue + [`QueueClass::Reliable`]/[`QueueClass::Coalescable`]:
    ///   wird **nicht** verworfen, sondern zurückgegeben
    ///   ([`EnqueueOutcome::Rejected`]).
    pub fn enqueue(&mut self, work: LuaWork) -> EnqueueOutcome {
        match self.queue.try_push(work) {
            Ok(()) => EnqueueOutcome::Accepted,
            Err(full) => {
                let item = full.into_item();
                match item.class {
                    QueueClass::Droppable => EnqueueOutcome::Dropped,
                    QueueClass::Reliable | QueueClass::Coalescable => {
                        EnqueueOutcome::Rejected(item)
                    }
                }
            }
        }
    }

    /// Entnimmt das älteste Event (FIFO).
    pub fn pop(&mut self) -> Option<LuaWork> {
        self.queue.pop()
    }

    /// Entnimmt alle Events (FIFO) und leert die Queue.
    pub fn drain(&mut self) -> impl Iterator<Item = LuaWork> + '_ {
        self.queue.drain()
    }
}

/// Begrenzter technischer Grundbaustein für den Worker -> Realm-Fluss.
///
/// Rust bleibt autoritativ (docs §8): Hier werden die vom Script über
/// `andora.emit/request` gesammelten [`RealmRequest`]s aus einer Queue
/// zum Realm transportiert. Anfragen werden nie wie Droppable verworfen;
/// bei voller Queue wird die Anfrage zurückgegeben.
#[derive(Debug, Clone)]
pub struct RequestQueue {
    queue: BoundedQueue<RealmRequest>,
}

impl RequestQueue {
    /// Neue leere Request-Queue mit fester Kapazität.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: BoundedQueue::with_capacity(capacity),
        }
    }

    /// Maximale Anzahl gleichzeitig aufgenommener Anfragen.
    pub fn capacity(&self) -> usize {
        self.queue.capacity()
    }

    /// Anzahl der aktuell aufgenommenen Anfragen.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// `true`, wenn keine Anfragen aufgenommen sind.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// `true`, wenn die Kapazität erreicht ist.
    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }

    /// Fügt eine Anfrage ein; bei voller Queue wird sie zurückgegeben.
    pub fn try_push(&mut self, request: RealmRequest) -> Result<(), QueueFull<RealmRequest>> {
        self.queue.try_push(request)
    }

    /// Entnimmt die älteste Anfrage (FIFO).
    pub fn pop(&mut self) -> Option<RealmRequest> {
        self.queue.pop()
    }

    /// Entnimmt alle Anfragen (FIFO) und leert die Queue.
    pub fn drain(&mut self) -> impl Iterator<Item = RealmRequest> + '_ {
        self.queue.drain()
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::ScriptContext;
    use super::*;

    fn work(class: QueueClass, name: &str) -> LuaWork {
        LuaWork::new(class, ScriptEvent::new(name, ScriptContext::new()))
    }

    // ── BoundedQueue: Kapazität ────────────────────────────────────

    #[test]
    fn bounded_queue_accepts_up_to_capacity() {
        let mut q: BoundedQueue<u32> = BoundedQueue::with_capacity(3);
        assert!(q.is_empty());
        for i in 0..3 {
            assert!(q.try_push(i).is_ok());
        }
        assert_eq!(q.len(), 3);
        assert!(q.is_full());
        assert_eq!(q.capacity(), 3);
    }

    #[test]
    fn bounded_queue_never_grows_beyond_capacity() {
        let mut q: BoundedQueue<u32> = BoundedQueue::with_capacity(2);
        assert!(q.try_push(1).is_ok());
        assert!(q.try_push(2).is_ok());
        let full = q.try_push(3).unwrap_err();
        assert_eq!(full.capacity, 2);
        let rejected = full.into_item();
        assert_eq!(rejected, 3);
        assert_eq!(q.len(), 2); // kein Wachstum über die Kapazität hinaus
    }

    #[test]
    fn bounded_queue_fifo_order() {
        let mut q: BoundedQueue<u32> = BoundedQueue::with_capacity(3);
        q.try_push(1).unwrap();
        q.try_push(2).unwrap();
        q.try_push(3).unwrap();
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
        assert_eq!(q.pop(), Some(3));
        assert!(q.is_empty());
    }

    #[test]
    fn bounded_queue_drain_empties() {
        let mut q: BoundedQueue<u32> = BoundedQueue::with_capacity(3);
        q.try_push(10).unwrap();
        q.try_push(20).unwrap();
        let drained: Vec<u32> = q.drain().collect();
        assert_eq!(drained, vec![10, 20]);
        assert!(q.is_empty());
    }

    #[test]
    #[should_panic(expected = "Kapazität muss größer als 0 sein")]
    fn bounded_queue_rejects_zero_capacity() {
        let _q: BoundedQueue<u32> = BoundedQueue::with_capacity(0);
    }

    // ── QoS-Klasse bleibt erhalten ─────────────────────────────────

    #[test]
    fn event_queue_class_uniquely_preserved() {
        let mut q = EventQueue::with_capacity(3);
        q.enqueue(work(QueueClass::Reliable, "rely"));
        q.enqueue(work(QueueClass::Coalescable, "coalesce"));
        q.enqueue(work(QueueClass::Droppable, "drop"));

        let first = q.pop().unwrap();
        assert_eq!(first.class, QueueClass::Reliable);
        assert_eq!(first.event.name, "rely");
        let second = q.pop().unwrap();
        assert_eq!(second.class, QueueClass::Coalescable);
        assert_eq!(second.event.name, "coalesce");
        let third = q.pop().unwrap();
        assert_eq!(third.class, QueueClass::Droppable);
        assert_eq!(third.event.name, "drop");
    }

    // ── QoS-Politik bei voller Queue ───────────────────────────────

    #[test]
    fn event_queue_drops_only_droppable_when_full() {
        let mut q = EventQueue::with_capacity(1);
        assert!(matches!(
            q.enqueue(work(QueueClass::Reliable, "first")),
            EnqueueOutcome::Accepted
        ));
        assert!(q.is_full());

        // Droppable: darf verworfen werden -> Queue bleibt unverändert
        match q.enqueue(work(QueueClass::Droppable, "wegwerfbar")) {
            EnqueueOutcome::Dropped => {}
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
        assert_eq!(q.len(), 1);

        // Reliable: wird NICHT verworfen, sondern zurückgegeben
        let kept = work(QueueClass::Reliable, "wichtig");
        match q.enqueue(kept.clone()) {
            EnqueueOutcome::Rejected(item) => {
                assert_eq!(item.class, QueueClass::Reliable);
                assert_eq!(item.event.name, "wichtig");
            }
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn event_queue_rejects_coalescable_when_full() {
        let mut q = EventQueue::with_capacity(1);
        q.enqueue(work(QueueClass::Coalescable, "tick"));
        // Coalescable darf nicht stillschweigend verworfen werden (kein
        // erfundener Merge); volle Queue -> Rejected.
        match q.enqueue(work(QueueClass::Coalescable, "tick2")) {
            EnqueueOutcome::Rejected(item) => assert_eq!(item.class, QueueClass::Coalescable),
            other => panic!("unerwartetes Ergebnis: {other:?}"),
        }
    }

    #[test]
    fn event_queue_accepts_after_pop() {
        let mut q = EventQueue::with_capacity(2);
        q.enqueue(work(QueueClass::Reliable, "a"));
        q.enqueue(work(QueueClass::Reliable, "b"));
        assert!(matches!(
            q.enqueue(work(QueueClass::Reliable, "c")),
            EnqueueOutcome::Rejected(_)
        ));
        q.pop();
        assert!(matches!(
            q.enqueue(work(QueueClass::Reliable, "c")),
            EnqueueOutcome::Accepted
        ));
    }

    // ── RequestQueue: bounded Grundbaustein ────────────────────────

    #[test]
    fn request_queue_bounded_and_never_drops() {
        let mut q = RequestQueue::with_capacity(2);
        q.try_push(request_emit("eins")).unwrap();
        q.try_push(request_emit("zwei")).unwrap();
        assert_eq!(q.len(), 2);

        let full = q.try_push(request_emit("drei")).unwrap_err();
        let rejected = full.into_item();
        assert_eq!(rejected.describe(), "emit('drei')"); // nie verworfen, zurückgegeben
        assert_eq!(q.len(), 2);

        let first = q.pop().unwrap();
        assert_eq!(first.describe(), "emit('eins')");
    }

    fn request_emit(name: &str) -> RealmRequest {
        RealmRequest::Emit {
            name: name.to_string(),
            payload: serde_json::json!({}),
        }
    }
}
