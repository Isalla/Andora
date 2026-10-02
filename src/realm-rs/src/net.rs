// net — WebSocket-Server + Message-Dispatcher (Port von src/realm net.ts).
// Drahtformat {seq, type, data} als JSON. Ungültiges JSON wird ignoriert
// (kein Crash). Unbekannte Typen werden geloggt. Disconnect: Parental-
// State abräumen, Position speichern, DESPAWN-Broadcast, Registry putzen.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use futures_util::{SinkExt, Stream, StreamExt};
use sqlx::{MySql, Pool};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{self, Message};

use crate::auth_api::AuthApi;
use crate::config::Config;
use crate::db;
use crate::group::SharedGroups;
use crate::handlers::{self, Ctx};
use crate::parental::{self, SharedParental};

// ---- Begrenzter Retry für den direkten `logout_at`-Write ----
// Norm: docs/Player_Persistenz.md §11 (Disconnect) und §30 (Shutdown),
// docs/Security.md `P-27`. Der Write ist der EINZIGE Ort, an dem ein
// Logout-Zeitpunkt entsteht (Spool/Recovery schreiben ihn nicht, und
// `mark_dirty` ist ausdrücklich kein Retry dafür). Alle Werte sind im
// Dokument festgelegt; sie werden hier nicht nachjustiert.

/// Zahl der Gesamtversuche einschließlich des Erstversuchs.
pub(crate) const LOGOUT_MAX_ATTEMPTS: u32 = 3;
/// Obergrenze für **einen** DB-Schreibvorgang. Der Produktionspool setzt
/// keinen eigenen `acquire_timeout`, es gilt der sqlx-Default von 30 s
/// (`db.rs:78`); ohne diese Grenze würde ein Versuch den Login am
/// per-player-Gate unerträglich lange blockieren.
pub(crate) const LOGOUT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Gesamtbudget des Disconnect-Retryblocks. Begrenzt die **vollständige**
/// Operation (alle Wartezeiten plus alle Schreibversuche), nicht nur einzelne
/// Aufrufe.
pub(crate) const LOGOUT_RETRY_BUDGET: Duration = Duration::from_secs(5);
/// Wartezeiten zwischen den Versuchen (Index = Versuchsnummern minus 1).
pub(crate) const LOGOUT_RETRY_WAITS: [Duration; 2] =
    [Duration::from_millis(200), Duration::from_millis(400)];
/// Globales Budget der direkten `logout_at`-Phase beim Graceful Shutdown
/// (docs/Player_Persistenz.md §30). Gilt für **alle** Charaktere zusammen.
pub(crate) const SHUTDOWN_LOGOUT_BUDGET: Duration = Duration::from_secs(30);

/// Plan eines begrenzten Retry-Blocks. Als eigener Typ, damit die
/// Festlegungen an einer Stelle stehen und Tests mit eigenen Werten laufen
/// können, ohne die Produktionswerte zu verändern.
#[derive(Clone, Copy)]
pub(crate) struct LogoutRetryPlan {
    pub max_attempts: u32,
    pub write_timeout: Duration,
    pub waits: &'static [Duration],
    pub budget: Duration,
}

/// Kürzt einen Retry-Plan auf das **verbleibende** globale Budget.
///
/// `None` bedeutet: die Deadline ist abgelaufen, es darf kein weiterer
/// `logout_at`-Write gestartet werden. Sonst werden Gesamtbudget und
/// Einzel-Timeout auf das Restbudget gekürzt, damit ein bereits gestarteter
/// Write **niemals** über die harte Shutdown-Deadline hinausläuft.
/// Reine Funktion, damit die Kürzung ohne echte Wartezeit prüfbar ist.
pub(crate) fn plan_within_budget(
    plan: LogoutRetryPlan,
    remaining: Duration,
) -> Option<LogoutRetryPlan> {
    if remaining.is_zero() {
        return None;
    }
    Some(LogoutRetryPlan {
        max_attempts: plan.max_attempts,
        write_timeout: plan.write_timeout.min(remaining),
        waits: plan.waits,
        budget: plan.budget.min(remaining),
    })
}

/// Produktionsplan: drei Versuche, 2 s je Write, 200/400 ms Wartezeit,
/// 5 s Gesamtbudget (Disconnect).
pub(crate) const LOGOUT_RETRY_PLAN: LogoutRetryPlan = LogoutRetryPlan {
    max_attempts: LOGOUT_MAX_ATTEMPTS,
    write_timeout: LOGOUT_WRITE_TIMEOUT,
    waits: &LOGOUT_RETRY_WAITS,
    budget: LOGOUT_RETRY_BUDGET,
};

/// Fehlerklasse des direkten `logout_at`-Writes. Es werden ausschließlich
/// diese beiden Klassen unterschieden und **niemals** die rohe DB-Meldung
/// geloggt: sie kann Verbindungs- oder Zugangsdaten enthalten.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LogoutErrorClass {
    /// Der Schreibvorgang hat das Zeitlimit überschritten.
    Timeout,
    /// Der Schreibvorgang wurde mit einem Fehler abgeschlossen.
    WriteError,
}

impl LogoutErrorClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::WriteError => "write_error",
        }
    }
}

/// Auslöser des Retry-Blocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LogoutContext {
    Disconnect,
    Shutdown,
}

impl LogoutContext {
    fn as_str(self) -> &'static str {
        match self {
            Self::Disconnect => "disconnect",
            Self::Shutdown => "shutdown",
        }
    }
}

/// Sanitisiertes, strukturiertes Logformat für den `logout_at`-Write.
///
/// Erlaubt sind ausschließlich `event`, `char_id`, `attempt`, `max_attempts`,
/// `context` und `error_class`. Bewusst **nicht** enthalten: Session-ID,
/// Handoff- oder andere Tokens, Passwort, Roh-IP, `conn_id`, Datenbank-URL und
/// die rohe DB-Fehlermeldung.
pub(crate) fn logout_log_line(
    event: &str,
    char_id: &str,
    attempt: u32,
    context: LogoutContext,
    class: LogoutErrorClass,
) -> String {
    format!(
        "{event} char_id={char_id} attempt={attempt} max_attempts={} context={} error_class={}",
        LOGOUT_MAX_ATTEMPTS,
        context.as_str(),
        class.as_str()
    )
}

/// Ergebnis eines begrenzten `logout_at`-Retry-Blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LogoutRetry {
    /// Der Write ist erfolgreich geschrieben.
    pub ok: bool,
    /// Ausgeführte Versuche (Erstversuch = 1).
    pub attempts: u32,
    /// Mindestens ein Wiederholungsversuch war nötig.
    pub retried: bool,
    /// Fehlerklasse des letzten Fehlversuchs; `None` bei Erfolg.
    pub class: Option<LogoutErrorClass>,
    /// Das Gesamtbudget wurde erschöpft, bevor alle Versuche liefen.
    pub budget_exhausted: bool,
}

/// Wartefunktion zwischen zwei Versuchen. Produktion: `tokio::time::sleep`.
/// Tests: sofortiges Zurückkehren (es gibt keine pausierte Tokio-Zeit im
/// Projekt, siehe Testmodul).
type LogoutWait<'a> = &'a (dyn Fn(Duration) -> BoxFuture<'static, ()> + Send + Sync);

/// Führt den direkten `logout_at`-Write mit begrenztem Retry aus.
///
/// Invarianten:
/// * `logout_at` wird **nicht** neu bestimmt; jeder Versuch verwendet exakt
///   denselben Wert.
/// * Das Gesamtbudget begrenzt die vollständige Operation: ein neuer Versuch
///   startet nur, wenn die bereits verstrichene Zeit zuzüglich des
///   Write-Zeitlimits noch in das Budget passt. Da jeder Versuch zusätzlich
///   auf `write_timeout` begrenzt ist, kann die Operation das Budget nicht
///   überschreiten.
/// * Die rohe Fehlermeldung des Writes wird **nicht** in das strukturierte Log
///   übernommen; sie verlässt den Block nicht.
pub(crate) async fn write_logout_with_retry(
    char_id: &str,
    write: &(dyn Fn(i64) -> BoxFuture<'static, Result<(), String>> + Send + Sync),
    logout_at: i64,
    context: LogoutContext,
    plan: LogoutRetryPlan,
    wait: LogoutWait<'_>,
) -> LogoutRetry {
    let started = Instant::now();
    let mut attempts: u32 = 0;
    loop {
        attempts += 1;
        let class = match tokio::time::timeout(plan.write_timeout, write(logout_at)).await {
            Ok(Ok(())) => {
                return LogoutRetry {
                    ok: true,
                    attempts,
                    retried: attempts > 1,
                    class: None,
                    budget_exhausted: false,
                }
            }
            // Die rohe Meldung wird bewusst verworfen und nur klassifiziert.
            Ok(Err(_)) => LogoutErrorClass::WriteError,
            Err(_elapsed) => LogoutErrorClass::Timeout,
        };
        // Ein weiterer Versuch startet nur, wenn er noch vollständig in das
        // Gesamtbudget passt.
        let max_reached = attempts >= plan.max_attempts;
        let wait_available = plan.waits.get((attempts - 1) as usize).is_some();
        let budget_left = started.elapsed() + plan.write_timeout <= plan.budget;
        if max_reached || !wait_available || !budget_left {
            return LogoutRetry {
                ok: false,
                attempts,
                retried: attempts > 1,
                class: Some(class),
                // `budget_exhausted` benennt **nur** das Budget als Auslöser,
                // nicht die maximale Versuchszahl.
                budget_exhausted: !budget_left,
            };
        }
        log::warn!(
            "{}",
            logout_log_line("logout_at_retry", char_id, attempts, context, class)
        );
        wait(plan.waits[(attempts - 1) as usize]).await;
    }
}

use crate::protocol::{c2s, Frame};
use crate::world::{close_conn, Shared};

/// Ergebnis der direkten Logoutphase beim Graceful Shutdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShutdownLogoutReport {
    /// Charaktere, für die mindestens ein Wiederholungsversuch nötig war.
    pub retried: u32,
    /// Charaktere, deren `logout_at` nach Versuchen und Budget **nicht**
    /// geschrieben werden konnte.
    pub failed: u32,
    /// Charaktere, für die wegen erschöpftem Budget **kein** DB-Versuch
    /// gestartet wurde.
    pub skipped: u32,
    /// Das globale Budget wurde erschöpft.
    pub budget_exhausted: bool,
}

/// Direkte `logout_at`-Phase des Graceful Shutdowns
/// (docs/Player_Persistenz.md §30; docs/Security.md `P-27`).
///
/// Das Budget gilt **global für alle Charaktere**. Ist es erschöpft, startet für
/// die verbleibenden Charaktere kein weiterer DB-Aufruf; sie zählen als
/// `skipped`. Pro Charakter wird der Logout-Zeitpunkt einmal bestimmt und über
/// alle Versuche beibehalten.
///
/// Die Zähler des Abschlussberichts sind der maßgebliche Nachweis; rohe
/// DB-Fehlermeldungen gelangen nicht in die Ausgabe.
pub(crate) async fn shutdown_logout_phase(
    persist: &crate::spool::PersistRuntime,
    shared: &crate::world::Shared,
    online: &[String],
    plan: LogoutRetryPlan,
    budget: Duration,
    wait: LogoutWait<'_>,
    make_write: &dyn Fn(&str) -> DisconnectLogout,
) -> ShutdownLogoutReport {
    let deadline = Instant::now() + budget;
    let mut report = ShutdownLogoutReport {
        retried: 0,
        failed: 0,
        skipped: 0,
        budget_exhausted: false,
    };
    for id in online {
        // (1) Deadline-Prüfung **zuerst**, noch vor jedem weiteren Schritt: ist
        // die harte Deadline abgelaufen, startet für diesen Charakter **kein**
        // weiterer direkter `logout_at`-Write.
        let expired = deadline.saturating_duration_since(Instant::now()).is_zero();
        // (2) Der bestehende finale Spool-Save läuft unverändert für **alle**
        // Spieler weiter; seine Semantik wird nicht verändert — auch nicht für
        // Charaktere, deren direkter Logout-Write ausfällt. Er ist ein lokaler
        // Dateischreibvorgang **ohne DB-Zugriff**, steht aber außerhalb des
        // 30-S-Budgets und kann es daher rechnerisch überschreiten.
        if let Err(e) = persist.persist_player(shared, id, true).await {
            log::error!("shutdown persist {id}: {e}");
        }
        // (3)+(4) Der Logout-Zeitpunkt wird **einmal** bestimmt und über alle
        // Versuche dieses Charakters beibehalten. Maßgeblich ist das **frisch**
        // gemessene Restbudget, weil `persist_player` Zeit benötigt; der Write
        // erhält es statt seines vollen Disconnect-Budgets. Damit kann ein
        // bereits gestarteter Write die harte Deadline nicht überschreiten.
        let remaining = deadline.saturating_duration_since(Instant::now());
        let capped = plan_within_budget(plan, remaining);
        let capped = match capped {
            Some(c) if !expired => c,
            _ => {
                report.skipped += 1;
                report.budget_exhausted = true;
                continue;
            }
        };
        let logout_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let write = make_write(id);
        let retry =
            write_logout_with_retry(id, &write, logout_at, LogoutContext::Shutdown, capped, wait)
                .await;
        if retry.retried {
            report.retried += 1;
        }
        if !retry.ok {
            report.failed += 1;
            log::error!(
                "{}",
                logout_log_line(
                    "logout_at_failed",
                    id,
                    retry.attempts,
                    LogoutContext::Shutdown,
                    retry.class.unwrap_or(LogoutErrorClass::WriteError),
                )
            );
        } else if retry.retried {
            log::info!(
                "{}",
                logout_log_line(
                    "logout_at_recovered",
                    id,
                    retry.attempts,
                    LogoutContext::Shutdown,
                    retry.class.unwrap_or(LogoutErrorClass::WriteError),
                )
            );
        }
    }
    report
}

/// Finaler Disconnect-Save (Stufe B, `force`) als Effekt. `Err` bedeutet:
/// Snapshot nicht durable geschrieben — der Player bleibt dann im RAM (§16).
type DisconnectFlush = Box<dyn FnOnce() -> BoxFuture<'static, Result<(), String>> + Send>;
/// `logout_at`-Write (docs/Player_Persistenz.md §11/§23) als Effekt.
/// Liefert das Ergebnis **durch**: der begrenzte Retry-Block in
/// `finish_owner` wertet es aus, die Closure selbst loggt nichts.
pub(crate) type DisconnectLogout =
    Box<dyn Fn(i64) -> BoxFuture<'static, Result<(), String>> + Send + Sync>;

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub async fn serve(
    cfg: Arc<Config>,
    db: Pool<MySql>,
    auth: AuthApi,
    shared: Shared,
    parental: SharedParental,
    groups: SharedGroups,
    persist: Arc<crate::spool::PersistRuntime>,
) -> Result<(), String> {
    let addrs = crate::config::bind_addrs(&cfg.ws_bind_host, cfg.ws_port)?;
    // Ability-Registry aus Content-Schicht laden (Migration 010).
    // Ein Ladefehler bricht den Start ab (kein halber Realm).
    let mut registry = crate::combat::ability::AbilityRegistry::new();
    let defs = db::load_ability_definitions(&db).await?;
    for row in defs {
        registry.register(crate::combat::ability::build_ability_def(&row));
    }
    log::info!("{} Ability-Definitionen geladen", registry.defs.len());
    let ctx = Arc::new(Ctx {
        cfg,
        db,
        auth,
        shared,
        parental,
        registry,
        groups,
        quest: crate::quest::QuestService::new(),
        persist,
    });
    let mut listeners = Vec::with_capacity(addrs.len());
    for addr in &addrs {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("websocket listen {addr}: {e}"))?;
        log::info!("websocket on {addr}");
        listeners.push(listener);
    }
    let mut tasks: Vec<tokio::task::JoinHandle<Result<(), String>>> =
        Vec::with_capacity(listeners.len());
    for listener in listeners {
        let ctx = ctx.clone();
        tasks.push(tokio::spawn(
            async move { accept_loop(listener, ctx).await },
        ));
    }
    // Erster Fehler beendet den Server; alle Listener-Tasks werden gestoppt.
    let (res, _, rest) = futures_util::future::select_all(tasks).await;
    for t in rest {
        t.abort();
    }
    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("websocket task: {e}")),
    }
}

