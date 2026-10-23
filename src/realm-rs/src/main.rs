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
mod conversation;
mod db;
mod group;
mod handlers;
mod health;
mod inventory;
mod inventory_actions;
mod item;
mod item_lifecycle;
mod loot;
mod lua;
mod mail;
mod migrations;
mod net;
mod npc;
mod npc_quest;
mod progression;
mod quest;
mod regen;
mod parental;
mod persist;
mod player_trade;
mod protocol;
mod security;
mod session_watch;
mod spool;
mod trade;
mod weather;
mod world;

use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    env_logger::init();
    if let Err(e) = run() {
        // `P-26`: Das Mapping bleibt unverändert — ein `Err` aus `run()` (Start
        // **oder** Shutdown-Abschlussentscheidung) ergibt Exit 1. Nur die
        // Meldung ist neutral gefasst, damit ein Fehler des
        // Shutdown-Abschlusses nicht als Startfehler gemeldet wird.
        log::error!("realm beendet mit Fehler: {e}");
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

/// `P-22`: Statusentscheidung nach einem **erfolgreichen** Tick des
/// periodischen Drainers (`Ok(Some(..))` wie `Ok(None)`).
///
/// Ein einzelner erfolgreicher Drain beweist die Start-Recovery **nicht** als
/// abgeschlossen: Solange die Start-Recovery offen ist und Spool-Arbeit
/// verbleibt, bleibt der Status `DEGRADED` und es wird nur der Restzähler
/// protokolliert. `READY` folgt erst nach bestätigtem Abschluss (Spool leer);
/// normale neue Spool-Arbeit im Regelbetrieb erzwingt weiterhin kein
/// `DEGRADED`. Die Fehlerbehandlung bleibt beim Aufrufer (`Err` → `DEGRADED`).
///
/// K2: Der Tick nutzt `apply_drain_tick_with_lifecycle` — derselbe gemeinsame
/// Finalisierungsmechanismus wie der Startpfad. Wird eine zuvor offene
/// Recovery erst hier vollständig, läuft die ausstehende Finalisierung genau
/// einmal nach, bevor READY gilt.
async fn apply_drain_tick_status(
    runtime: &crate::spool::PersistRuntime,
    pool: &sqlx::Pool<sqlx::MySql>,
) {
    match runtime.apply_drain_tick_with_lifecycle(pool).await {
        Ok(crate::spool::RecoveryStatusUpdate::Ready) => {}
        Ok(crate::spool::RecoveryStatusUpdate::RecoveryStillOpen { remaining }) => {
            // Datensparsam: nur der Restzähler.
            log::warn!(
                "Start-Recovery weiterhin offen: {} Spool-Batches verbleiben — Status bleibt DEGRADED",
                remaining
            );
        }
        Err(e) => {
            // Restcheck nicht ermittelbar: Status nicht eigenmächtig auf READY
            // heben; der bestehende Zustand bleibt, der Fehler wird protokolliert.
            log::error!("Drain-Statusprüfung fehlgeschlagen: {e}");
        }
    }
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
    // Zentraler Session-Revocation-Poller (docs/Security.md AUTH-02b).
    // Eigener Task neben dem Elternkontroll-Poller; gleiches Start-/Abort-
    // Muster, keine Vermischung der fachlichen Semantik.
    let session_poller = session_watch::start_poller(auth.clone(), shared.clone());
    let groups = group::new_shared_groups(cfg.group.clone());

    // Spieler-Persistenz Stufe B: Spool-Runtime (Durable-Batches) inkl.
    // Status (Recovering → Ready/Degraded, docs/Player_Persistenz.md
    // §34/§36). Verzeichnisse werden automatisch angelegt.
    let persist = Arc::new(crate::spool::PersistRuntime::new(
        std::path::Path::new(&cfg.persist.persistence_dir),
        &cfg.combat.weapon_skill_id,
    )?);

    // `P-23`: Monitoring **vor** der Recovery starten. Der Health-Server
    // beobachtet ausschließlich Config, Shared und PersistRuntime; der zusätzlich
    // sessiongeprüfte Charakterlistenpfad verwendet Auth und den RealmDB-Pool.
    // Alle Abhängigkeiten sind hier vorhanden. Damit sind Monitoring
    // und Administration während RECOVERING erreichbar und der Zustand ist
    // über `/status` (`persistence_status` = `recovering`) beobachtbar
    // (docs/Player_Persistenz.md §27/§28).
    //
    // Das ist **keine** Spielfreigabe: `net::serve` startet weiterhin erst
    // nach der Recovery, und Logins bleiben in `Recovering` blockiert
    // (handlers.rs). Handle, Fehlerbehandlung und das Shutdown-`abort()`
    // bleiben unverändert; Ports, Bind-Adressen und Auth werden nicht
    // verändert.
    let health_task = health::spawn_monitor(
        cfg.clone(),
        shared.clone(),
        persist.clone(),
        auth.clone(),
        pool.clone(),
    );

    // Startup-Recovery (§34) NACH den Migrationen und VOR `net::serve`:
    // im Spool liegende Batches (Crash/Wartung) werden auf die DB angewendet.
    // Ein DB-Fehler bricht den Start NICHT ab — der Realm startet dann im
    // Status DEGRADED (Login weiter möglich, Drain retryt periodisch).
    //
    // `P-22`: READY setzt der Start **nur** bei bestätigt abgeschlossener
    // Recovery. Blieb nach dem Recovery-Ende relevante Restarbeit in
    // `<base>/spool/` liegen (Limit erreicht oder Abbruch ohne Fortschritt),
    // startet der Realm DEGRADED; der periodische Drainer arbeitet sie weiter
    // ab und hebt den Status erst nach bestätigtem Abschluss auf READY.
    match persist.recover(&pool).await {
        Ok(outcome) => {
            let report = outcome.report;
            if report.batches_processed > 0 || report.batches_quarantined > 0 {
                log::info!(
                    "Recovery-Drain: {} Batches, {} angewendet, {} übersprungen, {} superseded, {} quarantäniert",
                    report.batches_processed,
                    report.entries_applied,
                    report.entries_skipped,
                    report.entries_superseded,
                    report.batches_quarantined
                );
            }
            match persist.apply_startup_recovery(&outcome) {
                crate::spool::RecoveryStatusUpdate::Ready => {}
                crate::spool::RecoveryStatusUpdate::RecoveryStillOpen { remaining } => {
                    // Datensparsam: ausschließlich der Restzähler, keine
                    // Dateinamen, Inhalte, Pfade oder Rohfehler.
                    log::warn!(
                        "Recovery unvollständig: {} Spool-Batches verbleiben — Realm startet als DEGRADED, periodischer Drain arbeitet weiter",
                        remaining
                    );
                }
            }
            // Item-Lifecycle (docs/inventory_system.md §18): Startup-
            // Finalisierung alter Runtime-Metadaten — ausschließlich bei
            // vollständig abgeschlossener Recovery. Bei unvollständiger
            // Recovery keine widersprüchliche Bereinigung und keine
            // vorzeitige Freigabe; die Metadaten bleiben bis zum Drain der
            // offenen Batches erhalten. Ein Fehler bricht den Start nicht ab
            // (DEGRADED, periodischer Drain/Neustart versucht erneut).
            // Gemeinsamer Mechanismus mit dem Drainer
            // (`finalize_startup_lifecycle`: einmalig, setzt die
            // Einmaligkeitsmarke nur bei Erfolg).
            if outcome.batches_remaining == 0 {
                match persist.finalize_startup_lifecycle(&pool).await {
                    Ok(report) => {
                        if report.deleted > 0 || report.metadata_dropped > 0 {
                            log::info!(
                                "Lifecycle-Startup-Finalisierung: {} geprüft, {} Instanzen finalisiert, {} referenziert erhalten, {} Metadaten bereinigt, {} unbekannter Grund",
                                report.scanned,
                                report.deleted,
                                report.kept_referenced,
                                report.metadata_dropped,
                                report.skipped_unknown_reason
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("Lifecycle-Startup-Finalisierung fehlgeschlagen: {e}");
                        persist.set_recovery_open(true);
                        persist.set_status(crate::spool::PersistStatus::Degraded);
                    }
                }
            } else {
                log::warn!(
                    "Lifecycle-Startup-Finalisierung übersprungen: Start-Recovery offen (keine Bereinigung bei unvollständiger Recovery)"
                );
            }
        }
        Err(e) => {
            log::error!("Recovery-Drain fehlgeschlagen: {e} — Realm startet als DEGRADED");
            // Batches liegen per Abbruchvertrag weiter im Spool: die Start-
            // Recovery ist damit offen, READY folgt erst nach dem Abarbeiten.
            persist.set_recovery_open(true);
            persist.set_status(crate::spool::PersistStatus::Degraded);
        }
    }

    let poller = parental::start_poller(parental.clone(), shared.clone());
    let tick_shared = shared.clone();
    let tick_groups = groups.clone();
    let tick_ms = cfg.tick_ms;
    let aofb = cfg.aofb_radius;
    let combat_cfg = cfg.combat.clone();
    let npc_cfg = cfg.npc.clone();
    let loot_cfg = cfg.loot.clone();
    let prog_cfg = cfg.progression.clone();
    let weather_cfg = cfg.weather.clone();
    let mut combat_rng = combat::SplitMix64::new(combat::SplitMix64::time_seed());
    let mut weather_rng = combat::SplitMix64::new(combat::SplitMix64::time_seed());

    // NPC-/Monster-Instanzen aus Content + persistentem Zustand laden
    // (Migration 009). Ein Ladefehler bremst den Start (kein halber Realm).
    let npcs = {
        let defs = db::load_npc_definitions(&pool).await?;
        let spawns = db::load_npc_spawns(&pool).await?;
        let states = db::load_npc_states(&pool).await.unwrap_or_default();
        npc::build_npcs(&defs, &spawns, &states)
    };
    log::info!("{} NPC-Instanzen geladen", npcs.len());
    // Content-Index for Quest-Reference-resolvability (the ACTUAL loaded
    // Monster-/NPC-Content — never a Clientclaim, §27.8).
    let monster_content_ids: std::collections::HashSet<String> = npcs
        .values()
        .map(|n| n.content_id.clone())
        .collect();
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
    let item_definition_ids: std::collections::HashSet<String> = item_definitions
        .keys()
        .cloned()
        .collect();
    {
        let mut w = tick_shared.lock().await;
        w.item_definitions = item_definitions;
    }

    // Quest V1 Content-Loader (docs/Quest-System.md §2 „Lua – statischer
    // Teil", §27.9 provisional Definitionformat — smallest compatible
    // proposal, see .tmp/quest-definition-loading/): Questdefinitions are
    // loaded from the documented Content-Pfad (`<content_root>/quests/*.lua`
    // via the existing Lua-Sandbox/ScriptManager) and validated COMPLETELY
    // before registration (unique IDs, supported Objectives, valid
    // quantities, resolvable Content-References). ALL-OR-NOTHING: a Ladefehler
    // leaves NO partial Definitionbestand and bremst den Start (kein halber
    // Realm). KEIN hartcodierte Testquest here. `content_root` leer → leer
    // Registry (Kill-Credit fail-closed).
    let quest_service = {
        let mut svc = crate::quest::QuestService::new();
        let content = crate::quest::ContentIndex {
            monsters: monster_content_ids,
            items: item_definition_ids,
        };
        let root = std::path::PathBuf::from(cfg.content_root.clone());
        match svc.load_quest_definitions(&root, &content) {
            Ok(n) => log::info!("{} Quest-Definitionen geladen", n),
            Err(e) => {
                log::error!("Quest-Definition-Loader: {e} — Realm-Start abgelehnt (kein halber Realm)");
                return Err(format!("Quest-Definition-Loader: {e}"));
            }
        }
        Arc::new(svc)
    };
    let ws_task = {
        let (cfg, shared, parental, groups, persist) = (
            cfg.clone(),
            shared.clone(),
            parental.clone(),
            groups.clone(),
            persist.clone(),
        );
        tokio::spawn(net::serve(
            cfg,
            pool.clone(),
            auth,
            shared,
            parental,
            groups,
            persist,
            quest_service.clone(),
        ))
    };
    let quest_retry_quest = quest_service.clone();
    let quest_shutdown_quest = quest_service.clone();

    // Loot-Tabellen (Loot System V1, Migration 017): Content-Schicht des
    // Lootsystems. Ein Ladefehler bremst den Start (kein halber Realm).
    let loot_tables = db::load_loot_tables(&pool).await?;
    log::info!("{} Loot-Tabellen geladen", loot_tables.len());
    {
        let mut w = tick_shared.lock().await;
        w.loot_tables = loot_tables;
    }

    // Händlerkatalog (NPC-Handel, Migration 022): Händlerrolle je NPC-Spawn
    // plus Sortiment mit Kauf-/Verkaufspreisen je RealmDB. Ein Ladefehler
    // bremst den Start (kein halber Realm).
    let merchant_catalog = db::load_merchant_catalog(&pool).await?;
    log::info!(
        "{} Händler, {} Angebote geladen",
        merchant_catalog.merchants.len(),
        merchant_catalog.offers.len()
    );
    {
        let mut w = tick_shared.lock().await;
        w.merchant_catalog = merchant_catalog;
    }

    // Serverautoritatives Wetter (docs/Weltzeit_und_Wettersystem.md §10/§12):
    // the persisted realm-wide state is RESUMED at startup (never reset to a
    // fixed standard). A missing/unknown row falls back to a safe initial
    // state; a load error does NOT abort the start (weather is atmospheric and
    // the realm must stay playable — unlike Content registries, which abort).
    {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let weather = match db::load_weather_state(&pool).await {
            Ok(Some(row)) => crate::weather::from_row(&row, now_ms, &cfg.weather),
            Ok(None) => crate::weather::Weather::initial(now_ms, &cfg.weather),
            Err(e) => {
                log::warn!("Wetter-Startup-Load: {e} — safe initial state (atmospheric, non-fatal)");
                crate::weather::Weather::initial(now_ms, &cfg.weather)
            }
        };
        log::info!("Wetter geladen: {} (rev {})", weather.current.key(), weather.revision);
        let mut w = tick_shared.lock().await;
        w.weather = weather;
    }

    let persist_interval = std::time::Duration::from_millis(cfg.npc.persist_interval_ms);
    let weather_persist_interval = std::time::Duration::from_millis(cfg.weather.persist_interval_ms.max(1));
    let persist_pool = pool.clone();
    let weather_pool = pool.clone();
    // Ability-Registry (Content, Migration 010) für den Tick.
    let mut ability_registry = combat::ability::AbilityRegistry::new();
    for row in db::load_ability_definitions(&persist_pool).await? {
        ability_registry.register(combat::ability::build_ability_def(&row));
    }
    log::info!("{} Ability-Definitionen geladen", ability_registry.defs.len());
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
        let mut last_persist = std::time::Instant::now();
        let mut last_weather_persist = std::time::Instant::now();
        loop {
            interval.tick().await;
            let mut world = tick_shared.lock().await;
            world::world_regen_tick(&mut world, tick_ms);
            world::world_tick(&mut world, aofb);
            // Serverautoritatives Wetter-Wechsel (docs §3/§9): advance the
            // realm-wide state; on a genuine change broadcast the confirmed
            // state to all online players (docs §13). No Tick-spam: only on
            // revision bump.
            {
                let wnow_ms = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                if crate::weather::advance(&mut world, &weather_cfg, &mut weather_rng, wnow_ms) {
                    world::broadcast_weather(&world);
                    log::debug!("Wetter change: {} (rev {})", world.weather.current.key(), world.weather.revision);
                }
            }
            let now = std::time::Instant::now();
            let wall_now = std::time::SystemTime::now();
            let mut groups_guard = tick_groups.lock().await;
            combat::combat_tick(
                &mut world,
                &combat_cfg,
                &loot_cfg,
                &prog_cfg,
                &groups_guard,
                &quest_service,
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
            // Loot System V1: abgelaufene Drops entfernen.
            loot::loot_tick(&mut world, &loot_cfg, now);
            // Spielerhandel: Einladungsfristen plus Standprüfung
            // (Reichweite/Leben/Anwesenheit) der Runtime-Dialoge.
            for (party, note) in
                crate::player_trade::player_trade_tick(&mut world, loot_cfg.pickup_radius, now)
            {
                if let Some(p) = world.players.get(&party) {
                    p.send(&crate::protocol::Frame::new(
                        0,
                        crate::protocol::s2c::PLAYER_TRADE,
                        note,
                    ));
                }
            }
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
                // Loot System V1: Boden-Loot-Ansprüche gleichermaßen übergeben.
                for l in world.loot_drops.values_mut() {
                    if l.claimed_by.as_deref() == Some(group_claim.as_str()) {
                        l.claimed_by = if last_pid.is_empty() {
                            None
                        } else {
                            Some(last_pid.clone())
                        };
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
            // Periodische Persistenz des autoritativen Wetterzustand (docs
            // §10/§12): survives a Realm-Neustart; a save error is logged, the
            // in-RAM state stays authoritative (same direction as NPC states).
            if now.duration_since(last_weather_persist) >= weather_persist_interval {
                last_weather_persist = now;
                let row = crate::weather::to_row(&world.weather);
                let wp = weather_pool.clone();
                tokio::spawn(async move {
                    db::save_weather_state(&wp, &row);
                });
            }
        }
    });

    // Stufe B: periodischer Player-Persist (alle online Spieler mit
    // Dirty-State werden als vollständige Durable-Spool-Batches gesichert,
    // docs/Player_Persistenz.md §33/§37).
    let player_persist_ms = cfg.persist.player_persist_interval_ms.max(1);
    let persist_player_shared = shared.clone();
    let persist_for_players = persist.clone();
    let player_persister = tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(player_persist_ms));
        interval.tick().await; // ersten Tick überspringen (Start-Reihenfolge)
        loop {
            interval.tick().await;
            let ids: Vec<String> = {
                let world = persist_player_shared.lock().await;
                world
                    .players
                    .values()
                    .filter(|p| p.dirty.any())
                    .map(|p| p.id.clone())
                    .collect()
            };
            // The runtime first confirms retained trade publications, then
            // gates each normal snapshot and defers both pending participants.
            // `P-12`/§35: **ein** Persistenzlauf erzeugt **eine** gemeinsame
            // Batch-Datei mit den dirty Spielern dieses Laufs. Der Lauf gibt
            // den reservierten Snapshot-Speicher nach der dauerhaften
            // Veröffentlichung frei, ohne auf die DB-Verarbeitung zu warten.
            match persist_for_players
                .persist_dirty_run(&persist_player_shared, &ids)
                .await
            {
                Ok(n) if n > 0 => {
                    log::info!("periodic persist: {n} dirty Snapshots in einem Batch gesichert");
                }
                Ok(_) => {}
                Err(e) => log::error!("periodic persist: {e}"),
            }
        }
    });

    // Stufe B: periodischer DB-Drain (älteste Spool-Batch auf die DB, §36)
    // + tägliche Retention der superseded-/Archiv-Dateien (§33).
    let drain_ms = cfg.persist.drain_interval_ms.max(1);
    let retention_every = (86_400_000_u64 / drain_ms).max(1);
    let drain_pool = pool.clone();
    let drain_runtime = persist.clone();
    let drainer = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(drain_ms));
        interval.tick().await; // ersten Tick überspringen
        let mut ticks = 0_u64;
        loop {
            interval.tick().await;
            ticks += 1;
            match drain_runtime.drain_one(&drain_pool).await {
                Ok(Some(report)) => {
                    if report.batches_processed > 0 || report.batches_quarantined > 0 {
                        log::info!(
                            "Drain: {} Batches, {} angewendet, {} übersprungen, {} superseded, {} quarantäniert",
                            report.batches_processed,
                            report.entries_applied,
                            report.entries_skipped,
                            report.entries_superseded,
                            report.batches_quarantined
                        );
                    }
                    apply_drain_tick_status(&drain_runtime, &drain_pool).await;
                }
                Ok(None) => {
                    apply_drain_tick_status(&drain_runtime, &drain_pool).await;
                }
                Err(e) => {
                    log::error!("Drain fehlgeschlagen: {e}");
                    drain_runtime.set_status(crate::spool::PersistStatus::Degraded);
                }
            }
            if ticks % retention_every == 0 {
                if let Err(e) = drain_runtime.run_retention() {
                    log::error!("Spool-Retention: {e}");
                }
            }
        }
    });

    // FIX_QUEST_ACCEPTANCE_RETRY_AND_SHUTDOWN_BLOCKING: periodische Resolution-
    // pass for outststanding Questannahme-Operations (docs/Quest-System.md §27.27
    // / Player_Persistenz.md §10). Bounded per call (ONE attempt per pending
    // entry); reuses `retry_quest_acceptances` over the SAME idempotent persist-
    // path. When the DB returns, the identical acceptance converges (RAM-Über-
    // nahme, no second state). Under permanent failure it keeps the entry pending
    // and releases the Gate every pass, so Logout/Takeover/Shutdown never hang.
    let quest_retry_interval = std::time::Duration::from_millis(cfg.persist.drain_interval_ms.max(1));
    let quest_retry_pool = pool.clone();
    let quest_retry_persist = persist.clone();
    let quest_retry_shared = shared.clone();
    let quest_retry_cfg = cfg.clone();
    let quest_retry = tokio::spawn(async move {
        let mut interval = tokio::time::interval(quest_retry_interval);
        interval.tick().await; // erste Tick überspringen (Start-Reihenfolge)
        loop {
            interval.tick().await;
            let has_pending = {
                let w = quest_retry_shared.lock().await;
                w.pending_quest_acceptances.values().any(|v| !v.is_empty())
                    || !w.prepared_quest_completions.is_empty()
                    || !w.prepared_mail_claims.is_empty()
            };
            if !has_pending {
                continue;
            }
            match crate::npc_quest::retry_quest_acceptances(
                crate::npc_quest::production_store(quest_retry_pool.clone()),
                &quest_retry_persist,
                quest_retry_shared.clone(),
                None,
                crate::npc_quest::QUEST_ACCEPT_IO_TIMEOUT,
            )
            .await {
                Ok(n) if n > 0 => log::info!("quest-acceptance retry: {n} resolved"),
                Ok(_) => {}
                Err(e) => log::error!("quest-acceptance retry: {e}"),
            }
            if let Err(e) = crate::persist::retry_quest_completions(
                quest_retry_persist.spool(),
                &quest_retry_shared,
                &quest_retry_cfg.progression,
                &quest_retry_quest,
                None,
            )
            .await {
                log::error!("quest-completion retry: {e}");
            }
            if let Err(e) = crate::mail::retry_mail_claims(
                quest_retry_persist.spool(),
                &quest_retry_shared,
                &quest_retry_pool,
                None,
            )
            .await {
                log::error!("mail-claim retry: {e}");
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
    session_poller.abort();
    ws_task.abort();
    player_persister.abort();
    drainer.abort();
    health_task.abort();
    quest_retry.abort();
    // Shutdown-Flush (docs/Player_Persistenz.md §23/§38): alle noch online
    // Spieler final als vollständige Spool-Batches sichern (force) und den
    // Logout-Zeitpunkt direkt setzen; danach ein letzter Drain.
    let online: Vec<String> = {
        let world = shared.lock().await;
        world.players.keys().cloned().collect()
    };
    if !online.is_empty() {
        log::info!("Shutdown-Flush für {} Spieler", online.len());
    }
    // Begrenzte Retry-Semantik für den direkten `logout_at`-Write
    // (docs/Player_Persistenz.md §30; docs/Security.md `P-27`): die Phase
    // besitzt ein **globales** Budget und erzeugt genau eine Abschlusszusammen-
    // fassung. Der kontrollierte Abschluss (Drain, `pool.close()`) bleibt
    // unverändert und läuft **immer** vollständig.
    let logout_wait = |d: std::time::Duration| {
        Box::pin(async move {
            tokio::time::sleep(d).await;
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
    };
    let make_logout_write = |id: &str| -> crate::net::DisconnectLogout {
        let pool = pool.clone();
        let pid = id.to_string();
        Box::new(move |ts: i64| {
            let pool = pool.clone();
            let pid = pid.clone();
            Box::pin(async move { db::write_logout_at(&pool, &pid, ts).await })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        })
    };
    let logout_report = crate::net::shutdown_logout_phase(
        &persist,
        &shared,
        &online,
        crate::net::LOGOUT_RETRY_PLAN,
        crate::net::SHUTDOWN_LOGOUT_BUDGET,
        &logout_wait,
        &make_logout_write,
    )
    .await;
    if logout_report.failed > 0 || logout_report.skipped > 0 {
        // Der Abschluss muss erkennen lassen, dass nicht vollständig
        // persistiert werden konnte. Ausgegeben werden ausschließlich Zähler,
        // keine Zeichenketten aus der Datenbank.
        persist.set_status(crate::spool::PersistStatus::Degraded);
        log::error!(
            "shutdown_logout_incomplete retried={} failed={} skipped={} budget_exhausted={}",
            logout_report.retried,
            logout_report.failed,
            logout_report.skipped,
            logout_report.budget_exhausted
        );
    }
    // FIX: ein geordem Shutdown muss nicht indefinitely verhindert werden durch
    // een outstanding Questannahme. ONE bounded resolution pass over the SAME
    // idempotent persist-path (no sleep, no endless task). Still-outstanding
    // acceptances are reported as a defined error state; the character Snapshots
    // were already secured above and the `quests`-Table remains the authority on
    // the next Login (§27.26 / Player_Persistenz §30/§41). No weakened Save-
    // guarantee, no SIGKILL as the regular completion path.
    //
    // The WHOLE retry pass is additionally capped by a real Deadline, so even a
    // hanging attempt (Acquire+SQL+COMMIT never returns) and a blocked per-
    // character Gate cannot push Shutdown beyond its budget. On Deadline-
    // elapsed the still-outstanding entries stay pending (registered before any
    // attempt) and are reported below.
    match tokio::time::timeout(
        crate::net::SHUTDOWN_LOGOUT_BUDGET,
        crate::npc_quest::retry_quest_acceptances(
            crate::npc_quest::production_store(pool.clone()),
            &persist,
            shared.clone(),
            None,
            crate::npc_quest::QUEST_ACCEPT_IO_TIMEOUT,
        ),
    )
    .await {
        Ok(Ok(resolved)) if resolved > 0 =>
            log::info!("shutdown: {resolved} open quest acceptances resolved"),
        Ok(Ok(_)) => {}
        Ok(Err(e)) => log::error!("shutdown quest-acceptance retry: {e}"),
        Err(_elapsed) =>
            log::error!("shutdown quest-acceptance retry exceeded deadline (entries stay pending)"),
    }
    // The SAME bounded Shutdown pass resolves an open quest-completion order
    // over the existing spool/drain path. Capped by the real Deadline; a still
    // open order stays retained (prepared entry + durable spool file) and is
    // reported below. No false COMPLETED, no claimed rollback.
    match tokio::time::timeout(
        crate::net::SHUTDOWN_LOGOUT_BUDGET,
        crate::persist::retry_quest_completions(
            persist.spool(),
            &shared,
            &cfg.progression,
            &quest_shutdown_quest,
            None,
        ),
    )
    .await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::error!("shutdown quest-completion retry: {e}"),
        Err(_elapsed) =>
            log::error!("shutdown quest-completion retry exceeded deadline (order stays pending)"),
    }
    // The SAME bounded Shutdown pass resolves an open mail-claim order over the
    // existing spool/drain path. Capped by the real Deadline; a still-open order
    // stays retained (prepared entry + durable spool file) and is reported below.
    // No false claim, no escrow loss.
    match tokio::time::timeout(
        crate::net::SHUTDOWN_LOGOUT_BUDGET,
        crate::mail::retry_mail_claims(
            persist.spool(),
            &shared,
            &pool,
            None,
        ),
    )
    .await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::error!("shutdown mail-claim retry: {e}"),
        Err(_elapsed) =>
            log::error!("shutdown mail-claim retry exceeded deadline (order stays pending)"),
    }
    let open_acceptances: usize = {
        let w = shared.lock().await;
        w.pending_quest_acceptances.values().map(|v| v.len()).sum()
    };
    if open_acceptances > 0 {
        persist.set_status(crate::spool::PersistStatus::Degraded);
        log::error!(
            "shutdown_open_quest_acceptances={} (quests-Table remains authority; next Login re-derives)",
            open_acceptances
        );
    }
    let open_completions: usize = {
        let w = shared.lock().await;
        w.prepared_quest_completions.len()
    };
    if open_completions > 0 {
        persist.set_status(crate::spool::PersistStatus::Degraded);
        log::error!(
            "shutdown_open_quest_completions={} (durable spool order/receipt remains authority; next Login re-derives)",
            open_completions
        );
    }
    let open_mail_claims: usize = {
        let w = shared.lock().await;
        w.prepared_mail_claims.len()
    };
    if open_mail_claims > 0 {
        persist.set_status(crate::spool::PersistStatus::Degraded);
        log::error!(
            "shutdown_open_mail_claims={} (durable spool order/receipt remains authority; next Login re-derives)",
            open_mail_claims
        );
    }
    if let Err(e) = persist.drain_one(&pool).await {
        log::error!("final drain: {e}");
    }
    pool.close().await;
    // `P-26`: Das aggregierte **Sicherungsergebnis** wird erst **nach** dem
    // vollständigen Cleanup ausgewertet — finaler Drain und `pool.close()`
    // laufen oben in jedem Fall, auch wenn der Zustand nicht bestätigt
    // gesichert werden konnte. Ein Skip des Cleanups ist ausgeschlossen.
    //
    // Maßgeblich ist ausschließlich der forcierte Spool-Save: ein gescheiterter
    // finaler Drain allein ändert die Aussage über die Snapshot-Sicherung
    // nicht, weil der Snapshot dann dauerhaft im Spool liegt und bei der
    // nächsten Start-Recovery angewendet wird (§30/§41 Fall 2). Das
    // bestehende Fehlerlog `final drain:` und der `DEGRADED`-Status bleiben
    // unverändert erhalten.
    //
    // Ein `Err` bedeutet: mindestens ein aktueller Spielerzustand wurde nicht
    // dauerhaft bestätigt gesichert. `main()` erzeugt daraus Exit 1. Der
    // Nichtnull-Exit **meldet** diesen Zustand und ersetzt **keine** Sicherung;
    // es wird ausdrücklich nicht behauptet, es existiere kein älterer
    // dauerhafter Stand (§41).
    if let Some(e) = logout_report.snapshot_security_error() {
        log::error!(
            "shutdown_persistence_unconfirmed: {}",
            logout_report.spool_failed
        );
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod p26_tests {
    //! `P-26`: statische Nachweise über die **echte** Produktionsquelle dieses
    //! Crate-Root. `async_main` lässt sich ohne laufende Datenbank nicht als
    //! Funktion aufrufen; die beiden entscheidenden Eigenschaften — die
    //! Reihenfolge des Cleanups gegenüber der Fehlerrückgabe und das
    //! Exit-Mapping von `main()` — sind deshalb **strukturell** belegt. Geprüft
    //! wird die reale Datei, nicht eine nachgebaute Entscheidung.

    /// Die Produktionsquelle selbst, zur Compile-Zeit eingelesen.
    const SRC: &str = include_str!("main.rs");

    fn idx(needle: &str) -> usize {
        SRC.find(needle)
            .unwrap_or_else(|| panic!("Marker fehlt in der Produktionsquelle: {needle}"))
    }

    /// Der finale Drain und `pool.close()` liegen **vor** der Rückgabe des
    /// aggregierten Sicherungsergebnisses. Notwendiges Cleanup wird deshalb
    /// auch dann **nicht** übersprungen, wenn der Shutdown als Fehler endet.
    #[test]
    fn p26_cleanup_precedes_the_error_result() {
        let drain = idx("if let Err(e) = persist.drain_one(&pool).await {");
        let close = idx("pool.close().await;");
        let decision = idx("logout_report.snapshot_security_error()");
        let ret = idx("return Err(e);");
        assert!(
            drain < close,
            "der finale Drain muss vor pool.close() laufen"
        );
        assert!(
            close < decision,
            "pool.close() muss vor der Abschlussentscheidung laufen"
        );
        assert!(
            decision < ret,
            "das Fehlerergebnis wird erst nach dem vollständigen Cleanup zurückgegeben"
        );
        // Der Erfolgsfall bleibt erreichbar: die Fehlerrückgabe ist bedingt.
        assert!(SRC.contains("    Ok(())\n}\n"));
    }

    /// `main()` bildet jedes `Err` aus `run()` auf Exit 1 ab — auch das
    /// Fehlerergebnis des Shutdown-Abschlusses. Es entsteht **kein** zweiter,
    /// eigener Exit-Pfad für die Persistenzentscheidung.
    #[test]
    fn p26_main_maps_any_error_to_exit_1() {
        // Nur der Rumpf von `main()` — vom Anfang bis zum Beginn von `run()`.
        let main_body = &SRC[idx("fn main() {")..idx("fn run() -> Result<(), String> {")];
        let run = main_body
            .find("if let Err(e) = run()")
            .expect("run()-Fehlerbehandlung in main() fehlt");
        let exit = main_body
            .find("std::process::exit(1)")
            .expect("Exit-1-Mapping in main() fehlt");
        assert!(
            run < exit,
            "ein Err aus run() muss zu std::process::exit(1) führen"
        );
        // Es gibt keine weitere Stelle, die den Prozess beendet.
        assert_eq!(
            main_body.matches("std::process::exit").count(),
            1,
            "genau eine Exit-Stelle in main()"
        );
    }
}
