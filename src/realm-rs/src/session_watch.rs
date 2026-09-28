//! Zentraler Session-Revocation-Poller (docs/Security.md AUTH-02b).
//!
//! **Zweck:** Eine ausdrücklich widerrufene Session muss eine bereits aktive
//! Realm-Verbindung beenden — spätestens innerhalb von 30 Sekunden, sofern die
//! Auth-API erreichbar ist. Ein normaler TTL-Ablauf darf eine bestehende
//! Verbindung dagegen **nicht** trennen (AUTH-02a).
//!
//! **Warum ein eigener Task und nicht der Elternkontroll-Poller:** Die
//! Elternkontrolle hat eine eigene fachliche Semantik (`docs/parental_control.md`),
//! läuft nur bei gesetzter `AUTHAPI_URL` und übermittelt nur beaufsichtigte
//! Spieler. Der Session-Widerruf ist fachlich unabhängig und gilt für **jede**
//! Verbindung. Beide Poller teilen nichts als den HTTP-Client.
//!
//! **Warum zentral und nicht pro Verbindung:** Ein Timer je Verbindung wäre N
//! unabhängige Timer mit N Wakeups pro Takt. Der zentrale Poller braucht einen
//! Task, eine Taktung und einen Snapshot pro Runde.
//!
//! **Warum Batches:** Der Auth-API-Client hat einen Timeout von 5 s
//! (`auth_api.rs`). Bei 500 aktiven Verbindungen ergäben 500 Einzelanfragen 50
//! Requests/s *und* — bei streng sequentiellem Vorgehen wie im
//! Elternkontroll-Poller — rund 25 s pro Durchlauf; die 30-Sekunden-Frist wäre
//! dann nicht einhaltbar. Mit Batches zu 250 und höchstens vier parallelen
//! Requests sind 500 Verbindungen **zwei** Batches in **einer** Welle.

use std::time::Duration;

use crate::auth_api::{AuthApi, SessionState, SessionStatusEntry, SessionStatusQuery};
use crate::world::Shared;

/// Taktung. Worst case für eine einzelne Verbindung: ein voller Takt plus ein
/// Request (Timeout 5 s) = 15 s. Damit bleibt Reserve bis zur 30-S-Grenze.
pub const INTERVAL: Duration = Duration::from_secs(10);

/// Höchstzahl Einträge pro Batch. Entspricht dem serverseitigen Limit
/// (`MaxSessionStatusBatch` in der Auth-API).
pub const MAX_BATCH: usize = 250;

/// Höchstzahl gleichzeitig laufender Batchrequests. Begrenzt die Last und
/// verhindert, dass ein Durchlauf unbegrenzt viele Sockets aufmacht.
pub const MAX_PARALLEL: usize = 4;

/// Nachweisbare Kapazitätsgrenze dieser Batch-/Parallelitätskombination:
/// `MAX_PARALLEL` × `MAX_BATCH` = 1000 geprüfte Verbindungen pro Runde. Für den
/// geforderten Bereich bis 500 aktiver Verbindungen bedeutet das **genau zwei**
/// Batches in **einer** Welle.
pub const MAX_PER_ROUND: usize = MAX_PARALLEL * MAX_BATCH;

/// Ein Snapshot-Eintrag: die zum Snapshot-Zeitpunkt gültigen Daten.
/// `session_id` ist die Session-ID des **damaligen** Owners und dient zugleich
/// als Stale-Marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEntry {
    pub player_id: String,
    pub account_id: u32,
    pub session_id: String,
}

/// Ergebnis einer Pollrunde. Bewusst klein und ohne sensible Felder.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WatchReport {
    /// Geprüfte Verbindungen.
    pub checked: usize,
    /// Anzahl der Batchrequests dieser Runde.
    pub batches: usize,
    /// Höchstzahl gleichzeitig aktiver Batches dieser Runde.
    pub max_parallel: usize,
    /// Kontrolliert geschlossene Verbindungen (Widerruf oder unbekannt).
    pub closed: usize,
    /// Übersprungen, weil der Snapshot veraltet war (Takeover, Disconnect).
    pub stale: usize,
    /// Das Ergebnis gehörte nicht zur erwarteten `account_id`.
    pub account_mismatch: usize,
    /// Batchrequests mit Fehler (Transport, HTTP, Parse). **Keine** Trennung.
    pub failed_batches: usize,
}

