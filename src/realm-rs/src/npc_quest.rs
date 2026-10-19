//! NPC-Angebot und ausdrückliche Questannahme auf dem bestehenden Questpfad.
//! Sperrfolge: Charakter-Gate -> World. DB niemals unter der World-Sperre.
//!
//! FIX_QUEST_ACCEPTANCE_RETRY_AND_SHUTDOWN_BLOCKING: Ein Uncertain-COMMIT
//! bleibt als ausstehende Annahme in `World.pending_quest_acceptances` nach-
//! vollbar erfasst. Es existiert kene endlicher Retry-Loop und kene losgelösten
//! unboundedly-running Task, der ein geordem Shutdown indefinitely verhindern.
//! `retry_quest_acceptances` (reused by Logout/Takeover, the periodic pass and
//! Shutdown) re-attempts the IDENTICAL immutable acceptance over the SAME
//! idempotent persist-path; on confirmed COMMIT it performs the RAM-Übernahme.
//! No false ACTIVE, no claimed rollback, no competing overwrite/progress-reset.
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::handlers::Ctx;
use crate::protocol::{s2c, Frame};
use crate::quest::{CharacterQuestState, QuestAcceptError, QuestError, QuestService, QuestState};
use crate::persist::QuestCompletionRequest;
use crate::world::{is_owner, Shared, World};

pub type AcceptFuture =
    Pin<Box<dyn Future<Output = Result<CharacterQuestState, QuestAcceptError>> + Send>>;

/// Kleine Persistenzgrenze für modellierte Offline-Nachweise; Produktion
/// verwendet ausschließlich QuestService::persist_acceptance und dieselbe DB.
pub trait AcceptStore: Send + Sync {
    fn write(&self, char_id: String, accepted: CharacterQuestState) -> AcceptFuture;
}

struct MySqlAcceptStore(sqlx::Pool<sqlx::MySql>);

impl AcceptStore for MySqlAcceptStore {
    fn write(&self, char_id: String, accepted: CharacterQuestState) -> AcceptFuture {
        let db = self.0.clone();
        Box::pin(async move { QuestService::persist_acceptance(&db, &char_id, &accepted).await })
    }
}

/// Production store from the Realm-Pool (no model, real persist-path).
pub fn production_store(db: sqlx::Pool<sqlx::MySql>) -> Arc<dyn AcceptStore> {
    Arc::new(MySqlAcceptStore(db))
}

/// Explicit Zeitgrenze um den **jeamlig** Questannahme-Persistenzversuch
/// (Acquire + SQL-Execute + COMMIT). Reuses the existing per-operation
/// Timeout-Muster (cf. `net.rs:190` logout `write_timeout`), NOT a global
/// Pool-Change. A Timeout is treated like an Uncertain COMMIT: the acceptance
/// stays outstanding (pending), NO false ACTIVE, NO claimed safe rollback.
pub const QUEST_ACCEPT_IO_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(2);

/// Handler-/Dispatch-Store: Production uses the Pool; the offline Nachweise
/// inject the modelled adapter via `Ctx.quest_accept_store` (test-only).
pub fn build_accept_store(ctx: &Ctx) -> Arc<dyn AcceptStore> {
    let production = production_store(ctx.db.clone());
    #[cfg(test)]
    return ctx.quest_accept_store.clone().unwrap_or(production);
    #[cfg(not(test))]
    production
}

/// An outstanding acceptance: the immutable content that must be re-attempted
/// identically. Keyed by char in `World.pending_quest_acceptances`; the
/// `quests`-Table stays the authority (§27.26), this is only the in-process
/// record that makes the open operation nachvollbar for Logout/Takeover/
/// Shutdown and the periodic resolution pass.
#[derive(Clone)]
pub struct PendingAcceptance {
    pub quest_id: String,
    pub accepted: CharacterQuestState,
}

fn pending_has(world: &World, pid: &str, quest_id: &str) -> bool {
    world.pending_quest_acceptances
        .get(pid)
        .iter()
        .flat_map(|v| v.iter())
        .any(|p| p.quest_id == quest_id)
}

fn reject(tx: &mpsc::UnboundedSender<String>, seq: i64, action: &str, npc: &str, reason: &str) {
    let _ = tx.send(
        Frame::new(
            seq,
            s2c::NPC_TEXT,
            json!({
                "ok": false, "action": action, "npc_id": npc, "reason": reason
            }),
        )
        .encode(),
    );
}

fn quest_reason(error: &QuestError) -> &'static str {
    match error {
        QuestError::UnknownQuest => "unknown_quest",
        QuestError::AlreadyActive => "quest_already_active",
        QuestError::AlreadyCompleted => "quest_already_completed",
        _ => "quest_not_available",
    }
}

/// Nur serverseitige NPC-Instanz und Charakterdaten. Die stabile giver-ID
/// wird ausschließlich mit Npc.content_id verglichen, niemals mit Spawn/Name.
fn interaction<'a>(
    world: &'a World,
    pid: &str,
    npc_id: &str,
    radius: f64,
) -> Result<&'a crate::npc::Npc, &'static str> {
    let player = world.players.get(pid).ok_or("unknown_player")?;
    if player.hp <= 0 {
        return Err("character_dead");
    }
    let npc = world.npcs.get(npc_id).ok_or("unknown_npc")?;
    if npc.status != crate::npc::NpcStatus::Alive || npc.hp <= 0 {
        return Err("npc_unavailable");
    }
    let distance = (player.x - npc.x).hypot(player.y - npc.y);
    if !distance.is_finite() || !radius.is_finite() || distance > radius {
        return Err("out_of_range");
    }
    Ok(npc)
}

fn completed(player: &crate::world::Player) -> HashSet<String> {
    player
        .quests
        .values()
        .filter(|q| q.state == QuestState::Completed)
        .map(|q| q.quest_id.clone())
        .collect()
}

