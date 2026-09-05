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
        return Err(format!("fehlende {}-DB-Konfiguration: {}", prefix, missing.join(", ")));
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
/// verhalten aus src/realm, Zone 0, Spawn 0,0).
pub async fn load_character(pool: &Pool<MySql>, char_id: &str) -> Result<Character, String> {
    let row: Option<(i64, String, f64, f64)> =
        sqlx::query_as("SELECT id, name, pos_x, pos_y FROM characters WHERE id = ?")
            .bind(char_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("Charakter laden: {e}"))?;
    if let Some((id, name, x, y)) = row {
        return Ok(Character { id: id.to_string(), name, x, y });
    }
    sqlx::query("INSERT INTO characters (name, race, char_class) VALUES (?, 'Mensch', 'Warrior')")
        .bind(char_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Charakter anlegen: {e}"))?;
    Ok(Character { id: char_id.to_string(), name: char_id.to_string(), x: 0.0, y: 0.0 })
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