async fn accept_loop(listener: tokio::net::TcpListener, ctx: Arc<Ctx>) -> Result<(), String> {
    loop {
        // Die direkte TCP-Peer-Adresse wird NICHT mehr verworfen: sie wird als
        // RAM-Angabe an der Connection geführt (`World.peer_addrs`,
        // docs/netzwerk_ip_schutz.md). Bewusst ohne Protokollierung — die
        // dauerhafte Takeover-IP-Protokollierung mit Löschfrist ist ein
        // eigener Auftrag (AUTH-03B). Es wird KEIN Reverse-Proxy-/
        // X-Forwarded-For-Vertrauen angenommen: es zählt der TCP-Peer.
        let (sock, peer) = listener
            .accept()
            .await
            .map_err(|e| format!("websocket accept: {e}"))?;
        let peer_addr = peer.ip().to_string();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(ctx, sock, peer_addr).await {
                log::error!("connection: {e}");
            }
        });
    }
}

async fn handle_conn(
    ctx: Arc<Ctx>,
    sock: tokio::net::TcpStream,
    peer_addr: String,
) -> Result<(), String> {
    let ws = tokio_tungstenite::accept_async(sock)
        .await
        .map_err(|e| format!("ws handshake: {e}"))?;
    log::info!("client connected");
    let conn_id = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    // V1-Schutzschicht je Verbindung (Rate-Fenster, Auffälligkeiten, Seq).
    let mut guard = crate::security::ConnGuard::default();
    let (mut sink, stream) = ws.split();
    // Spielzustand -> Socket läuft über einen Kanal (siehe world.rs);
    // gezieltes Schließen (HELLO-Ablehnung, Force-Logout) über closer.
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let (close_tx, close_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut world = ctx.shared.lock().await;
        world.closers.insert(conn_id, close_tx);
        // Peer-Adresse nur im RAM (siehe accept_loop).
        world.peer_addrs.insert(conn_id, peer_addr);
    }
    let forward = tokio::spawn(async move {
        let mut close_rx = close_rx;
        loop {
            tokio::select! {
                text = rx.recv() => {
                    match text {
                        Some(text) => {
                            if sink.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = &mut close_rx => break,
            }
        }
        let _ = sink.close().await;
    });

    let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
    read_loop(&ctx, &tx, conn_id, &mut guard, &sec_cfg, stream).await;

    // Zentraler Endpfad: JEDES Verbindungsende (Close-Frame, Lesefehler,
    // Rate-Trennung) läuft hier durch — kein Pfad überspringt das Cleanup.
    finish_conn(&ctx, conn_id).await;
    forward.abort();
    Ok(())
}

/// Lese-/Dispatch-Schleife einer Verbindung.
///
/// Ein Lesefehler beendet die Schleife kontrolliert (KEIN vorzeitiges `?`),
/// damit der aufrufende Pfad anschließend garantiert das zentrale,
/// verbindungsspezifische Cleanup ausführt (docs/Security.md AUTH-03).
async fn read_loop<S>(
    ctx: &Arc<Ctx>,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    guard: &mut crate::security::ConnGuard,
    sec_cfg: &crate::security::SecurityCfg,
    mut stream: S,
) where
    S: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    while let Some(msg) = stream.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                log::warn!("ws read: {e}");
                break;
            }
        };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };
        // Serverautorität V1 — frühe, billige Prüfung (Reihenfolge):
        // Größe → Format → Session → Sequenz → Rate Limit → Game Logic.
        // Offensichtlich Ungültiges erreicht nie DB/Kampf/Inventar/Welt/KI.
        if crate::security::frame_too_large(sec_cfg, text.len()) {
            log::warn!(
                "sec-reject conn={conn_id} reason=frame_too_large bytes={}",
                text.len()
            );
            if guard.add_violation(sec_cfg) {
                break;
            }
            continue;
        }
        let frame: Frame = match serde_json::from_str(&text) {
            Ok(f) => f,
            Err(_) => {
                log::warn!("sec-reject conn={conn_id} reason=bad_frame");
                if guard.add_violation(sec_cfg) {
                    break;
                }
                continue; // ungültiges JSON ignorieren, kein Crash
            }
        };
        if !crate::security::is_known_c2s(frame.msg_type) {
            log::warn!(
                "sec-reject conn={conn_id} reason=unknown_type type={}",
                frame.msg_type
            );
            if guard.add_violation(sec_cfg) {
                break;
            }
            continue;
        }
        if dispatch(ctx, tx, conn_id, guard, sec_cfg, frame).await {
            break; // massive/wiederholte Überschreitung → Disconnect, kein Bann
        }
    }
}

/// Zentraler, verbindungsspezifischer Endpfad einer Verbindung
/// (docs/Login_Realm_Architektur.md „Verbindungs-Einzigkeit und Takeover“).
///
/// Nimmt IMMER die konkrete `conn_id` entgegen und führt den eigentlichen,
/// dreifach abgesicherten Cleanup in `finish_owner` aus.
async fn finish_conn(ctx: &Arc<Ctx>, conn_id: u64) {
    let owner: Option<String> = {
        let mut world = ctx.shared.lock().await;
        world.closers.remove(&conn_id);
        // Peer-Adresse verlässt mit der Verbindung den RAM (nie protokolliert).
        world.peer_addrs.remove(&conn_id);
        world.by_conn.get(&conn_id).cloned()
    };
    // Kein HELLO erfolgt (nie eingeloggt) oder die Verbindung wurde verdrängt:
    // kein Player-, Persistenz- oder Gruppen-Cleanup.
    let Some(player_id) = owner else {
        return;
    };
    log::info!("ws-disconnect conn={conn_id} char={player_id}");
    // Produktionsverdrahtung der beiden Effekte: exakt die bestehenden
    // Pfadfunktionen (Stufe-B-Spool-Batch mit force, direkter `logout_at`-
    // Write). Sie werden als Parameter gereicht, damit der Interleaving-Fall
    // (Takeover während des asynchronen Schreibens) im Test kontrollierbar
    // nachstellbar ist — im Betrieb ändert sich nichts.
    let persist = ctx.persist.clone();
    let shared = ctx.shared.clone();
    let pool = ctx.db.clone();
    let pid_flush = player_id.clone();
    let pid_logout = player_id.clone();
    finish_owner(
        ctx,
        conn_id,
        &player_id,
        Box::new(move || {
            let persist = persist.clone();
            let shared = shared.clone();
            let pid = pid_flush.clone();
            Box::pin(async move {
                // Gate ist bereits gehalten → gate-freier Eintrag, sonst
                // Selbstverriegelung (tokio::Mutex ist nicht reentrant).
                // Das Fehler-Logging übernimmt `finish_owner` (§16-Fallback).
                persist.persist_player_gate_held(&shared, &pid, true).await
            })
        }),
        Box::new(move |logout_at| {
            let pool = pool.clone();
            let pid = pid_logout.clone();
            Box::pin(async move {
                // logout_at wird NICHT über den Drain geschrieben (gehört zum
                // finalen Disconnect-Save, docs §23), sondern direkt. Der
                // Fehler wird **durchgereicht**; Logging und Retry
                // übernimmt der zentrale Block in `finish_owner`.
                db::write_logout_at(&pool, &pid, logout_at).await
            })
        }),
    )
    .await;
}