pub async fn handle(
    ctx: &Ctx,
    tx: &mpsc::UnboundedSender<String>,
    conn: u64,
    seq: i64,
    data: &Value,
) {
    let action = data["action"].as_str().unwrap_or("").to_owned();
    let npc_id = data["npc_id"].as_str().unwrap_or("").to_owned();
    let quest_id = data["quest_id"].as_str().unwrap_or("").to_owned();
    let pid = {
        let world = ctx.shared.lock().await;
        let Some(pid) = world
            .by_conn
            .get(&conn)
            .filter(|pid| is_owner(&world, conn, pid))
        else {
            return;
        };
        pid.clone()
    };
    let shared = ctx.shared.clone();
    let persist = ctx.persist.clone();
    let service = ctx.quest.clone();
    let radius = ctx.cfg.loot.pickup_radius;
    let io = ctx.quest_io_timeout;
    let store: Arc<dyn AcceptStore> = build_accept_store(ctx);
    let tx = tx.clone();
    // The Worktask performs ONE bounded persist-attempt. It is cancellation-
    // fester only for that single attempt (a dropped receive-task must not
    // abort a mid-flight DB transaction). It NEVER loops: an Uncertain-COMMIT
    // is recorded in the registry and resolved later by retry_quest_acceptances.
    let task = tokio::spawn(async move {
        let _gate = persist.player_gate(&pid).await.lock_owned().await;
        let accepted = {
            let world = shared.lock().await;
            if !is_owner(&world, conn, &pid) {
                return;
            }
            let npc = match interaction(&world, &pid, &npc_id, radius) {
                Ok(npc) => npc,
                Err(reason) => {
                    reject(&tx, seq, &action, &npc_id, reason);
                    return;
                }
            };
            let player = &world.players[&pid];
            let done = completed(player);
            if action == "quest_offers" {
                let offers: Vec<Value> = service.definitions()
                    .filter(|def| def.giver.as_deref() == Some(npc.content_id.as_str()))
                    .filter(|def| !pending_has(&world, &pid, &def.id))
                    .filter(|def| service.state_for(def, player.quests.get(&def.id), player.level, &done) == QuestState::Available)
                    .map(|def| json!({"quest_id": def.id, "title_key": def.title_key,
                        "description_key": def.description_key, "state": "AVAILABLE", "can_accept": true,
                        "turn_in": def.turn_in,
                        "objectives": def.objectives.iter().map(|o| json!({
                            "id": o.id, "type": o.kind.key(), "target": o.target,
                            "required": o.required,
                        })).collect::<Vec<serde_json::Value>>()}))
                    .collect();
                let _ = tx.send(
                    Frame::new(
                        seq,
                        s2c::NPC_TEXT,
                        json!({"ok": true,
                    "action": action, "npc_id": npc_id, "quests": offers}),
                    )
                    .encode(),
                );
                return;
            }
            let Some(def) = service.find_definition(&quest_id) else {
                reject(&tx, seq, &action, &npc_id, "unknown_quest");
                return;
            };
            if def.giver.as_deref() != Some(npc.content_id.as_str()) {
                reject(&tx, seq, &action, &npc_id, "wrong_quest_giver");
                return;
            }
            // A still-outstanding acceptance for this (char,quest) never yields
            // a second state: it is reported as pending, no new attempt.
            if pending_has(&world, &pid, &quest_id) {
                reject(&tx, seq, &action, &npc_id, "quest_accept_pending");
                return;
            }
            // No Repeatable-Erweiterung: NPC-Offers/Annahme only for the
            // actually derived AVAILABLE, never COMPLETED.
            if service.state_for(def, player.quests.get(&quest_id), player.level, &done)
                != QuestState::Available
            {
                let reason = match player.quests.get(&quest_id).map(|q| q.state) {
                    Some(QuestState::Active) => "quest_already_active",
                    Some(QuestState::Completed) => "quest_already_completed",
                    _ => "quest_not_available",
                };
                reject(&tx, seq, &action, &npc_id, reason);
                return;
            }
            match service.accept(&quest_id, player.quests.get(&quest_id), player.level, &done) {
                Ok(state) => state,
                Err(error) => {
                    reject(&tx, seq, &action, &npc_id, quest_reason(&error));
                    return;
                }
            }
        };
        // Register the acceptance as OUTSTANDING BEFORE the timed attempt, so a
        // Cancellation between attempt, registration and RAM-Übernahme can never
        // lose it. The per-character Gate is held only over this bounded window.
        {
            let mut world = shared.lock().await;
            if world.players.contains_key(&pid) {
                if let Some(list) = world.pending_quest_acceptances.get_mut(&pid) {
                    if !list.iter().any(|p| p.quest_id == quest_id) {
                        list.push(PendingAcceptance {
                            quest_id: quest_id.clone(),
                            accepted: accepted.clone(),
                        });
                    }
                } else {
                    world.pending_quest_acceptances
                        .insert(pid.clone(), vec![PendingAcceptance {
                            quest_id: quest_id.clone(),
                            accepted: accepted.clone(),
                        }]);
                }
            }
        }
        // ONE bounded attempt: Acquire + SQL-Execute + COMMIT wrapped in the
        // existing Timeout-Muster (cf. net.rs logout write_timeout). No World-
        // Sperre during the DB-await. A Timeout == possible Uncertain COMMIT.
        match tokio::time::timeout(io, store.write(pid.clone(), accepted.clone())).await {
            Ok(Ok(state)) => {
                let mut world = shared.lock().await;
                if let Some(list) = world.pending_quest_acceptances.get_mut(&pid) {
                    list.retain(|x| x.quest_id != quest_id);
                }
                if world.pending_quest_acceptances.get(&pid).map(|v| v.is_empty()).unwrap_or(false) {
                    world.pending_quest_acceptances.remove(&pid);
                }
                if let Some(player) = world.players.get_mut(&pid) {
                    player.quests.insert(quest_id.clone(), state.clone());
                }
                if is_owner(&world, conn, &pid) {
                    if state.state == QuestState::Active {
                        let _ = tx.send(
                            Frame::new(
                                seq,
                                s2c::NPC_TEXT,
                                json!({"ok": true,
                            "action": action, "npc_id": npc_id, "quest_id": quest_id, "state": "ACTIVE"}),
                            )
                            .encode(),
                        );
                    } else {
                        reject(
                            &tx,
                            seq,
                            &action,
                            &npc_id,
                            if state.state == QuestState::Completed {
                                "quest_already_completed"
                            } else {
                                "quest_not_available"
                            },
                        );
                    }
                }
                return;
            }
            Ok(Err(QuestAcceptError::Uncertain(_))) => {
                // Uncertain COMMIT: entry stays pending (registered above). NO
                // ACTIVE, NO rollback claim.
                let world = shared.lock().await;
                if is_owner(&world, conn, &pid) {
                    reject(&tx, seq, &action, &npc_id, "quest_accept_pending");
                }
                return;
            }
            Ok(Err(_)) => {
                // Error BEFORE any COMMIT: drop the pending entry, no RAM, no
                // ACTIVE. (Distinct from a Timeout, which stays pending.)
                let mut world = shared.lock().await;
                if let Some(list) = world.pending_quest_acceptances.get_mut(&pid) {
                    list.retain(|x| x.quest_id != quest_id);
                }
                if world.pending_quest_acceptances.get(&pid).map(|v| v.is_empty()).unwrap_or(false) {
                    world.pending_quest_acceptances.remove(&pid);
                }
                reject(&tx, seq, &action, &npc_id, "quest_persistence_failed");
                return;
            }
            Err(_elapsed) => {
                // TIMEOUT: the whole attempt (Acquire+SQL+COMMIT) exceeded the
                // bound; a COMMIT may have landed -> keep the pending entry,
                // report pending (NO false ACTIVE, NO claimed rollback). The
                // Gate is released at the end of this block.
                let world = shared.lock().await;
                if is_owner(&world, conn, &pid) {
                    reject(&tx, seq, &action, &npc_id, "quest_accept_pending");
                }
                return;
            }
        }
    });
    if let Err(error) = task.await {
        log::error!("Questannahme-Arbeitstask fehlgeschlagen: {error}");
    }
}