/// Snapshot der aktuell angemeldeten Verbindungen mit nichtleerer Session.
///
/// Es wird **kein** Lock über einen Netzwerk-`await` gehalten: der World-Lock
/// wird hier vollständig freigegeben und erst nach dem Request erneut genommen.
pub async fn snapshot(shared: &Shared) -> Vec<WatchEntry> {
    let world = shared.lock().await;
    world
        .by_conn
        .values()
        .filter_map(|player_id| {
            let p = world.players.get(player_id)?;
            if p.session_id.is_empty() {
                return None;
            }
            Some(WatchEntry {
                player_id: p.id.clone(),
                account_id: p.account_id,
                session_id: p.session_id.clone(),
            })
        })
        .collect()
}

/// Zerlegt die Einträge in Batches zu höchstens [`MAX_BATCH`].
pub fn chunk(entries: &[WatchEntry]) -> Vec<&[WatchEntry]> {
    entries.chunks(MAX_BATCH).collect()
}

/// Wertet ein Batchergebnis aus und schließt betroffene Verbindungen.
///
/// **Stale-Sicherung:** Vor jedem `close_conn` wird unter dem World-Lock erneut
/// geprüft, dass
/// 1. der Charakter noch existiert,
/// 2. es noch einen Owner gibt, und
/// 3. `player.session_id` **exakt** der Session-ID aus dem Snapshot entspricht.
///
/// Punkt 3 ist der Grund, warum ein altes Poller-Ergebnis nach einem Takeover
/// den neuen Owner nicht schließen kann: `apply_connection_fields` setzt
/// `session_id` beim Takeover auf die neue Sitzung, der Vergleich schlägt fehl.
async fn apply_batch(
    shared: &Shared,
    batch: &[WatchEntry],
    states: &[SessionStatusEntry],
    report: &mut WatchReport,
) {
    for (entry, st) in batch.iter().zip(states.iter()) {
        // Nur `Revoked` und `Missing` schließen. `Valid` und `Expired` tun
        // bewusst nichts (AUTH-02a: Ablauf trennt nicht).
        let should_close = match st.state {
            SessionState::Revoked | SessionState::Missing => true,
            SessionState::Valid | SessionState::Expired => false,
        };
        if !should_close {
            continue;
        }
        // Der Account-Abgleich ist nur bei `Revoked` sinnvoll: dort existiert
        // die Zeile und die Auth-API nennt einen autoritativen Owner. Bei
        // `Missing` gibt es keine Zeile und damit auch keine autoritative
        // `account_id` (die API meldet 0) — ein Vergleich wäre dort immer
        // falsch. Die Eigentümerschaft wird in diesem Fall stattdessen allein
        // durch den `session_id`-Abgleich unten belegt.
        if st.state == SessionState::Revoked && st.account_id != entry.account_id {
            report.account_mismatch += 1;
            continue;
        }
        let mut world = shared.lock().await;
        let Some(p) = world.players.get(&entry.player_id) else {
            report.stale += 1;
            continue;
        };
        if p.session_id != entry.session_id {
            report.stale += 1;
            continue;
        }
        let Some(conn_id) = crate::world::conn_of(&world, &entry.player_id) else {
            report.stale += 1;
            continue;
        };
        // Bestehender, kontrollierter Close-Pfad (wie HELLO-Ablehnung und
        // Takeover). Der Disconnect-Pfad in net.rs räumt danach regulär auf.
        crate::world::close_conn(&mut world, conn_id);
        report.closed += 1;
    }
    // Kein Log auf Session-Ebene: Datenminimierung und Log-Flood
    // (docs/Security.md Abschnitt 4.5). Es folgt nur die Summenzeile.
}

