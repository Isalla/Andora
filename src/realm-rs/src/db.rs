// db — Einziger Datenbank-Pool des Realms: realm_state_<realm>.
// Zielarchitektur: Charakterdaten UND Realm-Zustand liegen in derselben
// Realm-Datenbank (docs/Datenbank_Architektur.md). Keine character- oder
// world_data-Pools (Alt-Architektur des TypeScript-Übergangsstands).
//
// Combat V2 (docs/Kampfsystem.md §§18–21, Boss-System.md §§2–6): Dieser
// Pool liest/schreibt zusätzlich die Content-Schicht (monster_definitions,
// monster_spawns, Migration 009) sowie den persistenten Runtime-Zustand
// (monster_instances: Status, Respawn-Timer, Boss-Claim) — Respawn- und
// Boss-Zustände überstehen Realm-Neustarts (Datenbank_Architektur.md §5).
use sqlx::{MySql, Pool};

use crate::config::DbConfig;

#[derive(Debug, Clone)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub level: u32,
    pub hp: i32,
    pub char_class: String,
    pub armor: i32,
}

/// Verbindet den Pool und prüft die Verbindung (SELECT 1).
/// Fehlt die Konfiguration, Abbruch mit klarer Meldung (kein Fallback).
pub async fn open_pool(prefix: &str, cfg: &DbConfig) -> Result<Pool<MySql>, String> {
    let mut missing = Vec::new();
    if cfg.host.is_empty() {
        missing.push(format!("{prefix}_HOST"));
    }
    if cfg.user.is_empty() {
        missing.push(format!("{prefix}_USER"));
    }
    if cfg.database.is_empty() {
        missing.push(format!("{prefix}_NAME"));
    }
    if !missing.is_empty() {
        return Err(format!(
            "fehlende {}-DB-Konfiguration: {}",
            prefix,
            missing.join(", ")
        ));
    }
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.url())
        .await
        .map_err(|e| {
            format!(
                "verbinden (host {}:{}, user {}, db {}): {e}",
                cfg.host, cfg.port, cfg.user, cfg.database
            )
        })?;
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(|e| format!("DB-Verbindung prüfen: {e}"))?;
    Ok(pool)
}

/// Lädt einen Charakter; erzeugt ihn bei Bedarf (Übergangs-Prototyp-
/// verhalten aus src/realm, Zone 0, Spawn 0,0). Kämpft damit mit
/// geladener Klasse, Level, HP und Rüstung ein (Combat V1).
pub async fn load_character(pool: &Pool<MySql>, char_id: &str) -> Result<Character, String> {
    type Row = (i64, String, i32, i32, String, f64, f64, i32);
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, name, level, hp, char_class, pos_x, pos_y, combat_armor FROM characters WHERE id = ?",
    )
    .bind(char_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Charakter laden: {e}"))?;
    if let Some((id, name, level, hp, char_class, x, y, armor)) = row {
        return Ok(Character {
            id: id.to_string(),
            name,
            x,
            y,
            level: level.max(0) as u32,
            hp,
            char_class,
            armor,
        });
    }
    sqlx::query("INSERT INTO characters (name, race, char_class) VALUES (?, 'Mensch', 'Warrior')")
        .bind(char_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Charakter anlegen: {e}"))?;
    Ok(Character {
        id: char_id.to_string(),
        name: char_id.to_string(),
        x: 0.0,
        y: 0.0,
        level: 1,
        hp: 100,
        char_class: "Warrior".to_string(),
        armor: 0,
    })
}

/// Level des Waffen-/Kampfskills (docs/Kampfsystem.md §4). Grundsätzlich
/// verfügbare Skills starten bei 1; kein Eintrag = 1.
pub async fn load_weapon_skill(pool: &Pool<MySql>, char_id: &str, skill_id: &str) -> u32 {
    let lvl: Option<i32> =
        sqlx::query_scalar("SELECT lvl FROM skills WHERE char_id = ? AND skill_id = ?")
            .bind(char_id)
            .bind(skill_id)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);
    lvl.filter(|l| *l > 1).unwrap_or(1) as u32
}

/// Speichert die Position (Fehler nur loggen — kein Kick, wie bisher).
pub async fn save_position(pool: &Pool<MySql>, char_id: &str, x: f64, y: f64) {
    if let Err(e) = sqlx::query("UPDATE characters SET pos_x = ?, pos_y = ? WHERE id = ?")
        .bind(x)
        .bind(y)
        .bind(char_id)
        .execute(pool)
        .await
    {
        log::error!("savePosition: {e}");
    }
}

/// Content-Definition eines Monsters (monster_definitions, Migration 009).
#[derive(Debug, Clone)]
pub struct NpcDefRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub attackable: bool,
    pub aggressive: bool,
    pub aggro_range: f64,
    pub attack_range: f64,
    pub attack_duration_ms: i64,
    pub weapon_damage: i32,
    pub weapon_skill: i64,
    pub armor: i32,
    pub max_hp: i32,
    pub move_speed: f64,
    pub respawn_ms: Option<i64>,
    pub faction: Option<String>,
}

/// Spawn-Platzierung (monster_spawns: Home-Zone, Leash, Pack, Overrides).
#[derive(Debug, Clone)]
pub struct NpcSpawnRow {
    pub id: i64,
    pub monster_id: String,
    /// Content-Feld für spätere zonenbasierte Filterung (aktuell Zone 0).
    #[allow(dead_code)]
    pub zone_id: i64,
    pub home_x: f64,
    pub home_y: f64,
    pub home_radius: f64,
    pub leash_radius: f64,
    pub attackable: Option<bool>,
    pub aggressive: Option<bool>,
    pub respawn_ms: Option<i64>,
    pub pack_id: Option<String>,
}

