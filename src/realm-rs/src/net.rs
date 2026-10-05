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
    /// `P-26`: Charaktere, deren **aktueller** Zustand beim Shutdown nicht
    /// dauerhaft bestätigt gesichert wurde — der forcierte Spool-Save
    /// (`force = true`) ist fehlgeschlagen, es existiert daher weder ein
    /// bestätigter Spool-Batch **noch** ein anwendbarer DB-Transfer für
    /// diesen Zustand.
    ///
    /// Bewusst **getrennt** von `failed`/`skipped`: dort geht es um den
    /// direkten `logout_at`-Write, der nach `docs/Player_Persistenz.md` §23
    /// **nicht** Teil des Snapshots ist. Ein `logout_at`-Fehler bei
    /// erfolgreich gesichertem Snapshot ist deshalb **kein** ungesicherter
    /// Spielerzustand und wird hier nicht gezählt (kein Doppelzählen, keine
    /// Pauschalbehauptung).
    pub spool_failed: u32,
}

impl ShutdownLogoutReport {
    /// `P-26`: Wurde der aktuelle Zustand **mindestens eines** Charakters
    /// beim Shutdown nicht dauerhaft bestätigt gesichert?
    ///
    /// Maßgeblich ist ausschließlich `spool_failed`. Ein gescheiterter
    /// `logout_at`-Write (`failed`/`skipped`) ebenso wie ein gescheiterter
    /// finaler DB-Drain ändern diese Aussage **nicht**: der Snapshot liegt in
    /// diesen Fällen dauerhaft im Spool und wird bei der nächsten
    /// Start-Recovery angewendet (`docs/Player_Persistenz.md` §30/§41).
    pub(crate) fn snapshot_security_failed(&self) -> bool {
        self.spool_failed > 0
    }

    /// `P-26`: Abschlussentscheidung **nach** dem vollständigen Shutdown-
    /// Cleanup (finaler Drain, `pool.close()`).
    ///
    /// `None` = der aktuelle Zustand aller Charaktere ist dauerhaft bestätigt
    /// gesichert. `Some(_)` = mindestens ein aktueller Spielerzustand ist
    /// nicht bestätigt gesichert; der Text nennt ausschließlich die **Anzahl**
    /// und eine neutrale, stabile Fehlerklasse — **keine** Spielernamen und
    /// **keine** unbelegte Aussage darüber, woher ein älterer dauerhafter
    /// Stand stammt.
    pub(crate) fn snapshot_security_error(&self) -> Option<String> {
        if !self.snapshot_security_failed() {
            return None;
        }
        Some(format!(
            "graceful shutdown: {} Spielerzustände nicht dauerhaft gesichert (Fehlerklasse: durable_snapshot_unconfirmed)",
            self.spool_failed
        ))
    }
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
///
/// `P-26`: Zusätzlich wird je Charakter der forcierte Spool-Save gezählt
/// (`spool_failed`). Dieser Schritt läuft **vor** der Budget-/Skip-
/// Entscheidung und daher für **alle** gelisteten Charaktere; Budgetablauf,
/// `skipped` oder ein Timeout des Logout-Writes verhindern ihn nicht.
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
        spool_failed: 0,
    };
    for id in online {
        // (1) Deadline-Prüfung **zuerst**, noch vor jedem anderen Schritt: ist
        // die harte Deadline abgelaufen, startet für diesen Charakter **kein**
        // weiterer direkter `logout_at`-Write.
        let expired = deadline.saturating_duration_since(Instant::now()).is_zero();
        // (2) Der bestehende finale Spool-Save läuft unverändert für **alle**
        // Spieler weiter; seine Semantik wird nicht verändert — auch nicht für
        // Charaktere, deren direkter Logout-Write ausfällt. Er ist ein lokaler
        // Dateischreibvorgang **ohne DB-Zugriff**, steht aber außerhalb des
        // 30-S-Budgets und kann es daher rechnerisch überschreiten.
        //
        // `P-26`: Dieser Schritt liegt bewusst **vor** der Budget-/Skip-
        // Entscheidung weiter unten. Budgetablauf, `skipped` und ein Timeout
        // des Logout-Writes können deshalb den forcierten Save **nicht**
        // verhindern; jeder gelistete Charakter durchläuft ihn. Ein Fehler
        // bedeutet: der aktuelle Zustand dieses Charakters ist nicht dauerhaft
        // bestätigt gesichert (§41) und wird deshalb gezählt. Der bestehende
        // Fehlerlog je Charakter bleibt unverändert.
        if let Err(e) = persist.persist_player(shared, id, true).await {
            report.spool_failed += 1;
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
    // `P-18`: Das persistente Cooldown-Set aus der **tatsächlich** geladenen
    // Registry in den World übernehmen. Bewusst VOR dem Binden der Listener
    // (`bind_addrs`/`TcpListener::bind` weiter unten): damit kann noch kein
    // Kampf stattgefunden haben, für den das Set noch fehlen würde.
    {
        let mut world = shared.lock().await;
        world.persistent_cooldown_ids = registry.persistent_cooldown_ids();
    }
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
    stream: S,
) where
    S: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    // Produktion liest die Uhr unverändert aus `Instant::now()`; die Uhr wird
    // nur als **Funktion** durchgereicht, damit die Rate-Stufe **am echten
    // `read_loop`** deterministisch prüfbar ist (Audit 4.4, L-1). Ohne diese
    // Seam müsste ein Test annehmen, dass sein Lauf sicher unter 1000 ms
    // bleibt — das wäre eine Annahme, kein Nachweis. Es gibt **keine**
    // verhaltensändernde Konfigurationsmöglichkeit: die Uhr ist kein
    // WebSocket-Eingang und nicht von außen steuerbar.
    //
    // Die Uhr wird bewusst **nicht** hier abgefragt: `dispatch` fragt sie erst
    // nach der World-Sperre ab, damit die Abfragezeit wie in der Basis
    // unmittelbar vor dem Gate liegt (siehe Kommentar dort).
    read_loop_with_clock(ctx, tx, conn_id, guard, sec_cfg, stream, Instant::now).await
}