/// Resolve outststanding acceptances over the SAME idempotent persist-path.
/// Bounded: exactly ONE attempt per pending entry per call, each attempt
/// (Acquire + SQL + COMMIT) wrapped in the explicit IO-Timeout `io`. Acquires
/// the per-character Gate (Gate -> World), never holds the World-Sperre during
/// the DB-await. On confirmed COMMIT it performs the RAM-Übernahme (only if the
/// character is still present) and removes the pending entry. Uncertain, a
/// transient Persistence error OR a Timeout KEEP the entry (the `quests`-Table
/// remains the authority; the next pass / Logout / Takeover / Shutdown retries
/// identically). Character-specific callers pass `requested` so they never wait
/// on a foreign (other character's) acceptance. No detached, unbounded task.
pub async fn retry_quest_acceptances(
    store: Arc<dyn AcceptStore>,
    persist: &Arc<crate::spool::PersistRuntime>,
    shared: Shared,
    requested: Option<&[String]>,
    io: std::time::Duration,
) -> Result<usize, String> {
    let targets: Vec<(String, Vec<PendingAcceptance>)> = {
        let world = shared.lock().await;
        let mut out: Vec<(String, Vec<PendingAcceptance>)> = Vec::new();
        for (id, list) in world.pending_quest_acceptances.iter() {
            if requested.is_none_or(|ids| ids.contains(id)) && !list.is_empty() {
                out.push((id.to_owned(), (*list).clone()));
            }
        }
        out
    };
    let mut resolved = 0;
    for (pid, list) in targets {
        let guard = persist.player_gate(&pid).await.lock_owned().await;
        for pa in list {
            let present = {
                let world = shared.lock().await;
                world.pending_quest_acceptances
                    .get(&pid)
                    .iter()
                    .flat_map(|v| v.iter())
                    .any(|x| x.quest_id == pa.quest_id)
            };
            if !present {
                continue;
            }
            // ONE bounded attempt; a Timeout is treated like Uncertain (keep
            // pending). The Gate is released after this attempt regardless.
            match tokio::time::timeout(io, store.write(pid.clone(), pa.accepted.clone())).await {
                Ok(Ok(state)) => {
                    let mut world = shared.lock().await;
                    if let Some(player) = world.players.get_mut(&pid) {
                        player.quests.insert(pa.quest_id.clone(), state.clone());
                    }
                    if let Some(list) = world.pending_quest_acceptances.get_mut(&pid) {
                        list.retain(|x| x.quest_id != pa.quest_id);
                        if list.is_empty() {
                            world.pending_quest_acceptances.remove(&pid);
                        }
                    }
                    resolved += 1;
                }
                Ok(Err(QuestAcceptError::Uncertain(_))) => {}
                Ok(Err(_)) => {}
                Err(_elapsed) => {}
            }
        }
    }
    Ok(resolved)
}

/// Resolve outstanding quest-completion orders over the SAME idempotent
/// spool/drain path. Bounded: one attempt per retained prepared order per call.
/// Reuses the IDENTICAL immutable content (never recomputes the reward). On
/// confirmed durable publication it performs the RAM handoff and removes the
/// prepared entry; an uncertain/failed write keeps it pending for the next
/// pass / Logout / Takeover / Shutdown. Character-scoped callers pass `requested`
/// so they never wait on a foreign character's completion. No detached task.
pub async fn retry_quest_completions_for(
    ctx: &Ctx,
    requested: Option<&[String]>,
) -> Result<(), String> {
    crate::persist::retry_quest_completions(
        ctx.persist.spool(),
        &ctx.shared,
        &ctx.cfg.progression,
        &ctx.quest,
        requested,
    )
    .await
}

