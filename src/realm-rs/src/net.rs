// net — WebSocket-Server + Message-Dispatcher (Port von src/realm net.ts).
// Drahtformat {seq, type, data} als JSON. Ungültiges JSON wird ignoriert
// (kein Crash). Unbekannte Typen werden geloggt. Disconnect: Parental-
// State abräumen, Position speichern, DESPAWN-Broadcast, Registry putzen.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
use crate::protocol::{c2s, Frame};
use crate::world::{close_conn, Shared};

/// Finaler Disconnect-Save (Stufe B, `force`) als Effekt. `Err` bedeutet:
/// Snapshot nicht durable geschrieben — der Player bleibt dann im RAM (§16).
type DisconnectFlush = Box<dyn FnOnce() -> BoxFuture<'static, Result<(), String>> + Send>;
/// `logout_at`-Write (docs/Player_Persistenz.md §23) als Effekt.
type DisconnectLogout = Box<dyn FnOnce(i64) -> BoxFuture<'static, ()> + Send>;

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
                // finalen Disconnect-Save, docs §23), sondern direkt.
                if let Err(e) = db::write_logout_at(&pool, &pid, logout_at).await {
                    log::error!("disconnect saveLogoutAt {pid}: {e}");
                }
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
    write_logout(logout_at).await;
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

    /// Testkontext ohne DB-Verbindung: der Pool wird lazy geöffnet und in
    /// diesen Tests nicht benutzt (getestet werden Lesepfad und Cleanup).
    /// Die Konfiguration wird im Speicher gebaut (kein Datei-I/O, damit der
    /// Test nicht von Mount-/Cache-Sichtbarkeit abhängt).
    async fn test_ctx() -> Arc<Ctx> {
        use std::collections::HashMap;
        let dir = std::env::temp_dir().join(format!(
            "andora-realm-net-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
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
        Arc::new(Ctx {
            cfg,
            db,
            auth,
            shared,
            parental,
            registry: crate::combat::ability::AbilityRegistry::new(),
            groups,
            quest: crate::quest::QuestService::new(),
            persist,
        })
    }

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
        let logout: DisconnectLogout = Box::new(move |logout_at| {
            let column = column_logout.clone();
            let order = order_logout.clone();
            let release = release_write.clone();
            let entered = entered_tx;
            Box::pin(async move {
                // Eigentümerprüfung (2) ist passiert, der Write startet …
                order.lock().unwrap().push("logout_write_start");
                let _ = entered.send(());
                release.notified().await;
                // … und landet erst hier vollständig in der DB.
                order.lock().unwrap().push("logout_write_done");
                *column.lock().unwrap() = Some(logout_at);
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
        let logout: DisconnectLogout = Box::new(move |_ts| Box::pin(async move {}));
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
