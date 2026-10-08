// migrations — Automatische, versionsbasierte SQL-Migrationen der EIGENEN
// Realm-Datenbank (realm_state_<realm>).
//
// Gleicher Mechanismus wie Auth-Service (Go) und TypeScript-Übergangsstand:
// db_version-Tabelle, NNN_name.sql-Dateien, numerische Reihenfolge, Start-
// abbruch bei Fehlern. Zielarchitektur: NUR realm_state — die character-/
// world_data-Pools des Übergangsstands entfallen hier bewusst (Charakter-
// tabellen liegen in realm_state, siehe docs/Datenbank_Architektur.md).
//
// Regeln wie im Übergangsstand: gültige Namen, eindeutige Nummern,
// lückenlose Folge ab 1, jede eingetragene Version braucht eine passende
// Datei, je Migration eine Transaktion (Statements + db_version-Eintrag),
// USE-Anweisungen werden ignoriert (Verbindung liegt bereits auf der
// konfigurierten DB), `-- destructive:`-Dateien nur mit Freigabe.
use sqlx::{MySql, Pool};

const DB_VERSION_DDL: &str = "CREATE TABLE IF NOT EXISTS db_version (
  version INT NOT NULL PRIMARY KEY,
  migration VARCHAR(255) NOT NULL,
  applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFile {
    pub num: u32,
    pub tag: String,
    pub file: String,
}

/// Zerlegt NNN_name.sql (Split am ERSTEN Unterstrich). Fehler bei
/// ungültigem Namen — nichts wird stillschweigend übersprungen.
pub fn parse_migration_name(file: &str) -> Result<MigrationFile, String> {
    let base = file
        .strip_suffix(".sql")
        .ok_or_else(|| format!("ungültiger Migrationsdateiname {file:?}: erwartet NNN_name.sql"))?;
    let us = base
        .find('_')
        .ok_or_else(|| format!("ungültiger Migrationsdateiname {file:?}: erwartet NNN_name.sql"))?;
    let (num_str, rest) = base.split_at(us);
    let tag = &rest[1..];
    let num: u32 = num_str
        .parse()
        .map_err(|_| format!("ungültiger Migrationsdateiname {file:?}: kein numerisches Präfix"))?;
    if num == 0
        || tag.is_empty()
        || !tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "ungültiger Migrationsdateiname {file:?}: erwartet NNN_name.sql"
        ));
    }
    Ok(MigrationFile {
        num,
        tag: tag.to_string(),
        file: file.to_string(),
    })
}

/// Teilt einen Migrations-Body in Statements (--Kommentare und USE
/// herausgefiltert; Rest ohne ";" am Ende wird mitgenommen).
pub fn split_statements(body: &str) -> Vec<String> {
    let mut stmts = Vec::new();
    let mut cur = String::new();
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("--") {
            continue;
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(t);
        if cur.ends_with(';') {
            cur.pop();
            let stmt = cur.trim().to_string();
            if !stmt.is_empty() && !is_use(&stmt) {
                stmts.push(stmt);
            }
            cur.clear();
        }
    }
    let rest = cur.trim().trim_end_matches(';').trim().to_string();
    if !rest.is_empty() && !is_use(&rest) {
        stmts.push(rest);
    }
    stmts
}

fn is_use(stmt: &str) -> bool {
    let upper = stmt.to_ascii_uppercase();
    upper.starts_with("USE ")
        && upper[4..]
            .trim()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '`' || c == '"' || c == '\'')
}

/// True bei Kopfzeile `-- destructive: <Grund>`.
pub fn is_destructive(body: &str) -> bool {
    body.lines().any(|l| {
        let t = l.trim_start();
        t.len() > 2
            && t[..2].eq_ignore_ascii_case("--")
            && t[2..]
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("destructive:")
    })
}

