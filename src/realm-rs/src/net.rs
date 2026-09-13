// net — WebSocket-Server + Message-Dispatcher (Port von src/realm net.ts).
// Drahtformat {seq, type, data} als JSON. Ungültiges JSON wird ignoriert
// (kein Crash). Unbekannte Typen werden geloggt. Disconnect: Parental-
// State abräumen, Position speichern, DESPAWN-Broadcast, Registry putzen.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use sqlx::{MySql, Pool};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::auth_api::AuthApi;
use crate::config::Config;
use crate::db;
use crate::group::SharedGroups;
use crate::handlers::{self, Ctx};
use crate::parental::{self, SharedParental};
use crate::protocol::{c2s, Frame};
use crate::world::{close_conn, disconnect_player, Shared};

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub async fn serve(
    cfg: Arc<Config>,
    db: Pool<MySql>,
    auth: AuthApi,
    shared: Shared,
    parental: SharedParental,
    groups: SharedGroups,
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
        let (sock, _) = listener
            .accept()
            .await
            .map_err(|e| format!("websocket accept: {e}"))?;
        let ctx = ctx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(ctx, sock).await {
                log::error!("connection: {e}");
            }
        });
    }
}

async fn handle_conn(ctx: Arc<Ctx>, sock: tokio::net::TcpStream) -> Result<(), String> {
    let ws = tokio_tungstenite::accept_async(sock)
        .await
        .map_err(|e| format!("ws handshake: {e}"))?;
    log::info!("client connected");
    let conn_id = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = ws.split();
    // Spielzustand -> Socket läuft über einen Kanal (siehe world.rs);
    // gezieltes Schließen (HELLO-Ablehnung, Force-Logout) über closer.
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let (close_tx, close_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut world = ctx.shared.lock().await;
        world.closers.insert(conn_id, close_tx);
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

    while let Some(msg) = stream.next().await {
        let msg = msg.map_err(|e| format!("ws read: {e}"))?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };
        let frame: Frame = match serde_json::from_str(&text) {
            Ok(f) => f,
            Err(_) => continue, // ungültiges JSON ignorieren, kein Crash
        };
        dispatch(&ctx, &tx, conn_id, frame).await;
    }

    // Disconnect: Parental-State abräumen, Position + EXP speichern,
    // DESPAWN-Broadcast, Registry putzen, Gruppenzustand (§9) aktualisieren.
    let pid: Option<String> = {
        let mut world = ctx.shared.lock().await;
        world.closers.remove(&conn_id);
        let pid = world.by_conn.get(&conn_id).cloned();
        if let Some(ref pid) = pid {
            if let Some(me) = world.players.get(pid) {
                let (id, x, y, level, exp, free_attr_points, rested_pool, gold) = (
                    me.id.clone(),
                    me.x,
                    me.y,
                    me.level,
                    me.exp,
                    me.free_attr_points,
                    me.rested_pool,
                    me.gold,
                );
                let mut inventory = me.inventory.clone();
                drop(world);
                // Rested-EXP §12: Logout-Zeitpunkt (Epoch-Sekunden) für die
                // einmalige Rested-Berechnung beim nächsten Login festhalten.
                let logout_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                db::save_position(&ctx.db, &id, x, y).await;
                db::save_progression(
                    &ctx.db,
                    &id,
                    level,
                    exp,
                    free_attr_points,
                    rested_pool,
                    Some(logout_at),
                )
                .await;
                // Loot System V1: Goldstand persistieren (Fehler nur loggen).
                db::save_gold(&ctx.db, &id, gold).await;
                // Inventory V1: Grundinventar/Rucksäcke/Equipment persistieren.
                // Der Sicherheits-Puffer (temporär) verfällt beim Logout
                // (docs/inventory_system.md §11); er wird nie persistiert,
                // daher sind hier keine DB-Aufträumungen nötig.
                if let Err(e) = db::save_inventory(&ctx.db, &id, &inventory).await {
                    log::error!("disconnect saveInventory {id}: {e}");
                }
                inventory.drop_buffer();
                let mut world = ctx.shared.lock().await;
                disconnect_player(&mut world, pid);
            }
        }
        pid
    };
    if let Some(pid) = pid {
        parental::detach(&ctx.parental, &pid).await;
        let mut groups = ctx.groups.lock().await;
        groups.on_disconnect(&pid, std::time::Instant::now());
        log::info!("client disconnected: {pid}");
    }
    forward.abort();
    Ok(())
}

async fn dispatch(ctx: &Arc<Ctx>, tx: &mpsc::UnboundedSender<String>, conn_id: u64, frame: Frame) {
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
        // NPC_TALK / AUCTION_*: künftig (wie Übergangsstand).
        other => log::info!("unknown type {other}"),
    }
}