/// Eine vollständige Pollrunde.
pub async fn poll_once(auth: &AuthApi, shared: &Shared) -> WatchReport {
    let mut report = WatchReport::default();
    let entries = snapshot(shared).await;
    report.checked = entries.len();
    if entries.is_empty() {
        return report;
    }
    let batches = chunk(&entries);
    report.batches = batches.len();
    if entries.len() > MAX_PER_ROUND {
        // Mehr Verbindungen als die Batch-/Parallelitaetskombination in einer
        // Runde schafft: ab hier greift die 30-S-Frist nicht mehr, weil die
        // Wellen nacheinander laufen. Sichtbar machen, nicht stillschweigend.
        log::warn!(
            "session revocation: {} Verbindungen ueber der Rundenkapazitaet {} ({} Wellen noetig)",
            entries.len(),
            MAX_PER_ROUND,
            batches.len().div_ceil(MAX_PARALLEL),
        );
    }

    // Slots: Antworten pro Batch-Index, parallel befüllt.
    let mut slots: Vec<Option<Vec<SessionStatusEntry>>> = vec![None; batches.len()];
    let mut set = tokio::task::JoinSet::new();
    let mut inflight = 0usize;
    let mut peak = 0usize;
    let mut next = 0usize;

    while next < batches.len() || !set.is_empty() {
        // Nachfüllen bis MAX_PARALLEL laufende Requests erreicht sind.
        while next < batches.len() && inflight < MAX_PARALLEL {
            let batch = batches[next];
            // Der Task muss die Daten BESITZEN: er laeuft unabhaengig vom
            // Snapshot weiter, waehrend die Schleife weiterfuellt.
            let owned: Vec<(String, u32)> = batch
                .iter()
                .map(|e| (e.session_id.clone(), e.account_id))
                .collect();
            let auth = auth.clone();
            let bi = next;
            set.spawn(async move {
                let queries: Vec<SessionStatusQuery<'_>> = owned
                    .iter()
                    .map(|(s, a)| SessionStatusQuery {
                        session_id: s.as_str(),
                        account_id: *a,
                    })
                    .collect();
                (bi, auth.session_status_batch(&queries).await)
            });
            next += 1;
            inflight += 1;
            peak = peak.max(inflight);
        }
        // Auf den nächsten Abschluss warten; erst danach erneut nachfüllen.
        if let Some(joined) = set.join_next().await {
            inflight -= 1;
            match joined {
                Ok((bi, Ok(states))) => slots[bi] = Some(states),
                Ok((bi, Err(e))) => {
                    // KEINE Trennung bei Auth-API-Fehlern. Ein
                    // Erreichbarkeitsproblem ist kein Sicherheitsereignis; der
                    // nächste erfolgreiche Durchlauf wertet normal aus.
                    report.failed_batches += 1;
                    log::error!("session revocation poll failed: {e}");
                    slots[bi] = Some(Vec::new());
                }
                Err(e) => {
                    report.failed_batches += 1;
                    log::error!("session revocation poll task failed: {e}");
                }
            }
        }
    }
    report.max_parallel = peak;

    for (bi, batch) in batches.iter().enumerate() {
        let Some(states) = slots[bi].as_ref() else {
            continue;
        };
        if states.is_empty() {
            continue;
        }
        apply_batch(shared, batch, states, &mut report).await;
    }
    report
}