/// Ermittelt + prüft die Migrationsdateien eines Verzeichnisses
/// (sortiert, eindeutig, lückenlos ab 1).
pub fn collect_files(dir: &std::path::Path) -> Result<Vec<MigrationFile>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        format!(
            "Migrationsverzeichnis nicht lesbar ({}): {e}",
            dir.display()
        )
    })?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Migrationsverzeichnis nicht lesbar: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".sql") {
            files.push(parse_migration_name(&name)?);
        }
    }
    files.sort_by_key(|f| f.num);
    for w in files.windows(2) {
        if w[0].num == w[1].num {
            return Err(format!(
                "doppelte Migrationsnummer {} ({} vs. {})",
                w[0].num, w[0].file, w[1].file
            ));
        }
    }
    for (i, f) in files.iter().enumerate() {
        if f.num != (i as u32) + 1 {
            return Err(format!(
                "Lücke in der Migrationsnummerierung: erwartet {}, gefunden {} ({})",
                i + 1,
                f.num,
                f.file
            ));
        }
    }
    Ok(files)
}

/// Wendet alle fehlenden Migrationen auf genau dieser Datenbank an.
/// Wirft bei jedem Problem (Aufrufer bricht den Start ab).
pub async fn apply_migrations(
    pool: &Pool<MySql>,
    db_name: &str,
    dir: &std::path::Path,
    allow_destructive: bool,
) -> Result<(), String> {
    let files = collect_files(dir)?;
    sqlx::query(DB_VERSION_DDL)
        .execute(pool)
        .await
        .map_err(|e| format!("[migration:realm_state] db_version anlegen ({db_name}): {e}"))?;
    let rows: Vec<(i32, String)> = sqlx::query_as("SELECT version, migration FROM db_version")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("[migration:realm_state] db_version lesen ({db_name}): {e}"))?;
    for (v, tag) in &rows {
        match files.iter().find(|f| f.num as i32 == *v) {
            None => {
                return Err(format!(
                    "[migration:realm_state] db_version enthält Version {v} ({tag}), \
                     aber keine passende Datei liegt in {}",
                    dir.display()
                ))
            }
            Some(f) if f.tag != *tag => {
                return Err(format!(
                    "[migration:realm_state] Version {v} ist als {tag:?} eingetragen, \
                     die Datei heißt {}",
                    f.file
                ))
            }
            _ => {}
        }
    }
    let applied: Vec<u32> = rows.iter().map(|(v, _)| *v as u32).collect();
    let current = applied.iter().copied().max().unwrap_or(0);
    let pending: Vec<_> = files.iter().filter(|f| !applied.contains(&f.num)).collect();
    if pending.is_empty() {
        log::info!(
            "[migration:realm_state] {db_name}: Schema aktuell (Version {current}, {} geprüft).",
            files.len()
        );
        return Ok(());
    }
    log::info!(
        "[migration:realm_state] {db_name}: Version {current}, {} ausstehend ...",
        pending.len()
    );
    for f in pending {
        let body = std::fs::read_to_string(dir.join(&f.file))
            .map_err(|e| format!("[migration:realm_state] {} lesen: {e}", f.file))?;
        if is_destructive(&body) && !allow_destructive {
            return Err(format!(
                "[migration:realm_state] {} ist als destruktiv markiert, aber \
                 ALLOW_DESTRUCTIVE_MIGRATIONS=1 ist nicht gesetzt (nur nach Backup \
                 im Realm-Update-Ablauf freigeben).",
                f.file
            ));
        }
        let stmts = split_statements(&body);
        if stmts.is_empty() {
            return Err(format!(
                "[migration:realm_state] {} enthält keine ausführbaren Statements",
                f.file
            ));
        }
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| format!("[migration:realm_state] Transaktion beginnen: {e}"))?;
        let mut failed: Option<String> = None;
        for (i, stmt) in stmts.iter().enumerate() {
            if let Err(e) = sqlx::query(stmt).execute(&mut *tx).await {
                failed = Some(format!("Statement {}/{}: {e}", i + 1, stmts.len()));
                break;
            }
        }
        if failed.is_none() {
            if let Err(e) = sqlx::query("INSERT INTO db_version (version, migration) VALUES (?, ?)")
                .bind(f.num)
                .bind(&f.tag)
                .execute(&mut *tx)
                .await
            {
                failed = Some(format!("db_version eintragen: {e}"));
            }
        }
        match failed {
            None => {
                tx.commit()
                    .await
                    .map_err(|e| format!("[migration:realm_state] {} commit: {e}", f.file))?;
                log::info!("[migration:realm_state] {db_name}: {} angewendet.", f.file);
            }
            Some(why) => {
                let _ = tx.rollback().await;
                return Err(format!(
                    "[migration:realm_state] {} (Version {}) fehlgeschlagen: {why}. \
                     Start wird abgebrochen (Hinweis: MariaDB-DDL committet implizit).",
                    f.file, f.num
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_first_underscore() {
        let f = parse_migration_name("004_world_servers.sql").unwrap();
        assert_eq!((f.num, f.tag.as_str()), (4, "world_servers"));
        let f = parse_migration_name("013_parental_control.sql").unwrap();
        assert_eq!((f.num, f.tag.as_str()), (13, "parental_control"));
        assert!(parse_migration_name("nope.sql").is_err());
        assert!(parse_migration_name("001.sql").is_err());
        assert!(parse_migration_name("001_.sql").is_err());
    }

    #[test]
    fn split_filters_use_and_comments() {
        let s = split_statements(
            "-- Kommentar\nUSE realm_state;\nCREATE TABLE a (x INT);\n\nCREATE INDEX i ON a(x);\n",
        );
        assert_eq!(s.len(), 2);
        assert!(s[0].starts_with("CREATE TABLE"));
    }

    #[test]
    fn destructive_marker() {
        assert!(is_destructive(
            "-- destructive: Spalte weg\nALTER TABLE t DROP COLUMN x;\n"
        ));
        assert!(is_destructive("SELECT 1;\n--   DESTRUCTIVE: foo\n"));
        assert!(!is_destructive("-- Kommentar\nSELECT 1;\n"));
    }

    fn tmpdir(files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "realmrs-mig-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
        }
        dir
    }

    #[test]
    fn collect_ok_and_sorted() {
        let dir = tmpdir(&[("002_b.sql", "SELECT 1;\n"), ("001_a.sql", "SELECT 1;\n")]);
        let f = collect_files(&dir).unwrap();
        assert_eq!(f.iter().map(|x| x.num).collect::<Vec<_>>(), vec![1, 2]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collect_rejects_gap_and_dup() {
        let dir = tmpdir(&[("001_a.sql", "SELECT 1;\n"), ("003_c.sql", "SELECT 1;\n")]);
        assert!(collect_files(&dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
        let dir = tmpdir(&[("001_a.sql", "SELECT 1;\n"), ("001_b.sql", "SELECT 1;\n")]);
        assert!(collect_files(&dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collect_rejects_missing_dir() {
        assert!(collect_files(std::path::Path::new("/gibt/es/nicht")).is_err());
    }

    /// Das ausgelieferte Migrationsverzeichnis folgt dem Projektmuster:
    /// lückenlos ab 1 nummeriert, einschließlich der
    /// Lifecycle-Metadaten-Migration (021) und der Händler-Migration (022).
    /// Nur Dateiebene — es wird keine Migration ausgeführt und keine
    /// Datenbank berührt.
    #[test]
    fn shipped_migrations_are_gapless_including_lifecycle() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let files = collect_files(&dir).expect("Migrationsverzeichnis lesbar");
        let nums: Vec<u32> = files.iter().map(|f| f.num).collect();
        assert_eq!(
            nums,
            (1..=22).collect::<Vec<_>>(),
            "Migrationen 001–022 lückenlos"
        );
        let last = files.last().expect("mindestens eine Migration");
        assert_eq!((last.num, last.tag.as_str()), (22, "merchant_trading"));
        assert_eq!(last.file, "022_merchant_trading.sql");
    }
}