/// Wie [`read_loop`], aber mit injizierbarer Uhrquelle.
///
/// Verhaltensneutral gegenüber `read_loop`: im Betrieb wird ausschließlich
/// `read_loop` aufgerufen, das `Instant::now` setzt. Die Parameterübergabe
/// dient allein der Testbarkeit der Rate-Stufe; der **Abfragezeitpunkt** der
/// Uhr bleibt der der Basis (in `dispatch`, nach der World-Sperre).
///
/// Nachweis des Abfragezeitpunkts:
/// `clock_is_queried_only_after_the_world_lock_is_released`.
async fn read_loop_with_clock<S, F>(
    ctx: &Arc<Ctx>,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    guard: &mut crate::security::ConnGuard,
    sec_cfg: &crate::security::SecurityCfg,
    mut stream: S,
    now: F,
) where
    S: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
    F: Fn() -> Instant + Sync,
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
        if dispatch(ctx, tx, conn_id, guard, sec_cfg, frame, &now).await {
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
    let _logout_gate = ctx.persist.player_gate(player_id).await.lock_owned().await;
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
    now: &(dyn Fn() -> Instant + Sync),
) -> bool {
    // Sequenz vermerken (Lag-tolerant: Duplikat/Out-of-Order ist kein Cheat,
    // wird nur vermerkt — keine Ablehnung, keine Verurteilung).
    guard.note_seq(frame.seq);
    let authenticated = {
        let world = ctx.shared.lock().await;
        world.by_conn.contains_key(&conn_id)
    };
    // Die Uhr wird **hier** abgefragt — also nach dem Ermitteln von
    // `authenticated` und nach dem Freigeben der World-Sperre, unmittelbar vor
    // dem Gate. Das ist exakt die Position, an der die Uhr vor der
    // Testbarkeit-Seam direkt beim `gate_frame`-Aufruf gelesen wurde (Basis
    // `9d590502`). Zwischen dieser Abfrage und `gate_frame` liegt bewusst
    // **kein** weiteres `await`: würde die Uhr früher abgefragt, wäre der
    // Zeitstempel unter Sperrkontention um die Wartezeit veraltet.
    match crate::security::gate_frame(sec_cfg, guard, frame.msg_type, authenticated, now()) {
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

    /// `P-26`: `PersistRuntime` über einem echten Spool-Stamm, dessen
    /// `spool/`-Unterverzeichnis bewusst **fehlt**. Der Durable-Write
    /// (`publish_new_file` → `File::create`) schlägt dadurch deterministisch
    /// fehl — dasselbe Muster wie der bestehende Nachweis
    /// `spool::tests::failed_spool_write_sets_degraded_and_keeps_dirty_and_revision`.
    ///
    /// Keine echte Datenbank, keine Betriebssystemstörung, keine
    /// Rechteveränderung: es fehlt lediglich ein Verzeichnis.
    fn p26_runtime_without_spool_dir() -> (crate::spool::PersistRuntime, PathBuf, Arc<TestDirGuard>)
    {
        let dir = alloc_test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let guard = Arc::new(TestDirGuard { path: dir.clone() });
        let rt = crate::spool::PersistRuntime::new(&dir, "ws").unwrap();
        std::fs::remove_dir_all(dir.join("spool")).unwrap();
        (rt, dir, guard)
    }

    /// Registriert `ids` als RAM-Spieler. Der Shutdown-Flush braucht keine
    /// Owner-Zuordnung; entscheidend ist nur die Anwesenheit in
    /// `world.players`, weil `persist_dirty_into` ausschließlich dort den
    /// Snapshot erfasst.
    async fn insert_players(shared: &crate::world::Shared, ids: &[&str]) {
        let mut world = shared.lock().await;
        for id in ids {
            let (ptx, _prx) = mpsc::unbounded_channel();
            world.players.insert(
                (*id).to_string(),
                crate::world::Player {
                    id: (*id).to_string(),
                    name: (*id).to_string(),
                    x: 0.0,
                    y: 0.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 100,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 1,
                    session_id: String::new(),
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
                    last_strike: None,
                    mana: 50,
                    max_mana: 50,
                    effects: Default::default(),
                    cooldowns: Default::default(),
                    active_cast: None,
                    learned_abilities: Default::default(),
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 100,
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
        }
    }

    /// `P-26` 1. Vollständig erfolgreiche Sicherung: Jeder gelistete Spieler
    /// erhält einen **dauerhaft veröffentlichten** Spool-Batch, es entsteht
    /// **kein** Fehlerergebnis, und der Shutdown darf als erfolgreich
    /// gemeldet werden.
    #[tokio::test]
    async fn p26_successful_forced_save_produces_no_persistence_error() {
        let ctx = test_ctx().await;
        insert_players(&ctx.shared, &["a", "b", "c"]).await;
        let online = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let make_write = |_id: &str| -> DisconnectLogout {
            Box::new(|_ts: i64| {
                Box::pin(async { Ok(()) }) as BoxFuture<'static, Result<(), String>>
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
        assert_eq!(report.spool_failed, 0, "kein Save darf fehlschlagen");
        assert!(!report.snapshot_security_failed());
        assert_eq!(report.snapshot_security_error(), None);
        // Der Zustand ist nicht nur "gemeldet erfolgreich", sondern tatsächlich
        // dauerhaft im Spool: für jeden Spieler liegt eine offene Batch-Datei
        // mit der gesicherten Revision.
        for id in ["a", "b", "c"] {
            assert!(
                matches!(ctx.persist.pending_revision(id), Ok(Some(1))),
                "Spieler {id} hat keinen dauerhaften Spool-Batch"
            );
        }
    }

    /// `P-26` 2. Der forcierte Spool-Save schlägt fehl: Der Fehler wird je
    /// Spieler gezählt, die **übrigen** Spieler werden weiterverarbeitet, die
    /// Dirty-Bits bleiben erhalten (kein vorgetäuschter Erfolg), der Status ist
    /// `DEGRADED` und die Abschlussentscheidung meldet den Fehler.
    ///
    /// Zusätzlich fehlt für einen Spieler der direkte `logout_at`-Write
    /// (permanent): beide Fehlerklassen werden **getrennt** gezählt, derselbe
    /// Spieler wird **nicht doppelt** in die Sicherheitsentscheidung gezählt.
    #[tokio::test]
    async fn p26_failed_forced_save_is_counted_and_reaches_the_exit_decision() {
        let ctx = test_ctx().await;
        insert_players(&ctx.shared, &["1001", "1002", "1003"]).await;
        let (persist, _dir, _guard) = p26_runtime_without_spool_dir();
        let online = vec!["1001".to_string(), "1002".to_string(), "1003".to_string()];
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c2 = calls.clone();
        let make_write = move |id: &str| -> DisconnectLogout {
            let id = id.to_string();
            let c = c2.clone();
            Box::new(move |_ts: i64| {
                let c = c.clone();
                let id = id.clone();
                Box::pin(async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if id == "1001" {
                        Err("db down".into())
                    } else {
                        Ok(())
                    }
                }) as BoxFuture<'static, Result<(), String>>
            })
        };
        let report = shutdown_logout_phase(
            &persist,
            &ctx.shared,
            &online,
            TEST_PLAN,
            Duration::from_secs(5),
            &no_wait(),
            &make_write,
        )
        .await;
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            5,
            "ein einzelner Fehler darf die übrigen Spieler nicht überspringen: \
             3 Versuche für '1001' (begrenzter Retry) + je 1 für '1002'/'1003'"
        );
        assert_eq!(
            report.spool_failed, 3,
            "alle drei Saves sind nicht bestätigt gesichert"
        );
        assert_eq!(report.failed, 1, "nur '1001' scheitert am logout_at-Write");
        assert!(report.snapshot_security_failed());
        let err = report
            .snapshot_security_error()
            .expect("nicht bestätigte Sicherung muss ein Fehlerergebnis liefern");
        assert!(err.contains('3'), "Fehlermeldung nennt die Anzahl: {err}");
        assert!(
            err.contains("durable_snapshot_unconfirmed"),
            "neutrale Fehlerklasse fehlt: {err}"
        );
        for id in ["1001", "1002", "1003"] {
            assert!(
                !err.contains(id),
                "Fehlermeldung darf keinen Spielernamen nennen: {err}"
            );
        }
        assert_eq!(
            persist.status(),
            crate::spool::PersistStatus::Degraded,
            "der Betriebsstatus bleibt DEGRADED"
        );
        // Kein vorgetäuschter Erfolg: Dirty-Bits bleiben gesetzt, es existiert
        // keine offene Batch-Datei für diese Spieler.
        {
            let world = ctx.shared.lock().await;
            for id in ["1001", "1002", "1003"] {
                assert!(
                    !world.players[id].dirty.any(),
                    "unbestätigter Save darf Dirty nicht bereinigen ({id})"
                );
            }
        }
        for id in ["1001", "1002", "1003"] {
            // `Ok(Some(_))` würde einen bestätigten Batch behaupten. Bei
            // fehlendem Spool-Verzeichnis liefert die Spool-Lesung stattdessen
            // `Err` — beides belegt: **kein** bestätigter Batch vorhanden.
            assert!(
                !matches!(persist.pending_revision(id), Ok(Some(_))),
                "es darf kein scheinbar gesicherter Batch existieren ({id})"
            );
        }
    }

    /// `P-26` 3. Budgetablauf, `skipped` und ein Timeout des Logout-Writes
    /// können den forcierten Save **nicht** verhindern: Der Save liegt in der
    /// Schleife bewusst **vor** der Skip-Entscheidung. Mit Budget 0 wird kein
    /// einziger DB-Write gestartet (`skipped == online.len()`), der
    /// Sicherungszähler erfasst aber trotzdem **alle** Spieler.
    #[tokio::test]
    async fn p26_budget_exhaustion_does_not_prevent_the_forced_save() {
        let ctx = test_ctx().await;
        insert_players(&ctx.shared, &["a", "b", "c"]).await;
        let (persist, _dir, _guard) = p26_runtime_without_spool_dir();
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
        let report = shutdown_logout_phase(
            &persist,
            &ctx.shared,
            &online,
            TEST_PLAN,
            Duration::ZERO,
            &no_wait(),
            &make_write,
        )
        .await;
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "nach Budgetablauf darf kein DB-Write starten"
        );
        assert_eq!(report.skipped, 3, "alle drei zählen als skipped");
        assert!(report.budget_exhausted);
        assert_eq!(
            report.spool_failed, 3,
            "der forcierte Save läuft unabhängig vom Budget für alle Spieler"
        );
        assert!(report.snapshot_security_failed());
    }

    /// `P-26` 4. Gescheiterter `logout_at`-Write bei **erfolgreicher**
    /// Snapshot-Sicherung ist **kein** ungesicherter Spielerzustand:
    /// `logout_at` gehört nicht zum Snapshot (§23), der Snapshot liegt
    /// dauerhaft im Spool und wird bei der Start-Recovery angewendet (§30).
    /// Der Shutdown darf hier als erfolgreich gemeldet werden.
    #[tokio::test]
    async fn p26_logout_failure_alone_is_no_unsecured_player_state() {
        let ctx = test_ctx().await;
        insert_players(&ctx.shared, &["dead", "recovered", "ok"]).await;
        let online = vec![
            "dead".to_string(),
            "recovered".to_string(),
            "ok".to_string(),
        ];
        let recovered_once = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let r2 = recovered_once.clone();
        let make_write = move |id: &str| -> DisconnectLogout {
            let id = id.to_string();
            let r = r2.clone();
            Box::new(move |_ts: i64| {
                let r = r.clone();
                let id = id.clone();
                Box::pin(async move {
                    match id.as_str() {
                        "dead" => Err("db down".into()),
                        "recovered" => {
                            if r.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                                Err("einmalig".into())
                            } else {
                                Ok(())
                            }
                        }
                        _ => Ok(()),
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
        assert_eq!(report.failed, 1, "nur 'dead' bleibt fehlgeschlagen");
        assert_eq!(
            report.retried, 2,
            "'dead' und 'recovered' brauchten jeweils mindestens einen Retry"
        );
        assert_eq!(
            report.spool_failed, 0,
            "der Snapshot wurde für alle drei Spieler gesichert"
        );
        assert!(
            !report.snapshot_security_failed(),
            "logout_at-Fehler ist kein ungesicherter Snapshot"
        );
        assert_eq!(report.snapshot_security_error(), None);
        // §41 Fall 2: Die Zustände bleiben lokal für die spätere Recovery
        // erhalten — jeder Spieler hat einen offenen Batch.
        for id in ["dead", "recovered", "ok"] {
            assert!(
                matches!(ctx.persist.pending_revision(id), Ok(Some(1))),
                "Batch von {id} bleibt für die Recovery erhalten"
            );
        }
    }

    /// `P-26` 5. Kein Fehlalarm: Ein Charakter, der zum Zeitpunkt des Flush
    /// nicht mehr in `world.players` steht, wurde zuvor bereits durch seinen
    /// Disconnect-Save dauerhaft gesichert (der Disconnect entfernt einen
    /// Player nur nach erfolgreichem Flush). `persist_dirty_into` liefert für
    /// ihn `Ok(())`; daraus darf **kein** Fehlerergebnis entstehen.
    #[tokio::test]
    async fn p26_player_removed_after_disconnect_is_not_an_unsecured_state() {
        let ctx = test_ctx().await;
        insert_players(&ctx.shared, &["live"]).await;
        // "gone" ist bewusst **nicht** in der World.
        let online = vec!["gone".to_string(), "live".to_string()];
        let make_write = |_id: &str| -> DisconnectLogout {
            Box::new(|_ts: i64| {
                Box::pin(async { Ok(()) }) as BoxFuture<'static, Result<(), String>>
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
        assert_eq!(
            report.spool_failed, 0,
            "ein bereits gesicherter, abwesender Charakter ist kein Fehlerfall"
        );
        assert_eq!(report.snapshot_security_error(), None);
        assert!(matches!(ctx.persist.pending_revision("live"), Ok(Some(1))));
    }

    /// `P-26` 6. Die Abschlussentscheidung selbst: Nur `spool_failed` erzeugt
    /// ein Fehlerergebnis. Reine Zählerentscheidung ohne DB und ohne Spool —
    /// sie ist die Produktionsfunktion, die `main.rs` nach dem Cleanup auswertet.
    #[test]
    fn p26_snapshot_security_decision_uses_only_the_forced_save_counter() {
        let base = ShutdownLogoutReport {
            retried: 0,
            failed: 0,
            skipped: 0,
            budget_exhausted: false,
            spool_failed: 0,
        };
        assert!(!base.snapshot_security_failed());
        assert_eq!(base.snapshot_security_error(), None);
        // Reiner `logout_at`-Fehler und reines Budgetüberschreiten ändern die
        // Sicherungsaussage nicht.
        let logout_only = ShutdownLogoutReport {
            retried: 4,
            failed: 3,
            skipped: 2,
            budget_exhausted: true,
            spool_failed: 0,
        };
        assert!(!logout_only.snapshot_security_failed());
        assert_eq!(logout_only.snapshot_security_error(), None);
        // Ein einziger nicht bestätigter Save genügt.
        let unconfirmed = ShutdownLogoutReport {
            spool_failed: 1,
            ..base
        };
        assert!(unconfirmed.snapshot_security_failed());
        let err = unconfirmed
            .snapshot_security_error()
            .expect("ein nicht bestätigter Save muss ein Fehlerergebnis liefern");
        assert!(err.contains('1'), "Anzahl fehlt: {err}");
        assert!(err.contains("durable_snapshot_unconfirmed"), "{err}");
    }

    /// `P-26` 7. Der Zähler ist **nicht** klebrig: Nach repariertem Spool
    /// sinkt er wieder auf 0, und derselbe Produktionspfad liefert kein
    /// Fehlerergebnis mehr. Damit ist belegt, dass die Entscheidung den
    /// tatsächlichen Sicherungsstand des jeweiligen Laufs beschreibt.
    #[tokio::test]
    async fn p26_counting_reflects_the_actual_save_result() {
        let ctx = test_ctx().await;
        insert_players(&ctx.shared, &["a"]).await;
        let online = vec!["a".to_string()];
        let make_write = |_id: &str| -> DisconnectLogout {
            Box::new(|_ts: i64| {
                Box::pin(async { Ok(()) }) as BoxFuture<'static, Result<(), String>>
            })
        };
        let (persist, dir, _guard) = p26_runtime_without_spool_dir();
        let failed = shutdown_logout_phase(
            &persist,
            &ctx.shared,
            &online,
            TEST_PLAN,
            Duration::from_secs(5),
            &no_wait(),
            &make_write,
        )
        .await;
        assert_eq!(failed.spool_failed, 1);
        assert!(failed.snapshot_security_failed());
        // Spool-Verzeichnis wiederherstellen: derselbe Produktionspfad meldet
        // jetzt eine bestätigte Sicherung.
        std::fs::create_dir_all(dir.join("spool")).unwrap();
        let ok = shutdown_logout_phase(
            &persist,
            &ctx.shared,
            &online,
            TEST_PLAN,
            Duration::from_secs(5),
            &no_wait(),
            &make_write,
        )
        .await;
        assert_eq!(ok.spool_failed, 0, "nach repariertem Spool kein Fehler");
        assert!(!ok.snapshot_security_failed());
        assert_eq!(ok.snapshot_security_error(), None);
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
                last_strike: None,
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
                last_strike: None,
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
                last_strike: None,
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
                last_strike: None,
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
                last_strike: None,
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
                last_strike: None,
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
                    last_strike: None,
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

    /// `P-18`: Der **erfolgreiche** Disconnect-Save erfasst die laufenden
    /// Cooldowns, bevor der Player entfernt wird. Geprüft wird die tatsächlich
    /// geschriebene Spool-Datei (Drahtformat), nicht ein Mock.
    ///
    /// Der Spool-Drain nach MariaDB ist hier **nicht** Teil der Prüfung; es wird
    /// ausdrücklich keine echte SQL-Ausführung behauptet.
    #[tokio::test]
    async fn p18_successful_disconnect_save_captures_cooldowns_before_player_removal() {
        let ctx = test_ctx().await;
        let ready_at =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000_123);
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
                last_strike: None,
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
            p.cooldowns.insert("fire_bolt".into(), ready_at);
            p.last_activity = std::time::Instant::now();
            world.players.insert("hero".into(), p);
            world.by_conn.insert(7, "hero".into());
            world.closers.insert(7, close_tx);
        }
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

        assert!(
            finish_owner(&ctx, 7, "hero", flush, logout).await,
            "Eigentümer-Disconnect muss den Commit abschließen"
        );

        // Player ist nach erfolgreichem Save entfernt (unverändert).
        {
            let world = ctx.shared.lock().await;
            assert!(
                !world.players.contains_key("hero"),
                "Nach erfolgreichem Flush wird der Player entfernt"
            );
        }

        // Die geschriebene Spool-Datei enthält den Ablaufzeitpunkt.
        let spool_dir = ctx.persist.spool().base_dir.join("spool");
        let files: Vec<_> = std::fs::read_dir(&spool_dir)
            .expect("Spool-Verzeichnis lesbar")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        assert_eq!(files.len(), 1, "genau eine Batch-Datei erwartet");
        let raw = std::fs::read_to_string(&files[0]).expect("Batch-Datei lesbar");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("Batch ist valides JSON");
        let cooldowns = &value["entries"][0]["cooldowns"];
        assert_eq!(cooldowns["fire_bolt"], 1_700_000_000_123i64);
        assert_eq!(
            cooldowns.as_object().unwrap().len(),
            1,
            "nur der laufende Cooldown wird gespeichert"
        );
    }

    /// `P-18`: Nach **fehlgeschlagenem** Disconnect-Save bleibt der Player im
    /// autoritativen RAM (§16) und die **Cooldowns bleiben erhalten**. Der
    /// nachfolgende Login adoptiert diesen Player — ein aus einer DB-Zeile
    /// gebauter Kandidat (leere Cooldown-Map) darf ihn nicht ersetzen.
    ///
    /// **Grenze der Aussage:** Geprüft wird der RAM-Übernahmepfad
    /// (`commit_login` → `Adopted`) nach einem **fehlgeschlagenen** Save. Das ist
    /// **kein** DB-/Login-Roundtrip: die Cooldowns werden hier bewusst **nicht**
    /// über die Datenbank geladen. Der Roundtrip über `character_cooldowns`
    /// (Snapshot → Drain → `load_character_cooldowns` → Kandidat) ist damit
    /// **nicht** abgedeckt und bleibt von der echten MariaDB-Integration abhängig.
    #[tokio::test]
    async fn p18_failed_disconnect_save_keeps_cooldowns_and_ram_adoption_keeps_them() {
        let ctx = test_ctx().await;
        let ready_at =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000_500);
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
                last_strike: None,
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
            p.cooldowns.insert("fire_bolt".into(), ready_at);
            p.mark_dirty(crate::persist::PersistComponent::Progression);
            world.players.insert("hero".into(), p);
            world.by_conn.insert(7, "hero".into());
            world.closers.insert(7, close_tx);
        }
        // Spool-Verzeichnis unbenutzbar machen: der Save schlägt fehl.
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
        assert!(finish_owner(&ctx, 7, "hero", flush, logout).await);

        // §16: Player bleibt im RAM — **mit** seinen Cooldowns.
        {
            let world = ctx.shared.lock().await;
            let p = world.players.get("hero").expect("Player bleibt im RAM");
            assert_eq!(
                p.cooldowns.get("fire_bolt").copied(),
                Some(ready_at),
                "Cooldown bleibt im autoritativen RAM erhalten"
            );
        }

        // Der Login übernimmt denselben RAM-Player (`Adopted`).
        let gate = ctx.persist.player_gate("hero").await;
        let _g = gate.lock_owned().await;
        let (new_tx, _new_rx) = mpsc::unbounded_channel();
        let candidate = crate::world::Player {
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
            last_strike: None,
            mana: 1,
            max_mana: 1,
            effects: Vec::new(),
            // Der Kandidat aus der DB-Zeile hätte **keine** Cooldowns.
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
            persist_revision: 99,
        };
        {
            let mut world = ctx.shared.lock().await;
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
            .expect("RAM-Player wird übernommen");
            assert_eq!(outcome, crate::world::CommitOutcome::Adopted);
            let p = &world.players["hero"];
            assert_eq!(
                p.cooldowns.get("fire_bolt").copied(),
                Some(ready_at),
                "Adoption darf die Cooldowns nicht leeren"
            );
            assert_eq!(p.persist_revision, 12, "RAM-Revision bleibt maßgeblich");
        }
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
                last_strike: None,
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
            last_strike: None,
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
                last_strike: None,
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

    // ── 4.7-Teilbefund: serverseitiger Angriffstakt ───────────────────────
    //
    // Der Client übermittelt Absichten (ATTACK). Zulässigkeit, Zeitpunkt und
    // Wirkung bestimmt der Server. `seq` ist dabei nur Korrelation
    // (`ConnGuard::note_seq`, Rückgabe verworfen) und darf niemals einen
    // zusätzlichen Schlag freischalten.
    //
    // Die folgenden Tests fahren den echten Produktionsweg: `dispatch`
    // (Frame → Whitelist/Gate → `handlers::handle_attack`) und danach
    // `combat::combat_tick` als Produktions-Schlagentscheidung. Der
    // Taktnachweis erfolgt über die vom Tick selbst geschriebenen Werte
    // (`CombatState.last_attack`) und über die Schadensanwendung am Ziel —
    // ohne Sleep und ohne Nachbildung der Taktformel im Test.

    /// Fester RNG-Wert: 0.9 ⇒ unter den Projektdefaults ein `Normal`-Treffer
    /// (miss<100, dodge<200, parry<250, block<350; Kritik ab 0.1 also nicht).
    /// Damit ist jeder ausgeführte Schlag exakt `weapon_damage` Schaden.
    struct FixedRoll(f64);

    impl crate::combat::CombatRng for FixedRoll {
        fn next(&mut self) -> f64 {
            self.0
        }
    }

    /// Legt Spieler an Positionen an und verbindet `conn_id` mit `a`.
    /// `ids` = (ID, x, y, hp).
    async fn insert_cadence_players(ctx: &Arc<Ctx>, ids: &[(&str, f64, f64, i32)]) {
        let mut w = ctx.shared.lock().await;
        for (id, x, y, hp) in ids {
            let (ptx, _prx) = mpsc::unbounded_channel();
            let mut p = crate::world::Player {
                id: (*id).to_string(),
                name: (*id).to_string(),
                x: *x,
                y: *y,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: *hp,
                max_hp: *hp,
                lang: "de".into(),
                account_id: 1,
                session_id: String::new(),
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
                last_strike: None,
                mana: 50,
                max_mana: 50,
                effects: Default::default(),
                cooldowns: Default::default(),
                active_cast: None,
                learned_abilities: Default::default(),
                attributes: Default::default(),
                max_hp_base: *hp,
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
            p.last_activity = std::time::Instant::now();
            w.players.insert((*id).to_string(), p);
        }
        w.by_conn.insert(7, "a".to_string());
    }

    /// Ein ATTACK-Frame über den echten Dispatcher (identische Absicht,
    /// wählbare `seq`).
    async fn attack_frame(
        ctx: &Arc<Ctx>,
        guard: &mut crate::security::ConnGuard,
        sec_cfg: &crate::security::SecurityCfg,
        tx: &mpsc::UnboundedSender<String>,
        seq: i64,
        data: serde_json::Value,
    ) {
        let frame = crate::protocol::Frame::new(seq, crate::protocol::c2s::ATTACK, data);
        assert!(
            !dispatch(ctx, tx, 7, guard, sec_cfg, frame, &Instant::now).await,
            "ATTACK-Frame darf hier keine Verbindung trennen"
        );
    }

    /// Eine Combat-Tick-Entscheidung mit übergebenem Zeitpunkt.
    async fn strike_tick(ctx: &Arc<Ctx>, now: std::time::Instant) {
        let groups = ctx.groups.lock().await;
        let mut w = ctx.shared.lock().await;
        crate::combat::combat_tick(
            &mut w,
            &ctx.cfg.combat,
            &ctx.cfg.loot,
            &ctx.cfg.progression,
            &groups,
            &mut FixedRoll(0.9),
            now,
            std::time::SystemTime::UNIX_EPOCH,
            ctx.cfg.aofb_radius,
        );
    }

    /// `P-4.7` Angriffstakt: erstmaliger Start schlägt sofort zu; wiederholte
    /// Absicht — mit **gleicher** und mit **neuer** `seq` — erzeugt innerhalb
    /// der Waffendauer keinen weiteren Schlag; nach Ablauf der Dauer ist der
    /// nächste Schlag wieder zulässig.
    #[tokio::test]
    async fn attack_intent_repeats_never_accelerate_the_server_side_cadence() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100), ("b", 1.0, 0.0, 1000)]).await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();
        let (tx, _rx) = mpsc::unbounded_channel();
        let duration = ctx.cfg.combat.weapon_duration_ms;
        let dmg = ctx.cfg.combat.weapon_damage;

        // 1) Erstmaliger zulässiger Start → Sofortschlag im ersten Tick.
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            100,
            serde_json::json!({"target_id": "b"}),
        )
        .await;
        {
            let w = ctx.shared.lock().await;
            assert_eq!(
                w.players["a"].combat.as_ref().map(|c| c.target_id.clone()),
                Some("b".to_string()),
                "gültige Absicht muss bewaffnen"
            );
        }
        strike_tick(&ctx, std::time::Instant::now()).await;
        let hp_after_first = {
            let w = ctx.shared.lock().await;
            w.players["b"].hp
        };
        assert_eq!(
            hp_after_first,
            1000 - dmg,
            "erstmaliger Start muss dokumentiert sofort schlagen"
        );

        // Takt-Referenz ist der vom Tick selbst geschriebene Zeitpunkt des
        // ausgeführten Schlags — kein Wert aus dem Test nachgebildet.
        let last_strike = {
            let w = ctx.shared.lock().await;
            w.players["a"]
                .combat
                .as_ref()
                .expect("Angriff bleibt aktiv")
                .last_attack
        };
        let just_before = last_strike + Duration::from_millis(duration - 1);
        let at_duration = last_strike + Duration::from_millis(duration);

        // 2) Identische Absicht mit GLEICHER seq → kein zusätzlicher Schlag.
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            100,
            serde_json::json!({"target_id": "b"}),
        )
        .await;
        strike_tick(&ctx, just_before).await;
        assert_eq!(
            ctx.shared.lock().await.players["b"].hp,
            hp_after_first,
            "wiederholtes ATTACK (gleiche seq) darf den Takt nicht zurücksetzen"
        );

        // 3) Gleiche Absicht mit NEUER seq → ebenfalls kein zusätzlicher Schlag.
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            101,
            serde_json::json!({"target_id": "b"}),
        )
        .await;
        strike_tick(&ctx, just_before).await;
        assert_eq!(
            ctx.shared.lock().await.players["b"].hp,
            hp_after_first,
            "neue seq darf den Takt nicht beschleunigen"
        );

        // 4) Nach Ablauf der Waffendauer ist der nächste Schlag zulässig.
        strike_tick(&ctx, at_duration).await;
        assert_eq!(
            ctx.shared.lock().await.players["b"].hp,
            hp_after_first - dmg,
            "nach Duration-Ablauf muss der nächste Schlag erfolgen"
        );
    }

    /// `P-4.7` Zielwechsel, Beenden/Neubeginn und Zieltod dürfen eine
    /// **laufende** serverseitige Wartezeit nicht umgehen. Alle drei Wege sind
    /// im Produktionscode erreichbar: `target_id`-Wechsel, `{"stop": true}`
    /// und der Tick-Entwaffnung bei totem Ziel.
    #[tokio::test]
    async fn attack_target_switch_stop_and_target_death_do_not_bypass_the_wait() {
        let ctx = test_ctx().await;
        insert_cadence_players(
            &ctx,
            &[
                ("a", 0.0, 0.0, 100),
                ("b", 1.0, 0.0, 1000),
                ("d", 0.0, 1.5, 1000),
                ("c", 1.5, 1.0, 10),
            ],
        )
        .await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();
        let (tx, _rx) = mpsc::unbounded_channel();
        let duration = ctx.cfg.combat.weapon_duration_ms;
        let dmg = ctx.cfg.combat.weapon_damage;

        // Start + erster Sofortschlag gegen "b".
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            10,
            serde_json::json!({"target_id": "b"}),
        )
        .await;
        strike_tick(&ctx, std::time::Instant::now()).await;
        let hp_b = {
            let w = ctx.shared.lock().await;
            assert_eq!(w.players["b"].hp, 1000 - dmg, "Sofortschlag erwartet");
            w.players["a"]
                .combat
                .as_ref()
                .expect("bewaffnet")
                .last_attack
        };
        let just_before = hp_b + Duration::from_millis(duration - 1);

        // Zielwechsel auf ein anderes gültiges Ziel umgeht die Wartezeit nicht.
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            11,
            serde_json::json!({"target_id": "d"}),
        )
        .await;
        strike_tick(&ctx, just_before).await;
        {
            let w = ctx.shared.lock().await;
            assert_eq!(w.players["b"].hp, 1000 - dmg, "kein Schlag auf b");
            assert_eq!(w.players["d"].hp, 1000, "kein Schlag auf d");
            assert_eq!(
                w.players["a"].combat.as_ref().map(|c| c.target_id.clone()),
                Some("d".to_string()),
                "Zielwechsel wird übernommen"
            );
        }

        // Beenden und unmittelbar neu beginnen umgeht sie ebenfalls nicht.
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            12,
            serde_json::json!({"stop": true}),
        )
        .await;
        assert!(
            ctx.shared.lock().await.players["a"].combat.is_none(),
            "Stop entwaffnet"
        );
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            13,
            serde_json::json!({"target_id": "b"}),
        )
        .await;
        strike_tick(&ctx, just_before).await;
        assert_eq!(
            ctx.shared.lock().await.players["b"].hp,
            1000 - dmg,
            "Neubeginn darf keinen Sofortschlag auslösen"
        );

        // Nach Ablauf der Wartezeit ist der Schlag wieder zulässig.
        let after_wait = hp_b + Duration::from_millis(duration);
        strike_tick(&ctx, after_wait).await;
        assert_eq!(
            ctx.shared.lock().await.players["b"].hp,
            1000 - 2 * dmg,
            "nach Ablauf muss der Schlag erfolgen"
        );

        // Zieltod: der Tick entwaffnet den Angreifer. Der unmittelbar
        // folgende Angriff auf ein anderes Ziel darf die Wartezeit nicht
        // umgehen.
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            14,
            serde_json::json!({"target_id": "c"}),
        )
        .await;
        let death_tick = after_wait + Duration::from_millis(duration);
        strike_tick(&ctx, death_tick).await;
        {
            let w = ctx.shared.lock().await;
            assert_eq!(w.players["c"].hp, 0, "Ziel c stirbt am Schlag");
            assert!(
                w.players["a"].combat.is_none(),
                "Zieltod entwaffnet den Angreifer"
            );
        }
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            15,
            serde_json::json!({"target_id": "d"}),
        )
        .await;
        strike_tick(&ctx, death_tick + Duration::from_millis(duration - 1)).await;
        assert_eq!(
            ctx.shared.lock().await.players["d"].hp,
            1000,
            "Angriff nach Zieltod darf keinen Sofortschlag auslösen"
        );
    }

    /// `P-4.7` Ungültige Absichten verändern den maßgeblichen Takt nicht: sie
    /// bewaffnen nicht, sie verwerfen keine laufende Wartezeit und sie
    /// verbrauchen den Sofortschlag der erstmaligen Aktivierung nicht.
    #[tokio::test]
    async fn invalid_attack_intent_neither_arms_nor_consumes_the_cadence() {
        let ctx = test_ctx().await;
        insert_cadence_players(
            &ctx,
            &[
                ("a", 0.0, 0.0, 100),
                ("b", 1.0, 0.0, 1000),
                ("far", 50.0, 0.0, 1000),
            ],
        )
        .await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();
        let (tx, _rx) = mpsc::unbounded_channel();
        let duration = ctx.cfg.combat.weapon_duration_ms;
        let dmg = ctx.cfg.combat.weapon_damage;

        // Ungültige Absichten: unbekanntes Ziel, eigenes Ziel, leere Ziel-ID,
        // Ziel außerhalb der Waffenreichweite.
        for (seq, data) in [
            (20, serde_json::json!({"target_id": "ghost"})),
            (21, serde_json::json!({"target_id": "a"})),
            (22, serde_json::json!({"target_id": ""})),
            (23, serde_json::json!({"target_id": "far"})),
        ] {
            attack_frame(&ctx, &mut guard, &sec_cfg, &tx, seq, data).await;
            assert!(
                ctx.shared.lock().await.players["a"].combat.is_none(),
                "ungültige Absicht darf nicht bewaffnen (seq {seq})"
            );
        }

        // Der dokumentierte Sofortschlag der erstmaligen Aktivierung besteht.
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            24,
            serde_json::json!({"target_id": "b"}),
        )
        .await;
        strike_tick(&ctx, std::time::Instant::now()).await;
        let hp_b = {
            let w = ctx.shared.lock().await;
            assert_eq!(
                w.players["b"].hp,
                1000 - dmg,
                "erstmalige Aktivierung schlägt sofort zu"
            );
            w.players["a"]
                .combat
                .as_ref()
                .expect("bewaffnet")
                .last_attack
        };

        // Eine ungültige Absicht während der Wartezeit verwirft den Angriff
        // nicht und beschleunigt den nächsten Schlag nicht.
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            25,
            serde_json::json!({"target_id": "far"}),
        )
        .await;
        assert_eq!(
            ctx.shared.lock().await.players["a"]
                .combat
                .as_ref()
                .map(|c| c.target_id.clone()),
            Some("b".to_string()),
            "ungültige Absicht darf einen laufenden Angriff nicht entwaffnen"
        );
        strike_tick(&ctx, hp_b + Duration::from_millis(duration - 1)).await;
        assert_eq!(
            ctx.shared.lock().await.players["b"].hp,
            1000 - dmg,
            "ungültige Absicht darf den Takt nicht verändern"
        );

        // Ein toter Angreifer erzeugt keinen Schlag (unveränderte V1-Regel).
        ctx.shared.lock().await.players.get_mut("a").unwrap().hp = 0;
        attack_frame(
            &ctx,
            &mut guard,
            &sec_cfg,
            &tx,
            26,
            serde_json::json!({"target_id": "b"}),
        )
        .await;
        strike_tick(&ctx, hp_b + Duration::from_millis(duration)).await;
        assert_eq!(
            ctx.shared.lock().await.players["b"].hp,
            1000 - dmg,
            "toter Angreifer schlägt nicht zu"
        );
    }

    // ── Audit 4.3 / T-1: Nachweis der Gate-Kette am echten `read_loop` ────
    //
    // Alle Tests in diesem Abschnitt rufen die **Produktionsfunktion**
    // `read_loop` mit echten `Message::Text`-Frames auf. Es wird bewusst
    // **keine** zweite Gate-Pipeline nachgebaut — anders als die test-eigene
    // `pipeline()` in `security.rs` (`security.rs:916`), die den echten
    // Empfangspfad nicht abbildet.
    //
    // **Reihenfolge-Beleg:** Die Aussage „vor dem Parse" wird **nicht** aus
    // fehlender Handler-Wirkung abgeleitet, sondern strukturell am
    // Produktionscode belegt: der Größen-Gate steht in `read_loop` bei
    // `net.rs:582` und endet mit `continue` (`:590`); der JSON-Parse
    // (`net.rs:592`) ist für einen übergroßen Frame dadurch unerreichbar.
    // Es folgen Typ-Whitelist (`:602`) und `dispatch` (`:612`); erst dort
    // liegen Sequenzbeobachtung (`net.rs:831`), Session-/Rate-Gate
    // (`security.rs:242`) und der Handler.
    //
    // **Zuordnung der Gate-Stufe:** Jede Stufe zählt über den vorhandenen
    // Diagnosewert `ConnGuard::violations` genau eine Auffälligkeit
    // (`net.rs:587`, `:596`, `:607`; Session-Zählung in `gate_frame`:
    // `security.rs:265`). Dieser Zähler
    // ordnet die Ablehnung eindeutig zu: ein unbekannter Typ, der die
    // Whitelist passieren würde, liefe im `other`-Zweig von `dispatch`
    // (`net.rs:929`) **ohne** Zählung; ein zu großer Frame, der die
    // Größenstufe passieren würde, würde geparst, gewhitelistet und
    // verlagert den Spieler.

    /// Baut einen echten C2S-Frame über den Produktions-Encoder.
    fn c2s_text(msg_type: i64, data: serde_json::Value) -> String {
        crate::protocol::Frame::new(1, msg_type, data).encode()
    }

    /// Füttert den echten `read_loop` mit echten Anwendungsframes.
    /// Verbindungs-ID 7 entspricht `insert_cadence_players`.
    async fn drive_read_loop(
        ctx: &Arc<Ctx>,
        guard: &mut crate::security::ConnGuard,
        sec_cfg: &crate::security::SecurityCfg,
        frames: Vec<String>,
    ) {
        let (tx, _rx) = mpsc::unbounded_channel();
        // Bewusst per Schleife statt per `map`: `tungstenite::Error` ist ein
        // großer `Err`-Typ, den ein `map`-Closure als Rückgabetyp aufspannt
        // (Clippy `result_large_err`) — die Basis hatte diese Warnung nicht.
        let mut items: Vec<Result<Message, tungstenite::Error>> =
            Vec::with_capacity(frames.len());
        for t in frames {
            items.push(Ok(Message::Text(t.into())));
        }
        let stream = futures_util::stream::iter(items);
        read_loop(ctx, &tx, 7, guard, sec_cfg, stream).await;
    }

    /// Füttert den echten `read_loop` mit **eingefrorener Uhr**.
    ///
    /// Audit 4.4 (L-1): Alle Frames liegen dadurch im selben 1000-ms-Fenster
    /// der Rate-Stufe. Das ist deterministisch und braucht **keine** Annahme
    /// darüber, dass der Testlauf sicher unter einer Sekunde bleibt, und
    /// keinen `sleep`. Die Uhrquelle selbst ist der einzige Testeingriff;
    /// der geprüfte Pfad (`read_loop` → `dispatch` → `gate_frame` →
    /// Handler) ist unverändert der Produktionspfad.
    async fn drive_read_loop_frozen(
        ctx: &Arc<Ctx>,
        guard: &mut crate::security::ConnGuard,
        sec_cfg: &crate::security::SecurityCfg,
        frames: Vec<String>,
    ) {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut items: Vec<Result<Message, tungstenite::Error>> =
            Vec::with_capacity(frames.len());
        for t in frames {
            items.push(Ok(Message::Text(t.into())));
        }
        let base = Instant::now();
        let stream = futures_util::stream::iter(items);
        read_loop_with_clock(ctx, &tx, 7, guard, sec_cfg, stream, || base).await;
    }

    /// F-1-Regression (Audit 4.4): Die Uhr darf **erst nach** der
    /// World-Sperre für `authenticated` abgefragt werden — also unmittelbar vor
    /// dem Gate, wie in der Basis `9d590502`. Wird sie früher abgefragt, ist
    /// der Zeitstempel unter Sperrkontention um die Wartezeit veraltet.
    ///
    /// **Ereignisgesteuert, ohne `sleep` und ohne Annahme über das
    /// Task-Scheduling:** Der Test hält die World-Sperre selbst fest. Ein
    /// `inspect`-Hook am Stream signalisiert, sobald der Frame *ausgeliefert*
    /// wurde — das passiert im selben Poll, in dem `dispatch` betreten wird,
    /// danach parkt der Read-Loop an der gesperrten World-Sperre. Erst wenn
    /// die Sperre freigegeben ist, darf die Uhr laufen.
    ///
    /// Vor der Korrektur (Uhrabfrage als Aufrufargument von `dispatch`) wäre
    /// `calls == 1`, **bevor** die Sperre freigegeben wird — der Test schlägt
    /// dann genau an dieser Zeile fehl.
    #[tokio::test]
    async fn clock_is_queried_only_after_the_world_lock_is_released() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100)]).await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);

        // Die World-Sperre selbst festhalten: `authenticated` kann erst ermittelt
        // werden, wenn der Test sie wieder freigibt. Der Guard bleibt hier im
        // Test-Scope, damit er gezielt freigegeben werden kann.
        let guard_world = ctx.shared.lock().await;
        let clock_calls = Arc::new(AtomicUsize::new(0));
        let clock_saw_free_lock = Arc::new(AtomicUsize::new(0));
        let base = Instant::now();
        let shared = ctx.shared.clone();

        let clock = {
            let calls = clock_calls.clone();
            let saw_free = clock_saw_free_lock.clone();
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                // War die World-Sperre zum Abfragezeitpunkt wieder frei?
                if shared.try_lock().is_ok() {
                    saw_free.store(1, Ordering::SeqCst);
                }
                base
            }
        };

        let (tx, _rx) = mpsc::unbounded_channel();
        let frame_yielded = Arc::new(tokio::sync::Notify::new());
        let stream = futures_util::stream::iter(vec![Ok::<
            Message,
            tungstenite::Error,
        >(Message::Text(
            c2s_text(crate::protocol::c2s::MOVE, serde_json::json!({"dir": [1, 0]}))
                .into(),
        ))])
        .inspect({
            let yielded = frame_yielded.clone();
            move |_| {
                yielded.notify_one();
            }
        });

        let mut rate_guard = crate::security::ConnGuard::default();
        let clock2 = clock.clone();
        let ctx_arc: Arc<Ctx> = ctx.ctx.clone();
        let reader = tokio::spawn(async move {
            read_loop_with_clock(&ctx_arc, &tx, 7, &mut rate_guard, &sec_cfg, stream, clock2).await;
        });

        // Ereignis: der Frame wurde ausgeliefert. Ab hier ist der Read-Loop
        // entweder an der World-Sperre geparkt (korrekt) oder hat die Uhr
        // bereits abgefragt (Fehlerfall).
        frame_yielded.notified().await;
        assert_eq!(
            clock_calls.load(Ordering::SeqCst),
            0,
            "die Uhr wurde abgefragt, bevor die World-Sperre freigegeben wurde"
        );

        // Sperre freigeben, dann den Read-Loop zu Ende laufen lassen.
        drop(guard_world);
        reader.await.expect("Read-Loop-Task");

        assert_eq!(
            clock_calls.load(Ordering::SeqCst),
            1,
            "die Uhr muss genau einmal abgefragt werden"
        );
        assert_eq!(
            clock_saw_free_lock.load(Ordering::SeqCst),
            1,
            "zum Abfragezeitpunkt muss die World-Sperre frei sein"
        );
    }

    /// L-1: Die Rate-Stufe wirkt **am echten `read_loop`**. Kleines
    /// Klassenbudget (`movement_per_sec = 2`), Uhr eingefroren:
    ///
    /// 1. die ersten zwei MOVEs passieren das Gate und **bewegen** den Spieler,
    /// 2. die folgenden zwei MOVEs derselben Klasse werden am Rate-Gate
    ///    verworfen — sichtbar an unveränderter Position und am
    ///    Verletzungszähler,
    /// 3. eine Nachricht **anderer** Klasse (ATTACK) wirkt trotzdem,
    /// 4. die Verbindung liest unterhalb der Verletzungsschwelle weiter.
    #[tokio::test]
    async fn read_loop_rate_gate_drops_excess_frames_and_keeps_other_classes() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100), ("b", 1.0, 0.0, 1000)]).await;
        let mut sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        sec_cfg.movement_per_sec = 2;
        let mut guard = crate::security::ConnGuard::default();

        let move_text = || {
            crate::protocol::Frame::new(
                1,
                crate::protocol::c2s::MOVE,
                serde_json::json!({"dir": [1, 0]}),
            )
            .encode()
        };
        drive_read_loop_frozen(
            &ctx,
            &mut guard,
            &sec_cfg,
            vec![
                move_text(),
                move_text(),
                move_text(),
                move_text(),
                c2s_text(
                    crate::protocol::c2s::ATTACK,
                    serde_json::json!({"target_id": "b"}),
                ),
            ],
        )
        .await;

        let world = ctx.shared.lock().await;
        // `dir:[1,0]` ist ein **1-Einheiten-Schritt**: `apply_move`
        // normalisiert die Richtung und begrenzt die Schrittlänge
        // (`min(|d|, 21 m)` bei tick_ms = 100). Genau zwei MOVEs dürfen
        // durch, also x = 2.0; die beiden weiteren verändern nichts.
        assert_eq!(
            world.players["a"].x, 2.0,
            "genau die zwei erlaubten MOVEs dürfen wirken"
        );
        assert_eq!(
            guard.violations, 2,
            "jeder Rate-Verstoß zählt genau einmal"
        );
        // Wirkungsnachweis der anderen Klasse: `combat` wird **bewaffnet**
        // (Zustandswechsel), nicht die Schlagfolge — der Taktfix darf die
        // Beobachtung nicht verdecken.
        let armed = world.players["a"].combat.as_ref().map(|c| c.target_id.clone());
        assert_eq!(
            armed.as_deref(),
            Some("b"),
            "ATTACK (andere Klasse) muss trotz erschöpftem MOVE-Budget wirken"
        );
    }

    /// L-1: Ein Rate-Verstoß **an der Verletzungsschwelle** beendet die
    /// Verarbeitung kontrolliert. `disconnect_after_violations = 2`:
    /// erster Verstoß ⇒ `Drop` (Verbindung liest weiter), zweiter ⇒
    /// `Disconnect`. Die danach folgenden Frames werden nicht mehr verarbeitet —
    /// belegt an der **ausbleibenden** Bewaffnung des Angriffs, nicht an einem
    /// unveränderten Combat-Takt.
    #[tokio::test]
    async fn read_loop_rate_violation_at_threshold_ends_processing() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100), ("b", 1.0, 0.0, 1000)]).await;
        let mut sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        sec_cfg.movement_per_sec = 1;
        sec_cfg.disconnect_after_violations = 2;
        let mut guard = crate::security::ConnGuard::default();

        let move_text = || {
            crate::protocol::Frame::new(
                1,
                crate::protocol::c2s::MOVE,
                serde_json::json!({"dir": [1, 0]}),
            )
            .encode()
        };
        drive_read_loop_frozen(
            &ctx,
            &mut guard,
            &sec_cfg,
            vec![
                move_text(),
                move_text(),
                move_text(),
                c2s_text(
                    crate::protocol::c2s::ATTACK,
                    serde_json::json!({"target_id": "b"}),
                ),
            ],
        )
        .await;

        let world = ctx.shared.lock().await;
        assert_eq!(world.players["a"].x, 1.0, "nur das erste MOVE darf wirken");
        assert_eq!(guard.violations, 2, "Pfad endet genau am Schwellwert");
        assert!(
            world.players["a"].combat.is_none(),
            "nach der Schwelle darf kein weiterer Frame verarbeitet werden"
        );
    }

    /// L-5: Nicht angemeldete, bekannte Spielnachrichten erhöhen den
    /// Verletzungszähler, verbrauchen **kein** Klassenbudget und lösen im
    /// Session-Zweig derzeit **auch beim Erreichen der Schwelle keinen
    /// Disconnect** aus. Hier am echten `read_loop` bestätigt: mit
    /// `disconnect_after_violations = 1` ist die Schwelle ab dem ersten
    /// Verstoß erreicht, die Schleife liest dennoch alle drei Frames.
    ///
    /// Das ist eine **offene Schutzentscheidung**, keine Aussage, dass das
    /// Verhalten ausreichend oder unbedenklich ist (Audit 4.4, R-5).
    #[tokio::test]
    async fn read_loop_session_violations_count_but_never_disconnect() {
        let ctx = test_ctx().await;
        // Player vorhanden, aber bewusst KEINE by_conn-Zuordnung.
        insert_players(&ctx.shared, &["a"]).await;
        let mut sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        sec_cfg.disconnect_after_violations = 1;
        let mut guard = crate::security::ConnGuard::default();

        drive_read_loop_frozen(
            &ctx,
            &mut guard,
            &sec_cfg,
            vec![
                c2s_text(
                    crate::protocol::c2s::MOVE,
                    serde_json::json!({"dir": [1, 0]}),
                );
                3
            ],
        )
        .await;

        assert_eq!(
            guard.violations, 3,
            "alle drei Frames wurden verarbeitet und gezählt (kein break)"
        );
        let world = ctx.shared.lock().await;
        assert_eq!(
            (world.players["a"].x, world.players["a"].y),
            (0.0, 0.0),
            "ohne Session darf keine Spielabsicht wirken"
        );
    }

    // Die folgenden Tests liegen bewusst hier (am Ende des Moduls) und nicht
    // neben `read_error_runs_through_central_cleanup`, weil sie dieselben
    // Helfer (`test_ctx`, `insert_cadence_players`, `insert_players`,
    // `SecurityCfg`) nutzen und die gemeinsame Lesereihenfolge
    // (`read_loop` → `dispatch` → Handler) an einem Ort dokumentieren.

    /// Übergroßer Text-Frame wird vor jeder Handler-Wirkung verworfen.
    /// Der Frame ist bewusst **gültiges JSON**, **gültiger Typ** und trägt
    /// eine wirksame `dir`-Angabe: fiele der Größen-Gate aus, bewegte sich
    /// der Spieler. Damit belegt der Test die Wirkung des Gates und nicht
    /// bloß das Fehlen einer Wirkung.
    #[tokio::test]
    async fn read_loop_drops_oversize_text_before_handler_effect() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100)]).await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();

        let oversize = c2s_text(
            crate::protocol::c2s::MOVE,
            serde_json::json!({"dir": [1, 0], "pad": "x".repeat(sec_cfg.max_frame_bytes)}),
        );
        assert!(
            oversize.len() > sec_cfg.max_frame_bytes,
            "Testframe muss die Grenze wirklich überschreiten"
        );
        drive_read_loop(&ctx, &mut guard, &sec_cfg, vec![oversize]).await;

        let world = ctx.shared.lock().await;
        let p = &world.players["a"];
        assert_eq!(
            (p.x, p.y),
            (0.0, 0.0),
            "übergroßer Frame darf keine Bewegung auslösen"
        );
        assert_eq!(guard.violations, 1, "Größen-Gate zählt genau eine Auffälligkeit");
    }

    /// Ungültiges JSON wird verworfen (kein Crash, keine Wirkung).
    #[tokio::test]
    async fn read_loop_drops_invalid_json_text() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100)]).await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();

        drive_read_loop(&ctx, &mut guard, &sec_cfg, vec!["{kein json".to_string()]).await;

        let world = ctx.shared.lock().await;
        let p = &world.players["a"];
        assert_eq!((p.x, p.y), (0.0, 0.0));
        assert_eq!(guard.violations, 1, "Parse-Gate zählt genau eine Auffälligkeit");
    }

    /// Unbekannter Nachrichtentyp wird an der Whitelist verworfen. Ohne die
    /// Whitelist liefe der Frame im `other`-Zweig (`net.rs:929`) und **ohne**
    /// Verletzungszählung; `violations == 1` ordnet ihn daher eindeutig der
    /// Whitelist zu.
    #[tokio::test]
    async fn read_loop_drops_unknown_message_type() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100)]).await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();

        drive_read_loop(
            &ctx,
            &mut guard,
            &sec_cfg,
            vec![c2s_text(4242, serde_json::json!({}))],
        )
        .await;

        let world = ctx.shared.lock().await;
        let p = &world.players["a"];
        assert_eq!((p.x, p.y), (0.0, 0.0));
        assert_eq!(guard.violations, 1, "Whitelist zählt genau eine Auffälligkeit");
    }

    /// Gültige, bekannte Nachricht einer **nicht eingeloggten** Verbindung
    /// wird am Session-Gate verworfen. Der Player existiert dabei
    /// ausdrücklich, ist aber über `by_conn` nicht verbunden — sonst ließe
    /// sich die Gate-Wirkung nicht von der Wirkungslosigkeit des Handlers
    /// unterscheiden (`handle_move` prüft selbst `by_conn`).
    #[tokio::test]
    async fn read_loop_drops_valid_known_message_without_session() {
        let ctx = test_ctx().await;
        // Spieler anlegen, aber bewusst KEINE by_conn-Zuordnung.
        insert_players(&ctx.shared, &["a"]).await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();

        drive_read_loop(
            &ctx,
            &mut guard,
            &sec_cfg,
            vec![c2s_text(
                crate::protocol::c2s::MOVE,
                serde_json::json!({"dir": [1, 0]}),
            )],
        )
        .await;

        let world = ctx.shared.lock().await;
        let p = &world.players["a"];
        assert_eq!(
            (p.x, p.y),
            (0.0, 0.0),
            "Session-Gate muss greifen, obwohl der Handler bereit wäre"
        );
        assert_eq!(guard.violations, 1, "Session-Gate zählt genau eine Auffälligkeit");
    }

    /// Kontrollnachweis: dieselbe Testumgebung erreicht über `read_loop`
    /// tatsächlich den Wirkungspfad. Ohne diesen Test wäre „keine Wirkung"
    /// in den Fällen oben nicht aussagekräftig.
    #[tokio::test]
    async fn read_loop_delivers_valid_known_message_to_handler() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100)]).await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();

        drive_read_loop(
            &ctx,
            &mut guard,
            &sec_cfg,
            vec![c2s_text(
                crate::protocol::c2s::MOVE,
                serde_json::json!({"dir": [1, 0]}),
            )],
        )
        .await;

        let world = ctx.shared.lock().await;
        let x = world.players["a"].x;
        assert!(x > 0.0, "gültige Nachricht muss den Handler erreichen (x={x})");
        assert_eq!(guard.violations, 0, "gültige Nachricht ist kein Gate-Verstoß");
    }

    /// Nach einzelnen verworfenen Frames wird weitergelesen, solange die
    /// vorhandene Verletzungsschwelle nicht erreicht ist: alle drei
    /// Ablehnungsarten nacheinander, danach wirkt die gültige Nachricht.
    #[tokio::test]
    async fn read_loop_keeps_reading_after_single_rejected_frames() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100)]).await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let mut guard = crate::security::ConnGuard::default();
        assert!(
            sec_cfg.disconnect_after_violations > 3,
            "Test setzt drei Ablehnungen unter die Schwelle"
        );
        let oversize = c2s_text(
            crate::protocol::c2s::MOVE,
            serde_json::json!({"dir": [1, 0], "pad": "x".repeat(sec_cfg.max_frame_bytes)}),
        );

        drive_read_loop(
            &ctx,
            &mut guard,
            &sec_cfg,
            vec![
                "{kein json".to_string(),
                oversize,
                c2s_text(4242, serde_json::json!({})),
                c2s_text(
                    crate::protocol::c2s::MOVE,
                    serde_json::json!({"dir": [1, 0]}),
                ),
            ],
        )
        .await;

        let world = ctx.shared.lock().await;
        assert!(
            world.players["a"].x > 0.0,
            "nach drei Ablehnungen wird weiter gelesen und verarbeitet"
        );
        assert_eq!(guard.violations, 3, "jede Ablehnung zählt genau einmal");
    }

    /// Beim vorhandenen Disconnect-Schwellwert endet der Pfad kontrolliert:
    /// bis zur Schwelle wird gezählt, der Folgeframe wird **nicht mehr**
    /// verarbeitet. `read_loop` kehrt dabei normal zurück (kein Panic).
    #[tokio::test]
    async fn read_loop_stops_at_existing_disconnect_threshold() {
        let ctx = test_ctx().await;
        insert_cadence_players(&ctx, &[("a", 0.0, 0.0, 100)]).await;
        let mut sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        sec_cfg.disconnect_after_violations = 3;
        let mut guard = crate::security::ConnGuard::default();

        drive_read_loop(
            &ctx,
            &mut guard,
            &sec_cfg,
            vec![
                "{kein json".to_string(),
                "{auch kein json".to_string(),
                c2s_text(4242, serde_json::json!({})),
                c2s_text(
                    crate::protocol::c2s::MOVE,
                    serde_json::json!({"dir": [1, 0]}),
                ),
            ],
        )
        .await;

        let world = ctx.shared.lock().await;
        let p = &world.players["a"];
        assert_eq!(
            (p.x, p.y),
            (0.0, 0.0),
            "Frame nach erreichter Schwelle darf nicht mehr wirken"
        );
        assert_eq!(guard.violations, 3, "Pfad endet genau am Schwellwert");
    }

    // ── Audit 4.3: Zahlen- und Arraygrenzen des MOVE-Pfads ───────────────
    //
    // Die Aussage „JSON erlaubt kein NaN, daher kein Rechenüberlauf" wird
    // hier **nicht** übernommen: JSON schließt NaN und ±∞ als Literale aus,
    // aber ein endlicher f64-Eingabewert garantiert **keine** endlichen
    // Zwischenergebnisse. Geprüft wird der tatsächliche Parse- und
    // Handlerpfad (Text-Frame → Parse → Whitelist → Session-Gate →
    // `handle_move` → `apply_move`).
    //
    // Hintergrund zum Nicht-Ändern: `apply_move` wäre für `dx = ±∞`
    // tatsächlich nicht endlich (`∞/∞ = NaN`, siehe `world.rs:679`). Diese
    // Eigenschaft ist über den Empfangspfad **nicht erreichbar**, weil der
    // Parser eine außerhalb des f64-Bereichs liegende Zahl ablehnt (siehe
    // `move_frame_numbers_outside_f64_range_never_reach_the_handler`). Der
    // Sicherheitsnachweis ist damit die **Parser-Grenze**, nicht die
    // Robustheit von `apply_move`. `apply_move` wird deshalb bewusst nicht
    // geändert.

    /// Fährt **einen** MOVE-Text-Frame über den echten `read_loop` und
    /// liefert die resultierende Position samt Verletzungszähler.
    async fn move_position_after(
        ctx: &Arc<Ctx>,
        sec_cfg: &crate::security::SecurityCfg,
        text: &str,
    ) -> (f64, f64, u32) {
        // Frischer Ausgangszustand je Fall.
        {
            let mut world = ctx.shared.lock().await;
            world.players.clear();
            world.by_conn.clear();
        }
        insert_cadence_players(ctx, &[("a", 0.0, 0.0, 100)]).await;
        let mut guard = crate::security::ConnGuard::default();
        drive_read_loop(ctx, &mut guard, sec_cfg, vec![text.to_string()]).await;
        let world = ctx.shared.lock().await;
        let p = &world.players["a"];
        (p.x, p.y, guard.violations)
    }

    /// Sehr große, aber endlich darstellbare Werte: `dir:[1e308, 0]`.
    /// Ergebnis muss eine **begrenzte, endliche** Bewegung sein. Der
    /// erwartete Weg ist die Normalisierung (`dx/len`) und der Cap
    /// (210 m/s × 100 ms Tick = 21 m), nicht das Versagen der Rechnung.
    #[tokio::test]
    async fn move_frame_with_finite_but_extreme_values_is_capped_not_overflowed() {
        let ctx = test_ctx().await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let (x, y, violations) = move_position_after(
            &ctx,
            &sec_cfg,
            r#"{"seq":1,"type":2,"data":{"dir":[1e308,0]}}"#,
        )
        .await;
        assert_eq!(violations, 0, "gültiger Frame ist kein Gate-Verstoß");
        assert!(x.is_finite() && y.is_finite(), "Position muss endlich sein ({x},{y})");
        assert_eq!(
            (x, y),
            (21.0, 0.0),
            "Richtungsnormierung und Speed-Cap müssen greifen"
        );
    }

    /// Zahlen, die der Parser nicht als f64 darstellen kann (`1e400`):
    /// `serde_json` weist sie ab („number out of range"), der Frame scheitert
    /// damit schon an der `Frame`-Deserialisierung und wird am Parse-Gate
    /// (`net.rs:592`) verworfen — **vor** Whitelist, Session-Gate und Handler.
    /// Damit ist `±∞` (und damit die NaN-Eigenschaft von `apply_move`) über
    /// den Empfangspfad nicht erreichbar.
    #[tokio::test]
    async fn move_frame_numbers_outside_f64_range_never_reach_the_handler() {
        let ctx = test_ctx().await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let text = r#"{"seq":1,"type":2,"data":{"dir":[1e400,0]}}"#;
        // Parser-Befund unabhängig vom Realm festhalten.
        assert!(
            serde_json::from_str::<crate::protocol::Frame>(text).is_err(),
            "out-of-range Zahl muss den Parser scheitern lassen"
        );
        let (x, y, violations) = move_position_after(&ctx, &sec_cfg, text).await;
        assert_eq!(violations, 1, "Parse-Gate zählt genau eine Auffälligkeit");
        assert_eq!(
            (x, y),
            (0.0, 0.0),
            "keine Bewegung und keine nicht endliche Position"
        );
    }

    /// Endliche Einzelwerte, deren **Betragsquadratsumme** überläuft
    /// (`hypot(1.7e308, 1.7e308) = ∞`): die Normalisierung teilt dann durch
    /// `∞` und ergibt 0. Das Ergebnis ist kontrolliert (keine Bewegung) und
    /// bleibt endlich — ein stilles No-op, kein Rechenüberlauf.
    #[tokio::test]
    async fn move_frame_with_overflowing_magnitude_stays_finite() {
        let ctx = test_ctx().await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        let (x, y, violations) = move_position_after(
            &ctx,
            &sec_cfg,
            r#"{"seq":1,"type":2,"data":{"dir":[1.7e308,1.7e308]}}"#,
        )
        .await;
        assert_eq!(violations, 0, "Frame ist syntaktisch und typseitig gültig");
        assert!(x.is_finite() && y.is_finite(), "Position muss endlich sein ({x},{y})");
        assert_eq!((x, y), (0.0, 0.0), "Überlauf führt zu kontrolliertem No-op");
    }

    /// Entartete und falsch typisierte `dir`-Formen: leeres Array, Array mit
    /// einem Element, falsche Elementtypen. Alle sind gültiges JSON und
    /// gültiger Typ, werden also **nicht** am Gate verworfen (keine
    /// Verletzungszählung), sondern vom Handler selbst folgenlos behandelt.
    #[tokio::test]
    async fn move_frame_with_degenerate_or_mistyped_dir_makes_no_movement() {
        let ctx = test_ctx().await;
        let sec_cfg = crate::security::SecurityCfg::from(&ctx.cfg.security);
        for text in [
            r#"{"seq":1,"type":2,"data":{"dir":[]}}"#,
            r#"{"seq":1,"type":2,"data":{"dir":[1]}}"#,
            r#"{"seq":1,"type":2,"data":{"dir":["a",null]}}"#,
            r#"{"seq":1,"type":2,"data":{"dir":null}}"#,
            r#"{"seq":1,"type":2,"data":{}}"#,
        ] {
            let (x, y, violations) = move_position_after(&ctx, &sec_cfg, text).await;
            assert_eq!(
                (x, y),
                (0.0, 0.0),
                "entartetes dir darf keine Bewegung auslösen: {text}"
            );
            assert!(
                x.is_finite() && y.is_finite(),
                "Position muss endlich bleiben: {text}"
            );
            assert_eq!(
                violations, 0,
                "diese Formen sind kein Gate-Verstoß, sondern Handler-Entscheidung: {text}"
            );
        }
    }
}
