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
    pub exp: i64,
    pub gold: i64,
    pub hp: i32,
    pub char_class: String,
    /// Typsichere Klassenbasis (docs/Klassensystem.md); aus
    /// `char_class` abgeleitet.
    pub class: crate::class::ClassStatus,
    /// Fraktions-Übergangs-Hook (L10-Regel; Fraktions-/Zonensystem folgt).
    pub faction_transition: bool,
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

/// DB-Zeile für `load_character` (19 Spalten; sqlx-Tupel-Limit ist 16,
/// daher strukturbasierte Zeile wie `NpcStateRow`).
#[derive(Debug, Clone)]
struct CharacterRow {
    id: i64,
    name: String,
    level: i32,
    exp: i64,
    gold: i64,
    hp: i32,
    char_class: String,
    faction_transition: i8,
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
            exp: row.try_get("exp")?,
            gold: row.try_get("gold")?,
            hp: row.try_get("hp")?,
            char_class: row.try_get("char_class")?,
            faction_transition: row.try_get("faction_transition")?,
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
        "SELECT id, name, level, exp, gold, hp, char_class, faction_transition, pos_x, pos_y, combat_armor, \
         mana, mana_max, race, strength, agility, intelligence, constitution, wisdom, luck, \
         endurance FROM characters WHERE id = ?",
    )
    .bind(char_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Charakter laden: {e}"))?;
    if let Some(row) = row {
        let class = crate::class::ClassStatus::from_db_name(&row.char_class);
        return Ok(Character {
            id: row.id.to_string(),
            name: row.name,
            x: row.x,
            y: row.y,
            level: row.level.max(0) as u32,
            exp: row.exp.max(0),
            gold: row.gold.max(0),
            hp: row.hp,
            char_class: row.char_class,
            class,
            faction_transition: row.faction_transition != 0,
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
    // Neuer Charakter beginnt als Abenteurer
    // (docs/Klassensystem.md; Grundklassenwahl ab L9).
    let class = crate::class::ClassStatus::Adventurer;
    sqlx::query("INSERT INTO characters (name, race, char_class) VALUES (?, 'Mensch', ?)")
        .bind(char_id)
        .bind(class.canonical_db_name())
        .execute(pool)
        .await
        .map_err(|e| format!("Charakter anlegen: {e}"))?;
    Ok(Character {
        id: char_id.to_string(),
        name: char_id.to_string(),
        x: 0.0,
        y: 0.0,
        level: 1,
        exp: 0,
        gold: 0,
        hp: 100,
        char_class: class.canonical_db_name().to_string(),
        class,
        faction_transition: false,
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

/// Speichert EXP-Punktestand (Fehler nur loggen — kein Crash).
pub async fn save_exp(pool: &Pool<MySql>, char_id: &str, exp: i64) {
    if let Err(e) = sqlx::query("UPDATE characters SET exp = ? WHERE id = ?")
        .bind(exp)
        .bind(char_id)
        .execute(pool)
        .await
    {
        log::error!("saveExp {char_id}: {e}");
    }
}

/// Speichert den Goldstand (Fehler nur loggen — kein Crash).
pub async fn save_gold(pool: &Pool<MySql>, char_id: &str, gold: i64) {
    if let Err(e) = sqlx::query("UPDATE characters SET gold = ? WHERE id = ?")
        .bind(gold)
        .bind(char_id)
        .execute(pool)
        .await
    {
        log::error!("saveGold {char_id}: {e}");
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
    #[allow(dead_code)]
    pub exp_reward: i64,
    pub loot_table_id: Option<i64>,
}

/// DB-Zeile für `load_npc_definitions` (17 Spalten; sqlx-Tupel-Limit ist 16,
/// daher strukturbasierte Zeile wie `CharacterRow`).
#[derive(Debug, Clone)]
struct NpcDefSqlRow {
    id: String,
    name: String,
    kind: String,
    attackable: bool,
    aggressive: bool,
    aggro_range: f64,
    attack_range: f64,
    attack_duration_ms: i64,
    weapon_damage: i32,
    weapon_skill: i64,
    armor: i32,
    max_hp: i32,
    move_speed: f64,
    respawn_ms: Option<i64>,
    faction: Option<String>,
    exp_reward: i64,
    loot_table_id: Option<i64>,
}

impl sqlx::FromRow<'_, sqlx::mysql::MySqlRow> for NpcDefSqlRow {
    fn from_row(row: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(NpcDefSqlRow {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            kind: row.try_get("kind")?,
            attackable: row.try_get("attackable")?,
            aggressive: row.try_get("aggressive")?,
            aggro_range: row.try_get("aggro_range")?,
            attack_range: row.try_get("attack_range")?,
            attack_duration_ms: row.try_get("attack_duration_ms")?,
            weapon_damage: row.try_get("weapon_damage")?,
            weapon_skill: row.try_get("weapon_skill")?,
            armor: row.try_get("armor")?,
            max_hp: row.try_get("max_hp")?,
            move_speed: row.try_get("move_speed")?,
            respawn_ms: row.try_get("respawn_ms")?,
            faction: row.try_get("faction")?,
            exp_reward: row.try_get("exp_reward")?,
            loot_table_id: row.try_get("loot_table_id")?,
        })
    }
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
    let rows = sqlx::query_as::<_, NpcDefSqlRow>(
        "SELECT id, name, kind, attackable, aggressive, aggro_range, attack_range, \
         attack_duration_ms, weapon_damage, weapon_skill, armor, max_hp, move_speed, \
         respawn_ms, faction, exp_reward, loot_table_id FROM monster_definitions",
    )
    .persistent(false)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| format!("Definitions laden: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|r| NpcDefRow {
            id: r.id,
            name: r.name,
            kind: r.kind,
            attackable: r.attackable,
            aggressive: r.aggressive,
            aggro_range: r.aggro_range,
            attack_range: r.attack_range,
            attack_duration_ms: r.attack_duration_ms,
            weapon_damage: r.weapon_damage,
            weapon_skill: r.weapon_skill,
            armor: r.armor,
            max_hp: r.max_hp,
            move_speed: r.move_speed,
            respawn_ms: r.respawn_ms,
            faction: r.faction,
            exp_reward: r.exp_reward,
            loot_table_id: r.loot_table_id,
        })
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

/// Persistiert die permanente Klassenwahl (L9) sowie den Fraktions-
/// Übergang (L10-Hook) aus `char_class` (docs/Klassensystem.md). Fehler
/// werden nur geloggt (wie save_position), damit kein Login abgebrochen
/// wird. Hook für den noch ausstehenden Trainer-Flow (docs/Klassensystem.md).
#[allow(dead_code)]
pub async fn save_character_class(
    pool: &Pool<MySql>,
    char_id: &str,
    class: crate::class::ClassStatus,
    faction_transition: bool,
) {
    if let Err(e) = sqlx::query(
        "UPDATE characters SET char_class = ?, faction_transition = ? WHERE id = ?",
    )
    .bind(class.canonical_db_name())
    .bind(faction_transition)
    .bind(char_id)
    .execute(pool)
    .await
    {
        log::error!("saveCharacterClass {char_id}: {e}");
    }
}

// ===== Item System V1 (Migrationen 014/015) =====
// Persistente Grundlage: Statische Definitionen (item_definitions) und
// individuelle Instanzen (item_instances) in der OWN Realm-Datenbank
// (docs/Datenbank_Architektur.md §5/§19). Effektiver Wert =
// Basiswert (Definition) + Instanz-Modifikation. Die konkrete Nutzung
// (Inventory/Crafting/Loot) ist NICHT Teil von Item System V1.

/// Lädt alle statischen Item-Definitionen inkl. Klassen/Attributen/
/// Resistenzen (Content-Schicht der Realm-Inhaltsversion).
pub async fn load_item_definitions(pool: &Pool<MySql>) -> Result<Vec<crate::item::ItemDefinition>, String> {
    use crate::item::{BindingRule, ItemCategory, ItemDefinition, Rarity};

    let rows =
        sqlx::query_as::<_, (String, String, Option<String>, String, String, i32, f64, i64, f64, Option<f64>, Option<i64>, Option<f64>, Option<String>, Option<f64>, Option<i32>, String)>(
            "SELECT id, name, description, category, rarity, item_level, base_quality, \
             max_stack, weight, base_damage, duration_ms, range, weapon_type, armor_value, \
             min_level, binding_rule FROM item_definitions",
        )
        .persistent(false)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Item-Definitionen laden: {e}"))?;

    let mut defs = Vec::new();
    for (
        id,
        name,
        description,
        category,
        rarity,
        item_level,
        base_quality,
        max_stack,
        weight,
        base_damage,
        duration_ms,
        range,
        weapon_type,
        armor_value,
        min_level,
        binding_rule,
    ) in rows
    {
        let category = match ItemCategory::from_db(&category) {
            Some(c) => c,
            None => {
                log::error!("Item-Definition {id}: unbekannte Kategorie; übersprungen");
                continue;
            }
        };
        let rarity = match Rarity::from_db(&rarity) {
            Some(r) => r,
            None => {
                log::error!("Item-Definition {id}: unbekannte Seltenheit; übersprungen");
                continue;
            }
        };
        let binding_rule = match BindingRule::from_db(&binding_rule) {
            Some(b) => b,
            None => {
                log::error!("Item-Definition {id}: unbekannte Bindungsregel; übersprungen");
                continue;
            }
        };

        // Erlaubte Klassen (leer = keine Beschränkung).
        let classes: Vec<(String,)> = sqlx::query_as(
            "SELECT class FROM item_definition_classes WHERE item_id = ? ORDER BY class",
        )
        .bind(&id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Item-Klassen laden ({id}): {e}"))?;
        let allowed_classes = classes
            .into_iter()
            .map(|(c,)| crate::class::ClassStatus::from_db_name(&c))
            .collect();

        // Attributboni und Resistenzen.
        let attrs: Vec<(String, f64)> =
            sqlx::query_as("SELECT attribute, bonus FROM item_definition_attributes WHERE item_id = ?")
                .bind(&id)
                .fetch_all(pool)
                .await
                .map_err(|e| format!("Item-Attribute laden ({id}): {e}"))?;
        let resists: Vec<(String, f64)> =
            sqlx::query_as("SELECT resistance, bonus FROM item_definition_resistances WHERE item_id = ?")
                .bind(&id)
                .fetch_all(pool)
                .await
                .map_err(|e| format!("Item-Resistenzen laden ({id}): {e}"))?;

        let def = ItemDefinition {
            item_id: id,
            name,
            description,
            category,
            rarity,
            item_level: item_level as i64,
            base_quality,
            max_stack,
            weight,
            weapon_type,
            base_damage,
            duration_ms,
            range,
            armor_value,
            min_level: min_level.map(i64::from),
            allowed_classes,
            binding_rule,
            attribute_bonuses: attrs.into_iter().collect(),
            resistances: resists.into_iter().collect(),
        };
        if let Err(e) = def.validate() {
            log::error!("Item-Definition {} ungültig: {e}", def.item_id);
            continue;
        }
        defs.push(def);
    }
    Ok(defs)
}

/// Lädt die Loot-Tabellen inkl. aller Einträge (item | gold | chest,
/// Migration 017). Tabellen ohne Einträge erscheinen als leere Tabellen.
pub async fn load_loot_tables(
    pool: &Pool<MySql>,
) -> Result<std::collections::HashMap<i64, crate::loot::LootTable>, String> {
    let tables = sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM loot_tables")
        .persistent(false)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Loot-Tabellen laden: {e}"))?;

    let entries =
        sqlx::query_as::<_, (i64, i64, String, Option<String>, i64, i64, f64, Option<i64>)>(
            "SELECT id, loot_table_id, kind, item_id, min_quantity, max_quantity, chance, \
             content_table_id FROM loot_entries",
        )
        .persistent(false)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Loot-Einträge laden: {e}"))?;

    let mut out: std::collections::HashMap<i64, crate::loot::LootTable> = tables
        .into_iter()
        .map(|(id, name)| {
            (
                id,
                crate::loot::LootTable {
                    id,
                    name,
                    entries: Vec::new(),
                },
            )
        })
        .collect();

    for (id, table_id, kind, item_id, min_quantity, max_quantity, chance, content_table_id) in
        entries
    {
        let Some(t) = out.get_mut(&table_id) else {
            log::error!("Loot-Eintrag {id}: Tabelle {table_id} existiert nicht; übersprungen");
            continue;
        };
        let Some(kind) = crate::loot::LootKind::from_db(&kind) else {
            log::error!("Loot-Eintrag {id}: unbekannter kind '{kind}'; übersprungen");
            continue;
        };
        // Mengen-Korrektur: bei max < min setzen wir beide auf max.
        let (mut min, max) = (min_quantity, max_quantity);
        if max < min {
            min = max;
        }
        t.entries.push(crate::loot::LootEntry {
            kind,
            item_id,
            min_quantity: min,
            max_quantity: max,
            chance: chance.clamp(0.0, 1.0),
            content_table_id,
        });
    }
    Ok(out)
}

/// Lädt eine individuelle Item-Instanz inkl. Modifikatoren.
/// Hook für Inventory/Crafting V1 (noch kein Aufrufer im Realm-Loop).
#[allow(dead_code)]
pub async fn load_item_instance(
    pool: &Pool<MySql>,
    item_uuid: &str,
) -> Result<Option<crate::item::ItemInstance>, String> {
    use crate::item::{BindingState, ItemInstance, ItemModifiers};

    let row = sqlx::query_as::<_, (String, String, i32, Option<i32>, Option<i32>, String, Option<i64>)>(
        "SELECT item_uuid, item_id, count, durability_current, durability_max, binding, creator_id \
         FROM item_instances WHERE item_uuid = ?",
    )
    .bind(item_uuid)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Item-Instanz laden ({item_uuid}): {e}"))?;

    let Some((uuid, item_id, count, durab_cur, durab_max, binding, creator_id)) = row else {
        return Ok(None);
    };
    let binding = match BindingState::from_db(&binding) {
        Some(b) => b,
        None => BindingState::Tradeable,
    };

    let mods = sqlx::query_as::<_, (f64, f64, f64, f64)>(
        "SELECT damage_modifier, armor_modifier, weight_modifier, quality_modifier \
         FROM item_instance_modifiers WHERE item_uuid = ?",
    )
    .bind(item_uuid)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Item-Modifikatoren laden ({item_uuid}): {e}"))?
    .map(|(damage, armor, weight, quality)| ItemModifiers {
        damage_modifier: damage,
        armor_modifier: armor,
        weight_modifier: weight,
        quality_modifier: quality,
        ..Default::default()
    })
    .unwrap_or_default();

    let attr_mods: Vec<(String, f64)> = sqlx::query_as(
        "SELECT attribute, modifier FROM item_instance_attribute_modifiers WHERE item_uuid = ?",
    )
    .bind(item_uuid)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Item-Attribut-Mods laden ({item_uuid}): {e}"))?;
    let res_mods: Vec<(String, f64)> = sqlx::query_as(
        "SELECT resistance, modifier FROM item_instance_resistance_modifiers WHERE item_uuid = ?",
    )
    .bind(item_uuid)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Item-Resistenz-Mods laden ({item_uuid}): {e}"))?;

    let instance = ItemInstance {
        item_uuid: uuid,
        item_id,
        count: count.into(),
        durability_current: durab_cur.map(i64::from),
        durability_max: durab_max.map(i64::from),
        binding,
        creator_id,
        modifiers: ItemModifiers {
            attribute_modifiers: attr_mods.into_iter().collect(),
            resistance_modifiers: res_mods.into_iter().collect(),
            ..mods
        },
    };
    Ok(Some(instance))
}

/// Schreibt eine individuelle Item-Instanz samt Modifikatoren (Upsert).
#[allow(dead_code)]
pub async fn save_item_instance(
    pool: &Pool<MySql>,
    instance: &crate::item::ItemInstance,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Item-Transaktion beginnen: {e}"))?;
    write_item_instance(&mut tx, instance).await?;
    tx.commit()
        .await
        .map_err(|e| format!("Item-Instanz commit: {e}"))?;
    Ok(())
}

/// Interne Transaktionshilfe: Item-Instanz + Modifikatoren in einen
/// laufenden Transaction upserten. Wird von save_item_instance und
/// save_inventory gemeinsam genutzt.
async fn write_item_instance(
    tx: &mut sqlx::Transaction<'_, MySql>,
    instance: &crate::item::ItemInstance,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO item_instances \
           (item_uuid, item_id, count, durability_current, durability_max, binding, creator_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE item_id = VALUES(item_id), count = VALUES(count), \
           durability_current = VALUES(durability_current), \
           durability_max = VALUES(durability_max), binding = VALUES(binding), \
           creator_id = VALUES(creator_id)",
    )
    .bind(&instance.item_uuid)
    .bind(&instance.item_id)
    .bind(instance.count)
    .bind(instance.durability_current)
    .bind(instance.durability_max)
    .bind(instance.binding.as_db())
    .bind(instance.creator_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("Item-Instanz speichern: {e}"))?;

    sqlx::query(
        "INSERT INTO item_instance_modifiers \
           (item_uuid, damage_modifier, armor_modifier, weight_modifier, quality_modifier) \
         VALUES (?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE damage_modifier = VALUES(damage_modifier), \
           armor_modifier = VALUES(armor_modifier), weight_modifier = VALUES(weight_modifier), \
           quality_modifier = VALUES(quality_modifier)",
    )
    .bind(&instance.item_uuid)
    .bind(instance.modifiers.damage_modifier)
    .bind(instance.modifiers.armor_modifier)
    .bind(instance.modifiers.weight_modifier)
    .bind(instance.modifiers.quality_modifier)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("Item-Modifikatoren speichern: {e}"))?;

    for (attribute, value) in &instance.modifiers.attribute_modifiers {
        sqlx::query(
            "INSERT INTO item_instance_attribute_modifiers (item_uuid, attribute, modifier) \
             VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE modifier = VALUES(modifier)",
        )
        .bind(&instance.item_uuid)
        .bind(attribute)
        .bind(value)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("Item-Attribut-Mod speichern: {e}"))?;
    }
    for (resistance, value) in &instance.modifiers.resistance_modifiers {
        sqlx::query(
            "INSERT INTO item_instance_resistance_modifiers (item_uuid, resistance, modifier) \
             VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE modifier = VALUES(modifier)",
        )
        .bind(&instance.item_uuid)
        .bind(resistance)
        .bind(value)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("Item-Resistenz-Mod speichern: {e}"))?;
    }
    Ok(())
}

// ===== Inventory System V1 (Migration 016) =====
// Realm-autoritative Persistierung von Grundinventar, Rucksäcken, Equipment
// und dem temporären Sicherheits-Puffer (docs/inventory_system.md).
//
// Volles Transaktions-Schreiben (Vollwrite je Mutation): Alle Instanzen
// werden in item_instances upsertet, danach alle Platzierungs-Tabellen
// komplett neu befüllt. Puffer-Zeilen werden beim Logout gelöscht.

/// Hilfsfunktion: Item-Instanz laden oder None (verschwundene
/// FK-Zeilen → leerer Slot).
async fn load_instance_opt(
    pool: &Pool<MySql>,
    uuid: &str,
) -> Option<crate::item::ItemInstance> {
    match load_item_instance(pool, uuid).await {
        Ok(Some(i)) => Some(i),
        _ => None,
    }
}

/// Lädt das Inventar eines Charakters (Migration 016): Grundinventar,
/// Rucksäcke + Slots, Equipment sowie den Sicherheits-Puffer.
/// `base_slots` ist die Anzahl der konfigurierten Basis-Slots
/// (INVENTORY_BASE_SLOTS, Config-Wert).
pub async fn load_inventory(
    pool: &Pool<MySql>,
    char_id: &str,
    base_slots: usize,
) -> Result<crate::inventory::InventoryState, String> {
    use crate::inventory::{EquipSlot, InventoryState};

    let mut state = InventoryState::new(base_slots);

    // --- Grundinventar (Basis-Slots) ---
    let rows: Vec<(i64, Option<String>)> =
        sqlx::query_as("SELECT slot, item_uuid FROM character_inventory WHERE char_id = ? ORDER BY slot")
            .bind(char_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Inventar laden: {e}"))?;
    for (slot, uuid) in rows {
        if slot < 0 || slot as usize >= state.base_slots.len() {
            log::warn!(
                "loadInventory {char_id}: Slot {slot} ausserhalb Basis ({base_slots}); ignoriert"
            );
            continue;
        }
        if let Some(u) = uuid {
            state.base_slots[slot as usize] = load_instance_opt(pool, &u).await;
        }
    }

    // --- Rucksäcke (Definitionen) ---
    let bag_rows: Vec<(i64, String, i64)> =
        sqlx::query_as("SELECT bag_id, name, slot_count FROM character_bags WHERE char_id = ? ORDER BY bag_id")
            .bind(char_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Rucksäcke laden: {e}"))?;
    for (bag_id, name, slot_count) in bag_rows {
        let n = slot_count.max(0) as usize;
        if n == 0 {
            continue;
        }
        state.bags.push(crate::inventory::Bag {
            bag_id: bag_id as u64,
            name,
            slots: vec![None; n],
        });
    }
    // --- Bag-Slots (Inhalt) ---
    let slot_rows: Vec<(i64, i64, Option<String>)> =
        sqlx::query_as(
            "SELECT bag_id, slot, item_uuid FROM bag_slots WHERE char_id = ? ORDER BY bag_id, slot",
        )
        .bind(char_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Rucksack-Slots laden: {e}"))?;
    for (bag_id, slot, uuid) in slot_rows {
        if let Some(b) = state.bags.iter_mut().find(|b| b.bag_id == bag_id as u64) {
            if slot >= 0 && (slot as usize) < b.slots.len() {
                if let Some(u) = uuid {
                    b.slots[slot as usize] = load_instance_opt(pool, &u).await;
                }
            }
        }
    }

    // --- Equipment-Slots ---
    let eq_rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT slot, item_uuid FROM character_equipment WHERE char_id = ?")
            .bind(char_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Equipment laden: {e}"))?;
    for (slot, uuid) in eq_rows {
        if let (Some(u), Some(es)) = (uuid, EquipSlot::from_db(&slot)) {
            if let Some(inst) = load_instance_opt(pool, &u).await {
                state.equipped.insert(es, inst);
            }
        }
    }

    // --- Sicherheits-Puffer (temporär) ---
    let buf_rows: Vec<(i64, Option<String>)> =
        sqlx::query_as("SELECT slot, item_uuid FROM inventory_buffer WHERE char_id = ? ORDER BY slot")
            .bind(char_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Puffer laden: {e}"))?;
    for (slot, uuid) in buf_rows {
        if slot < 0 {
            continue;
        }
        let idx = slot as usize;
        if idx >= state.buffer.len() {
            state.buffer.resize(idx + 1, None);
        }
        if let Some(u) = uuid {
            state.buffer[idx] = load_instance_opt(pool, &u).await;
        }
    }

    Ok(state)
}

/// Volles Transaktions-Schreiben (Vollwrite) des Inventar-Zustands:
/// Alle aktuell vorhandenen item_instances werden upsertet, danach alle
/// Platzierungs-Tabellen (Inventar, Rucksäcke, Equipment) komplett neu
/// befüllt. Der Sicherheits-Puffer wird NICHT persistiert (temporär,
///docs/inventory_system.md §11).
pub async fn save_inventory(
    pool: &Pool<MySql>,
    char_id: &str,
    state: &crate::inventory::InventoryState,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Inventar-Transaktion beginnen: {e}"))?;

    // --- 1. Alle betroffenen item_instances upserten ---
    let mut seen = std::collections::HashSet::new();
    let instances: Vec<&crate::item::ItemInstance> = state
        .base_slots
        .iter()
        .chain(state.bags.iter().flat_map(|b| &b.slots))
        .filter_map(|s| s.as_ref())
        .chain(state.equipped.values())
        .collect();
    for inst in &instances {
        if seen.insert(inst.item_uuid.as_str()) {
            write_item_instance(&mut tx, inst).await?;
        }
    }

    // --- 2. Platzierungen komplett neu schreiben (Transaktions-Vollwrite) ---
    sqlx::query("DELETE FROM character_inventory WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Altes Inventar löschen: {e}"))?;
    sqlx::query("DELETE FROM bag_slots WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Alte Bag-Slots löschen: {e}"))?;
    sqlx::query("DELETE FROM character_bags WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Alte Rucksäcke löschen: {e}"))?;
    sqlx::query("DELETE FROM character_equipment WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Altes Equipment löschen: {e}"))?;

    // Grundinventar (nur belegte Slots werden geschrieben).
    for (i, slot) in state.base_slots.iter().enumerate() {
        if let Some(inst) = slot {
            sqlx::query(
                "INSERT INTO character_inventory (char_id, slot, item_uuid) VALUES (?, ?, ?)",
            )
            .bind(char_id)
            .bind(i as i64)
            .bind(&inst.item_uuid)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Inventar-Slot speichern: {e}"))?;
        }
    }
    // Rucksäcke + Bag-Slots.
    for b in &state.bags {
        sqlx::query(
            "INSERT INTO character_bags (char_id, bag_id, name, slot_count) VALUES (?, ?, ?, ?)",
        )
        .bind(char_id)
        .bind(b.bag_id as i64)
        .bind(&b.name)
        .bind(b.slots.len() as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Rucksack speichern: {e}"))?;
        for (i, slot) in b.slots.iter().enumerate() {
            if let Some(inst) = slot {
                sqlx::query(
                    "INSERT INTO bag_slots (char_id, bag_id, slot, item_uuid) VALUES (?, ?, ?, ?)",
                )
                .bind(char_id)
                .bind(b.bag_id as i64)
                .bind(i as i64)
                .bind(&inst.item_uuid)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("Bag-Slot speichern: {e}"))?;
            }
        }
    }
    // Equipment-Slots.
    for (slot, inst) in &state.equipped {
        sqlx::query(
            "INSERT INTO character_equipment (char_id, slot, item_uuid) VALUES (?, ?, ?)",
        )
        .bind(char_id)
        .bind(slot.as_db())
        .bind(&inst.item_uuid)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Equipment speichern: {e}"))?;
    }
    // Puffer: in V1 nicht persistiert (temporär; §11). Zeilen werden
    // beim Logout gelöscht (wipe_logout_buffer) und niemals geschrieben.

    tx.commit()
        .await
        .map_err(|e| format!("Inventar-Transaktion commit: {e}"))?;
    Ok(())
}

/// Löscht beim Logout den Sicherheits-Puffer (docs/inventory_system.md §11):
/// Puffer-Zeilen des Charakters sowie die zugehörigen, nun verwaisten
/// item_instances-Zeilen (drop_buffer liefert die verfallenen UUIDs).
/// In V1 werden Pufferzeilen nie geschrieben, daher ist dieser Aufruf
/// ein No-Op; die Funktion steht für künftige Crash-Recovery bereit.
#[allow(dead_code)]
pub async fn wipe_logout_buffer(
    pool: &Pool<MySql>,
    char_id: &str,
    uuids: &[String],
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Puffer-Transaktion beginnen: {e}"))?;
    sqlx::query("DELETE FROM inventory_buffer WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Puffer-Zeilen löschen: {e}"))?;
    for u in uuids {
        sqlx::query("DELETE FROM item_instances WHERE item_uuid = ?")
            .bind(u)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Verwaiste Instanz löschen ({u}): {e}"))?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("Puffer-Transaktion commit: {e}"))?;
    Ok(())
}
