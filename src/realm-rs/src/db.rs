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
use sqlx::{MySql, Pool, Row as _};

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
    pub mana: i32,
    pub mana_max: i32,
    // Geladen (echte DB-Spalte); konkrete Rassensystem-Nutzung folgt (§2).
    #[allow(dead_code)]
    pub race: String,
    pub strength: i32,
    pub dexterity: i32,
    pub intelligence: i32,
    pub constitution: i32,
    pub wisdom: i32,
    pub luck: i32,
    pub endurance: i32,
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

/// DB-Zeile für `load_character` (18 Spalten; sqlx-Tupel-Limit ist 16,
/// daher strukturbasierte Zeile wie `NpcStateRow`).
#[derive(Debug, Clone)]
struct CharacterRow {
    id: i64,
    name: String,
    level: i32,
    hp: i32,
    char_class: String,
    x: f64,
    y: f64,
    armor: i32,
    mana: i32,
    mana_max: i32,
    race: String,
    strength: i32,
    dexterity: i32,
    intelligence: i32,
    constitution: i32,
    wisdom: i32,
    luck: i32,
    endurance: i32,
}

impl sqlx::FromRow<'_, sqlx::mysql::MySqlRow> for CharacterRow {
    fn from_row(row: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(CharacterRow {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            level: row.try_get("level")?,
            hp: row.try_get("hp")?,
            char_class: row.try_get("char_class")?,
            x: row.try_get("pos_x")?,
            y: row.try_get("pos_y")?,
            armor: row.try_get("combat_armor")?,
            mana: row.try_get("mana")?,
            mana_max: row.try_get("mana_max")?,
            race: row.try_get("race")?,
            strength: row.try_get("strength")?,
            dexterity: row.try_get("agility")?,
            intelligence: row.try_get("intelligence")?,
            constitution: row.try_get("constitution")?,
            wisdom: row.try_get("wisdom")?,
            luck: row.try_get("luck")?,
            endurance: row.try_get("endurance")?,
        })
    }
}

/// Lädt einen Charakter; erzeugt ihn bei Bedarf (Übergangs-Prototyp-
/// verhalten aus src/realm, Zone 0, Spawn 0,0). Kämpft damit mit
/// geladener Klasse, Level, HP, Mana und Rüstung ein (Combat V1/V3).
pub async fn load_character(pool: &Pool<MySql>, char_id: &str) -> Result<Character, String> {
    let row: Option<CharacterRow> = sqlx::query_as::<_, CharacterRow>(
        "SELECT id, name, level, hp, char_class, pos_x, pos_y, combat_armor, mana, mana_max, \
         race, strength, agility, intelligence, constitution, wisdom, luck, endurance \
         FROM characters WHERE id = ?",
    )
    .bind(char_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Charakter laden: {e}"))?;
    if let Some(row) = row {
        return Ok(Character {
            id: row.id.to_string(),
            name: row.name,
            x: row.x,
            y: row.y,
            level: row.level.max(0) as u32,
            hp: row.hp,
            char_class: row.char_class,
            armor: row.armor,
            mana: row.mana,
            mana_max: row.mana_max.max(row.mana),
            race: row.race,
            strength: row.strength.max(1),
            dexterity: row.dexterity.max(1),
            intelligence: row.intelligence.max(1),
            constitution: row.constitution.max(1),
            wisdom: row.wisdom.max(1),
            luck: row.luck.max(1),
            endurance: row.endurance.max(1),
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
        mana: 50,
        mana_max: 50,
        race: "Mensch".to_string(),
        strength: 10,
        dexterity: 10,
        intelligence: 10,
        constitution: 10,
        wisdom: 10,
        luck: 10,
        endurance: 10,
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

/// Content-Definition einer Fähigkeit (ability_definitions, Migration 010).
use crate::combat::ability::AbilityDefRow;

/// Lädt alle Ability-Definitionen (Content-Schicht, Migration 010).
pub async fn load_ability_definitions(pool: &Pool<MySql>) -> Result<Vec<AbilityDefRow>, String> {
    let rows = sqlx::query(
        "SELECT id, name, exec_type, semantic_category, mana_cost, cooldown_ms, \
               cooldown_persistent, cast_time_ms, range, aoe_type, aoe_radius, \
               host_effect, effect_kind, effect_value, duration_ms, tick_ms, effect_group \
         FROM ability_definitions",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Ability-Definitionen laden: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|r| AbilityDefRow {
            id: r.get("id"),
            name: r.get("name"),
            exec_type: r.get("exec_type"),
            semantic_category: r.get("semantic_category"),
            mana_cost: r.get("mana_cost"),
            cooldown_ms: r.get("cooldown_ms"),
            cooldown_persistent: r.get("cooldown_persistent"),
            cast_time_ms: r.get("cast_time_ms"),
            range: r.get("range"),
            aoe_type: r.get("aoe_type"),
            aoe_radius: r.get("aoe_radius"),
            host_effect: r.get("host_effect"),
            effect_kind: r.get("effect_kind"),
            effect_value: r.get("effect_value"),
            duration_ms: r.get("duration_ms"),
            tick_ms: r.get("tick_ms"),
            effect_group: r.get("effect_group"),
        })
        .collect())
}

/// Gelernte Fähigkeiten eines Charakters (character_abilities, Migration 010).
pub async fn load_character_abilities(
    pool: &Pool<MySql>,
    char_id: &str,
) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT ability_id FROM character_abilities WHERE char_id = ?")
            .bind(char_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Charakter-Fähigkeiten laden: {e}"))?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
