// config — Konfiguration aus config.env neben dem Binary (Zielarchitektur:
// genau EINE Realm-Datenbank realm_state_<realm>; keine character- oder
// world_data-Pools wie im TypeScript-Übergangsstand).
// Pfad auch per argv[1] oder REALM_CONFIG. Format: KEY=VALUE, #-Kommentare.
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::PathBuf;

use crate::group::GroupCfg;
use crate::inventory::InventoryCfg;

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
    /// Ein IPv6-Host wird mit Klammern geschrieben:
    /// mysql://u:p@[2001:db8::1]:3306/db.
    pub fn url(&self) -> String {
        format!(
            "mysql://{}:{}@{}:{}/{}",
            self.user,
            self.password,
            bracket_host(&self.host),
            self.port,
            self.database
        )
    }
}

/// Klammer einen IPv6-Host (auch "[::1]" bleibt unverändert).
pub fn bracket_host(host: &str) -> String {
    let h = host.trim();
    if h.is_empty() || h.starts_with('[') || !h.contains(':') {
        return h.to_string();
    }
    format!("[{h}]")
}

/// Klammer einen ungeklammerten IPv6-Literal im Host-Anteil einer URL
/// (Port-zuerst: "http://2001:db8::1:8080/x" -> "http://[2001:db8::1]:8080/x").
/// Bare IPv6-Adressen (ohne Port) bitte geklammert angeben: "ws://[::1]/ws".
pub fn bracket_url_host(raw: &str) -> String {
    let Some(scheme_end) = raw.find("://") else {
        return raw.to_string();
    };
    let rest = &raw[scheme_end + 3..];
    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let auth = &rest[..auth_end];
    if auth.is_empty() || auth.starts_with('[') || !auth.contains(':') {
        return raw.to_string();
    }
    // host:port-Form: letzter Doppelpunkt trennt einen dezimalen Port.
    if let Some(li) = auth.rfind(':') {
        let (host_part, port_part) = auth.split_at(li);
        let port_part = &port_part[1..];
        if !port_part.is_empty() && port_part.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(ip) = host_part
                .trim_matches(|c| c == '[' || c == ']')
                .parse::<IpAddr>()
            {
                if ip.is_ipv6() {
                    return format!(
                        "{}[{host_part}]:{port_part}{}",
                        &raw[..scheme_end + 3],
                        &rest[auth_end..]
                    );
                }
            }
        }
    }
    // Bare IPv6-Adresse.
    if let Ok(ip) = auth.parse::<IpAddr>() {
        if ip.is_ipv6() {
            return format!("{}[{auth}]{}", &raw[..scheme_end + 3], &rest[auth_end..]);
        }
    }
    raw.to_string()
}

/// Bind-Adressen aus einem Bind-Host-Wert (ein oder zwei SocketAddr).
///
///   "" | "auto"   -> 0.0.0.0 (IPv4, bisheriges Verhalten)
///   "ipv4" | "4"  -> 0.0.0.0
///   "ipv6" | "6"  -> [::] (IPv6-only)
///   "dual"|"both" -> 0.0.0.0 + [::] (zwei explizite Listener)
///   IP-Literal    -> diese Adresse (Family des Literals)
///   Hostname      -> erste v4- und erste v6-Adresse aus der Auflösung
pub fn bind_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let h = host.to_lowercase();
    match h.trim() {
        "" | "auto" | "ipv4" | "4" => Ok(vec![SocketAddr::from((
            IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            port,
        ))]),
        "ipv6" | "6" => Ok(vec![SocketAddr::from((
            IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
            port,
        ))]),
        "dual" | "both" => Ok(vec![
            SocketAddr::from((IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), port)),
            SocketAddr::from((IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), port)),
        ]),
        other => {
            let lit = other.trim_matches(|c| c == '[' || c == ']');
            if let Ok(ip) = lit.parse::<IpAddr>() {
                return Ok(vec![SocketAddr::from((ip, port))]);
            }
            resolve_addrs(other.trim(), port)
        }
    }
}