/// Finaler Persistenz- und Player-Cleanup einer Eigentümer-Verbindung.
///
/// **Serialisierungsgrenze:** Der Aufrufer hält das bestehende per-player-Gate
/// (`Spool::player_gate`) über den gesamten Logout-Commit. Ein
/// `commit_login`/`handle_hello` für dieselbe `player_id` (netz.rs-Loginpfad:
/// `handlers::handle_hello`) wartet damit, bis der alte `logout_at`-Write
/// beendet ist, und kann danach nicht mehr von ihm markiert werden. Das Gate
/// wird hier bewusst VOR der ersten World-Sperre geholt (Reihenfolge
/// Gate → World → Elternkontrolle/Gruppen).
///
/// Drei Eigentümerprüfungen, jeweils unter der World-Sperre:
///
/// 1. vor dem Flush: eine verdrängte Verbindung persistiert nicht,
/// 2. unmittelbar vor `logout_at`: schneller Vorab-Check (der eigentliche
///    Schutz gegen das Restfenster ist das Gate, nicht diese Prüfung),
/// 3. unter derselben Sperre, in der entfernt wird.
///
/// Gibt true zurück, wenn der Player samt DESPAWN entfernt wurde.
async fn finish_owner(
    ctx: &Arc<Ctx>,
    conn_id: u64,
    player_id: &str,
    flush: DisconnectFlush,
    write_logout: DisconnectLogout,
) -> bool {
    // Gate zuerst: verhindert, dass ein Login-/Takeover für dieselbe player_id
    // zwischen Eigentümerprüfung und `logout_at`-Write abschließt.
    let gate = ctx.persist.player_gate(player_id).await;
    let _logout_gate = gate.lock_owned().await;
    // (1) vor dem Flush — finaler Disconnect-Save nur als Eigentümer
    // (vollständiger Durable-Spool-Batch via zentralem Pfad; der
    // Inventar-Sicherheits-Puffer ist temporär und wird nie persistiert).
    {
        let world = ctx.shared.lock().await;
        if !crate::world::is_owner(&world, conn_id, player_id) {
            log::info!("connection {conn_id} superseded — kein Persistenz-/logout_at-Cleanup");
            return false;
        }
    }
    // Logout-Zeitpunkt (Epoch-Sekunden) für die einmalige Rested-Berechnung
    // beim nächsten Login festhalten (§12).
    let logout_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // docs/Player_Persistenz.md §16: Bei Fehler bleibt der Spieler im
    // autoritativen RAM (Dirty-State/Revision unverändert), ein späterer Flush
    // versucht erneut. Wäre er entfernt, ginge der letzte autoritative Zustand
    // verloren und der nächste Login baute aus einer älteren DB-Zeile.
    let flushed = match flush().await {
        Ok(()) => true,
        Err(e) => {
            log::error!(
                "disconnect persist {player_id} fehlgeschlagen: {e} — Player bleibt im RAM (§16)"
            );
            false
        }
    };
    // (2) unmittelbar vor `logout_at`: Vorab-Check, damit ein bereits
    // verdrängter Owner keinen unnötigen DB-Roundtrip erzeugt. Die Garantie
    // gegen das Markieren der neuen Sitzung liefert das Gate oben, nicht
    // diese Prüfung (siehe Modulkommentar).
    {
        let world = ctx.shared.lock().await;
        if !crate::world::is_owner(&world, conn_id, player_id) {
            log::info!("connection {conn_id} superseded during persist — kein logout_at");
            return false;
        }
    }
    // (2b) Begrenzter Retry des direkten `logout_at`-Writes
    // (docs/Player_Persistenz.md §11; docs/Security.md P-27). Der bereits
    // ermittelte Zeitstempel wird unverändert an **alle** Versuche gereicht;
    // während des Blocks entsteht kein neuerer. Das per-player-Gate ist über
    // alle Versuche gehalten, und der Owner-Check oben unmittelbar vor diesem
    // Block gilt für die gesamte Operation: ein Takeover während der Retries
    // wartet am selben Gate und kann erst nach dem Cleanup committen. Der
    // Retry ist damit **kein** Wiederholen des Logout-Ablaufs und erzeugt
    // keine zweite Eigentümer-Zuordnung.
    let retry = write_logout_with_retry(
        player_id,
        &write_logout,
        logout_at,
        LogoutContext::Disconnect,
        LOGOUT_RETRY_PLAN,
        &(|d: Duration| Box::pin(tokio::time::sleep(d)) as BoxFuture<'static, ()>),
    )
    .await;
    if !retry.ok {
        // Endgültig erfolglos (Versuche oder Budget erschöpft): globaler
        // Persistenzstatus auf DEGRADED. Das ist ein **Betriebsindikator**, kein
        // Reparaturversuch: weder Spool noch `mark_dirty` schreiben
        // `logout_at`. Der Cleanup läuft kontrolliert weiter, es erfolgt kein
        // Bann und keine Sanktion.
        ctx.persist
            .set_status(crate::spool::PersistStatus::Degraded);
        log::error!(
            "{}",
            logout_log_line(
                "logout_at_failed",
                player_id,
                retry.attempts,
                LogoutContext::Disconnect,
                retry.class.unwrap_or(LogoutErrorClass::WriteError),
            )
        );
    } else if retry.retried {
        log::info!(
            "{}",
            logout_log_line(
                "logout_at_recovered",
                player_id,
                retry.attempts,
                LogoutContext::Disconnect,
                retry.class.unwrap_or(LogoutErrorClass::WriteError),
            )
        );
    }
    // (3) Eigentümerprüfung UNTER der Sperre, in der entfernt wird: ein
    // Takeover während des Flushes darf nicht zurückgerollt werden.
    let released = {
        let mut world = ctx.shared.lock().await;
        if !crate::world::is_owner(&world, conn_id, player_id) {
            false
        } else if flushed {
            crate::world::disconnect_conn(&mut world, conn_id).is_some()
        } else {
            // Flush fehlgeschlagen: nur die Eigentümerschaft freigeben, der
            // Player bleibt maßgeblich im RAM (§16).
            crate::world::release_conn(&mut world, conn_id).is_some()
        }
    };
    if released {
        parental::detach(&ctx.parental, player_id).await;
        let mut groups = ctx.groups.lock().await;
        groups.on_disconnect(player_id, std::time::Instant::now());
        log::info!("client disconnected: {player_id}");
    } else {
        log::info!("connection {conn_id} superseded — kein Player-Cleanup");
    }
    released
}

/// Dispatcher mit V1-Gate: Session-, Sequenz- und Rate-Prüfung laufen VOR
/// der Spiellogik. Gibt true zurück, wenn die Verbindung wegen massiver/
/// wiederholter Überschreitung getrennt werden soll (kein permanenter Bann).
async fn dispatch(
    ctx: &Arc<Ctx>,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    guard: &mut crate::security::ConnGuard,
    sec_cfg: &crate::security::SecurityCfg,
    frame: Frame,
) -> bool {
    // Sequenz vermerken (Lag-tolerant: Duplikat/Out-of-Order ist kein Cheat,
    // wird nur vermerkt — keine Ablehnung, keine Verurteilung).
    guard.note_seq(frame.seq);
    let authenticated = {
        let world = ctx.shared.lock().await;
        world.by_conn.contains_key(&conn_id)
    };
    match crate::security::gate_frame(sec_cfg, guard, frame.msg_type, authenticated, std::time::Instant::now()) {
        crate::security::GateDecision::Allow => {}
        crate::security::GateDecision::Drop => {
            let world = ctx.shared.lock().await;
            crate::security::log_reject(
                world
                    .by_conn
                    .get(&conn_id)
                    .and_then(|pid| world.players.get(pid)),
                conn_id,
                &crate::security::RejectInfo {
                    reason: if authenticated {
                        "rate_limited".into()
                    } else {
                        "no_session".into()
                    },
                    msg_type: frame.msg_type,
                    detail: String::new(),
                },
                guard.violations,
            );
            return false;
        }
        crate::security::GateDecision::Disconnect => {
            log::warn!(
                "sec-disconnect conn={conn_id} type={} violations={}",
                frame.msg_type,
                guard.violations
            );
            return true;
        }
    }
    let data = frame.data.clone();
    match frame.msg_type {
        c2s::HELLO => {
            if let Err(reason) = handlers::handle_hello(ctx, tx, conn_id, frame.seq, &data).await {
                log::info!("HELLO rejected ({reason})");
                // Einstieg verweigert: Socket gezielt schließen; den Rest
                // (Registry, Parental, Position) erledigt der Disconnect-Pfad.
                let mut world = ctx.shared.lock().await;
                close_conn(&mut world, conn_id);
            }
        }
        c2s::HEARTBEAT => {
            handlers::handle_heartbeat(&ctx.shared, tx, conn_id, frame.seq, &data).await
        }
        c2s::MOVE => handlers::handle_move(&ctx.shared, conn_id, &data, ctx.cfg.tick_ms).await,
        c2s::ATTACK => {
            handlers::handle_attack(&ctx.shared, conn_id, &data, &ctx.cfg.combat, &ctx.cfg.npc)
                .await
        }
        c2s::ABILITY => {
            handlers::handle_ability(ctx, conn_id, &data).await
        }
        c2s::CHAT => {
            handlers::handle_chat(
                &ctx.parental,
                &ctx.shared,
                tx,
                conn_id,
                frame.seq,
                &data,
                ctx.cfg.aofb_radius,
            )
            .await
        }
        c2s::GROUP_INVITE => handlers::handle_group_invite(ctx, conn_id, &data).await,
        c2s::GROUP_INVITE_REACT => handlers::handle_group_invite_react(ctx, conn_id, &data).await,
        c2s::GROUP_SUGGEST => handlers::handle_group_suggest(ctx, conn_id, &data).await,
        c2s::GROUP_SUGGEST_DECIDE => handlers::handle_group_suggest_decide(ctx, conn_id, &data).await,
        c2s::GROUP_LEAVE => handlers::handle_group_leave(ctx, conn_id, &data).await,
        c2s::GROUP_KICK => handlers::handle_group_kick(ctx, conn_id, &data).await,
        c2s::GROUP_TRANSFER => handlers::handle_group_transfer(ctx, conn_id, &data).await,
        c2s::PICKUP => handlers::handle_pickup(ctx, conn_id, &data).await,
        c2s::SPEND_ATTRIBUTE => {
            handlers::handle_spend_attribute(ctx, tx, conn_id, frame.seq, &data).await
        }
        c2s::AUCTION_BUY => {
            handlers::handle_auction_buy(ctx, tx, conn_id, frame.seq, &data).await
        }
        c2s::PARENTAL => {
            let pid: Option<String> = {
                let world = ctx.shared.lock().await;
                world.by_conn.get(&conn_id).cloned()
            };
            if let Some(pid) = pid {
                let action = data.get("action").and_then(|v| v.as_str()).unwrap_or("");
                let pin = data.get("pin").and_then(|v| v.as_str()).unwrap_or("");
                parental::handle_message(&ctx.parental, tx, &pid, frame.seq, action, pin).await;
            }
        }
        // NPC_TALK / AUCTION_LIST / AUCTION_BID: künftig (wie Übergangsstand).
        // Unbekannte Typen werden bereits vor dem Gate verworfen.
        other => log::info!("unknown type {other}"),
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Laufzähler für die temporäre Basis eines Testkontexts.
    ///
    /// `SystemTime::now()` allein garantiert keine Eindeutigkeit: die Uhr hat
    /// hier nur grobe Auflösung (gemessen 18 ns), sodass bei parallel
    /// laufenden Threads identische Nanosekunden entstehen können. Zwei
    /// `test_ctx()`-Aufrufe mit gleichem Namen teilen dann ein
    /// Basisverzeichnis, und `failed_disconnect_flush_retains_ram_player_and_
    /// login_adopts_it` ersetzt darin `spool` durch eine Datei — was
    /// `login_is_fail_closed_while_newer_snapshot_is_pending_in_spool` mit
    /// `Not a directory (os error 20)` scheitern lässt. Der Zähler erzwingt
    /// Eindeutigkeit prozessweit, unabhängig von der Uhrenauflösung.
    static NEXT_TEST_DIR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Namenspraefix jedes temporaeren Teststamms. Der Guard entfernt
    /// ausschliesslich Pfade, deren letzter Bestandteil damit beginnt.
    const TEST_DIR_PREFIX: &str = "andora-realm-net-test-";

    /// Allokiert einen neuen temporaeren Teststamm. Eindeutigkeit entsteht
    /// prozessweit aus dem atomaren `NEXT_TEST_DIR`, unabhaengig von der
    /// Uhrenauflösung; Prozess-ID und Zeitwert trennen Prozesse voneinander.
    fn alloc_test_dir_at(stamp: u128) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{}{}-{}-{}",
            TEST_DIR_PREFIX,
            std::process::id(),
            stamp,
            NEXT_TEST_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    fn alloc_test_dir() -> PathBuf {
        alloc_test_dir_at(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        )
    }

    /// Prueft, ob `path` genau ein eigener temporaerer Teststamm ist.
    ///
    /// Bewusst streng: es wird nichts entfernt, wenn der Pfad nicht unterhalb
    /// von `std::env::temp_dir()` liegt, nicht mit `TEST_DIR_PREFIX` beginnt,
    /// dem Tempverzeichnis selbst entspricht, leer ist oder keinen konkreten
    /// letzten Bestandteil besitzt.
    fn is_removable_test_dir(path: &Path) -> bool {
        if path.as_os_str().is_empty() {
            return false;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        if !name.starts_with(TEST_DIR_PREFIX) {
            return false;
        }
        let tmp = std::env::temp_dir();
        if path == tmp || !path.starts_with(&tmp) {
            return false;
        }
        path.parent().is_some()
    }

    /// Entfernt genau den uebergebenen Teststamm. Fehlende Pfade gelten als
    /// bereits bereinigt. Ist der Stamm selbst eine Datei, wird nur diese
    /// validierte Datei entfernt. Symlinks werden nicht verfolgt.
    fn cleanup_test_dir(path: &Path) {
        if !is_removable_test_dir(path) {
            return;
        }
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
            // `symlink_metadata` meldet einen Symlink auf ein Verzeichnis als
            // kein Verzeichnis; er landet damit in `remove_file` und wird
            // nicht traversiert.
            Ok(m) if m.is_dir() => {
                let _ = std::fs::remove_dir_all(path);
            }
            Ok(_) => {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Privater RAII-Guard: entfernt beim Drop genau seinen eigenen Stamm.
    /// `Drop` löst nie einen Panic aus und verdeckt keinen Testfehler.
    struct TestDirGuard {
        path: PathBuf,
    }

    impl Drop for TestDirGuard {
        fn drop(&mut self) {
            cleanup_test_dir(&self.path);
        }
    }

    /// Testkontext mit gemeinsamem Stammpfad-Cleanup. Alle Klone teilen denselben
    /// Guard, der Pfad besteht damit, solange mindestens ein Besitzer lebt.
    ///
    /// `Deref` haelt die bestehende Feld- und Argumentverwendung unveraendert;
    /// `Ctx` selbst bleibt unberuehrt (Produktionscode in `handlers.rs`).
    #[derive(Clone)]
    struct TestCtx {
        ctx: Arc<Ctx>,
        _dir: Arc<TestDirGuard>,
    }

    impl std::ops::Deref for TestCtx {
        /// Ziel ist bewusst der `Arc`, nicht der `Ctx`: dadurch bleiben sowohl
        /// der Feldzugriff (`ctx.persist`) als auch Aufrufe, die `&Arc<Ctx>`
        /// erwarten, unverändert auflösbar.
        type Target = Arc<Ctx>;
        fn deref(&self) -> &Arc<Ctx> {
            &self.ctx
        }
    }

    // ---- Cleanup des temporären Teststamms (RAII-Guard) ----

    /// Legt Stamm + Guard an und liefert beides zurück.
    fn guarded_dir() -> (PathBuf, Arc<TestDirGuard>) {
        let dir = alloc_test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let guard = Arc::new(TestDirGuard { path: dir.clone() });
        (dir, guard)
    }

    /// A. Eindeutigkeit: zwei Allokationen unterscheiden sich — auch bei
    /// identischem simuliertem Zeitwert, weil der Zähler entscheidet.
    #[test]
    fn test_dir_allocation_is_unique_even_with_identical_timestamp() {
        let a = alloc_test_dir_at(42);
        let b = alloc_test_dir_at(42);
        assert_ne!(a, b, "Zähler muss Eindeutigkeit erzwingen");
        let c = alloc_test_dir();
        let d = alloc_test_dir();
        assert_ne!(
            c, d,
            "zwei aufeinanderfolgende Allokationen kollidieren nicht"
        );
        for p in [&a, &b, &c, &d] {
            assert!(is_removable_test_dir(p), "{p:?} muss validierbar sein");
        }
    }

    /// B. Lebensdauer: nach Drop eines von zwei Besitzern besteht der Pfad
    /// weiter, nach Drop des letzten ist er entfernt.
    #[test]
    fn test_dir_survives_until_last_owner_is_dropped() {
        let (dir, guard) = guarded_dir();
        let second = guard.clone();
        assert!(dir.is_dir());
        drop(guard);
        assert!(dir.is_dir(), "Pfad muss nach Drop eines Besitzers bestehen");
        drop(second);
        assert!(
            !dir.exists(),
            "Pfad muss nach Drop des letzten Besitzers weg sein"
        );
    }

    /// C. Panic-Unwinding: ein Panic im Scope entfernt den Stamm trotzdem.
    #[test]
    fn test_dir_is_cleaned_up_on_panic_unwind() {
        let mut observed = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (dir, _guard) = guarded_dir();
            observed = Some(dir.clone());
            assert!(dir.is_dir());
            panic!("absichtliches Panic im Test-Scope");
        }));
        assert!(result.is_err(), "Panic muss ausgeloest worden sein");
        let dir = observed.expect("Pfad muss innerhalb des Scopes bekannt sein");
        assert!(!dir.exists(), "Stamm muss auch nach Unwind entfernt sein");
    }

    /// D. Fremdschutz: ein zweiter Stamm, `TMPDIR` und das gemeinsame
    /// Elternverzeichnis bleiben beim Drop des ersten Guard bestehen.
    #[test]
    fn test_dir_cleanup_leaves_foreign_paths_untouched() {
        let (mine, guard) = guarded_dir();
        let other = alloc_test_dir();
        std::fs::create_dir_all(&other).unwrap();
        let tmp = std::env::temp_dir();
        drop(guard);
        assert!(!mine.exists(), "eigener Stamm muss entfernt sein");
        assert!(other.is_dir(), "fremder Stamm muss bestehen bleiben");
        assert!(tmp.is_dir(), "TMPDIR muss bestehen bleiben");
        std::fs::remove_dir_all(&other).unwrap();
    }

    /// E. Bereits entfernt: ein vorzeitig manuell entfernter Stamm macht den
    /// Drop panicfrei.
    #[test]
    fn test_dir_drop_is_panic_free_when_already_removed() {
        let (dir, guard) = guarded_dir();
        std::fs::remove_dir_all(&dir).unwrap();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            drop(guard);
        }));
        assert!(res.is_ok(), "Drop darf bei fehlendem Pfad nicht paniken");
    }

    /// Ergänzend: `cleanup_test_dir` lehnt fremde Ziele strikt ab, statt
    /// irgendeine Ersatzbereinigung auszuführen.
    #[test]
    fn cleanup_rejects_paths_outside_its_own_test_dir() {
        let tmp = std::env::temp_dir();
        assert!(!is_removable_test_dir(Path::new("")));
        assert!(
            !is_removable_test_dir(&tmp),
            "TMPDIR selbst ist kein Teststamm"
        );
        assert!(!is_removable_test_dir(&tmp.join("fremdes-verzeichnis")));
        assert!(!is_removable_test_dir(Path::new("/")));
        assert!(!is_removable_test_dir(Path::new("/tmp")));
        // Ein Stamm, der als Datei angelegt wurde, wird exakt entfernt.
        let dir = alloc_test_dir();
        std::fs::write(&dir, b"kein verzeichnis").unwrap();
        cleanup_test_dir(&dir);
        assert!(!dir.exists(), "validierte Datei muss entfernt sein");
    }

    /// Testkontext ohne DB-Verbindung: der Pool wird lazy geöffnet und in
    /// diesen Tests nicht benutzt (getestet werden Lesepfad und Cleanup).
    /// Die Konfiguration wird im Speicher gebaut (kein Datei-I/O, damit der
    /// Test nicht von Mount-/Cache-Sichtbarkeit abhängt).
    async fn test_ctx() -> TestCtx {
        use std::collections::HashMap;
        let dir = alloc_test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let dir_guard = Arc::new(TestDirGuard { path: dir.clone() });
        let env = HashMap::<String, String>::new();
        let cfg = Arc::new(crate::config::Config {
            realm_id: 1,
            ws_port: 3001,
            health_port: 3002,
            ws_bind_host: String::new(),
            health_bind_host: String::new(),
            tick_ms: 100,
            aofb_radius: 20.0,
            render_cap: 64,
            ollama_url: String::new(),
            // Auth-API bewusst deaktiviert (kein Login im Test).
            auth_api: crate::config::AuthApiConfig {
                url: String::new(),
                service_id: String::new(),
                secret: String::new(),
            },
            realm_db: crate::config::DbConfig {
                host: "127.0.0.1".into(),
                port: 3306,
                user: "u".into(),
                password: "p".into(),
                database: "realm_state_test".into(),
            },
            migrations_dir: String::new(),
            allow_destructive: false,
            combat: crate::config::combat_config(&env),
            npc: crate::config::npc_config(&env),
            group: crate::config::group_config(&env),
            inventory: crate::config::inventory_config(&env),
            loot: crate::config::loot_config(&env),
            progression: crate::config::progression_config(&env),
            persist: crate::config::persist_config(&env),
            security: crate::config::security_config(&env),
        });
        let db = sqlx::mysql::MySqlPoolOptions::new()
            .connect_lazy("mysql://u:p@127.0.0.1:3306/realm_state_test")
            .unwrap();
        let auth = AuthApi::new(&cfg.auth_api).unwrap();
        let shared = crate::world::new_shared();
        let parental = crate::parental::new_shared(auth.clone());
        let groups = crate::group::new_shared_groups(cfg.group.clone());
        let persist = std::sync::Arc::new(
            crate::spool::PersistRuntime::new(&dir, &cfg.combat.weapon_skill_id).unwrap(),
        );
        let ctx = Arc::new(Ctx {
            cfg,
            db,
            auth,
            shared,
            parental,
            registry: crate::combat::ability::AbilityRegistry::new(),
            groups,
            quest: crate::quest::QuestService::new(),
            persist,
        });
        TestCtx {
            ctx,
            _dir: dir_guard,
        }
    }

    // ---- P-27: begrenzter Retry des direkten `logout_at`-Writes ----

    /// Testplan: gleiche Struktur wie `LOGOUT_RETRY_PLAN`, aber mit winzigen
    /// Wartezeiten und einem Timeout, der im Test deterministisch greift. Die
    /// **reale** Dauer der Produktionswerte (2 s je Write, 5 s Disconnect-
    /// Budget, 30 s Shutdown-Budget) ist damit ausdrücklich **nicht** bewiesen
    /// — das ist eine Timer-Integrationsgrenze (kein `test-util`-Feature im
    /// Projekt, also keine pausierte Tokio-Zeit).
    const TEST_WAITS: [Duration; 2] = [Duration::from_millis(1), Duration::from_millis(2)];
    const TEST_PLAN: LogoutRetryPlan = LogoutRetryPlan {
        max_attempts: 3,
        write_timeout: Duration::from_millis(40),
        waits: &TEST_WAITS,
        budget: Duration::from_millis(500),
    };
    /// Sofortige Wartefunktion: im Projekt existiert keine pausierte Tokio-Zeit,
    /// deshalb wird hier nicht gewartet.
    fn no_wait() -> impl Fn(Duration) -> BoxFuture<'static, ()> + Send + Sync {
        |_d: Duration| Box::pin(async {})
    }

    /// Zähler, die alle Aufrufe und alle gesehenen Zeitstempel aufzeichnen.
    #[derive(Default, Clone)]
    struct WriteSpy {
        calls: std::sync::Arc<std::sync::Mutex<Vec<i64>>>,
    }

    impl WriteSpy {
        fn calls(&self) -> Vec<i64> {
            self.calls.lock().unwrap().clone()
        }
    }

    /// 1. Erfolg beim ersten Versuch: genau ein Aufruf, keine Wiederholung.
    #[tokio::test]
    async fn logout_retry_succeeds_on_first_attempt() {
        let spy = WriteSpy::default();
        let write = |ts: i64| {
            let calls = spy.calls.clone();
            Box::pin(async move {
                calls.lock().unwrap().push(ts);
                Ok(())
            }) as BoxFuture<'static, Result<(), String>>
        };
        let r = write_logout_with_retry(
            "1",
            &write,
            1_700_000_000,
            LogoutContext::Disconnect,
            TEST_PLAN,
            &no_wait(),
        )
        .await;
        assert_eq!(spy.calls(), vec![1_700_000_000]);
        assert!(r.ok, "erster Versuch muss genügen");
        assert_eq!(r.attempts, 1);
        assert!(!r.retried, "kein Wiederholungsversuch");
        assert!(!r.budget_exhausted);
        assert_eq!(r.class, None, "bei Erfolg gibt es keine Fehlerklasse");
    }

    /// 2. + 5. Erster Versuch schlägt fehl, zweiter gelingt: genau zwei Aufrufe,
    /// danach **kein** weiterer Versuch.
    #[tokio::test]
    async fn logout_retry_recovers_on_second_attempt() {
        let spy = WriteSpy::default();
        let write = |ts: i64| {
            let calls = spy.calls.clone();
            Box::pin(async move {
                let mut seen = calls.lock().unwrap();
                seen.push(ts);
                let n = seen.len();
                drop(seen);
                if n == 1 {
                    Err("erster Fehler".into())
                } else {
                    Ok(())
                }
            }) as BoxFuture<'static, Result<(), String>>
        };
        let r = write_logout_with_retry(
            "7",
            &write,
            42,
            LogoutContext::Disconnect,
            TEST_PLAN,
            &no_wait(),
        )
        .await;
        assert_eq!(spy.calls(), vec![42, 42]);
        assert!(r.ok);
        assert_eq!(r.attempts, 2);
        assert!(r.retried, "ein Wiederholungsversuch war nötig");
    }

    /// 3. Alle Versuche schlagen fehl: begrenzte Versuchszahl, WriteError.
    #[tokio::test]
    async fn logout_retry_gives_up_after_all_attempts() {
        let spy = WriteSpy::default();
        let write = |ts: i64| {
            let calls = spy.calls.clone();
            Box::pin(async move {
                calls.lock().unwrap().push(ts);
                Err("dauerhaft".into())
            }) as BoxFuture<'static, Result<(), String>>
        };
        let r = write_logout_with_retry(
            "7",
            &write,
            42,
            LogoutContext::Disconnect,
            TEST_PLAN,
            &no_wait(),
        )
        .await;
        assert_eq!(spy.calls().len(), 3, "max_attempts = 3");
        assert!(!r.ok);
        assert_eq!(r.attempts, 3);
        assert_eq!(r.class, Some(LogoutErrorClass::WriteError));
        assert!(!r.budget_exhausted, "Budget war nicht der Auslöser");
    }

    /// 4. Bei **allen** Versuchen wird exakt derselbe Zeitstempel verwendet.
    #[tokio::test]
    async fn logout_retry_uses_the_same_timestamp_on_every_attempt() {
        for context in [LogoutContext::Disconnect, LogoutContext::Shutdown] {
            let spy = WriteSpy::default();
            let write = |ts: i64| {
                let calls = spy.calls.clone();
                Box::pin(async move {
                    calls.lock().unwrap().push(ts);
                    Err("immer Fehler".into())
                }) as BoxFuture<'static, Result<(), String>>
            };
            let r = write_logout_with_retry("9", &write, 1_234_567, context, TEST_PLAN, &no_wait())
                .await;
            let seen = spy.calls();
            assert_eq!(seen.len(), 3, "drei Versuche");
            assert!(
                seen.iter().all(|ts| *ts == 1_234_567),
                "der Logout-Zeitpunkt darf zwischen den Versuchen nicht wechseln: {seen:?}"
            );
            assert!(!r.ok);
        }
    }

    /// 8. Timeout wird als `timeout`, ein normaler Fehler als `write_error`
    /// klassifiziert. Der Timeout wird deterministisch über den (winzigen)
    /// Test-Plan ausgelöst: der Write hängt, das Write-Timeout greift.
    #[tokio::test]
    async fn logout_retry_classifies_timeout_and_write_error() {
        let hang = |_ts: i64| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(())
            }) as BoxFuture<'static, Result<(), String>>
        };
        let r = write_logout_with_retry(
            "1",
            &hang,
            1,
            LogoutContext::Disconnect,
            TEST_PLAN,
            &no_wait(),
        )
        .await;
        assert!(!r.ok);
        assert_eq!(
            r.class,
            Some(LogoutErrorClass::Timeout),
            "ein hängender Write muss als timeout klassifiziert werden"
        );

        let fail = |_ts: i64| {
            Box::pin(async { Err("Verbindung abgelehnt".into()) })
                as BoxFuture<'static, Result<(), String>>
        };
        let r = write_logout_with_retry(
            "1",
            &fail,
            1,
            LogoutContext::Disconnect,
            TEST_PLAN,
            &no_wait(),
        )
        .await;
        assert_eq!(r.class, Some(LogoutErrorClass::WriteError));
    }

    /// Das Gesamtbudget begrenzt die **vollständige** Operation: mit einem
    /// erschöpften Budget wird kein weiterer Versuch gestartet. Geprüft wird die
    /// Budgetentscheidung und der Abbruchpfad, **nicht** die reale Dauer.
    #[tokio::test]
    async fn logout_retry_aborts_when_budget_is_exhausted() {
        const NO_BUDGET: LogoutRetryPlan = LogoutRetryPlan {
            max_attempts: 3,
            write_timeout: Duration::from_millis(40),
            waits: &TEST_WAITS,
            budget: Duration::from_millis(0),
        };
        let spy = WriteSpy::default();
        let write = |ts: i64| {
            let calls = spy.calls.clone();
            Box::pin(async move {
                calls.lock().unwrap().push(ts);
                Err("Fehler".into())
            }) as BoxFuture<'static, Result<(), String>>
        };
        let r = write_logout_with_retry(
            "1",
            &write,
            5,
            LogoutContext::Disconnect,
            NO_BUDGET,
            &no_wait(),
        )
        .await;
        assert_eq!(
            spy.calls().len(),
            1,
            "kein zweiter Versuch nach Budgetablauf"
        );
        assert!(!r.ok);
        assert!(r.budget_exhausted, "Budget muss als Auslöser markiert sein");
    }

    /// 9. Das erzeugte Logformat enthält ausschließlich die erlaubten Felder.
    #[test]
    fn logout_log_line_has_no_sensitive_fields() {
        let line = logout_log_line(
            "logout_at_failed",
            "char-42",
            3,
            LogoutContext::Disconnect,
            LogoutErrorClass::Timeout,
        );
        for f in [
            "logout_at_failed",
            "char_id=char-42",
            "attempt=3",
            "max_attempts=3",
            "context=disconnect",
            "error_class=timeout",
        ] {
            assert!(line.contains(f), "Pflichtfeld fehlt: {f} in {line}");
        }
        for verboten in [
            "session",
            "token",
            "password",
            "ip=",
            "127.0.0.1",
            "conn_id",
            "mysql://",
            "err=",
            "dauerhaft",
            "Verbindung abgelehnt",
        ] {
            assert!(
                !line.to_lowercase().contains(&verboten.to_lowercase()[..]),
                "verbotenes Feld im Logformat: {verboten} in {line}"
            );
        }
        // Kontext- und Klassenvarianten.
        assert!(logout_log_line(
            "logout_at_retry",
            "c",
            1,
            LogoutContext::Shutdown,
            LogoutErrorClass::WriteError
        )
        .contains("context=shutdown error_class=write_error"));
    }

    /// 10. Der Shutdown zählt `retried`, `failed` und `skipped` korrekt.
    #[tokio::test]
    async fn shutdown_logout_phase_counts_retried_failed_and_skipped() {
        let ctx = test_ctx().await;
        let online = vec![
            "ok".to_string(),
            "recovered".to_string(),
            "dead".to_string(),
        ];
        let recovered = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rec2 = recovered.clone();
        let make_write = move |id: &str| -> DisconnectLogout {
            let id = id.to_string();
            let rec = rec2.clone();
            Box::new(move |_ts: i64| {
                let id = id.clone();
                let rec = rec.clone();
                Box::pin(async move {
                    match id.as_str() {
                        // immer erfolgreich -> kein Retry
                        "ok" => Ok(()),
                        // erster Versuch Fehler, zweiter erfolgreich
                        "recovered" => {
                            if rec.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                                Err("einmalig".into())
                            } else {
                                Ok(())
                            }
                        }
                        // dauerhaft fehlschlagend
                        _ => Err("dauerhaft".into()),
                    }
                }) as BoxFuture<'static, Result<(), String>>
            })
        };
        let report = shutdown_logout_phase(
            &ctx.persist,
            &ctx.shared,
            &online,
            TEST_PLAN,
            Duration::from_secs(5),
            &no_wait(),
            &make_write,
        )
        .await;
        // `retried` zählt Zeichen mit **mindestens** einem Retry: 'recovered'
        // (1 Retry) und 'dead' (2 Retries). 'ok' braucht keinen.
        assert_eq!(
            report.retried, 2,
            "'recovered' und 'dead' brauchten Retries"
        );
        assert_eq!(report.failed, 1, "nur 'dead' bleibt fehlgeschlagen");
        assert_eq!(report.skipped, 0, "Budget war nicht erschöpft");
        assert!(!report.budget_exhausted);
    }

    /// 11. Nach Ablauf des globalen Shutdown-Budgets wird **kein** weiterer
    /// DB-Aufruf gestartet; die verbleibenden Charaktere zählen als `skipped`.
    #[tokio::test]
    async fn shutdown_logout_phase_makes_no_db_call_after_budget() {
        let ctx = test_ctx().await;
        let online = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c2 = calls.clone();
        let make_write = move |_id: &str| -> DisconnectLogout {
            let c = c2.clone();
            Box::new(move |_ts: i64| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                }) as BoxFuture<'static, Result<(), String>>
            })
        };
        // Budget 0 -> der erste Charakter wird bereits übersprungen.
        let report = shutdown_logout_phase(
            &ctx.persist,
            &ctx.shared,
            &online,
            TEST_PLAN,
            Duration::from_millis(0),
            &no_wait(),
            &make_write,
        )
        .await;
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "nach Budgetablauf darf kein einziger DB-Aufruf starten"
        );
        assert_eq!(report.skipped, 3, "alle drei Charaktere zählen als skipped");
        assert_eq!(report.failed, 0);
        assert!(report.budget_exhausted);
    }

    /// Beweis A (reine Funktion, keine Wartezeit): das verbleibende globale
    /// Budget wird an den letzten zulässigen Write weitergegeben und kürzt
    /// sowohl das Gesamtbudget als auch den Einzel-Timeout.
    #[test]
    fn plan_within_budget_caps_budget_and_write_timeout() {
        let plan = LogoutRetryPlan {
            max_attempts: 3,
            write_timeout: Duration::from_secs(2),
            waits: &TEST_WAITS,
            budget: Duration::from_secs(5),
        };
        // (a) Restbudget kleiner als der Einzel-Timeout -> beides wird gekürzt.
        let rest = Duration::from_millis(250);
        let capped = plan_within_budget(plan, rest).expect("Restbudget > 0");
        assert_eq!(capped.budget, rest, "Gesamtbudget = verbleibendes Budget");
        assert_eq!(capped.write_timeout, rest, "Timeout auf Restbudget gekürzt");
        // Versuche und Wartezeiten bleiben unangetastet: nur die *Dauer* wird
        // gekürzt, das Retry-Verhalten nicht.
        assert_eq!(capped.max_attempts, 3);
        assert_eq!(capped.waits, &TEST_WAITS);
        // (b) Restbudget größer als der Einzel-Timeout -> Plan bleibt ungekürzt.
        let roomy = plan_within_budget(plan, Duration::from_secs(30)).expect("Restbudget > 0");
        assert_eq!(roomy.budget, Duration::from_secs(5));
        assert_eq!(roomy.write_timeout, Duration::from_secs(2));
        // (c) Deadline abgelaufen -> kein Write darf starten.
        assert!(plan_within_budget(plan, Duration::ZERO).is_none());
    }

    /// Beweis B (deterministisch, ohne echte 30-S-Wartezeit): ein global sehr
    /// kleines Budget bei einem **riesigen** Write-Timeout. Der einzige Write,
    /// der startet, hängt; er kann nur durch den **gekürzten** Timeout enden,
    /// denn ungekürzt liefe er 30 s.
    ///
    /// Der Test behauptet **keine** Wandzeit, sondern zählt Aufrufe: genau ein
    /// Write, danach `skipped` für alle verbleibenden Charaktere. Die Kürzung
    /// selbst wird zusätzlich von `plan_within_budget_caps_budget_and_write_timeout`
    /// per Assertion bewiesen; eine entfernte Kürzung lässt **diesen** Test von
    /// ~0,6 s auf 30 s laufen (verifiziert per Mutationsprobe), nicht scheitern.
    #[tokio::test]
    async fn shutdown_logout_phase_shortens_last_write_to_remaining_budget() {
        let ctx = test_ctx().await;
        let online: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c2 = calls.clone();
        let make_write = move |_id: &str| -> DisconnectLogout {
            let c = c2.clone();
            Box::new(move |_ts: i64| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    // Hängt: nur der gekürzte Timeout kann diesen Write beenden.
                    std::future::pending::<()>().await;
                    Ok(())
                }) as BoxFuture<'static, Result<(), String>>
            })
        };
        let huge = LogoutRetryPlan {
            max_attempts: 3,
            write_timeout: Duration::from_secs(30),
            waits: &TEST_WAITS,
            budget: Duration::from_secs(30),
        };
        let report = shutdown_logout_phase(
            &ctx.persist,
            &ctx.shared,
            &online,
            huge,
            Duration::from_millis(40),
            &no_wait(),
            &make_write,
        )
        .await;
        // Genau ein Write kam zustande — der letzte zulässige. Er wurde auf das
        // Restbudget gekürzt, sonst hätte der hängende Write nicht beendet.
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "nur der erste Charakter erreicht die Deadline, alle anderen werden skipped"
        );
        assert_eq!(report.failed, 1, "der gekürzte Write endet per Timeout");
        assert_eq!(
            report.skipped, 3,
            "alle verbleibenden Charaktere zählen als skipped"
        );
        assert!(report.budget_exhausted);
        assert_eq!(
            report.retried, 0,
            "kein zweiter Versuch: Budget war aufgebraucht"
        );
    }

    /// Registriert einen RAM-Player **mit** Owner-Zuordnung (Voraussetzung für
    /// `finish_owner`: die Eigentümerprüfung läuft vor allem Weiteren).
    async fn insert_owned_hero(ctx: &Ctx) {
        let (close_tx, _close_rx) = tokio::sync::oneshot::channel::<()>();
        let (ptx, _prx) = mpsc::unbounded_channel();
        let mut world = ctx.shared.lock().await;
        world.players.insert(
            "hero".into(),
            crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 100,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-1".into(),
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
        world.by_conn.insert(7, "hero".into());
        world.closers.insert(7, close_tx);
    }

    /// 6. + 7. + 12. Nach endgültigem Fehlschlag läuft der Cleanup weiter, der
    /// globale Persistenzstatus wird auf `Degraded` gesetzt, und **kein**
    /// Spool-/Dirty-Reparaturpfad für `logout_at` wird ausgelöst: Der
    /// RAM-Zustand des Players bleibt in Dirty-Flags und Revision unverändert.
    #[tokio::test]
    async fn logout_write_failure_continues_cleanup_and_sets_degraded() {
        let ctx = test_ctx().await;
        insert_owned_hero(&ctx).await;
        assert_ne!(
            ctx.persist.status(),
            crate::spool::PersistStatus::Degraded,
            "Ausgangslage: nicht degradiert"
        );
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c2 = calls.clone();
        let logout: DisconnectLogout = Box::new(move |_ts: i64| {
            let c = c2.clone();
            Box::pin(async move {
                c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err("Verbindung nicht erreichbar".into())
            }) as BoxFuture<'static, Result<(), String>>
        });
        // Flush erfolgreich (Snapshot ok), damit der Spool-Cleanup-Zweig
        // (`disconnect_conn`) betroffen ist.
        let flush: DisconnectFlush = Box::new(|| Box::pin(async { Ok(()) }));
        let released = finish_owner(&ctx, 7, "hero", flush, logout).await;
        assert!(
            released,
            "Cleanup muss nach endgültigem Fehler weiterlaufen"
        );
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "begrenzte Versuche müssen alle stattgefunden haben"
        );
        // 7) Degraded erst nach der Erschöpfung.
        assert_eq!(
            ctx.persist.status(),
            crate::spool::PersistStatus::Degraded,
            "globaler Persistenzstatus muss nach endgültigem Fehler gesetzt sein"
        );
        {
            let world = ctx.shared.lock().await;
            assert!(!world.by_conn.contains_key(&7), "Owner-Zuordnung entfernt");
            assert!(!world.players.contains_key("hero"), "Player-Cleanup lief");
        }
        // 12) Kein Spool-/Dirty-Reparaturpfad: Revision und Dirty-Flags des
        // Players wurden vom fehlgeschlagenen Logout-Write nicht verändert.
        assert!(
            matches!(ctx.persist.pending_revision("hero"), Ok(None)),
            "der Logout-Retry darf keinen Spool-Batch erzeugen"
        );
    }

    /// 13. Der Retry verändert die Takeover-/Owner-Invarianten nicht: Er läuft
    /// vollständig **innerhalb** des gehaltenen per-player-Gates, und ein
    /// verdrängter Owner schreibt **kein** `logout_at`.
    #[tokio::test]
    async fn logout_retry_preserves_owner_and_takeover_invariants() {
        let ctx = test_ctx().await;
        insert_owned_hero(&ctx).await;
        // Ein neuer Login übernimmt während des Disconnect-Flushes (Takeover).
        let entered_tx = std::sync::Arc::new(std::sync::Mutex::new(Some({
            let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
            tx
        })));
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let entered = entered_tx.clone();
        let logout_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = logout_called.clone();
        let logout: DisconnectLogout = Box::new(move |_ts: i64| {
            let seen = seen.clone();
            Box::pin(async move {
                seen.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }) as BoxFuture<'static, Result<(), String>>
        });
        let flush: DisconnectFlush = Box::new({
            let entered = entered.clone();
            let release = release.clone();
            move || {
                let entered = entered.clone();
                let release = release.clone();
                Box::pin(async move {
                    // Warten, bis der Takeover den neuen Owner gesetzt hat.
                    if let Some(tx) = entered.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    release.notified().await;
                    Ok(())
                }) as BoxFuture<'static, Result<(), String>>
            }
        });
        let task = {
            let ctx = ctx.clone();
            tokio::spawn(async move { finish_owner(&ctx, 7, "hero", flush, logout).await })
        };
        // Takeover: eine neue Verbindung übernimmt den RAM-Player. Der
        // Player ist nicht `Clone`, deshalb wird er für den Commit entfernt und
        // unmittelbar wieder eingesetzt (entspricht dem AUTH-03A-Muster der
        // Bestandstests).
        {
            let (new_tx, _new_rx) = mpsc::unbounded_channel();
            let mut world = ctx.shared.lock().await;
            // Der RAM-Player bleibt unangetastet; der Commit übernimmt ihn und
            // aktualisiert nur die verbindungsbezogenen Felder.
            let candidate = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 1,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-2".into(),
                entities: Default::default(),
                last_activity: std::time::Instant::now(),
                tx: new_tx.clone(),
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
            };
            let outcome = crate::world::commit_login(
                &mut world,
                8,
                candidate,
                crate::world::ConnectionFields {
                    tx: new_tx,
                    session_id: "sess-2".into(),
                    lang: "de".into(),
                },
            )
            .unwrap();
            assert_eq!(
                outcome,
                crate::world::CommitOutcome::Takeover { old_conn_id: 7 }
            );
        }
        release.notify_one();
        let cleaned = task.await.unwrap();
        assert!(
            !logout_called.load(std::sync::atomic::Ordering::SeqCst),
            "ein verdrängter Owner darf kein logout_at schreiben"
        );
        assert!(!cleaned, "verdrängte Verbindung meldet kein Player-Cleanup");
        {
            let world = ctx.shared.lock().await;
            assert!(
                crate::world::is_owner(&world, 8, "hero"),
                "neuer Owner aktiv"
            );
            assert!(!world.by_conn.contains_key(&7), "alte conn_id entfernt");
            assert_eq!(world.by_conn.len(), 1, "genau ein Owner");
            assert!(world.players.contains_key("hero"), "Player erhalten");
            assert_eq!(world.players["hero"].session_id, "sess-2");
        }
    }

    /// AUTH-03: Ein WebSocket-Lesefehler beendet die Schleife kontrolliert
    /// AUTH-03: Ein WebSocket-Lesefehler beendet die Schleife kontrolliert
    /// (kein vorzeitiges `?`) — der zentrale Cleanup-Pfad läuft danach immer.
    /// Geprüft wird die Sequenz read_loop -> finish_conn: der Lesefehler
    /// überspringt das Cleanup nicht, und das Cleanup arbeitet
    /// verbindungsspezifisch.
    #[tokio::test]
    async fn read_error_runs_through_central_cleanup() {
        let ctx = test_ctx().await;
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut guard = crate::security::ConnGuard::default();
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let read_error = futures_util::stream::iter(vec![Err::<Message, tungstenite::Error>(
            tungstenite::Error::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "read error",
            )),
        )]);
        // Schleife kehrt kontrolliert zurück (kein Err, kein Abbruch per `?`).
        read_loop(&ctx, &tx, 4242, &mut guard, &sec_cfg, read_error).await;
        // Der Endpfad läuft danach für dieselbe conn_id …
        finish_conn(&ctx, 4242).await;
        {
            let world = ctx.shared.lock().await;
            // … und entfernt für eine nie eingeloggte Verbindung nichts.
            assert!(world.players.is_empty());
            assert!(world.by_conn.is_empty());
            assert!(!world.closers.contains_key(&4242));
            assert!(!world.peer_addrs.contains_key(&4242));
        }
        // Gleicher Pfad für eine Eigentümer-Verbindung: Spieler-Cleanup läuft.
        {
            let (close_tx, _close_rx) = tokio::sync::oneshot::channel::<()>();
            let mut world = ctx.shared.lock().await;
            world.closers.insert(77, close_tx);
            world.peer_addrs.insert(77, "203.0.113.7".to_string());
            let (ptx, _prx) = mpsc::unbounded_channel();
            let p = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 100,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-1".into(),
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
            };
            world.players.insert("hero".into(), p);
            world.by_conn.insert(77, "hero".into());
        }
        finish_conn(&ctx, 77).await;
        let world = ctx.shared.lock().await;
        assert!(
            !world.players.contains_key("hero"),
            "Player nicht bereinigt"
        );
        assert!(!world.by_conn.contains_key(&77));
        assert!(!world.closers.contains_key(&77));
        // Peer-Adresse nur im RAM, verlässt sie beim Verbindungsende.
        assert!(!world.peer_addrs.contains_key(&77));
    }

    /// AUTH-03 (Interleaving): Takeover WÄHREND eines laufenden
    /// Disconnect-Flushs der alten Verbindung.
    ///
    /// Reihenfolge: alter Owner beginnt den Disconnect → der echte
    /// Persistenzpfad wird zwischen Snapshot-Erfassung und Write angehalten →
    /// neuer Owner übernimmt und mutiert den Player → der alte Persistenzvorgang
    /// läuft weiter.
    ///
    /// Erwartung: kein `logout_at` für die weiterhin aktive Sitzung, neuer
    /// Owner und Player bleiben bestehen, und Dirty-State/Revision des neuen
    /// Owners bleiben korrekt (§15/§39: der ältere Snapshot darf die
    /// Revision nicht zurücksetzen und den Dirty-Status nicht löschen).
    #[tokio::test]
    async fn takeover_during_persist_flush_skips_logout_and_keeps_new_owner() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let ctx = test_ctx().await;
        {
            let mut world = ctx.shared.lock().await;
            let (close_tx, _close_rx) = tokio::sync::oneshot::channel::<()>();
            let (ptx, _prx) = mpsc::unbounded_channel();
            let mut p = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 100,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-1".into(),
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
                persist_revision: 7,
            };
            p.last_activity = std::time::Instant::now();
            world.players.insert("hero".into(), p);
            world.by_conn.insert(7, "hero".into());
            world.closers.insert(7, close_tx);
            world.peer_addrs.insert(7, "203.0.113.7".to_string());
        }
        // Gruppe: `on_disconnect` darf für die verdrängte Sitzung nicht laufen.
        let gid = {
            let mut groups = ctx.groups.lock().await;
            groups
                .create_group("hero", std::time::Instant::now())
                .unwrap()
        };
        // Laufzeitänderung der ALTEN Sitzung -> Position dirty, Generation 1.
        crate::handlers::handle_move(
            &ctx.shared,
            7,
            &serde_json::json!({"dir": [1.0, 0.0]}),
            1000,
        )
        .await;
        let (x_before, gen_before) = {
            let world = ctx.shared.lock().await;
            let p = &world.players["hero"];
            (p.x, p.persist_generation)
        };
        assert!(gen_before >= 1, "Ausgangslage ohne dirty Player");

        // Flush-Effekt: echter zentraler Persistenzpfad, aber die Phase-2
        // (durable Write) wird angehalten, während Phase 1 den Snapshot bereits
        // unter der World-Sperre erfasst hat.
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel::<()>();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let release_flush = release.clone();
        let shared = ctx.shared.clone();
        let spool = ctx.persist.spool().clone();
        let flush: DisconnectFlush = Box::new(move || {
            let shared = shared.clone();
            let spool = spool.clone();
            let release = release_flush.clone();
            Box::pin(async move {
                let res = crate::persist::persist_dirty_into(
                    &shared,
                    "hero",
                    true,
                    |snapshot| async move {
                        // Snapshot liegt vor, der Schreibvorgang wartet.
                        let _ = entered_tx.send(());
                        release.notified().await;
                        spool.write_batch(&snapshot)
                    },
                )
                .await;
                res
            })
        });
        let logout_called = std::sync::Arc::new(AtomicBool::new(false));
        let seen = logout_called.clone();
        let logout: DisconnectLogout = Box::new(move |_logout_at| {
            let seen = seen.clone();
            Box::pin(async move {
                seen.store(true, Ordering::SeqCst);
                Ok(())
            })
        });

        // Alter Owner beginnt den Disconnect (läuft in den Flush hinein).
        let task = {
            let ctx = ctx.clone();
            tokio::spawn(async move { finish_owner(&ctx, 7, "hero", flush, logout).await })
        };
        entered_rx
            .await
            .expect("Flush nicht erreicht — Disconnect übersprungen");

        // Neuer Owner übernimmt, während der alte Schreibvorgang wartet …
        {
            let mut world = ctx.shared.lock().await;
            let (new_tx, _new_rx) = mpsc::unbounded_channel();
            let mut candidate = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 999.0,
                y: 999.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 1,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-2".into(),
                entities: Default::default(),
                last_activity: std::time::Instant::now(),
                tx: new_tx.clone(),
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
            };
            candidate.last_activity = std::time::Instant::now();
            let outcome = crate::world::commit_login(
                &mut world,
                8,
                candidate,
                crate::world::ConnectionFields {
                    tx: new_tx,
                    session_id: "sess-2".into(),
                    lang: "de".into(),
                },
            )
            .unwrap();
            assert_eq!(
                outcome,
                crate::world::CommitOutcome::Takeover { old_conn_id: 7 }
            );
        }
        // … und mutiert den Player (eigene Sitzung, höhere Generation).
        crate::handlers::handle_move(
            &ctx.shared,
            8,
            &serde_json::json!({"dir": [0.0, 1.0]}),
            1000,
        )
        .await;
        // Erwartungswerte des neuen Owners festhalten (unveränderter RAM-Stand).
        let expected = {
            let world = ctx.shared.lock().await;
            let p = &world.players["hero"];
            (p.x, p.y, p.hp, p.persist_generation)
        };

        // Alter Persistenzvorgang wird fortgesetzt.
        release.notify_one();
        let cleaned = task.await.unwrap();

        // Kein `logout_at` für die weiterhin aktive Sitzung.
        assert!(
            !logout_called.load(Ordering::SeqCst),
            "logout_at wurde für die aktive Sitzung des neuen Owners geschrieben"
        );
        assert!(
            !cleaned,
            "verdrängte Verbindung darf kein Player-Cleanup melden"
        );

        let world = ctx.shared.lock().await;
        // Neuer Owner und Player bleiben bestehen.
        assert!(world.players.contains_key("hero"), "Player entfernt");
        assert!(crate::world::is_owner(&world, 8, "hero"));
        assert!(!world.by_conn.contains_key(&7));
        assert_eq!(world.by_conn.len(), 1);
        // RAM-Zustand des neuen Owners unangetastet vom alten Snapshot.
        let p = &world.players["hero"];
        assert_eq!(
            (p.x, p.y),
            (expected.0, expected.1),
            "Position überschrieben"
        );
        assert_eq!(p.x, x_before, "x-Position des neuen Owners verändert");
        assert!(p.y > 0.0, "Bewegung des neuen Owners fehlt");
        assert_eq!(p.hp, 100, "HP des neuen Owners überschrieben");
        assert_eq!(p.session_id, "sess-2");
        // §39: Revision folgt dem durable Stand (hier: RAM 7 -> Snapshot 8)
        // und wird NICHT auf den alten Stand zurückgesetzt.
        assert_eq!(p.persist_revision, 8, "Persistenzrevision regressiert");
        // §15: der ältere Snapshot (Generation 1) darf den Dirty-Status des
        // neuen Owners (Generation 2) NICHT löschen — der neuere RAM-Zustand
        // geht damit nicht verloren.
        assert_eq!(p.persist_generation, 2, "Generation unerwartet verändert");
        assert!(
            p.dirty.is_dirty(crate::persist::PersistComponent::Position),
            "Dirty-Status des neuen Owners wurde vom alten Flush gelöscht"
        );
        drop(world);

        // Gruppenstatus unangetastet (on_disconnect lief nicht).
        let groups = ctx.groups.lock().await;
        assert!(
            groups.get_group(gid).unwrap().members["hero"].online,
            "Gruppenstatus des neuen Owners auf offline gesetzt"
        );
    }

    /// AUTH-03 (Restfenster `logout_at`): Der `logout_at`-Write wird VOR
    /// seinem Abschluss blockiert; währenddessen startet ein neuer Login, der
    /// über dasselbe per-player-Gate den Commit ausführen will.
    ///
    /// Deterministisch (nur Gates/Kanäle, kein Sleep):
    /// 1. Eigentümerprüfung unmittelbar vor `logout_at` läuft durch,
    /// 2. der DB-Write blockiert,
    /// 3. der neue Login wartet am Gate und schließt den Commit NICHT ab,
    /// 4. nach Freigabe: Logout-Write → DB-Reset des Logins → Commit,
    /// 5. Endzustand: neuer Owner aktiv und `logout_at = NULL` (der
    ///    Disconnect-Commit schließt vorher vollständig ab, der Login
    ///    registriert danach neu).
    #[tokio::test]
    async fn login_waits_for_logout_write_and_leaves_no_active_logout() {
        let ctx = test_ctx().await;
        {
            let mut world = ctx.shared.lock().await;
            let (close_tx, _close_rx) = tokio::sync::oneshot::channel::<()>();
            let (ptx, _prx) = mpsc::unbounded_channel();
            let mut p = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 100,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-1".into(),
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
                persist_revision: 7,
            };
            p.last_activity = std::time::Instant::now();
            world.players.insert("hero".into(), p);
            world.by_conn.insert(7, "hero".into());
            world.closers.insert(7, close_tx);
        }

        // Fake-DB-Spalte `logout_at` + Reihenfolge-Protokoll.
        let column = std::sync::Arc::new(std::sync::Mutex::new(None::<i64>));
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
        let shared = ctx.shared.clone();
        let spool = ctx.persist.spool().clone();
        let order_flush = order.clone();
        // Alter Owner: Disconnect-Commit mit echtem Persistenzpfad …
        let flush: DisconnectFlush = Box::new(move || {
            let shared = shared.clone();
            let spool = spool.clone();
            let order = order_flush.clone();
            Box::pin(async move {
                let res = crate::persist::persist_dirty_into(
                    &shared,
                    "hero",
                    true,
                    |snapshot| async move { spool.write_batch(&snapshot) },
                )
                .await;
                order.lock().unwrap().push("flush_done");
                res
            })
        });
        // … und blockierendem `logout_at`-Write.
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel::<()>();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let release_write = release.clone();
        let column_logout = column.clone();
        let order_logout = order.clone();
        // Der Write-Typ ist `Fn` (mehrere Versuche möglich), deshalb wird der
        // einmalige Signal-Sender pro Aufruf entnommen statt bewegt.
        let entered = std::sync::Arc::new(std::sync::Mutex::new(Some(entered_tx)));
        let logout: DisconnectLogout = Box::new(move |logout_at| {
            let column = column_logout.clone();
            let order = order_logout.clone();
            let release = release_write.clone();
            let entered = entered.clone();
            Box::pin(async move {
                // Eigentümerprüfung (2) ist passiert, der Write startet …
                order.lock().unwrap().push("logout_write_start");
                if let Some(tx) = entered.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                release.notified().await;
                // … und landet erst hier vollständig in der DB.
                order.lock().unwrap().push("logout_write_done");
                *column.lock().unwrap() = Some(logout_at);
                Ok(())
            })
        });
        let disconnect = {
            let ctx = ctx.clone();
            tokio::spawn(async move { finish_owner(&ctx, 7, "hero", flush, logout).await })
        };

        // Der Write blockiert: die Prüfung davor ist durchgelaufen.
        entered_rx.await.expect("Logout-Write nicht erreicht");
        assert_eq!(
            *order.lock().unwrap(),
            vec!["flush_done", "logout_write_start"]
        );

        // Neuer Login startet und nimmt dasselbe per-player-Gate (wie
        // `handle_hello`). Der Marker VOR dem Gate-Lock zeigt, dass er läuft.
        let login = {
            let ctx = ctx.clone();
            let order = order.clone();
            let column = column.clone();
            tokio::spawn(async move {
                order.lock().unwrap().push("login_started");
                let gate = ctx.persist.player_gate("hero").await;
                let _g = gate.lock_owned().await;
                // Simulierter DB-Reset des Logins (db::save_progression(..., None)).
                let _ = column.lock().unwrap().take();
                order.lock().unwrap().push("login_db_reset");
                let (new_tx, _new_rx) = mpsc::unbounded_channel();
                let mut world = ctx.shared.lock().await;
                let mut candidate = crate::world::Player {
                    id: "hero".into(),
                    name: "hero".into(),
                    x: 999.0,
                    y: 999.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 1,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 7,
                    session_id: "sess-2".into(),
                    entities: Default::default(),
                    last_activity: std::time::Instant::now(),
                    tx: new_tx.clone(),
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
                };
                candidate.last_activity = std::time::Instant::now();
                let outcome = crate::world::commit_login(
                    &mut world,
                    8,
                    candidate,
                    crate::world::ConnectionFields {
                        tx: new_tx,
                        session_id: "sess-2".into(),
                        lang: "de".into(),
                    },
                );
                order.lock().unwrap().push("commit");
                outcome
            })
        };

        // Warten, bis der Login läuft — und belegen, dass er am Gate steht.
        loop {
            if order.lock().unwrap().contains(&"login_started") {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            *order.lock().unwrap(),
            vec!["flush_done", "logout_write_start", "login_started"],
            "Login passierte das Gate, obwohl der Logout-Write lief"
        );

        // Interleaving kontrolliert freigeben. Der Disconnect-Commit läuft
        // vollständig durch (er war beim Start des Logins noch Eigentümer),
        // erst danach kommt der Login am Gate vorbei.
        release.notify_one();
        assert!(
            disconnect.await.unwrap(),
            "Eigentümer-Disconnect muss seinen Commit vollständig abschließen"
        );
        assert_eq!(
            login.await.unwrap().unwrap(),
            crate::world::CommitOutcome::Registered,
            "Login muss nach dem Disconnect-Commit neu registrieren"
        );

        // Reihenfolge garantiert: Logout-Write VOR dem DB-Reset des Logins.
        assert_eq!(
            *order.lock().unwrap(),
            vec![
                "flush_done",
                "logout_write_start",
                "login_started",
                "logout_write_done",
                "login_db_reset",
                "commit"
            ]
        );
        // Endzustand: neuer Owner aktiv, kein wirksames logout_at.
        let world = ctx.shared.lock().await;
        assert!(crate::world::is_owner(&world, 8, "hero"));
        assert!(!world.by_conn.contains_key(&7));
        assert_eq!(world.by_conn.len(), 1);
        assert!(world.players.contains_key("hero"));
        assert_eq!(world.players["hero"].session_id, "sess-2");
        drop(world);
        assert_eq!(
            *column.lock().unwrap(),
            None,
            "logout_at markiert die aktive Sitzung des neuen Owners"
        );
    }

    /// AUTH-03 (Login nach fehlgeschlagenem Disconnect-Flush, Teil 1): Der
    /// Disconnect-Save scheitert. Der Player muss im autoritativen RAM
    /// BLEIBEN (docs/Player_Persistenz.md §16) und der am Gate wartende Login
    /// muss ihn übernehmen — darf aber keinen aus einer älteren DB-Zeile
    /// gebauten Player als aktive Instanz registrieren.
    #[tokio::test]
    async fn failed_disconnect_flush_retains_ram_player_and_login_adopts_it() {
        let ctx = test_ctx().await;
        {
            let mut world = ctx.shared.lock().await;
            let (close_tx, _close_rx) = tokio::sync::oneshot::channel::<()>();
            let (ptx, _prx) = mpsc::unbounded_channel();
            let mut p = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 11.0,
                y: 22.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 77,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-1".into(),
                entities: Default::default(),
                last_activity: std::time::Instant::now(),
                tx: ptx,
                char_class: "Adventurer".into(),
                class: crate::class::ClassStatus::Adventurer,
                faction_transition: false,
                level: 4,
                exp: 900,
                free_attr_points: 0,
                rested_pool: 0,
                idia: 555,
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
                persist_revision: 12,
            };
            p.last_activity = std::time::Instant::now();
            p.mark_dirty(crate::persist::PersistComponent::Position);
            p.mark_dirty(crate::persist::PersistComponent::Idia);
            world.players.insert("hero".into(), p);
            world.by_conn.insert(7, "hero".into());
            world.closers.insert(7, close_tx);
        }
        // Spool-Verzeichnis unbenutzbar machen: der Snapshot kann nicht
        // geschrieben werden (`write_batch` schlägt fehl).
        let spool_dir = ctx.persist.spool().base_dir.join("spool");
        let _ = std::fs::remove_dir_all(&spool_dir);
        let _ = std::fs::write(&spool_dir, "kein verzeichnis");
        let flush_persist = ctx.persist.clone();
        let flush_shared = ctx.shared.clone();
        let flush: DisconnectFlush = Box::new(move || {
            let persist = flush_persist.clone();
            let shared = flush_shared.clone();
            Box::pin(async move {
                persist
                    .persist_player_gate_held(&shared, "hero", true)
                    .await
            })
        });
        let logout: DisconnectLogout = Box::new(move |_ts| Box::pin(async move { Ok(()) }));
        // Der Disconnect-Commit läuft komplett (er war Eigentümer).
        assert!(
            finish_owner(&ctx, 7, "hero", flush, logout).await,
            "Eigentümer-Disconnect muss den Commit abschließen"
        );
        {
            // §16: Player bleibt im RAM, Dirty-State/Revision unverändert,
            // nur die Eigentümerschaft ist freigegeben.
            let world = ctx.shared.lock().await;
            let p = world
                .players
                .get("hero")
                .expect("Player muss im RAM bleiben");
            assert_eq!((p.x, p.y, p.hp, p.idia), (11.0, 22.0, 77, 555));
            assert_eq!(p.persist_revision, 12);
            assert_eq!(p.persist_generation, 2);
            assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Position));
            assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Idia));
            assert!(world.by_conn.is_empty(), "Eigentümerschaft muss frei sein");
        }
        // Der Login (am selben Gate, wie handle_hello) übernimmt den RAM-Player.
        let gate = ctx.persist.player_gate("hero").await;
        let _g = gate.lock_owned().await;
        let (new_tx, _new_rx) = mpsc::unbounded_channel();
        let mut candidate = crate::world::Player {
            id: "hero".into(),
            name: "hero".into(),
            x: 0.0,
            y: 0.0,
            face: 0.0,
            ping_ms: 0,
            zone_id: 0,
            hp: 1,
            max_hp: 1,
            lang: "de".into(),
            account_id: 7,
            session_id: "sess-2".into(),
            entities: Default::default(),
            last_activity: std::time::Instant::now(),
            tx: new_tx.clone(),
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
            mana: 1,
            max_mana: 1,
            effects: Vec::new(),
            cooldowns: Default::default(),
            active_cast: None,
            learned_abilities: Default::default(),
            attributes: Default::default(),
            max_hp_base: 1,
            max_mana_base: 1,
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
        };
        candidate.last_activity = std::time::Instant::now();
        let outcome = {
            let mut world = ctx.shared.lock().await;
            crate::world::commit_login(
                &mut world,
                8,
                candidate,
                crate::world::ConnectionFields {
                    tx: new_tx,
                    session_id: "sess-2".into(),
                    lang: "de".into(),
                },
            )
            .unwrap()
        };
        assert_eq!(outcome, crate::world::CommitOutcome::Adopted);
        let world = ctx.shared.lock().await;
        // Der DB-Kandidat (hp 1, idia 0, level 1) wurde NICHT aktiv.
        let p = &world.players["hero"];
        assert_eq!((p.x, p.y, p.hp, p.idia, p.level), (11.0, 22.0, 77, 555, 4));
        assert_eq!(p.session_id, "sess-2");
        assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Idia));
        assert!(crate::world::is_owner(&world, 8, "hero"));
        assert_eq!(world.by_conn.len(), 1);
    }

    /// AUTH-03 (Teil 2): Liegt ein NEUERER Snapshot im Spool als die DB-Zeile,
    /// ist der Login fail-closed. Geprüft werden der echte Spool-Zugriff und
    /// die Entscheidungsfunktion, die `handle_hello` auswertet.
    #[tokio::test]
    async fn login_is_fail_closed_while_newer_snapshot_is_pending_in_spool() {
        let ctx = test_ctx().await;
        let shared = ctx.shared.clone();
        {
            let mut world = ctx.shared.lock().await;
            let (ptx, _prx) = mpsc::unbounded_channel();
            let mut p = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 3.0,
                y: 4.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 100,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-1".into(),
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
                persist_revision: 7,
            };
            p.last_activity = std::time::Instant::now();
            p.mark_dirty(crate::persist::PersistComponent::Position);
            world.players.insert("hero".into(), p);
        }
        // Finaler Save: Snapshot mit Revision 8 liegt danach im Spool.
        ctx.persist
            .persist_player_gate_held(&shared, "hero", true)
            .await
            .expect("Spool-Write muss gelingen");
        // Ein zweiter Spieler: dessen Login ist NICHT betroffen.
        assert_eq!(ctx.persist.pending_revision("hero"), Ok(Some(8)));
        assert_eq!(ctx.persist.pending_revision("other"), Ok(None));
        // DB-Zeile steht noch auf Revision 7 → Login wird abgewiesen.
        assert!(crate::world::db_row_is_stale(
            7,
            ctx.persist.pending_revision("hero")
        ));
        // Nach dem Drain (Datei weg) ist die DB-Zeile wieder aktuell.
        let removed = ctx
            .persist
            .spool()
            .base_dir
            .join("spool")
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("json"));
        std::fs::remove_file(removed.expect("Batch-Datei")).unwrap();
        assert_eq!(ctx.persist.pending_revision("hero"), Ok(None));
        assert!(!crate::world::db_row_is_stale(
            7,
            ctx.persist.pending_revision("hero")
        ));
    }
}
