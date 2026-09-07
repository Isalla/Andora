// db — Einziger Datenbank-Pool des Realms: realm_state_<realm>.
// Zielarchitektur: Charakterdaten UND Realm-Zustand liegen in derselben
// Realm-Datenbank (docs/Datenbank_Architektur.md). Keine character- oder
// world_data-Pools (Alt-Architektur des TypeScript-Übergangsstands).
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
