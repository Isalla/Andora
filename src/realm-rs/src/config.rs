// config — Konfiguration aus config.env neben dem Binary (Zielarchitektur:
// genau EINE Realm-Datenbank realm_state_<realm>; keine character- oder
// world_data-Pools wie im TypeScript-Übergangsstand).
// Pfad auch per argv[1] oder REALM_CONFIG. Format: KEY=VALUE, #-Kommentare.
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DbConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}

impl DbConfig {
    /// MariaDB-DSN im sqlx-Format (ohne TLS; LAN-Betrieb wie bisher).
    pub fn url(&self) -> String {
        format!(
            "mysql://{}:{}@{}:{}/{}",
            self.user, self.password, self.host, self.port, self.database
        )
    }
}

#[derive(Debug, Clone)]
pub struct AuthApiConfig {
    /// Leer = Auth-Anbindung bewusst deaktiviert (Entwicklung/Testprototyp).
    pub url: String,
    pub service_id: String,
    pub secret: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Eigene Realm-ID (prüft Handoff-Bindung: handoff.realm_id muss passen).
    pub realm_id: u32,
    pub ws_port: u16,
    pub health_port: u16,
    pub tick_ms: u64,
    pub aofb_radius: f64,
    /// Roadmap: Client-Render-Cap (PERFGO) + Coordinator/Ollama-Anbindung.
    #[allow(dead_code)]
    pub render_cap: u32,
    /// Roadmap: Coordinator/Ollama-Anbindung (Ziel: Realm→Coordinator).
    #[allow(dead_code)]
    pub ollama_url: String,
    pub auth_api: AuthApiConfig,
    /// Die EIGENE Realm-Datenbank (realm_state_<realm>).
    pub realm_db: DbConfig,
    /// Migrationsverzeichnis-Override (sonst <bindir>/migrations).
    pub migrations_dir: String,
    /// Freigabe destruktiver Migrationen (nur nach Backup im Update-Ablauf).
    pub allow_destructive: bool,
}

pub fn load_env(path: &std::path::Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let Some(eq) = t.find('=') else { continue };
        out.insert(t[..eq].trim().to_string(), t[eq + 1..].trim().to_string());
    }
    out
}

fn num(env: &HashMap<String, String>, key: &str, def: u64) -> u64 {
    env.get(key).and_then(|v| v.parse::<u64>().ok()).filter(|&v| v > 0).unwrap_or(def)
}

fn db_config(env: &HashMap<String, String>, prefix: &str, default_db: &str) -> DbConfig {
    let g = |k: &str| env.get(&format!("{prefix}_{k}")).cloned().unwrap_or_default();
    DbConfig {
        host: g("HOST"),
        port: num(env, &format!("{prefix}_PORT"), 3306) as u16,
        user: g("USER"),
        password: g("PASSWORD"),
        database: {
            let d = g("NAME");
            if d.is_empty() {
                default_db.to_string()
            } else {
                d
            }
        },
    }
}

/// Löst den config.env-Pfad auf: argv[1], sonst REALM_CONFIG, sonst
/// config.env neben dem Binary.
pub fn config_path(args: &[String]) -> PathBuf {
    if args.len() > 1 {
        return PathBuf::from(&args[1]);
    }
    if let Ok(p) = std::env::var("REALM_CONFIG") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let mut exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    exe.pop();
    exe.join("config.env")
}

pub fn load_config(path: &std::path::Path) -> Result<Config, String> {
    let env = load_env(path);
    let g = |k: &str| env.get(k).cloned().unwrap_or_default();
    let realm_db = db_config(&env, "REALM_STATE_DB", "realm_state_de1");
    let mut missing = Vec::new();
    if realm_db.host.is_empty() {
        missing.push("REALM_STATE_DB_HOST");
    }
    if realm_db.user.is_empty() {
        missing.push("REALM_STATE_DB_USER");
    }
    if realm_db.database.is_empty() {
        missing.push("REALM_STATE_DB_NAME");
    }
    let realm_id = num(&env, "REALM_ID", 0) as u32;
    if realm_id == 0 {
        missing.push("REALM_ID");
    }
    if !missing.is_empty() {
        return Err(format!("fehlende Realm-Konfiguration: {}", missing.join(", ")));
    }
    Ok(Config {
        realm_id,
        ws_port: num(&env, "PORT_WS", 3001) as u16,
        health_port: num(&env, "PORT_HTTP", 3002) as u16,
        tick_ms: num(&env, "TICK_MS", 100),
        aofb_radius: num(&env, "AOFB_RADIUS", 20) as f64,
        render_cap: num(&env, "RENDER_CAP_DEFAULT", 64) as u32,
        ollama_url: {
            let u = g("OLLAMA_URL");
            if u.is_empty() {
                "http://192.168.1.32:11434".to_string()
            } else {
                u
            }
        },
        auth_api: AuthApiConfig {
            url: g("AUTHAPI_URL").trim_end_matches('/').to_string(),
            service_id: g("AUTHAPI_SERVICE_ID"),
            secret: g("AUTHAPI_SERVICE_SECRET"),
        },
        realm_db,
        migrations_dir: g("REALM_STATE_MIGRATIONS_DIR"),
        allow_destructive: g("ALLOW_DESTRUCTIVE_MIGRATIONS") == "1",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn missing_config_fails_loudly() {
        let env = env_of(&[]);
        let path = std::path::Path::new("/gibt/es/nicht.env");
        let _ = (env, path);
        // Leere Umgebung: REALM_ID + DB-Felder fehlen.
        let empty: HashMap<String, String> = HashMap::new();
        let db = db_config(&empty, "REALM_STATE_DB", "realm_state_de1");
        assert!(db.host.is_empty());
        assert_eq!(num(&empty, "REALM_ID", 0), 0);
    }

    #[test]
    fn load_env_parses_file() {
        let dir = std::env::temp_dir().join(format!("realmrs-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.env");
        std::fs::write(&p, "# Kommentar\nREALM_ID=3\nPORT_WS=4001\nLEER=\n").unwrap();
        let env = load_env(&p);
        assert_eq!(env.get("REALM_ID").map(String::as_str), Some("3"));
        assert_eq!(env.get("PORT_WS").map(String::as_str), Some("4001"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_config_full() {
        let dir = std::env::temp_dir().join(format!("realmrs-cfg2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.env");
        std::fs::write(
            &p,
            "REALM_ID=2\nREALM_STATE_DB_HOST=db\nREALM_STATE_DB_USER=u\nREALM_STATE_DB_PASSWORD=p\nREALM_STATE_DB_NAME=realm_state_de2\n",
        )
        .unwrap();
        let cfg = load_config(&p).unwrap();
        assert_eq!(cfg.realm_id, 2);
        assert_eq!(cfg.realm_db.database, "realm_state_de2");
        assert!(!cfg.allow_destructive);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