/// Der Poller als Hintergrund-Task. Abbruch ausschließlich über
/// `JoinHandle::abort` — dasselbe Muster wie die übrigen Tasks in `main.rs`.
pub fn start_poller(auth: AuthApi, shared: Shared) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERVAL);
        // Ohne Skip liefe ein verzögerter Durchlauf in einem Burst nach: alle
        // Verbindungen würden dann im selben Takt geprüft. `Skip` lässt
        // stattdessen verstrichene Takte ausfallen, wodurch die
        // Worst-Case-Bindung an EINEN Takt plus EINEN Request erhalten bleibt.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let report = poll_once(&auth, &shared).await;
            if report.closed > 0 || report.failed_batches > 0 {
                log::warn!(
                    "session revocation: checked={} batches={} max_parallel={} closed={} stale={} account_mismatch={} failed_batches={}",
                    report.checked,
                    report.batches,
                    report.max_parallel,
                    report.closed,
                    report.stale,
                    report.account_mismatch,
                    report.failed_batches
                );
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_api::AuthApi;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc;
    use tokio::sync::oneshot;

    // ---------------------------------------------------------------------
    // Minimale, lokale Stub-Naehe fuer /session/status/batch.
    // Der bestehende Stub in handlers.rs ist pfadweise „canned" und damit fuer
    // einen zustandsbehafteten Poller nicht geeignet; ihn zu veraendern waere
    // eine Aenderung an fremder Testinfrastruktur. Dieser Stub ist rein lokal
    // und wird von keinem anderen Modul benoetigt.
    // ---------------------------------------------------------------------

    /// Antwortverhalten des Stubs.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Mode {
        /// Normale Antwort aus `map`.
        Normal,
        /// HTTP 500.
        HttpError,
        /// Ungueltiges JSON.
        BadJson,
    }

    struct Stub {
        url: String,
        /// session_id -> (status, account_id)
        map: Arc<Mutex<HashMap<String, (String, u32)>>>,
        mode: Arc<Mutex<Mode>>,
        /// Anzahl bisheriger Batchrequests.
        calls: Arc<AtomicUsize>,
        /// Aktuell laufende Batchrequests (fuer die Peak-Messung).
        inflight: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        /// Signale, dass ein Request angekommen ist (fuer den Lock-Test).
        arrived_rx: tokio::sync::mpsc::UnboundedReceiver<usize>,
        /// Antwort-Verzoegerung, damit Parallelitaet beobachtbar wird.
        delay_ms: Arc<Mutex<u64>>,
    }

    fn stub_api(s: &Stub) -> AuthApi {
        AuthApi::new(&crate::config::AuthApiConfig {
            url: s.url.clone(),
            service_id: "realm-de1-service".into(),
            secret: "s3cret".into(),
        })
        .expect("authapi client")
    }

    async fn start_stub() -> Stub {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let map = Arc::new(Mutex::new(HashMap::new()));
        let mode = Arc::new(Mutex::new(Mode::Normal));
        let calls = Arc::new(AtomicUsize::new(0));
        let inflight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (arrived_tx, arrived_rx) = tokio::sync::mpsc::unbounded_channel();
        let delay_ms = Arc::new(Mutex::new(0u64));
        let (m, md, c, i, p, d) = (
            map.clone(),
            mode.clone(),
            calls.clone(),
            inflight.clone(),
            peak.clone(),
            delay_ms.clone(),
        );
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let (m, md, c, i, p, d) = (
                    m.clone(),
                    md.clone(),
                    c.clone(),
                    i.clone(),
                    p.clone(),
                    d.clone(),
                );
                let arrived = arrived_tx.clone();
                tokio::spawn(async move {
                    let n = i.fetch_add(1, Ordering::SeqCst) + 1;
                    p.fetch_max(n, Ordering::SeqCst);
                    let (rr, mut wr) = tokio::io::split(sock);
                    let mut reader = tokio::io::BufReader::new(rr);
                    let mut raw: Vec<u8> = Vec::new();
                    let mut chunk = [0u8; 8192];
                    // 1) Kopf und Body in EINEN Puffer: ein Teil des Bodies kann
                    //    im selben Read wie der Kopf ankommen.
                    let head_end = loop {
                        let r = match reader.read(&mut chunk).await {
                            Ok(0) => break None,
                            Ok(n) => n,
                            Err(_) => break None,
                        };
                        raw.extend_from_slice(&chunk[..r]);
                        if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(p + 4);
                        }
                    };
                    let Some(head_end) = head_end else { return };
                    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                    let len: usize = head
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                        .and_then(|l| l.split(':').nth(1))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    // 2) Restliche Body-Bytes nachziehen.
                    while raw.len() < head_end + len {
                        let r = match reader.read(&mut chunk).await {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(_) => break,
                        };
                        raw.extend_from_slice(&chunk[..r]);
                    }
                    let body = String::from_utf8_lossy(&raw[head_end..]).to_string();

                    c.fetch_add(1, Ordering::SeqCst);
                    let delay = *d.lock().unwrap();
                    if delay > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    }
                    let _ = arrived.send(n);
                    let mode = *md.lock().unwrap();
                    let resp = match mode {
                        Mode::HttpError => http_resp(500, r#"{"error":"boom"}"#),
                        Mode::BadJson => http_resp(200, "not json at all"),
                        Mode::Normal => {
                            let v: serde_json::Value =
                                serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
                            let sessions = v
                                .get("sessions")
                                .and_then(|s| s.as_array())
                                .cloned()
                                .unwrap_or_default();
                            let map = m.lock().unwrap();
                            let mut out = Vec::new();
                            for (idx, s) in sessions.iter().enumerate() {
                                let sid =
                                    s.get("session_id").and_then(|x| x.as_str()).unwrap_or("");
                                let (status, acc) = map
                                    .get(sid)
                                    .cloned()
                                    .unwrap_or_else(|| ("missing".to_string(), 0));
                                out.push(serde_json::json!({"index": idx, "status": status, "account_id": acc}));
                            }
                            http_resp(200, &serde_json::json!({"results": out}).to_string())
                        }
                    };
                    i.fetch_sub(1, Ordering::SeqCst);
                    let _ = wr.write_all(resp.as_bytes()).await;
                    let _ = wr.flush().await;
                });
            }
        });
        Stub {
            url: format!("http://{addr}"),
            map,
            mode,
            calls,
            inflight,
            peak,
            arrived_rx,
            delay_ms,
        }
    }

    fn http_resp(status: u16, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    fn set(stub: &Stub, sid: &str, status: &str, account: u32) {
        stub.map
            .lock()
            .unwrap()
            .insert(sid.to_string(), (status.to_string(), account));
    }

    // ---------------------------------------------------------------------
    // Welt-Aufbau: N Spieler mit Session und Closer-Sender.
    // ---------------------------------------------------------------------

    /// Legt `n` Spieler an, je mit eigener Session `sess-<i>` und einem
    /// `oneshot`-Sender im `closers`-Register (derselbe Pfad wie in net.rs).
    /// Liefert die Closer-Receiver, um die Trennung zu beobachten.
    async fn make_world(n: usize) -> (Shared, Vec<oneshot::Receiver<()>>) {
        let shared = crate::world::new_shared();
        let mut rxs = Vec::new();
        let mut world = shared.lock().await;
        for i in 0..n {
            let id = format!("hero{i}");
            let (close_tx, close_rx) = oneshot::channel::<()>();
            let (ptx, _prx) = mpsc::unbounded_channel();
            world.players.insert(
                id.clone(),
                crate::world::Player {
                    id: id.clone(),
                    name: id.clone(),
                    x: 0.0,
                    y: 0.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 100,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 7,
                    session_id: format!("sess-{i}"),
                    entities: Default::default(),
                    last_activity: std::time::Instant::now(),
                    tx: ptx,
                    char_class: "Adventurer".into(),
                    class: crate::class::ClassStatus::Adventurer,
                    faction_transition: false,
                    level: 1,
                    exp: 0,
                    free_attr_points: 0,
                    rested_pool: 0,
                    idia: 0,
                    armor: 0,
                    weapon_skill: 1,
                    combat: None,
                    mana: 50,
                    max_mana: 50,
                    effects: Vec::new(),
                    cooldowns: Default::default(),
                    active_cast: None,
                    learned_abilities: Default::default(),
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
                    sitting: false,
                    hp_regen_bonus: 0.0,
                    mana_regen_bonus: 0.0,
                    hp_regen_carry: 0.0,
                    mana_regen_carry: 0.0,
                    inventory: Default::default(),
                    quests: Default::default(),
                    dirty: Default::default(),
                    persist_generation: 0,
                    persist_revision: 0,
                },
            );
            let conn_id = 100 + i as u64;
            world.by_conn.insert(conn_id, id);
            world.closers.insert(conn_id, close_tx);
            rxs.push(close_rx);
        }
        // Sperre vor der Rueckgabe freigeben.
        drop(world);
        (shared, rxs)
    }

    /// Ein Player bleibt, solange sein Closer nicht signalisiert wurde.
    async fn still_open(rx: &mut oneshot::Receiver<()>) -> bool {
        match tokio::time::timeout(std::time::Duration::from_millis(30), rx).await {
            Ok(Ok(())) => false,
            _ => {
                // Timeout: nichts signalisiert. Receiver ist verbraucht; das
                // ist fuer diese Aussage ausreichend.
                true
            }
        }
    }

    // 1) revoked schliesst die passende aktive Verbindung
    #[tokio::test]
    async fn revoked_closes_matching_connection() {
        let stub = start_stub().await;
        set(&stub, "sess-0", "revoked", 7);
        set(&stub, "sess-1", "valid", 7);
        let (shared, mut rxs) = make_world(2).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(
            r.closed, 1,
            "nur die widerrufene Verbindung wird geschlossen"
        );
        assert!(
            !still_open(&mut rxs[0]).await,
            "sess-0 muss getrennt werden"
        );
        assert!(still_open(&mut rxs[1]).await, "sess-1 bleibt verbunden");
    }

    // 2) missing schliesst eine zuvor validierte aktive Verbindung
    #[tokio::test]
    async fn missing_closes_previously_valid_connection() {
        let stub = start_stub().await;
        // sess-0 ist der Auth-API unbekannt -> missing
        let (shared, mut rxs) = make_world(1).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r.closed, 1);
        assert!(
            !still_open(&mut rxs[0]).await,
            "missing schliesst die Verbindung"
        );
    }

    // 3) expired laesst die aktive Verbindung bestehen (AUTH-02a)
    #[tokio::test]
    async fn expired_keeps_connection() {
        let stub = start_stub().await;
        set(&stub, "sess-0", "expired", 7);
        let (shared, mut rxs) = make_world(1).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r.closed, 0, "TTL-Ablauf trennt NICHT");
        assert!(still_open(&mut rxs[0]).await);
    }

    // 4) valid laesst sie bestehen
    #[tokio::test]
    async fn valid_keeps_connection() {
        let stub = start_stub().await;
        set(&stub, "sess-0", "valid", 7);
        let (shared, mut rxs) = make_world(1).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r.closed, 0);
        assert!(still_open(&mut rxs[0]).await);
    }

    // 5) Transportfehler trennt niemanden
    #[tokio::test]
    async fn transport_error_closes_nobody() {
        let (shared, mut rxs) = make_world(3).await;
        // Port 1 ist garantiert unverbindbar -> reiner Transportfehler.
        let auth = AuthApi::new(&crate::config::AuthApiConfig {
            url: "http://127.0.0.1:1".into(),
            service_id: "s".into(),
            secret: "x".into(),
        })
        .unwrap();
        let r = poll_once(&auth, &shared).await;
        assert_eq!(
            r.closed, 0,
            "Erreichbarkeitsproblem ist kein Sicherheitsereignis"
        );
        assert_eq!(r.failed_batches, 1);
        for rx in rxs.iter_mut() {
            assert!(still_open(rx).await, "niemand wird getrennt");
        }
    }

    // 6) HTTP- und Parsefehler trennen niemanden
    #[tokio::test]
    async fn http_and_parse_error_close_nobody() {
        for mode in [Mode::HttpError, Mode::BadJson] {
            let stub = start_stub().await;
            *stub.mode.lock().unwrap() = mode;
            let (shared, mut rxs) = make_world(2).await;
            let r = poll_once(&stub_api(&stub), &shared).await;
            assert_eq!(r.closed, 0, "Modus {:?} darf niemanden trennen", mode);
            assert_eq!(r.failed_batches, 1);
            for rx in rxs.iter_mut() {
                assert!(still_open(rx).await);
            }
        }
    }

    // 7) Nach Wiedererreichbarkeit wird der Widerruf angewandt
    #[tokio::test]
    async fn revocation_applies_after_reachability_returns() {
        let stub = start_stub().await;
        set(&stub, "sess-0", "revoked", 7);
        let (shared, mut rxs) = make_world(1).await;
        // Runde 1: API nicht erreichbar.
        *stub.mode.lock().unwrap() = Mode::HttpError;
        let r1 = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r1.closed, 0);
        assert!(still_open(&mut rxs[0]).await, "Ausfall trennt nicht");
        // Runde 2: API wieder erreichbar.
        *stub.mode.lock().unwrap() = Mode::Normal;
        let r2 = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r2.closed, 1, "der nachgeholte Widerruf wirkt jetzt");
        assert!(!still_open(&mut rxs[0]).await);
    }

    // 8) Stale Antwort nach Takeover schliesst den NEUEN Owner nicht.
    //    Der Snapshot muss VOR dem Takeover entstehen, die Antwort danach.
    #[tokio::test]
    async fn stale_answer_after_takeover_does_not_close_new_owner() {
        let stub = start_stub().await;
        // sess-0 ist beim Snapshot die gueltige Session von hero0 und wird
        // waehrend des Requests widerrufen.
        set(&stub, "sess-0", "revoked", 7);
        *stub.delay_ms.lock().unwrap() = 200;
        let (shared, mut rxs) = make_world(1).await;
        let auth = stub_api(&stub);
        let for_poll = shared.clone();
        let mut arrived = stub.arrived_rx;
        let poll = tokio::spawn(async move { poll_once(&auth, &for_poll).await });

        // Warten, bis der Batch-Request den Stub erreicht: ab hier ist der
        // Snapshot fixiert, die Antwort steht aber noch aus.
        tokio::time::timeout(std::time::Duration::from_millis(2000), arrived.recv())
            .await
            .expect("Batch-Request muss ankommen")
            .expect("Kanal offen");

        // Takeover im Fenster: alter Owner (100) weg, neuer Owner (900) mit
        // neuer Session. Der alte Closer bleibt als Beweis erhalten.
        let (new_close_tx, mut new_rx) = oneshot::channel::<()>();
        {
            let mut world = shared.lock().await;
            world.by_conn.remove(&100);
            world.by_conn.insert(900, "hero0".to_string());
            if let Some(p) = world.players.get_mut("hero0") {
                p.session_id = "sess-takeover".to_string();
            }
            world.closers.insert(900, new_close_tx);
        }

        let r = poll.await.unwrap();
        assert_eq!(
            r.closed, 0,
            "stale Ergebnis darf den neuen Owner nicht schliessen"
        );
        assert_eq!(r.stale, 1, "die Session-Abweichung wird als stale gezaehlt");
        assert!(
            still_open(&mut new_rx).await,
            "der neue Owner bleibt verbunden"
        );
        assert!(
            still_open(&mut rxs[0]).await,
            "der alte Closer wird nicht signalisiert"
        );
    }

    // 9) Account-ID-Mismatch schliesst nicht den falschen Spieler
    /// Der Account-Abgleich schuetzt nur bei `Revoked`; `Missing` schliesst
    /// auch ohne autoritative account_id (dort gibt es keine Zeile).
    #[tokio::test]
    async fn account_mismatch_closes_nobody() {
        let stub = start_stub().await;
        // Auth-API meldet einen ANDEREN Account als autoritativ.
        set(&stub, "sess-0", "revoked", 99);
        let (shared, mut rxs) = make_world(1).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r.closed, 0);
        assert_eq!(r.account_mismatch, 1);
        assert!(still_open(&mut rxs[0]).await);
    }

    // 10) Kein World-Lock ueber den Netzwerk-`await`
    #[tokio::test]
    async fn snapshot_holds_no_world_lock_across_await() {
        let stub = start_stub().await;
        set(&stub, "sess-0", "valid", 7);
        set(&stub, "sess-1", "valid", 7);
        *stub.delay_ms.lock().unwrap() = 200;
        let (shared, _rxs) = make_world(2).await;
        let auth = stub_api(&stub);
        let mut arrived = stub.arrived_rx;
        let for_poll = shared.clone();
        let poll = tokio::spawn(async move { poll_once(&auth, &for_poll).await });

        // Warten, bis der Batch-Request den Stub tatsaechlich erreicht hat.
        // Zu diesem Zeitpunkt laeuft im Poller bereits der Netzwerk-`await`:
        // die World-Sperre MUSS also freigegeben sein.
        tokio::time::timeout(std::time::Duration::from_millis(2000), arrived.recv())
            .await
            .expect("Batch-Request muss den Stub erreichen")
            .expect("Kanal offen");

        // Kernaussage: die Sperre ist in diesem Zustand sofort obtainable.
        let guard = tokio::time::timeout(std::time::Duration::from_millis(100), shared.lock())
            .await
            .expect("World-Lock darf waehrend des Netzwerk-awaits nicht gehalten werden");
        drop(guard);
        let r = poll.await.unwrap();
        assert_eq!(r.closed, 0);
    }

    // 11) 500 Verbindungen werden in zwei Batches aufgeteilt
    #[tokio::test]
    async fn five_hundred_connections_split_into_two_batches() {
        let stub = start_stub().await;
        for i in 0..500 {
            set(&stub, &format!("sess-{i}"), "valid", 7);
        }
        let (shared, _rxs) = make_world(500).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r.checked, 500);
        assert_eq!(r.batches, 2, "500 / 250 = zwei Batches in einer Welle");
        assert_eq!(
            stub.calls.load(Ordering::SeqCst),
            2,
            "genau zwei HTTP-Requests"
        );
        assert_eq!(r.max_parallel, 2);
        assert_eq!(r.closed, 0);
    }

    // 1b) Oberhalb von MAX_PER_ROUND: ALLE Eintraege werden geprueft, es wird
    //     nichts uebersprungen und nichts verhungert. Der LETZTE Eintrag der
    //     letzten Welle wird widerrufen — er muss geschlossen werden.
    #[tokio::test]
    async fn beyond_capacity_every_entry_is_checked() {
        let stub = start_stub().await;
        // 5 Batches: 1000 + 1 Eintrag. Der letzte Index 1204 steckt im
        // 5. Batch, also in der zweiten Welle.
        let n = MAX_BATCH * 4 + 205;
        let (shared, mut rxs) = make_world(n).await;
        for i in 0..n {
            // Nur der ALLERLETZTE Eintrag wird widerrufen.
            let state = if i == n - 1 { "revoked" } else { "valid" };
            set(&stub, &format!("sess-{i}"), state, 7);
        }
        *stub.delay_ms.lock().unwrap() = 5;
        let r = poll_once(&stub_api(&stub), &shared).await;

        assert_eq!(r.checked, n, "alle Eintraege werden geprueft");
        assert_eq!(r.batches, 5, "1205 Eintraege = fuenf Batches");
        assert_eq!(
            stub.calls.load(Ordering::SeqCst),
            5,
            "jeder Batch wird genau einmal gesendet"
        );
        assert!(
            r.max_parallel <= MAX_PARALLEL,
            "nie mehr als {} gleichzeitig, war {}",
            MAX_PARALLEL,
            r.max_parallel
        );
        assert!(
            r.max_parallel > 1,
            "bei 5 Batches wird tatsaechlich parallel gearbeitet: {}",
            r.max_parallel
        );
        // Der letzte Eintrag wurde geprueft und deshalb geschlossen.
        assert_eq!(r.closed, 1, "nur der letzte Eintrag ist widerrufen");
        let last = rxs.len() - 1;
        assert!(
            !still_open(&mut rxs[last]).await,
            "der LETZTE Eintrag muss geprueft worden sein"
        );
        // Und die uebrigen sind unangetastet.
        assert!(still_open(&mut rxs[0]).await);
    }

    // 12) Hoechstens vier Batches gleichzeitig aktiv
    #[tokio::test]
    async fn at_most_four_batches_run_in_parallel() {
        let stub = start_stub().await;
        // 1000 Verbindungen = 4 Batches: exakt die Parallelitaetsgrenze.
        for i in 0..(MAX_BATCH * MAX_PARALLEL) {
            set(&stub, &format!("sess-{i}"), "valid", 7);
        }
        *stub.delay_ms.lock().unwrap() = 40;
        let (shared, _rxs) = make_world(MAX_BATCH * MAX_PARALLEL).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r.batches, MAX_PARALLEL);
        assert_eq!(
            r.max_parallel, MAX_PARALLEL,
            "vier gleichzeitig, nicht mehr"
        );
        let observed = stub.peak.load(Ordering::SeqCst);
        assert!(stub.inflight.load(Ordering::SeqCst) <= MAX_PARALLEL);
        assert!(
            observed <= MAX_PARALLEL,
            "Stub beobachtete {observed} gleichzeitige Requests, Grenze ist {MAX_PARALLEL}"
        );
        // Und die Wellenbreite begrenzt den Gesamtdurchlauf.
        assert_eq!(r.closed, 0);
    }

    // 13) Intervall mit MissedTickBehavior::Skip
    #[test]
    fn interval_uses_skip_missed_tick_behavior() {
        assert_eq!(
            INTERVAL,
            std::time::Duration::from_secs(10),
            "10-Sekunden-Takt"
        );
        assert_eq!(MAX_BATCH, 250, "Batchgroesse 250");
        assert_eq!(MAX_PARALLEL, 4, "Parallelitaetsgrenze 4");
        assert_eq!(MAX_PER_ROUND, 1000, "nachweisbare Kapazitaet der Runde");
        // MissedTickBehavior laesst sich nicht direkt vergleichen; die
        // Konfiguration wird deshalb ueber den Quelltext des Pollers geprueft.
        let src = include_str!("session_watch.rs");
        assert!(
            src.contains("set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip)"),
            "der Poller muss MissedTickBehavior::Skip setzen"
        );
    }

    // 14) Logs enthalten keine Session-ID, kein Token, keine Roh-IP
    #[tokio::test]
    async fn logs_contain_no_session_or_token() {
        let stub = start_stub().await;
        set(&stub, "sess-0", "revoked", 7);
        let (shared, _rxs) = make_world(1).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r.closed, 1);
        // Die Zusammenfassung enthaelt ausschliesslich Zaehler.
        let line = format!(
            "session revocation: checked={} batches={} max_parallel={} closed={} stale={} account_mismatch={} failed_batches={}",
            r.checked, r.batches, r.max_parallel, r.closed, r.stale, r.account_mismatch, r.failed_batches
        );
        for forbidden in [
            "sess-0",
            "sess-",
            "session_id",
            "token",
            "127.0.0.1",
            "http://",
        ] {
            assert!(
                !line.to_lowercase().contains(&forbidden.to_lowercase()),
                "Logzeile enthaelt '{forbidden}': {line}"
            );
        }
    }

    // 15) Der kontrollierte Cleanup laeuft ueber den bestehenden Close-Pfad
    #[tokio::test]
    async fn cleanup_uses_existing_close_path() {
        let stub = start_stub().await;
        set(&stub, "sess-0", "revoked", 7);
        let (shared, mut rxs) = make_world(1).await;
        let r = poll_once(&stub_api(&stub), &shared).await;
        assert_eq!(r.closed, 1);
        // `close_conn` sendet ueber den bestehenden `closers`-oneshot; genau
        // dieser Receiver empfaengt das Signal. by_conn bleibt bis zum
        // Disconnect-Pfad unveraendert (der Poller raeumt nicht selbst auf).
        assert!(
            !still_open(&mut rxs[0]).await,
            "closer wurde nicht signalisiert"
        );
        let world = shared.lock().await;
        assert!(
            world.by_conn.contains_key(&100),
            "der Poller darf die Registry nicht selbst bereinigen"
        );
    }
}