/// Lädt alle Monster-Definitionen (Content-Schicht).
pub async fn load_npc_definitions(pool: &Pool<MySql>) -> Result<Vec<NpcDefRow>, String> {
    let mut conn = pool.acquire().await.map_err(|e| format!("pool: {e}"))?;
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            bool,
            bool,
            f64,
            f64,
            i64,
            i32,
            i64,
            i32,
            i32,
            f64,
            Option<i64>,
            Option<String>,
        ),
    >(
        "SELECT id, name, kind, attackable, aggressive, aggro_range, attack_range, \
         attack_duration_ms, weapon_damage, weapon_skill, armor, max_hp, move_speed, \
         respawn_ms, faction FROM monster_definitions",
    )
    .persistent(false)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| format!("Definitions laden: {e}"))?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                name,
                kind,
                attackable,
                aggressive,
                aggro_range,
                attack_range,
                attack_duration_ms,
                weapon_damage,
                weapon_skill,
                armor,
                max_hp,
                move_speed,
                respawn_ms,
                faction,
            )| {
                NpcDefRow {
                    id,
                    name,
                    kind,
                    attackable,
                    aggressive,
                    aggro_range,
                    attack_range,
                    attack_duration_ms,
                    weapon_damage,
                    weapon_skill,
                    armor,
                    max_hp,
                    move_speed,
                    respawn_ms,
                    faction,
                }
            },
        )
        .collect())
}

/// Lädt alle Monster-Spawns (Content-Schicht, zone/zone_id >= 0).
pub async fn load_npc_spawns(pool: &Pool<MySql>) -> Result<Vec<NpcSpawnRow>, String> {
    let mut conn = pool.acquire().await.map_err(|e| format!("pool: {e}"))?;
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            i64,
            f64,
            f64,
            f64,
            f64,
            Option<bool>,
            Option<bool>,
            Option<i64>,
            Option<String>,
        ),
    >(
        "SELECT id, monster_id, zone_id, home_x, home_y, home_radius, leash_radius, \
         attackable, aggressive, respawn_ms, pack_id FROM monster_spawns ORDER BY id",
    )
    .persistent(false)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| format!("Spawns laden: {e}"))?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                monster_id,
                zone_id,
                home_x,
                home_y,
                home_radius,
                leash_radius,
                attackable,
                aggressive,
                respawn_ms,
                pack_id,
            )| {
                NpcSpawnRow {
                    id,
                    monster_id,
                    zone_id,
                    home_x,
                    home_y,
                    home_radius,
                    leash_radius,
                    attackable,
                    aggressive,
                    respawn_ms,
                    pack_id,
                }
            },
        )
        .collect())
}

/// Gespeicherter NPC-Runtime-Zustand (monster_instances).
#[derive(Debug, Clone)]
pub struct NpcStateRow {
    pub spawn_id: i64,
    pub status: String,
    pub hp: i32,
    pub x: f64,
    pub y: f64,
    pub respawn_after_ms: Option<i64>,
    pub claimed_by: Option<String>,
    pub claim_at_ms: Option<i64>,
}

/// Lädt den persistenten NPC-Zustand aller Instanzen (monster_instances).
/// Zu jedem Spawn-Slot existiert genau eine Zeile (§21/Boss §6: Respawn-
/// und Boss-Zustände überstehen Realm-Neustarts).
pub async fn load_npc_states(pool: &Pool<MySql>) -> Result<Vec<NpcStateRow>, String> {
    sqlx::query_as::<_, NpcStateRow>(
        "SELECT spawn_id, status, hp, x, y, respawn_after_ms, claimed_by, claim_at_ms \
         FROM monster_instances",
    )
    .persistent(false)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("NPC-Zustände laden: {e}"))
}

impl sqlx::FromRow<'_, sqlx::mysql::MySqlRow> for NpcStateRow {
    fn from_row(row: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(NpcStateRow {
            spawn_id: row.try_get("spawn_id")?,
            status: row.try_get("status")?,
            hp: row.try_get("hp")?,
            x: row.try_get("x")?,
            y: row.try_get("y")?,
            respawn_after_ms: row.try_get("respawn_after_ms")?,
            claimed_by: row.try_get("claimed_by")?,
            claim_at_ms: row.try_get("claim_at_ms")?,
        })
    }
}

/// Schreibt den persistenten NPC-Zustand einer Instanz. Die Münze des
/// Realm-Neustart-Schutzes: Ein "returning"/"alive"-Zustand + laufender
/// Respawn-Timer (respawn_after_ms) bleibt damit auch über Neustarts exakt
/// erhalten (bei radikaleren Neustarts werden tote NPCs wiederbelebt).
#[allow(clippy::too_many_arguments)]
pub async fn save_npc_state(
    pool: &Pool<MySql>,
    spawn_id: i64,
    status: &str,
    hp: i32,
    x: f64,
    y: f64,
    respawn_after_ms: Option<i64>,
    claimed_by: Option<&str>,
    claim_at_ms: Option<i64>,
) {
    let result = sqlx::query(
        "INSERT INTO monster_instances \
           (spawn_id, status, hp, x, y, respawn_after_ms, claimed_by, claim_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE status = VALUES(status), hp = VALUES(hp), \
           x = VALUES(x), y = VALUES(y), \
           respawn_after_ms = VALUES(respawn_after_ms), \
           claimed_by = VALUES(claimed_by), claim_at_ms = VALUES(claim_at_ms)",
    )
    .bind(spawn_id)
    .bind(status)
    .bind(hp)
    .bind(x)
    .bind(y)
    .bind(respawn_after_ms)
    .bind(claimed_by)
    .bind(claim_at_ms)
    .execute(pool)
    .await;
    if let Err(e) = result {
        log::error!("saveNpcState {spawn_id}: {e}");
    }
}
