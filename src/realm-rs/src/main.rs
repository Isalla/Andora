// andora-realm — Realm-Server (Rust), Einstiegspunkt.
// Zielimplementierung des Andora-Realmservers (siehe docs/architecture.md):
// Spiel-Autorität eines Realms (Combat, Loot, AH, NPC-AI via Coordinator),
// Persistenz ausschließlich in der eigenen realm_state_<realm>-Datenbank.
// Portiert nach dem Node.js/TypeScript-Übergangsstand (src/realm), ohne
// dessen Alt-Architektur-Annahmen (keine character-/world_data-Pools;
// Einstieg per Handoff statt reiner Session).
mod attributes;
mod auth_api;
mod class;
mod combat;
mod config;
mod db;
mod group;
mod handlers;
mod health;
mod item;
mod migrations;
mod net;
mod npc;
mod regen;
mod parental;
mod protocol;
mod world;

use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    env_logger::init();
    if let Err(e) = run() {
        log::error!("startup failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime: {e}"))?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let cfg = Arc::new(config::load_config(&config::config_path(&args))?);

    // Genau EINE Datenbank: die eigene realm_state_<realm>.
    let pool = db::open_pool("REALM_STATE_DB", &cfg.realm_db).await?;

    // Automatische Migrationen VOR Health/WebSocket: Fehler brechen den
    // Start mit Exit 1 ab — keine Spieler vor aktuellem Schema.
    let mig_dir: PathBuf = if cfg.migrations_dir.is_empty() {
        let mut exe = std::env::current_exe().map_err(|e| format!("exe dir: {e}"))?;
        exe.pop();
        exe.join("migrations")
    } else {
        PathBuf::from(&cfg.migrations_dir)
    };
    migrations::apply_migrations(
        &pool,
        &cfg.realm_db.database,
        &mig_dir,
        cfg.allow_destructive,
    )
    .await?;

    let auth = auth_api::AuthApi::new(&cfg.auth_api)?;
    let shared = world::new_shared();
    let parental = parental::new_shared(auth.clone());
    let groups = group::new_shared_groups(cfg.group.clone());

    let health_task = {
        let (cfg, shared) = (cfg.clone(), shared.clone());
        tokio::spawn(async move {
            if let Err(e) = health::serve(cfg, shared).await {
                log::error!("health: {e}");
            }
        })
    };
    let ws_task = {
        let (cfg, shared, parental, groups) =
            (cfg.clone(), shared.clone(), parental.clone(), groups.clone());
        tokio::spawn(net::serve(cfg, pool.clone(), auth, shared, parental, groups))
    };
    let poller = parental::start_poller(parental.clone(), shared.clone());
    let tick_shared = shared.clone();
    let tick_groups = groups.clone();
    let tick_ms = cfg.tick_ms;
    let aofb = cfg.aofb_radius;
    let combat_cfg = cfg.combat.clone();
    let npc_cfg = cfg.npc.clone();
    let mut combat_rng = combat::SplitMix64::new(combat::SplitMix64::time_seed());

    // NPC-/Monster-Instanzen aus Content + persistentem Zustand laden
    // (Migration 009). Ein Ladefehler bremst den Start (kein halber Realm).
    let npcs = {
        let defs = db::load_npc_definitions(&pool).await?;
        let spawns = db::load_npc_spawns(&pool).await?;
        let states = db::load_npc_states(&pool).await.unwrap_or_default();
        npc::build_npcs(&defs, &spawns, &states)
    };
    log::info!("{} NPC-Instanzen geladen", npcs.len());
    {
        let mut w = tick_shared.lock().await;
        w.npcs = npcs;
    }

    // Item-Definitionen (Item System V1, Migration 014): Content-Schicht der
    // Realm-Inhaltsversion. Ein Ladefehler bremst den Start (kein halber Realm).
    // Nutzung durch Inventory/Crafting/Loot folgt in späteren Systemen.
    let item_definitions: std::collections::HashMap<String, item::ItemDefinition> =
        db::load_item_definitions(&pool)
            .await?
            .into_iter()
            .map(|d| (d.item_id.clone(), d))
            .collect();
    log::info!("{} Item-Definitionen geladen", item_definitions.len());
    {
        let mut w = tick_shared.lock().await;
        w.item_definitions = item_definitions;
    }

    let persist_interval = std::time::Duration::from_millis(cfg.npc.persist_interval_ms);
    let persist_pool = pool.clone();
    // Ability-Registry (Content, Migration 010) für den Tick.
    let mut ability_registry = combat::ability::AbilityRegistry::new();
    for row in db::load_ability_definitions(&persist_pool).await? {
        ability_registry.register(combat::ability::build_ability_def(&row));
    }
    log::info!("{} Ability-Definitionen geladen", ability_registry.defs.len());
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
        let mut last_persist = std::time::Instant::now();
        loop {
            interval.tick().await;
            let mut world = tick_shared.lock().await;
            world::world_regen_tick(&mut world, tick_ms);
            world::world_tick(&mut world, aofb);
            let now = std::time::Instant::now();
            let wall_now = std::time::SystemTime::now();
            let mut groups_guard = tick_groups.lock().await;
            combat::combat_tick(
                &mut world,
                &combat_cfg,
                &groups_guard,
                &mut combat_rng,
                now,
                wall_now,
                aofb,
            );
            npc::npc_tick(
                &mut world,
                &combat_cfg,
                &npc_cfg,
                &mut combat_rng,
                now,
                wall_now,
                tick_ms,
                aofb,
            );
            combat::ability::ability_tick(
                &mut world,
                &ability_registry,
                now,
                wall_now,
                tick_ms,
                aofb,
            );
            // Gruppensystem §5: Reconnect-Frist ablaufen lassen.
            for pid in groups_guard.tick(now) {
                log::info!("Reconnect-Frist für Gruppenmitglied {pid} abgelaufen");
            }
            // §8: Gruppenauflösung — Claim "g:<gid>" geht aufs letzte Mitglied.
            for (gid, last_pid) in groups_guard.take_dissolutions() {
                let group_claim = format!("g:{gid}");
                for n in world.npcs.values_mut() {
                    if n.claimed_by.as_deref() == Some(group_claim.as_str()) {
                        // Kein Mitglied übrig → Claim entfällt ersatzlos.
                        n.claimed_by = if last_pid.is_empty() { None } else { Some(last_pid.clone()) };
                    }
                }
            }
            drop(groups_guard);
            // Periodische Persistenz des NPC-Zustands über Realm-Neustarts.
            if now.duration_since(last_persist) >= persist_interval {
                last_persist = now;
                let wall_epoch_ms = wall_now
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                // Snapshot ohne Sperre bauen (DB-I/O außerhalb der Sperre).
                let snapshot: Vec<db::NpcStateRow> = {
                    world
                        .npcs
                        .values()
                        .map(|n| db::NpcStateRow {
                            spawn_id: n.spawn_id,
                            status: n.status.key().to_string(),
                            hp: n.hp,
                            x: n.x,
                            y: n.y,
                            respawn_after_ms: n.respawn_after.map(|r| {
                                r.duration_since(std::time::SystemTime::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as i64)
                                    .unwrap_or(0)
                            }),
                            claimed_by: n.claimed_by.clone(),
                            claim_at_ms: n.claimed_by.is_some().then_some(wall_epoch_ms),
                        })
                        .collect()
                };
                let persist_pool2 = persist_pool.clone();
                tokio::spawn(async move {
                    for st in &snapshot {
                        db::save_npc_state(
                            &persist_pool2,
                            st.spawn_id,
                            &st.status,
                            st.hp,
                            st.x,
                            st.y,
                            st.respawn_after_ms,
                            st.claimed_by.as_deref(),
                            st.claim_at_ms,
                        )
                        .await;
                    }
                });
                log::debug!("NPC-Zustand persistiert");
            }
        }
    });

    log::info!(
        "realm {} started (tick {tick_ms}ms, AOFB {aofb}m, db {})",
        cfg.realm_id,
        cfg.realm_db.database
    );

    // Shutdown: SIGINT/SIGTERM -> Tasks stoppen, Pool schließen.
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| format!("signal: {e}"))?;
    log::info!("shutting down");
    ticker.abort();
    poller.abort();
    ws_task.abort();
    health_task.abort();
    pool.close().await;
    Ok(())
}
