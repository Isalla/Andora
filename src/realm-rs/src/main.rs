// andora-realm — Realm-Server (Rust), Einstiegspunkt.
// Zielimplementierung des Andora-Realmservers (siehe docs/architecture.md):
// Spiel-Autorität eines Realms (Combat, Loot, AH, NPC-AI via Coordinator),
// Persistenz ausschließlich in der eigenen realm_state_<realm>-Datenbank.
// Portiert nach dem Node.js/TypeScript-Übergangsstand (src/realm), ohne
// dessen Alt-Architektur-Annahmen (keine character-/world_data-Pools;
// Einstieg per Handoff statt reiner Session).
mod auth_api;
mod combat;
mod config;
mod db;
mod handlers;
mod health;
mod migrations;
mod net;
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

    let health_task = {
        let (cfg, shared) = (cfg.clone(), shared.clone());
        tokio::spawn(async move {
            if let Err(e) = health::serve(cfg, shared).await {
                log::error!("health: {e}");
            }
        })
    };
    let ws_task = {
        let (cfg, shared, parental) = (cfg.clone(), shared.clone(), parental.clone());
        tokio::spawn(net::serve(cfg, pool.clone(), auth, shared, parental))
    };
    let poller = parental::start_poller(parental.clone(), shared.clone());
    let tick_shared = shared.clone();
    let tick_ms = cfg.tick_ms;
    let aofb = cfg.aofb_radius;
    let combat_cfg = cfg.combat.clone();
    let mut combat_rng = combat::SplitMix64::new(combat::SplitMix64::time_seed());
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
        loop {
            interval.tick().await;
            let mut world = tick_shared.lock().await;
            world::world_tick(&mut world, aofb);
            let now = std::time::Instant::now();
            combat::combat_tick(&mut world, &combat_cfg, &mut combat_rng, now, aofb);
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