/// Vorprüfung der Abgabe. Der mutierende Abschluss bleibt bis zur Freigabe des
/// gemeinsamen Persistenzvertrags gesperrt (docs/Quest-Turn-In-Persistenz.md).
pub async fn handle_turn_in(
    ctx: &Ctx,
    tx: &mpsc::UnboundedSender<String>,
    conn: u64,
    seq: i64,
    data: &Value,
) {
    let action = data["action"].as_str().unwrap_or("").to_owned();
    let npc_id = data["npc_id"].as_str().unwrap_or("").to_owned();
    let quest_id = data["quest_id"].as_str().unwrap_or("").to_owned();
    let pid = {
        let world = ctx.shared.lock().await;
        let Some(pid) = world
            .by_conn
            .get(&conn)
            .filter(|pid| is_owner(&world, conn, pid))
        else {
            return;
        };
        pid.clone()
    };
    let gate = ctx.persist.player_gate(&pid).await;
    let _gate = match tokio::time::timeout(ctx.quest_io_timeout, gate.lock_owned()).await {
        Ok(guard) => guard,
        Err(_) => {
            let world = ctx.shared.lock().await;
            if is_owner(&world, conn, &pid) {
                reject(tx, seq, &action, &npc_id, "quest_turn_in_busy");
            }
            return;
        }
    };
    let request: QuestCompletionRequest = {
        let world = ctx.shared.lock().await;
        if !is_owner(&world, conn, &pid) {
            return;
        }
        let npc = match interaction(&world, &pid, &npc_id, ctx.cfg.loot.pickup_radius) {
            Ok(npc) => npc,
            Err(reason) => {
                reject(tx, seq, &action, &npc_id, reason);
                return;
            }
        };
        let player = &world.players[&pid];
        if action == "quest_turn_ins" {
            let quests: Vec<_> = ctx.quest.definitions()
                .filter(|def| def.turn_in.as_deref() == Some(npc.content_id.as_str()))
                .filter(|def| def.objectives.iter().all(|o| o.kind == crate::quest::ObjectiveType::Kill))
                .filter_map(|def| {
                    let state = player.quests.get(&def.id)?;
                    (state.state == QuestState::Active).then(|| {
                        let met = ctx.quest.objectives_complete(&def.id, state);
                        json!({
                            "quest_id": def.id, "title_key": def.title_key,
                            "description_key": def.description_key, "state": "ACTIVE",
                            "objectives_met": met,
                            "rewards": {"exp": def.rewards.exp, "idia": def.rewards.idia},
                            "can_turn_in": met,
                            "turn_in": def.turn_in,
                            "objectives": def.objectives.iter().map(|o| json!({
                                "id": o.id, "type": o.kind.key(), "target": o.target,
                                "required": o.required,
                                "current": state.progress.iter()
                                    .find(|pp| pp.objective_id == o.id)
                                    .map(|pp| pp.current).unwrap_or(0u32),
                            })).collect::<Vec<serde_json::Value>>(),
                        })
                    })
                }).collect();
            let _ = tx.send(Frame::new(seq, s2c::NPC_TEXT, json!({
                "ok": true, "action": action, "npc_id": npc_id, "quests": quests
            })).encode());
            return;
        }
        let Some(def) = ctx.quest.find_definition(&quest_id) else {
            reject(tx, seq, &action, &npc_id, "unknown_quest");
            return;
        };
        if def.turn_in.as_deref() != Some(npc.content_id.as_str()) {
            reject(tx, seq, &action, &npc_id, "wrong_quest_turn_in");
            return;
        }
        let Some(cur) = player.quests.get(&quest_id) else {
            reject(tx, seq, &action, &npc_id, "quest_not_active");
            return;
        };
        if cur.state != QuestState::Active {
            reject(tx, seq, &action, &npc_id,
                if cur.state == QuestState::Completed { "quest_already_completed" } else { "quest_not_active" });
            return;
        }
        if !def.objectives.iter().all(|o| o.kind == crate::quest::ObjectiveType::Kill) {
            reject(tx, seq, &action, &npc_id, "quest_turn_in_unsupported");
            return;
        }
        if !ctx.quest.objectives_complete(&quest_id, cur) {
            reject(tx, seq, &action, &npc_id, "quest_objectives_not_met");
            return;
        }
        let progression = crate::progression::Progression::new(
            player.level, player.exp, player.free_attr_points, player.rested_pool,
            player.class, player.faction_transition,
        );
        if let Err(reason) = def.rewards.prepare(&progression, player.idia, &ctx.cfg.progression) {
            reject(tx, seq, &action, &npc_id, reason);
            return;
        }
        // A still-open completion for this character yields no second order.
        if world.quest_completion_pending(&pid) {
            reject(tx, seq, &action, &npc_id, "quest_turn_in_pending");
            return;
        }
        let base_revision = player.persist_revision;
        crate::persist::QuestCompletionRequest {
            commit_id: crate::persist::quest_commit_id(&pid, &quest_id, base_revision),
            character_id: pid.clone(),
            quest_id: quest_id.clone(),
            rewards: def.rewards.clone(),
        }
    };
    // Durable publication + RAM handoff under the held Gate; the immutable
    // order is built once and retained verbatim on an uncertain write.
    match crate::persist::finish_quest_completion(
        ctx.persist.spool(),
        &ctx.shared,
        request.clone(),
        &ctx.cfg.progression,
        &ctx.quest,
        |artifact| async move { ctx.persist.spool().write_quest_completion(&artifact) },
    )
    .await {
        Ok(()) => {
            let world = ctx.shared.lock().await;
            if is_owner(&world, conn, &pid) {
                let _ = tx.send(Frame::new(seq, s2c::NPC_TEXT, json!({
                    "ok": true, "action": action, "npc_id": npc_id,
                    "quest_id": request.quest_id, "state": "COMPLETED",
                    "rewards": {"exp": request.rewards.exp, "idia": request.rewards.idia},
                })).encode());
            }
        }
        Err(e) => {
            let world = ctx.shared.lock().await;
            if is_owner(&world, conn, &pid) {
                let reason: String = if e.contains("pending") || e.contains("conflict") {
                    "quest_turn_in_pending".into()
                } else {
                    e
                };
                reject(tx, seq, &action, &npc_id, &reason);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    use tokio::sync::{Notify, Semaphore};

    /// Modell der Transaktionsgrenze, ausdrücklich kein SQL-Treiber.
    pub struct ModelStore {
        pub rows: Arc<Mutex<BTreeMap<String, CharacterQuestState>>>,
        pub calls: Arc<AtomicUsize>,
        pub entered: Arc<Notify>,
        pub release: Arc<Semaphore>,
        /// 0: Erfolg, 1: Fehler vor COMMIT, 2: COMMIT erfolgt, Antwort
        /// verloren (then success), 3: COMMIT erfolgt, Antwort verloren,
        /// afterwards PERMANENT failure (DB stays down), 4: the whole attempt
        /// (Acquire+SQL+COMMIT) NEVER returns (hang) -> the IO-Timeout must fire.
        pub mode: Arc<AtomicUsize>,
        pub pause: bool,
    }

    impl AcceptStore for ModelStore {
        fn write(&self, char: String, accepted: CharacterQuestState) -> AcceptFuture {
            let rows = self.rows.clone();
            let calls = self.calls.clone();
            let entered = self.entered.clone();
            let release = self.release.clone();
            let mode = self.mode.clone();
            let pause = self.pause;
            Box::pin(async move {
                let call = calls.fetch_add(1, Ordering::SeqCst);
                entered.notify_one();
                if pause && call == 0 {
                    release.acquire().await.unwrap().forget();
                }
                let m = mode.load(Ordering::SeqCst);
                if m == 1 {
                    return Err(QuestAcceptError::Persistence("modellierter Fehler".into()));
                }
                if m == 4 {
                    // Hang: a 0-permit acquire that is never released. The
                    // surrounding tokio::time::timeout cancels this future; NO
                    // row is committed (the COMMIT outcome stays unknown).
                    release.acquire().await.unwrap().forget();
                    return Err(QuestAcceptError::Uncertain("unreachable after hang".into()));
                }
                if m == 5 {
                    if call == 0 {
                        // (1) first attempt ends Uncertain via Timeout (never
                        // returns) -> acceptance stays pending, COMMIT unknown.
                        release.acquire().await.unwrap().forget();
                        return Err(QuestAcceptError::Uncertain("unreachable after hang".into()));
                    }
                    if call == 1 {
                        // (2) identical retry fails PRE-COMMIT (Pool-Acquire):
                        // this proves NOTHING about the first COMMIT and must NOT
                        // undo the original uncertainty; the row stays untouched.
                        return Err(QuestAcceptError::Persistence("Pool-Acquire fehlert vor COMMIT".into()));
                    }
                    // (3) DB returns: identical retry resolves over the idempotent
                    // path; or_insert keeps the single row (no double, no reset).
                    let result = rows.lock().unwrap().entry(char).or_insert(accepted).clone();
                    return Ok(result);
                }
                // Idempotent insert-if-missing (mirrors write_quest_acceptance).
                let result = rows.lock().unwrap().entry(char).or_insert(accepted).clone();
                if m == 2 && call == 0 {
                    return Err(QuestAcceptError::Uncertain("modellierter Antwortverlust".into()));
                }
                if m == 3 {
                    if call == 0 {
                        return Err(QuestAcceptError::Uncertain("modellierter Antwortverlust".into()));
                    }
                    return Err(QuestAcceptError::Persistence("dauerhaft DB ausfall".into()));
                }
                Ok(result)
            })
        }
    }

    pub async fn fixture(mode: usize, pause: bool) -> (Arc<Ctx>, Arc<ModelStore>) {
        let mut ctx = crate::handlers::tests::test_ctx().await;
        crate::handlers::tests::insert_trade_setup(&ctx).await;
        let store = Arc::new(ModelStore {
            rows: Default::default(),
            calls: Default::default(),
            entered: Arc::new(Notify::new()),
            release: Arc::new(Semaphore::new(0)),
            mode: Arc::new(AtomicUsize::new(mode)),
            pause,
        });
        let mut service = QuestService::new();
        for (id, giver, level, requires) in [
            ("q", Some("borin"), 1, vec![]),
            ("hidden", Some("borin"), 6, vec![]),
            ("chain", Some("borin"), 1, vec!["predecessor".to_owned()]),
            ("ungiven", None, 1, vec![]),
            ("other", Some("elsewhere"), 1, vec![]),
        ] {
            service
                .register(crate::quest::QuestDefinition {
                    id: id.into(),
                    giver: giver.map(str::to_owned),
                    turn_in: None,
                    rewards: crate::quest::RewardBundle::default(),
                    title_key: format!("quest.{id}.title"),
                    description_key: format!("quest.{id}.description"),
                    min_level: level,
                    requires,
                    repeatable: false,
                    objectives: vec![crate::quest::QuestObjective {
                        id: "kill".into(),
                        kind: crate::quest::ObjectiveType::Kill,
                        target: "wolf".into(),
                        required: 5,
                    }],
                })
                .unwrap();
        }
        {
            let c = Arc::get_mut(&mut ctx).unwrap();
            c.quest = Arc::new(service);
            c.quest_accept_store = Some(store.clone());
        }
        ctx.shared
            .lock()
            .await
            .npcs
            .get_mut("npc_5")
            .unwrap()
            .content_id = "borin".into();
        (ctx, store)
    }

    async fn request(ctx: &Ctx, conn: u64, action: &str, npc: &str, quest: &str) -> Vec<Frame> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::handlers::handle_npc_talk(
            ctx,
            &tx,
            conn,
            72,
            &json!({"action": action, "npc_id": npc, "quest_id": quest,
                "account_id": 999, "level": 999, "state": "ACTIVE"}),
        )
        .await;
        let mut frames = Vec::new();
        while let Ok(raw) = rx.try_recv() {
            frames.push(serde_json::from_str(&raw).unwrap());
        }
        frames
    }

    pub(crate) async fn turn_in_fixture(fulfilled: bool) -> (Arc<Ctx>, Arc<ModelStore>) {
        let (mut ctx, store) = fixture(0, false).await;
        let mut def = ctx.quest.find_definition("q").unwrap().clone();
        def.turn_in = Some("borin".into());
        def.rewards = crate::quest::RewardBundle { exp: 11, idia: 7 };
        let mut svc = QuestService::new();
        svc.register(def).unwrap();
        Arc::get_mut(&mut ctx).unwrap().quest = Arc::new(svc);
        let mut state = ctx.quest.accept("q", None, 1, &HashSet::new()).unwrap();
        if fulfilled {
            ctx.quest.add_progress("q", &mut state, "kill", 5).unwrap();
        }
        // Canonical numeric character id (the DB `characters` FK requires it).
        {
            let mut w = ctx.shared.lock().await;
            let mut p = w.players.remove("held").unwrap();
            p.id = "7".into();
            w.players.insert("7".into(), p);
            w.by_conn.insert(7, "7".into());
            w.item_lifecycle.insert("7".into(), crate::item_lifecycle::ItemLifecycle::new());
            w.players.get_mut("7").unwrap().quests.insert("q".into(), state);
        }
        (ctx, store)
    }

    #[tokio::test]
    async fn turn_in_completes_killquest_once_with_reward() {
        let (ctx, store) = turn_in_fixture(true).await;
        let frames = request(&ctx, 7, "quest_turn_ins", "npc_5", "").await;
        assert_eq!(frames[0].seq, 72);
        let row = &frames[0].data["quests"][0];
        assert_eq!(row["state"], "ACTIVE");
        assert_eq!(row["objectives_met"], true);
        assert_eq!(row["rewards"], json!({"exp": 11, "idia": 7}));
        assert_eq!(row["can_turn_in"], true);
        let (idia0, lvl0, pts0, rev0) = {
            let world = ctx.shared.lock().await;
            let p = &world.players["7"];
            (p.idia, p.level, p.free_attr_points, p.persist_revision)
        };
        let answer = request(&ctx, 7, "quest_turn_in", "npc_5", "q").await;
        assert_eq!(answer[0].data["state"], "COMPLETED");
        assert_eq!(answer[0].data["rewards"], json!({"exp": 11, "idia": 7}));
        {
            let world = ctx.shared.lock().await;
            let p = &world.players["7"];
            assert_eq!(p.quests["q"].state, QuestState::Completed);
            assert_eq!(p.idia, idia0 + 7);
            assert!(p.level >= lvl0);
            assert!(p.free_attr_points >= pts0);
            assert_eq!(p.persist_revision, rev0 + 1);
        }
        assert_eq!(ctx.persist.spool().count_batches().unwrap(), 1);
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
        let again = request(&ctx, 7, "quest_turn_in", "npc_5", "q").await;
        assert_eq!(again[0].data["reason"], "quest_already_completed");
        assert_eq!(ctx.persist.spool().count_batches().unwrap(), 1);
    }

    #[tokio::test]
    async fn turn_in_overflow_rejects_without_mutation() {
        let (ctx, store) = turn_in_fixture(true).await;
        // A declared reward that fits i64 but whose SUM overflows the current
        // Idia stand is rejected at the turn-in preflight (no mutation).
        ctx.shared.lock().await.players.get_mut("7").unwrap().idia = i64::MAX;
        let answer = request(&ctx, 7, "quest_turn_in", "npc_5", "q").await;
        assert_eq!(answer[0].data["reason"], "quest_reward_overflow");
        let w = ctx.shared.lock().await;
        assert_eq!(w.players["7"].quests["q"].state, QuestState::Active);
        assert_eq!(w.players["7"].persist_revision, 0);
        assert_eq!(w.players["7"].idia, i64::MAX);
        assert!(w.prepared_quest_completions.is_empty());
        drop(w);
        assert_eq!(ctx.persist.spool().count_batches().unwrap(), 0);
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn turn_in_preflight_rejects_invalid_npc_progress_and_ownership() {
        let (ctx, store) = turn_in_fixture(false).await;
        assert_eq!(request(&ctx, 7, "quest_turn_in", "npc_5", "q").await[0].data["reason"], "quest_objectives_not_met");
        assert_eq!(request(&ctx, 7, "quest_turn_in", "missing", "q").await[0].data["reason"], "unknown_npc");
        assert_eq!(request(&ctx, 7, "quest_turn_in", "npc_5", "missing").await[0].data["reason"], "unknown_quest");
        assert!(request(&ctx, 999, "quest_turn_ins", "npc_5", "").await.is_empty());
        ctx.shared.lock().await.npcs.get_mut("npc_5").unwrap().content_id = "elsewhere".into();
        assert_eq!(request(&ctx, 7, "quest_turn_in", "npc_5", "q").await[0].data["reason"], "wrong_quest_turn_in");
        ctx.shared.lock().await.npcs.get_mut("npc_5").unwrap().x = 1_000.0;
        assert_eq!(request(&ctx, 7, "quest_turn_in", "npc_5", "q").await[0].data["reason"], "out_of_range");
        ctx.shared.lock().await.by_conn.remove(&7);
        assert!(request(&ctx, 7, "quest_turn_in", "npc_5", "q").await.is_empty());
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn turn_in_preflight_does_not_infer_giver_fallback() {
        let (ctx, _) = fixture(0, false).await;
        assert!(request(&ctx, 7, "quest_turn_ins", "npc_5", "").await[0].data["quests"].as_array().unwrap().is_empty());
        assert_eq!(request(&ctx, 7, "quest_turn_in", "npc_5", "q").await[0].data["reason"], "wrong_quest_turn_in");
    }

    /// Reuse the SAME modelled persist-path for the resolution pass (the
    /// shared rows/calls/mode Arcs make this the identical idempotent retry).
    async fn retry(ctx: &Ctx, store: Arc<ModelStore>, ids: &[String]) -> usize {
        let store: Arc<dyn AcceptStore> = Arc::new(
            ModelStore {
                rows: store.rows.clone(),
                calls: store.calls.clone(),
                entered: store.entered.clone(),
                release: store.release.clone(),
                mode: store.mode.clone(),
                pause: false,
            },
        );
        let persist = Arc::clone(&ctx.persist);
        let shared = ctx.shared.clone();
        retry_quest_acceptances(store, &persist, shared, Some(ids), ctx.quest_io_timeout)
            .await.unwrap()
    }

    #[tokio::test]
    async fn npc_quest_offers_use_content_id_and_real_availability() {
        let (ctx, store) = fixture(0, false).await;
        let frames = request(&ctx, 7, "quest_offers", "npc_5", "").await;
        assert_eq!(frames[0].seq, 72);
        assert_eq!(frames[0].msg_type, s2c::NPC_TEXT);
        assert_eq!(frames[0].data["quests"].as_array().unwrap().len(), 1);
        assert_eq!(frames[0].data["quests"][0]["quest_id"], "q");
        assert_eq!(frames[0].data["quests"][0]["can_accept"], true);
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn npc_quest_accepts_once_and_preserves_progress() {
        let (ctx, store) = fixture(0, false).await;
        assert_eq!(
            request(&ctx, 7, "quest_accept", "npc_5", "q").await[0].data["state"],
            "ACTIVE"
        );
        let original = store.rows.lock().unwrap()["held"].clone();
        ctx.shared
            .lock()
            .await
            .players
            .get_mut("held")
            .unwrap()
            .quests
            .get_mut("q")
            .unwrap()
            .progress[0]
            .current = 3;
        let retry = request(&ctx, 7, "quest_accept", "npc_5", "q").await;
        assert_eq!(retry[0].data["reason"], "quest_already_active");
        assert_eq!(
            ctx.shared.lock().await.players["held"].quests["q"].progress[0].current,
            3
        );
        assert_eq!(store.rows.lock().unwrap()["held"], original);
        assert_eq!(store.calls.load(Ordering::SeqCst), 1);
        assert!(
            request(&ctx, 7, "quest_offers", "npc_5", "").await[0].data["quests"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn npc_quest_rejects_wrong_unknown_hidden_and_ungiven() {
        let (ctx, store) = fixture(0, false).await;
        for (quest, reason) in [
            ("missing", "unknown_quest"),
            ("hidden", "quest_not_available"),
            ("chain", "quest_not_available"),
            ("other", "wrong_quest_giver"),
            ("ungiven", "wrong_quest_giver"),
        ] {
            assert_eq!(
                request(&ctx, 7, "quest_accept", "npc_5", quest).await[0].data["reason"],
                reason
            );
        }
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn npc_quest_rechecks_npc_alive_player_alive_and_distance() {
        let (ctx, store) = fixture(0, false).await;
        assert_eq!(
            request(&ctx, 7, "quest_accept", "npc_999", "q").await[0].data["reason"],
            "unknown_npc"
        );
        ctx.shared.lock().await.npcs.get_mut("npc_5").unwrap().x = 1000.0;
        assert_eq!(
            request(&ctx, 7, "quest_accept", "npc_5", "q").await[0].data["reason"],
            "out_of_range"
        );
        {
            let mut w = ctx.shared.lock().await;
            let n = w.npcs.get_mut("npc_5").unwrap();
            n.x = 1.0;
            n.hp = 0;
        }
        assert_eq!(
            request(&ctx, 7, "quest_accept", "npc_5", "q").await[0].data["reason"],
            "npc_unavailable"
        );
        {
            let mut w = ctx.shared.lock().await;
            w.npcs.get_mut("npc_5").unwrap().hp = 100;
            w.players.get_mut("held").unwrap().hp = 0;
        }
        assert_eq!(
            request(&ctx, 7, "quest_accept", "npc_5", "q").await[0].data["reason"],
            "character_dead"
        );
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn npc_quest_foreign_and_superseded_connections_have_no_effect() {
        let (ctx, store) = fixture(0, false).await;
        assert!(request(&ctx, 999, "quest_accept", "npc_5", "q")
            .await
            .is_empty());
        {
            let mut w = ctx.shared.lock().await;
            w.by_conn.remove(&7);
            w.by_conn.insert(8, "held".into());
        }
        assert!(request(&ctx, 7, "quest_accept", "npc_5", "q")
            .await
            .is_empty());
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn npc_quest_persistence_failure_never_activates() {
        let (ctx, store) = fixture(1, false).await;
        let frames = request(&ctx, 7, "quest_accept", "npc_5", "q").await;
        assert_eq!(frames[0].data["reason"], "quest_persistence_failed");
        assert!(!ctx.shared.lock().await.players["held"]
            .quests
            .contains_key("q"));
        assert!(store.rows.lock().unwrap().is_empty());
    }

    /// Uncertain COMMIT followed by PERMANENT failure: the acceptance stays
    /// outstanding in the registry, no ACTIVE in RAM, no rollback claim, and
    /// the retry pass terminates (no endless loop, gate free -> no hang).
    #[tokio::test]
    async fn npc_quest_uncertain_then_permanent_failure_stays_pending_no_hang() {
        let (ctx, store) = fixture(3, false).await;
        let frames = request(&ctx, 7, "quest_accept", "npc_5", "q").await;
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data["reason"], "quest_accept_pending");
        // No false ACTIVE in RAM; the modelled COMMIT landed in the row only.
        assert!(!ctx.shared.lock().await.players["held"].quests.contains_key("q"));
        assert_eq!(store.rows.lock().unwrap().len(), 1);
        // The open operation is nachvollbar erfasst in the registry.
        assert!(
            ctx.shared.lock().await.pending_quest_acceptances
                .get("held")
                .iter()
                .flat_map(|v| v.iter())
                .any(|p| p.quest_id == "q")
        );
        // Gate is free (no detached unbounded holder) -> retry can acquire it.
        assert!(ctx.persist.player_gate("held").await.try_lock_owned().await.is_some());
        // A retry pass under permanent failure keeps it pending, no ACTIVE,
        // no hang (bounded: one attempt per entry).
        let resolved = retry(&ctx, store.clone(), &["held".to_string()]).await;
        assert_eq!(resolved, 0);
        assert!(!ctx.shared.lock().await.players["held"].quests.contains_key("q"));
        assert_eq!(store.rows.lock().unwrap().len(), 1);
        assert_eq!(store.calls.load(Ordering::SeqCst), 2);
    }

    /// Identical resumption after the DB returns: the SAME immutable content
    /// is re-attempted over the SAME idempotent path -> exactly one state,
    /// no double acceptance, RAM converges to ACTIVE.
    #[tokio::test]
    async fn npc_quest_identical_resumption_after_db_returns_no_double() {
        let (ctx, store) = fixture(2, false).await;
        let frames = request(&ctx, 7, "quest_accept", "npc_5", "q").await;
        assert_eq!(frames[0].data["reason"], "quest_accept_pending");
        assert!(!ctx.shared.lock().await.players["held"].quests.contains_key("q"));
        assert_eq!(store.rows.lock().unwrap().len(), 1);
        // DB returns (mode 2 -> later attempts succeed): identical retry.
        let resolved = retry(&ctx, store.clone(), &["held".to_string()]).await;
        assert_eq!(resolved, 1);
        // RAM converged to ACTIVE; pending entry removed; still ONE row.
        assert_eq!(
            ctx.shared.lock().await.players["held"].quests["q"].state,
            QuestState::Active
        );
        assert!(
            ctx.shared.lock().await.pending_quest_acceptances
                .get("held")
                .iter()
                .flat_map(|v| v.iter())
                .collect::<Vec<_>>()
                .is_empty()
        );
        assert_eq!(store.rows.lock().unwrap().len(), 1);
        assert_eq!(store.calls.load(Ordering::SeqCst), 2);
    }

    /// A second accept while an acceptance is outstanding yields no second
    /// state and no second attempt (reported pending, registry unchanged).
    #[tokio::test]
    async fn npc_quest_pending_blocks_second_accept() {
        let (ctx, store) = fixture(3, false).await;
        assert_eq!(
            request(&ctx, 7, "quest_accept", "npc_5", "q").await[0].data["reason"],
            "quest_accept_pending"
        );
        // Second accept: pending guard -> no new attempt, no second entry.
        assert_eq!(
            request(&ctx, 7, "quest_accept", "npc_5", "q").await[0].data["reason"],
            "quest_accept_pending"
        );
        assert_eq!(store.calls.load(Ordering::SeqCst), 1);
        let pending = ctx.shared
            .lock()
            .await
            .pending_quest_acceptances
            .get("held")
            .iter()
            .flat_map(|v| v.iter())
            .filter(|p| p.quest_id == "q")
            .count();
        assert_eq!(pending, 1);
    }

    /// An outstanding acceptance is not offered again as annehbar.
    #[tokio::test]
    async fn npc_quest_offers_exclude_pending_acceptance() {
        let (ctx, store) = fixture(3, false).await;
        assert_eq!(
            request(&ctx, 7, "quest_accept", "npc_5", "q").await[0].data["reason"],
            "quest_accept_pending"
        );
        assert!(
            request(&ctx, 7, "quest_offers", "npc_5", "").await[0].data["quests"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.calls.load(Ordering::SeqCst), 1);
    }

    /// HANG-Pfad: the whole attempt (Acquire+SQL+COMMIT) never returns. The
    /// explicit IO-Timeout fires; the acceptance stays pending (registered
    /// BEFORE the attempt), no false ACTIVE, and the Gate is released.
    #[tokio::test]
    async fn npc_quest_hang_first_attempt_stays_pending_gate_free() {
        let (ctx, store) = fixture(4, false).await;
        let frames = request(&ctx, 7, "quest_accept", "npc_5", "q").await;
        assert_eq!(frames[0].data["reason"], "quest_accept_pending");
        assert!(ctx.shared.lock().await.pending_quest_acceptances
            .get("held").iter().flat_map(|v| v.iter()).any(|p| p.quest_id == "q"));
        assert!(ctx.shared.lock().await.players["held"].quests.get("q").is_none());
        assert!(ctx.persist.player_gate("held").await.try_lock_owned().await.is_some());
        assert_eq!(store.calls.load(Ordering::SeqCst), 1);
    }

    /// Retry of a hanging entry times out (keeps pending, Gate free); after the
    /// DB returns the IDENTICAL content resolves over the same idempotent path,
    /// exactly one state, no double acceptance.
    #[tokio::test]
    async fn npc_quest_retry_hang_then_db_returns_resolves_no_double() {
        let (ctx, store) = fixture(4, false).await;
        request(&ctx, 7, "quest_accept", "npc_5", "q").await;
        assert_eq!(store.calls.load(Ordering::SeqCst), 1);
        let r = retry(&ctx, store.clone(), &["held".to_string()]).await;
        assert_eq!(r, 0);
        assert_eq!(store.calls.load(Ordering::SeqCst), 2);
        assert!(ctx.persist.player_gate("held").await.try_lock_owned().await.is_some());
        // DB returns -> identical retry resolves, no second state.
        store.mode.store(0, Ordering::SeqCst);
        let r2 = retry(&ctx, store.clone(), &["held".to_string()]).await;
        assert_eq!(r2, 1);
        assert_eq!(
            ctx.shared.lock().await.players["held"].quests["q"].state,
            QuestState::Active
        );
        assert_eq!(store.rows.lock().unwrap().len(), 1);
        assert_eq!(store.calls.load(Ordering::SeqCst), 3);
    }

    /// Cancellation of the receive/acceptance task mid-hang must not lose the
    /// outstanding acceptance: it was registered BEFORE the attempt.
    #[tokio::test]
    async fn npc_quest_cancellation_around_hang_pending_survives() {
        let (ctx, store) = fixture(4, false).await;
        let c = ctx.clone();
        let acceptance = tokio::spawn(async move {
            let (tx, _rx) = mpsc::unbounded_channel();
            crate::handlers::handle_npc_talk(&c, &tx, 7, 1, &json!({
                "action": "quest_accept", "npc_id": "npc_5", "quest_id": "q"})).await;
        });
        store.entered.notified().await;
        acceptance.abort();
        assert!(ctx.shared.lock().await.pending_quest_acceptances
            .get("held").iter().flat_map(|v| v.iter()).any(|p| p.quest_id == "q"));
        assert!(ctx.shared.lock().await.players["held"].quests.get("q").is_none());
    }

    /// A character-scoped retry caller (Logout/Login) must NOT wait on a foreign
    /// character's hanging acceptance: requesting another char never attempts
    /// or blocks the foreign entry.
    #[tokio::test]
    async fn npc_quest_char_scoped_retry_does_not_wait_on_foreign_hang() {
        let (ctx, store) = fixture(4, false).await;
        request(&ctx, 7, "quest_accept", "npc_5", "q").await;
        let r = retry(&ctx, store.clone(), &["other".to_string()]).await;
        assert_eq!(r, 0);
        // foreign entry untouched and NOT attempted by the scoped caller.
        assert!(ctx.shared.lock().await.pending_quest_acceptances
            .get("held").iter().flat_map(|v| v.iter()).any(|p| p.quest_id == "q"));
        assert_eq!(store.calls.load(Ordering::SeqCst), 1);
    }

    /// Regression: (1) first attempt ends Uncertain via Timeout -> pending;
    /// (2) an identical retry fails PRE-COMMIT (Pool-Acquire) -> the original
    /// uncertainty MUST stay pending (a later pre-COMMIT failure proves NO
    /// rollback of the first); (3) after the DB returns the identical retry
    /// resolves -> exactly one ACTIVE, no double acceptance, no progress reset.
    #[tokio::test]
    async fn npc_quest_uncertain_then_precommit_retry_failure_keeps_pending_then_resolves() {
        let (ctx, store) = fixture(5, false).await;
        // (1) Timeout-first attempt -> pending registered before the attempt.
        request(&ctx, 7, "quest_accept", "npc_5", "q").await;
        assert!(ctx.shared.lock().await.pending_quest_acceptances
            .get("held").iter().flat_map(|v| v.iter()).any(|p| p.quest_id == "q"));
        assert!(ctx.shared.lock().await.players["held"].quests.get("q").is_none());
        assert_eq!(store.calls.load(Ordering::SeqCst), 1);
        // (2) retry fails PRE-COMMIT -> pending MUST remain, no rollback claim.
        let r = retry(&ctx, store.clone(), &["held".to_string()]).await;
        assert_eq!(r, 0);
        assert!(ctx.shared.lock().await.pending_quest_acceptances
            .get("held").iter().flat_map(|v| v.iter()).any(|p| p.quest_id == "q"));
        assert!(ctx.shared.lock().await.players["held"].quests.get("q").is_none());
        assert_eq!(store.calls.load(Ordering::SeqCst), 2);
        // (3) DB returns -> identical retry resolves, one state, no double/reset.
        let r2 = retry(&ctx, store.clone(), &["held".to_string()]).await;
        assert_eq!(r2, 1);
        assert_eq!(
            ctx.shared.lock().await.players["held"].quests["q"].state,
            QuestState::Active
        );
        assert_eq!(store.rows.lock().unwrap().len(), 1);
        assert!(ctx.shared.lock().await.pending_quest_acceptances
            .get("held").iter().flat_map(|v| v.iter()).collect::<Vec<_>>().is_empty());
        assert_eq!(store.calls.load(Ordering::SeqCst), 3);
    }
}