/// Löse einen Hostnamen auf und melde höchstens EINE Adresse pro Family.
fn resolve_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("bind host {host:?} auflösen: {e}"))?;
    let mut v4: Option<SocketAddr> = None;
    let mut v6: Option<SocketAddr> = None;
    for a in addrs {
        match a {
            a if a.is_ipv4() => {
                if v4.is_none() {
                    v4 = Some(a);
                }
            }
            a => {
                if v6.is_none() {
                    v6 = Some(a);
                }
            }
        }
    }
    let mut out = Vec::new();
    if let Some(a) = v4 {
        out.push(a);
    }
    if let Some(a) = v6 {
        out.push(a);
    }
    if out.is_empty() {
        return Err(format!("bind host {host:?} ergab keine Adresse"));
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct AuthApiConfig {
    /// Leer = Auth-Anbindung bewusst deaktiviert (Entwicklung/Testprototyp).
    pub url: String,
    pub service_id: String,
    pub secret: String,
}

/// Vorläufige NPC/Combat-V2-Mechanikwerte (docs/Kampfsystem.md §§18–21).
/// Alle Werte sind per config.env übersteuerbar und werden anhand späterer
/// Praxistests angepasst — keine Architekturwerte.
#[derive(Debug, Clone)]
pub struct NpcCfg {
    /// Sozialer Aggro-Radius: Umstehende Gegner derselben Fraktion steigen
    /// in den Kampf ein, wenn ein Mitglied angegriffen wird (§21).
    pub social_aggro_radius: f64,
    /// Zeit in ms ohne gültigen Kampfbezug (Ziel weg/außer Reichweite),
    /// bevor ein Gegner Evade/Return auslöst (§20).
    pub no_link_ms: u64,
    /// Rücklauf-Geschwindigkeit in m/s für Home-Zone-Return (§20).
    pub return_speed: f64,
    /// Intervall in ms zwischen periodischen Persistenz-Flushes.
    pub persist_interval_ms: u64,
}

/// Vorläufige Combat-V1-Balancingwerte (docs/Kampfsystem.md §§4–7, 17).
/// Alle Werte sind per config.env übersteuerbar und werden anhand späterer
/// Praxistests angepasst — keine Architekturwerte.
#[derive(Debug, Clone)]
pub struct CombatCfg {
    /// Waffen-/Kampfskill-Schlüssel (skills-Tabelle), Level startet bei 1.
    pub weapon_skill_id: String,
    /// Grundschaden der (V1-Platzhalter-)Waffe (docs/Kampfsystem.md §6).
    pub weapon_damage: i32,
    /// Zeit zwischen zwei automatischen Grundangriffen in ms (§3).
    pub weapon_duration_ms: u64,
    /// Angriffsreichweite der Waffe (§9).
    pub weapon_range: f64,
    /// Trefferwahrscheinlichkeiten in Promille (§5, vorläufig).
    pub hit_miss_permille: u32,
    pub hit_dodge_permille: u32,
    pub hit_parry_permille: u32,
    pub hit_block_permille: u32,
    pub hit_crit_permille: u32,
    /// Kritischer Schaden in Prozent (150 = 1,5×).
    pub hit_crit_mult_percent: u32,
    /// Schadensreduktion durch Blocken in Prozent (§5).
    pub hit_block_reduce_percent: u32,
    /// Prozentpunkt Schadensreduktion je Rüstungspunkt (§7, vorläufig).
    pub armor_pct_per_point: u32,
    /// Maximale physische Schadensreduktion je Klassen-Gruppe (§7).
    pub armor_cap_tank: u32,
    pub armor_cap_mage: u32,
    pub armor_cap_default: u32,
    /// Miss-Reduktion in Promille je Skillpunkt über 1 (§4/§5).
    pub skill_hit_bonus_permille: u32,
}

fn num1(env: &HashMap<String, String>, key: &str, def: u64) -> u64 {
    env.get(key)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(def)
}

fn numf(env: &HashMap<String, String>, key: &str, def: f64) -> f64 {
    env.get(key)
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(def)
}

pub fn combat_config(env: &HashMap<String, String>) -> CombatCfg {
    let g = |k: &str| env.get(k).cloned().unwrap_or_default();
    CombatCfg {
        weapon_skill_id: {
            let s = g("COMBAT_WEAPON_SKILL_ID");
            if s.is_empty() {
                "schwerter".to_string()
            } else {
                s
            }
        },
        weapon_damage: num1(env, "COMBAT_WEAPON_DAMAGE", 10) as i32,
        weapon_duration_ms: num1(env, "COMBAT_WEAPON_DURATION_MS", 2000),
        weapon_range: numf(env, "COMBAT_WEAPON_RANGE", 2.0),
        hit_miss_permille: num1(env, "COMBAT_HIT_MISS_PERMILLE", 100) as u32,
        hit_dodge_permille: num1(env, "COMBAT_HIT_DODGE_PERMILLE", 100) as u32,
        hit_parry_permille: num1(env, "COMBAT_HIT_PARRY_PERMILLE", 50) as u32,
        hit_block_permille: num1(env, "COMBAT_HIT_BLOCK_PERMILLE", 100) as u32,
        hit_crit_permille: num1(env, "COMBAT_HIT_CRIT_PERMILLE", 100) as u32,
        hit_crit_mult_percent: num1(env, "COMBAT_HIT_CRIT_MULT_PERCENT", 150) as u32,
        hit_block_reduce_percent: num1(env, "COMBAT_HIT_BLOCK_REDUCE_PERCENT", 50) as u32,
        armor_pct_per_point: num1(env, "COMBAT_ARMOR_PCT_PER_POINT", 2) as u32,
        armor_cap_tank: num1(env, "COMBAT_ARMOR_CAP_TANK", 50) as u32,
        armor_cap_mage: num1(env, "COMBAT_ARMOR_CAP_MAGE", 20) as u32,
        armor_cap_default: num1(env, "COMBAT_ARMOR_CAP_DEFAULT", 30) as u32,
        skill_hit_bonus_permille: num1(env, "COMBAT_SKILL_HIT_BONUS_PERMILLE", 5) as u32,
    }
}

pub fn npc_config(env: &HashMap<String, String>) -> NpcCfg {
    NpcCfg {
        social_aggro_radius: numf(env, "NPC_SOCIAL_AGGO_RADIUS", 15.0),
        no_link_ms: num1(env, "NPC_NO_LINK_MS", 5000),
        return_speed: numf(env, "NPC_RETURN_SPEED", 5.0),
        persist_interval_ms: num1(env, "NPC_PERSIST_INTERVAL_MS", 30000),
    }
}

pub fn group_config(env: &HashMap<String, String>) -> GroupCfg {
    GroupCfg {
        max_members: num1(env, "GROUP_MAX_SIZE", 4) as u32,
        range: numf(env, "GROUP_RANGE", 100.0),
        reconnect_ms: num1(env, "GROUP_RECONNECT_MS", 300_000),
    }
}

pub fn inventory_config(env: &HashMap<String, String>) -> InventoryCfg {
    let base = num1(env, "INVENTORY_BASE_SLOTS", 8) as u16;
    let max_bags = env
        .get("INVENTORY_MAX_EQUIPPED_BAGS")
        .and_then(|v| {
            let t = v.trim().to_lowercase();
            if t.is_empty() || t == "none" || t == "unlimited" {
                None
            } else {
                t.parse::<u16>().ok()
            }
        });
    InventoryCfg {
        base_slots: base,
        max_equipped_bags: max_bags,
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Eigene Realm-ID (prüft Handoff-Bindung: handoff.realm_id muss passen).
    pub realm_id: u32,
    pub ws_port: u16,
    pub health_port: u16,
    /// Bind-Hosts für WebSocket- und Health-Listener (siehe bind_addrs).
    pub ws_bind_host: String,
    pub health_bind_host: String,
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
    /// Vorläufige Combat-V1-Balancingwerte.
    pub combat: CombatCfg,
    /// Vorläufige NPC/Combat-V2-Mechanikwerte.
    pub npc: NpcCfg,
    /// Gruppensystem V1 (docs/Gruppensystem.md §§1–9).
    pub group: GroupCfg,
    /// Inventory System V1 (docs/inventory_system.md).
    pub inventory: InventoryCfg,
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
    env.get(key)
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(def)
}

fn db_config(env: &HashMap<String, String>, prefix: &str, default_db: &str) -> DbConfig {
    let g = |k: &str| {
        env.get(&format!("{prefix}_{k}"))
            .cloned()
            .unwrap_or_default()
    };
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
        return Err(format!(
            "fehlende Realm-Konfiguration: {}",
            missing.join(", ")
        ));
    }
    Ok(Config {
        realm_id,
        ws_port: num(&env, "PORT_WS", 3001) as u16,
        health_port: num(&env, "PORT_HTTP", 3002) as u16,
        ws_bind_host: g("WS_BIND_HOST"),
        health_bind_host: g("HEALTH_BIND_HOST"),
        tick_ms: num(&env, "TICK_MS", 100),
        aofb_radius: num(&env, "AOFB_RADIUS", 20) as f64,
        render_cap: num(&env, "RENDER_CAP_DEFAULT", 64) as u32,
        ollama_url: {
            let u = g("OLLAMA_URL");
            if u.is_empty() {
                "http://192.168.1.32:11434".to_string()
            } else {
                bracket_url_host(&u.trim().to_string())
            }
        },
        auth_api: AuthApiConfig {
            url: bracket_url_host(&g("AUTHAPI_URL").trim_end_matches('/').to_string()),
            service_id: g("AUTHAPI_SERVICE_ID"),
            secret: g("AUTHAPI_SERVICE_SECRET"),
        },
        realm_db,
        migrations_dir: g("REALM_STATE_MIGRATIONS_DIR"),
        allow_destructive: g("ALLOW_DESTRUCTIVE_MIGRATIONS") == "1",
        combat: combat_config(&env),
        npc: npc_config(&env),
        group: group_config(&env),
        inventory: inventory_config(&env),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
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

    #[test]
    fn db_url_brackets_ipv6() {
        let v4 = DbConfig {
            host: "127.0.0.1".into(),
            port: 3306,
            user: "u".into(),
            password: "p".into(),
            database: "realm_state_de1".into(),
        };
        assert_eq!(v4.url(), "mysql://u:p@127.0.0.1:3306/realm_state_de1");
        let v6 = DbConfig {
            host: "2001:db8::1".into(),
            ..v4.clone()
        };
        assert_eq!(v6.url(), "mysql://u:p@[2001:db8::1]:3306/realm_state_de1");
        let v6b = DbConfig {
            host: "[::1]".into(),
            ..v4
        };
        assert_eq!(v6b.url(), "mysql://u:p@[::1]:3306/realm_state_de1");
    }

    #[test]
    fn bracket_host_uniform() {
        assert_eq!(bracket_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(bracket_host("db"), "db");
        assert_eq!(bracket_host("2001:db8::1"), "[2001:db8::1]");
        assert_eq!(bracket_host("[::1]"), "[::1]");
    }

    #[test]
    fn url_host_bracketing() {
        assert_eq!(
            bracket_url_host("http://127.0.0.1:8080/x"),
            "http://127.0.0.1:8080/x"
        );
        assert_eq!(
            bracket_url_host("http://[2001:db8::1]:8080/x"),
            "http://[2001:db8::1]:8080/x"
        );
        assert_eq!(
            bracket_url_host("http://2001:db8::1:8080/x"),
            "http://[2001:db8::1]:8080/x"
        );
        assert_eq!(bracket_url_host("ws://::1:3001/ws"), "ws://[::1]:3001/ws");
        assert_eq!(bracket_url_host("http://::1"), "http://[::1]");
    }

    #[test]
    fn bind_addrs_explicit_families() {
        use std::net::Ipv4Addr;
        let v4 = bind_addrs("", 3001).unwrap();
        assert_eq!(
            v4,
            vec![SocketAddr::from((IpAddr::V4(Ipv4Addr::UNSPECIFIED), 3001))]
        );
        let ipv6 = bind_addrs("ipv6", 3001).unwrap();
        assert_eq!(ipv6.len(), 1);
        assert!(ipv6[0].is_ipv6());
        let dual = bind_addrs("dual", 3001).unwrap();
        assert_eq!(dual.len(), 2, "dual must yield two listeners");
        assert!(dual[0].is_ipv4() && dual[1].is_ipv6());
        let lit = bind_addrs("127.0.0.1", 3001).unwrap();
        assert_eq!(
            lit,
            vec![SocketAddr::from((
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                3001
            ))]
        );
        let v6lit = bind_addrs("::1", 3001).unwrap();
        assert_eq!(v6lit.len(), 1);
        assert!(v6lit[0].is_ipv6());
        let v6litb = bind_addrs("[::1]", 3001).unwrap();
        assert_eq!(v6lit, v6litb);
    }
}
