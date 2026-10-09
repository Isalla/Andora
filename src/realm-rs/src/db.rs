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

// ── Fehlerklassifizierung für DB-Logs (Audit 4.5, B3) ───────────────────
//
// Grund: die Persistenzpfade in dieser Datei gaben bisher den ROHEN
// sqlx-Fehlertext aus (`{e}`). Derselbe Roh-Treibertext, der hier bewusst
// nicht erscheint, ist in den Fehlertexten anderer Komponenten bereits als
// Grund für die Klassifizierung dokumentiert: er kann Verbindungs- oder
// Zugangsdaten enthalten (siehe `net.rs`, `LogoutErrorClass`). Die
// Klassifizierung ist damit KEINE neue Regel, sondern die konsequente
// Anwendung der bestehenden auf diesen Dateibereich.
//
// Es werden ausschließlich stabile, neutrale Klassen ausgegeben: **kein**
// Treibertext, **keine** DSN, **kein** SQL, **keine** Parameter.
//
// Die Zuordnung ist bewusst KONSERVATIV: nur Fehlerklassen, die sich ohne
// Treibertext sicher unterscheiden lassen. Alles andere fällt in
// `DbErrorClass::Other`. Es wird keine Detaildiagnose erfunden und keine
// Fehlerbehandlung geändert — Rückgabewerte, Fehlerweitergabe, Retry und
// alle DB-Operationen bleiben unverändert.

/// Stabile Fehlerklasse einer DB-Operation im Log.
///
/// `Begin` und `Commit` bezeichnen Fehler, deren Klasse sich allein aus der
/// Operation ergibt; sie werden in `save_idia` gesetzt. `Connect` ist derzeit
/// ohne Produktionsaufrufer (`connect_pool` reicht seinen Fehler als
/// `Err(String)` nach oben und loggt nicht selbst), gehört aber zur
/// geschlossenen Klassenliste und wird deshalb nicht entfernt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub(crate) enum DbErrorClass {
    /// Der Verbindungsaufbau bzw. das Pool-Holen ist gescheitert.
    Connect,
    /// Die Transaktion konnte nicht gestartet werden.
    Begin,
    /// Das Commit ist fehlgeschlagen (Transaktion zurückgerollt).
    Commit,
    /// Ein Zeitlimit der Verbindung oder des Pools wurde überschritten.
    Timeout,
    /// Der Treiber meldete einen Fehler ohne näher benannten Grund.
    Driver,
    /// Sonstiger, nicht näher klassifizierter Fehler.
    Other,
}

impl DbErrorClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Begin => "begin",
            Self::Commit => "commit",
            Self::Timeout => "timeout",
            Self::Driver => "driver",
            Self::Other => "other",
        }
    }
}

/// Bildet einen Fehler auf eine stabile Klasse ab. Nimmt bewusst **keinen**
/// Fehlertext entgegen und gibt **niemals** Rohinhalt zurück.
pub(crate) fn db_error_class(e: &sqlx::Error) -> DbErrorClass {
    match e {
        sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) => DbErrorClass::Timeout,
        sqlx::Error::Database(_) | sqlx::Error::AnyDriverError(_) => DbErrorClass::Driver,
        _ => DbErrorClass::Other,
    }
}

/// Zeile eines fehlgeschlagenen DB-Schreibvorgangs.
///
/// Enthält ausschließlich die Operation, die vorhandene zulässige Kennung
/// und die Fehlerklasse. Bewusst **nicht** enthalten: Treibertext, DSN, SQL,
/// Parameter, Session-ID, Token und Roh-IP.
///
/// `char_id`/`spawn_id`/`item_id` sind serverseitige, bereits kanonisch
/// validierte Kennungen; sie laufen zur Sicherheit durch `reject_field`, damit
/// auch hier keine Zeile entsteht oder ein Feld vorgetäuscht wird.
pub(crate) fn db_error_line(operation: &str, id: &str, class: DbErrorClass) -> String {
    format!(
        "db_error operation={} id={} error_class={}",
        operation,
        crate::security::reject_field(id),
        class.as_str(),
    )
}

#[derive(Debug, Clone)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub level: u32,
    pub exp: i64,
    /// Freie Attributpunkte (docs/Erfahrung_und_Progressionssystem.md §5).
    pub free_attr_points: u32,
    /// Rested-EXP-Pool (docs/Erfahrung_und_Progressionssystem.md §12).
    pub rested_pool: i64,
    /// Epoch-Sekunden des letzten Ausloggens (None = kein Zeitstempel).
    pub logout_at: Option<i64>,
    /// Kanonische Spielerwährung (docs/Player_Persistenz.md §23) — Spalte
    /// `idia`; Content-Loot-Typ 'gold' erhöht diese Währung.
    pub idia: i64,
    /// Persistenz-Revision (docs/Player_Persistenz.md §29): Revisionsnummer
    /// des zuletzt angewendeten Player-Snapshots (Rename-/Superseded-/Quarantäne-
    /// Logik des Spools; Stufe B).
    pub persist_revision: i64,
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

/// DB-Zeile für `load_character` (26 Spalten; sqlx-Tupel-Limit ist 16,
/// daher strukturbasierte Zeile wie `NpcStateRow`).
#[derive(Debug, Clone)]
struct CharacterRow {
    id: i64,
    name: String,
    level: i32,
    exp: i64,
    free_attr_points: i32,
    rested_pool: i64,
    logout_at: Option<i64>,
    idia: i64,
    persist_revision: i64,
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
            free_attr_points: row.try_get("free_attr_points")?,
            rested_pool: row.try_get("rested_pool")?,
            logout_at: row.try_get("logout_at")?,
            idia: row.try_get("idia")?,
            persist_revision: row.try_get("persist_revision")?,
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

/// Kanonische serverseitige Charakter-ID.
///
/// `char_id` ist laut `docs/Login_Realm_Architektur.md` (Abschnitt 6) eine
/// **serverseitig vergebene positive Datenbank-ID**; das Schema legt sie als
/// `characters.id INT AUTO_INCREMENT PRIMARY KEY` fest (MariaDB `INT`, 32 Bit,
/// signed → 1..=2147483647).
///
/// Akzeptiert wird ausschließlich die kanonische Dezimaldarstellung: nur
/// ASCII-Ziffern, keine Vorzeichen, keine führenden Nullen, kein Whitespace und
/// keine weiteren Zeichen. Dadurch bilden Aliase derselben ID („1", „01",
/// "+1", „ 1") **keine** unterschiedlichen Gate-, World- oder DB-Lookup-Schlüssel:
/// Nicht-kanonische Eingaben werden abgelehnt, kanonische Eingaben liefern
/// genau einen Schlüssel (`value.to_string()`).
///
/// `Err` enthält nur einen statischen Grund ohne die rohe Eingabe, damit die
/// Meldung keine untrusted Daten (Whitespace, Steuertags, Steuerzeichen)
/// weiterreicht.
pub fn parse_character_id(raw: &str) -> Result<i32, &'static str> {
    if raw.is_empty() {
        return Err("empty");
    }
    if !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err("not-canonical");
    }
    if raw.len() > 1 && raw.starts_with('0') {
        return Err("not-canonical");
    }
    let value: i32 = raw.parse().map_err(|_| "out-of-range")?;
    if value <= 0 {
        return Err("not-positive");
    }
    Ok(value)
}

/// Lädt einen Charakter **ausschließlich lesend** und ausschließlich bei
/// nachgewiesener Ownership: `char_id` muss der mit dem Handoff
/// authentifizierten `account_id` gehören (`characters.account_id`).
///
/// * Treffer: der vorhandene Character wird zurückgegeben.
/// * Kein Treffer: `Ok(None)` — der Aufrufer lehnt fail-closed ab.
/// * Datenbankfehler: `Err` — vom Aufrufer getrennt davon fail-closed behandelt.
///
/// Die Funktion führt **keinen Schreibzugriff** aus. Ein Charakter wird nicht
/// angelegt: Character-Erstellung ist ein separater, authentifizierter Ablauf
/// und nicht Teil des Realm-Einstiegs (`docs/Login_Realm_Architektur.md`
/// Abschnitte 6 und 16, `docs/Charaktererstellung_und_Charakterdarstellung.md`).
pub async fn load_character(
    pool: &Pool<MySql>,
    account_id: u32,
    char_id: i32,
) -> Result<Option<Character>, String> {
    let row: Option<CharacterRow> = sqlx::query_as::<_, CharacterRow>(
        "SELECT id, name, level, exp, free_attr_points, rested_pool, logout_at, idia, persist_revision, hp, char_class, faction_transition, pos_x, pos_y, combat_armor, \
         mana, mana_max, race, strength, agility, intelligence, constitution, wisdom, luck, \
         endurance FROM characters WHERE id = ? AND account_id = ?",
    )
    .bind(char_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Charakter laden: {e}"))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let class = crate::class::ClassStatus::from_db_name(&row.char_class);
    Ok(Some(Character {
        id: row.id.to_string(),
        name: row.name,
        x: row.x,
        y: row.y,
        level: row.level.max(0) as u32,
        exp: row.exp.max(0),
        free_attr_points: row.free_attr_points.max(0) as u32,
        rested_pool: row.rested_pool.max(0),
        logout_at: row.logout_at,
        idia: row.idia.max(0),
        persist_revision: row.persist_revision,
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
    }))
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
        log::error!(
            "{}",
            db_error_line("save_position", char_id, db_error_class(&e))
        );
    }
}

/// Interne Transaktionshilfe: Position in eine laufende sqlx-/MariaDB-
/// Transaktion schreiben. Spiegel-Baustein zu `write_progression`/
/// `write_idia`/`write_inventory`/`write_quest_state`, damit der zentrale
/// Player-Persistenzpfad (docs/Player_Persistenz.md §15) die Position
/// zusammen mit den anderen dirty Komponenten in EINEM Transaktionskontext
/// schreiben kann. Der bisherige Einzelaufrufer `save_position` bleibt
/// unverändert bestehen.
pub(crate) async fn write_position(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    x: f64,
    y: f64,
) -> Result<(), String> {
    sqlx::query("UPDATE characters SET pos_x = ?, pos_y = ? WHERE id = ?")
        .bind(x)
        .bind(y)
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("savePosition {char_id}: {e}"))?;
    Ok(())
}

/// Interne Transaktionshilfe: Progressionsstand in eine laufende sqlx-/
/// MariaDB-Transaktion schreiben (docs/Erfahrung_und_Progressionssystem.md
/// §§4/7/12). Wird vom bisherigen Einzel-Save (`save_progression`) und vom
/// atomaren Questabschluss genutzt (Quest V1.2a.2, docs/Quest-System.md
/// §27.26 „atomare/transaktionale Sicherheitsgrenze").
#[allow(clippy::too_many_arguments)]
pub(crate) async fn write_progression(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    level: u32,
    exp: i64,
    free_attr_points: u32,
    rested_pool: i64,
    logout_at: Option<i64>,
) -> Result<(), String> {
    write_progression_fields(tx, char_id, level, exp, free_attr_points, rested_pool).await?;
    sqlx::query("UPDATE characters SET logout_at = ? WHERE id = ?")
        .bind(logout_at)
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("saveProgression {char_id} (logout_at): {e}"))?;
    Ok(())
}

/// Interne Transaktionshilfe: Progressionsstand OHNE `logout_at` in eine
/// laufende Transaktion schreiben (level/exp/free_attr_points/rested_pool).
/// Der Spool-Drain der Stufe B nutzt diesen Baustein statt `write_progression`,
/// damit angewendete Snapshots den (ausschließlich vom finalen Disconnect-
/// Save gesetzten) Logout-Zeitpunkt NIE überschreiben (docs/Player_Persistenz.md
/// §23/§30: logout_at gehört nicht in den Snapshot).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn write_progression_fields(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    level: u32,
    exp: i64,
    free_attr_points: u32,
    rested_pool: i64,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE characters SET level = ?, exp = ?, free_attr_points = ?, rested_pool = ? \
         WHERE id = ?",
    )
    .bind(level as i32)
    .bind(exp)
    .bind(free_attr_points as i32)
    .bind(rested_pool)
    .bind(char_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("saveProgression {char_id}: {e}"))?;
    Ok(())
}

/// Speichert den kompletten Progressionsstand (docs/Erfahrung_und_
/// Progressionssystem.md §§4/7/12): Level, EXP, freie Attributpunkte,
/// Rested-Pool und optional den Logout-Zeitpunkt (Epoch-Sekunden).
/// `logout_at=None` setzt die Spalte auf NULL (nach der Rested-Berechnung
/// beim Login).
///
/// **Atomar:** Rested-Pool und `logout_at` werden in EINER Transaktion
/// geschrieben (docs/Login_Realm_Architektur.md, „Offline-Abrechnung
/// unmittelbar vor dem Commit"). Schlägt ein Teilschritt fehl, wird die
/// Transaktion verworfen — es entsteht **kein** Teilwrite.
///
/// **Zielzeile:** Vor dem ersten Write wird die Charakterzeile innerhalb
/// derselben Transaktion per `SELECT … FOR UPDATE` validiert und gesperrt.
/// Das unterscheidet „Zeile fehlt" eindeutig von „Wert war bereits gleich":
/// eine vorhandene Zeile liefert genau einen Datensatz, unabhängig davon, ob
/// die folgenden UPDATEs etwas ändern — anders als `rows_affected`, dessen
/// Bedeutung vom Treiber-Capability `FOUND_ROWS` abhinge. Die InnoDB-Sperre
/// (MariaDB-Standard; die Migration setzt keine Engine explizit) verhindert
/// zudem, dass die Zeile zwischen Prüfung und Write verschwindet.
/// Das Prädikat enthält `account_id`: eine Zeile, die einem anderen Account
/// gehört, gilt als nicht vorhanden — die Ownership-Bindung bleibt damit auch
/// zum Abrechnungszeitpunkt erhalten.
///
/// Fehler werden an den Aufrufer zurückgegeben: der Login-Pfad entscheidet
/// fail-closed, ob der Einstieg committet wird; ein stillschweigend
/// verschluckter Fehler würde einen RAM-Stand fortschreiben, den die DB nicht
/// kennt.
#[allow(clippy::too_many_arguments)]
pub async fn save_progression(
    pool: &Pool<MySql>,
    char_id: &str,
    account_id: u32,
    level: u32,
    exp: i64,
    free_attr_points: u32,
    rested_pool: i64,
    logout_at: Option<i64>,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("saveProgression {char_id} (Transaktion beginnen): {e}"))?;
    let owner: Option<i32> = sqlx::query_scalar(
        "SELECT account_id FROM characters WHERE id = ? AND account_id = ? FOR UPDATE",
    )
    .bind(char_id)
    .bind(account_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("saveProgression {char_id} (Zielzeile prüfen): {e}"))?;
    if owner.is_none() {
        // Kein erfolgreicher Settlement-Rückgabewert: `tx` wird ohne Commit
        // verworfen, es gab keinen vorherigen Write in dieser Transaktion.
        return Err(format!(
            "saveProgression {char_id}: Zielzeile fehlt oder gehört nicht zu diesem Account"
        ));
    }
    write_progression(
        &mut tx,
        char_id,
        level,
        exp,
        free_attr_points,
        rested_pool,
        logout_at,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("saveProgression {char_id} (Commit): {e}"))
}

/// Schreibt `logout_at` direkt (finaler Disconnect-Save, docs/
/// Player_Persistenz.md §23): Der Logout-Zeitpunkt ist NICHT Teil des
/// vollständigen Snapshots/Drains (sonst würde ein später angewendeter
/// Batch ihn überschreiben). Fehler nur loggen (wie save_position).
pub(crate) async fn write_logout_at(
    pool: &Pool<MySql>,
    char_id: &str,
    logout_at: i64,
) -> Result<(), String> {
    sqlx::query("UPDATE characters SET logout_at = ? WHERE id = ?")
        .bind(logout_at)
        .bind(char_id)
        .execute(pool)
        .await
        .map_err(|e| format!("saveLogoutAt {char_id}: {e}"))?;
    Ok(())
}

/// Interne Transaktionshilfe: Geldstand (idia) in eine laufende Transaktion
/// schreiben. Wird vom bisherigen Einzel-Save (`save_idia`) und vom
/// Spool-Drain der Stufe B (docs/Player_Persistenz.md §30) genutzt.
pub(crate) async fn write_idia(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    idia: i64,
) -> Result<(), String> {
    sqlx::query("UPDATE characters SET idia = ? WHERE id = ?")
        .bind(idia)
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("saveIdia {char_id}: {e}"))?;
    Ok(())
}

/// Speichert den Geldstand (idia; Fehler nur loggen — kein Crash).
pub async fn save_idia(pool: &Pool<MySql>, char_id: &str, idia: i64) {
    // Die Fehlerklasse von `begin` und `commit` ergibt sich aus der
    // Operation selbst; der konkrete Fehlerwert wird bewusst nicht
    // ausgewertet, weil daraus kein stabiler, treibertextfreier Zusatz
    // zu gewinnen wäre.
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            log::error!(
                "{}",
                db_error_line("save_idia_begin", char_id, DbErrorClass::Begin)
            );
            return;
        }
    };
    if write_idia(&mut tx, char_id, idia).await.is_err() {
        // `write_idia` liefert bewusst `Result<(), String>` (kein
        // `sqlx::Error`), weil der Spool-Drain sie mitnutzt. Der String wird
        // hier nicht ausgegeben; die Klasse folgt aus der Operation.
        log::error!(
            "{}",
            db_error_line("save_idia_write", char_id, DbErrorClass::Driver)
        );
        return;
    }
    if tx.commit().await.is_err() {
        log::error!(
            "{}",
            db_error_line("save_idia_commit", char_id, DbErrorClass::Commit)
        );
    }
}

/// Interne Transaktionshilfe: aktuelle HP/Mana in eine laufende Transaktion
/// schreiben (docs/Player_Persistenz.md §23, Stufe B Full Snapshot).
pub(crate) async fn write_resources(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    hp: i32,
    mana: i32,
) -> Result<(), String> {
    sqlx::query("UPDATE characters SET hp = ?, mana = ? WHERE id = ?")
        .bind(hp)
        .bind(mana)
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("saveResources {char_id}: {e}"))?;
    Ok(())
}

/// Interne Transaktionshilfe: die sieben Grundattribute in eine laufende
/// Transaktion schreiben (docs/Attribute_und_Regeneration.md §11).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn write_attributes(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    attrs: &crate::attributes::Attributes,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE characters SET strength = ?, agility = ?, intelligence = ?, \
         constitution = ?, wisdom = ?, luck = ?, endurance = ? WHERE id = ?",
    )
    .bind(attrs.strength)
    .bind(attrs.dexterity)
    .bind(attrs.intelligence)
    .bind(attrs.constitution)
    .bind(attrs.wisdom)
    .bind(attrs.luck)
    .bind(attrs.endurance)
    .bind(char_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("saveAttributes {char_id}: {e}"))?;
    Ok(())
}

/// Interne Transaktionshilfe: permanente Klassenwahl + Fraktions-Übergang
/// in eine laufende Transaktion schreiben (docs/Klassensystem.md).
pub(crate) async fn write_character_class(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    class: crate::class::ClassStatus,
    faction_transition: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE characters SET char_class = ?, faction_transition = ? WHERE id = ?")
        .bind(class.canonical_db_name())
        .bind(faction_transition)
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("saveCharacterClass {char_id}: {e}"))?;
    Ok(())
}

/// Interne Transaktionshilfe: Level des Waffen-/Kampfskills (skills-Tabelle,
/// docs/Kampfsystem.md §4) in eine laufende Transaktion schreiben.
pub(crate) async fn write_weapon_skill(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    weapon_skill_id: &str,
    lvl: u32,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO skills (char_id, skill_id, lvl) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE lvl = VALUES(lvl)",
    )
    .bind(char_id)
    .bind(weapon_skill_id)
    .bind(lvl as i32)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("saveWeaponSkill {char_id}: {e}"))?;
    Ok(())
}

/// Interne Transaktionshilfe: gelernte Fähigkeiten als VOLLERSATZ in eine
/// laufende Transaktion schreiben (character_abilities, docs/Ability-System.md
/// §9). Delete + Insert in derselben Transaktion — abwesende Fähigkeiten
/// gelten als nicht gelernt.
pub(crate) async fn write_character_abilities(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    abilities: &[String],
) -> Result<(), String> {
    sqlx::query("DELETE FROM character_abilities WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("saveAbilities {char_id} (delete): {e}"))?;
    for ability_id in abilities {
        sqlx::query("INSERT INTO character_abilities (char_id, ability_id) VALUES (?, ?)")
            .bind(char_id)
            .bind(ability_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("saveAbilities {char_id} (insert {ability_id}): {e}"))?;
    }
    Ok(())
}

/// `P-18`: Persistierte Ablaufzeitpunkte der laufenden Ability-Cooldowns in
/// einer laufenden Transaktion **vollständig ersetzen** (docs §23).
///
/// Modell wie `write_character_abilities`: der Bestand des Charakters wird
/// gelöscht und die Map neu geschrieben. Eine bewusst leere Map entfernt
/// damit zuvor gespeicherte Cooldowns, sodass sie beim nächsten Laden nicht
/// wieder erscheinen. Fehler bleiben hart: der Drain bricht ab, das alte
/// Batch bleibt liegen.
pub(crate) async fn write_character_cooldowns(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    cooldowns: &std::collections::BTreeMap<String, i64>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM character_cooldowns WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("saveCooldowns {char_id} (delete): {e}"))?;
    for (ability_id, ready_at_ms) in cooldowns {
        sqlx::query(
            "INSERT INTO character_cooldowns (char_id, ability_id, ready_at_ms) VALUES (?, ?, ?)",
        )
        .bind(char_id)
        .bind(ability_id)
        .bind(*ready_at_ms)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("saveCooldowns {char_id} (insert {ability_id}): {e}"))?;
    }
    Ok(())
}

/// Interne Transaktionshilfe: persistierte Persistenz-Revision eines
/// Charakters in einer laufenden Transaktion setzen (docs §29).
pub(crate) async fn write_persist_revision(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    revision: i64,
) -> Result<(), String> {
    sqlx::query("UPDATE characters SET persist_revision = ? WHERE id = ?")
        .bind(revision)
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("writePersistRevision {char_id}: {e}"))?;
    Ok(())
}

/// Pair revisions are read and locked inside the SAME transaction as the
/// transfer. A pool-level pre-read is not a trade commit guard.
pub(crate) async fn lock_persist_revision(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
) -> Result<Option<i64>, String> {
    sqlx::query_scalar("SELECT persist_revision FROM characters WHERE id = ? FOR UPDATE")
        .bind(char_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("trade revision lock: {e}"))
}

pub(crate) async fn load_trade_commit_proof(
    tx: &mut sqlx::Transaction<'_, MySql>,
    commit_id: &str,
) -> Result<Option<String>, String> {
    sqlx::query_scalar(
        "SELECT artifact_json FROM character_trade_commits WHERE commit_id = ? FOR UPDATE",
    )
    .bind(commit_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("trade proof read: {e}"))
}

pub(crate) async fn write_trade_commit_proof(
    tx: &mut sqlx::Transaction<'_, MySql>,
    trade: &crate::persist::TradeArtifact,
) -> Result<(), String> {
    // INSERT only, never overwrite an existing identity with a different trade.
    sqlx::query("INSERT INTO character_trade_commits (commit_id, artifact_json) VALUES (?, ?)")
        .bind(&trade.commit_id)
        .bind(trade.commit_payload()?)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("trade proof write: {e}"))?;
    Ok(())
}

/// Global placement validation includes buffer and all characters. Newly
/// acquired unsaved RAM items may have no DB placement yet; existing placements
/// must belong exactly to the witnessed source, never to a third character.
pub(crate) async fn validate_trade_placements(
    tx: &mut sqlx::Transaction<'_, MySql>,
    trade: &crate::persist::TradeArtifact,
) -> Result<bool, String> {
    let mut before = std::collections::BTreeMap::new();
    for c in &trade.characters {
        for it in crate::persist::persistent_instances(&c.before_inventory) {
            if before
                .insert(it.item_uuid.clone(), c.snapshot.player_id.clone())
                .is_some()
            {
                return Ok(false);
            }
        }
    }
    let mut uuids: std::collections::BTreeSet<String> = before.keys().cloned().collect();
    for c in &trade.characters {
        uuids.extend(c.snapshot.inventory.persistent_uuids());
    }
    for t in &trade.transfers {
        uuids.insert(t.moved.item_uuid.clone());
    }
    for uuid in &uuids {
        let mut refs = Vec::new();
        for table in [
            "character_inventory",
            "bag_slots",
            "character_equipment",
            "inventory_buffer",
        ] {
            let rows: Vec<i32> = sqlx::query_scalar(&format!(
                "SELECT char_id FROM {table} WHERE item_uuid = ? FOR UPDATE"
            ))
            .bind(uuid)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| format!("trade placements: {e}"))?;
            for owner in rows {
                refs.push((owner.to_string(), table == "inventory_buffer"));
            }
        }
        if !trade_placement_refs_valid(before.get(uuid).map(String::as_str), &refs) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn trade_placement_refs_valid(expected: Option<&str>, refs: &[(String, bool)]) -> bool {
    refs.len() <= 1
        && refs
            .iter()
            .all(|(owner, buffer)| !buffer && Some(owner.as_str()) == expected)
}

/// Ownership transfer is NOT Sold. Remove old obligations for continuing UUIDs
/// globally, including obligations retained by a previous owner. A genuinely
/// retired UUID remains governed by the recipient's Merged obligation.
pub(crate) async fn clear_transferred_lifecycle(
    tx: &mut sqlx::Transaction<'_, MySql>,
    trade: &crate::persist::TradeArtifact,
) -> Result<(), String> {
    for t in &trade.transfers {
        sqlx::query("DELETE FROM item_instance_finalizations WHERE item_uuid = ?")
            .bind(&t.moved.item_uuid)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("trade lifecycle transfer: {e}"))?;
    }
    Ok(())
}

/// Persistierte Persistenz-Revision eines Charakters lesen
/// (docs/Player_Persistenz.md §29). `None` = kein Charakter-Datensatz
/// (der Drain quarantäniert solche Batches, statt den Drain zu blockieren).
pub(crate) async fn load_persist_revision(
    pool: &Pool<MySql>,
    char_id: &str,
) -> Result<Option<i64>, String> {
    sqlx::query_scalar("SELECT persist_revision FROM characters WHERE id = ?")
        .bind(char_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("persist_revision lesen {char_id}: {e}"))
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
    /// Gegnerlevel der Definition (docs/Erfahrung_und_Progressionssystem.md
    /// §7: Leveldifferenz-Multiplikator beim Kill).
    pub level: u32,
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
    level: i32,
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
            level: row.try_get("level")?,
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
         respawn_ms, faction, exp_reward, level, loot_table_id FROM monster_definitions",
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
            level: r.level.max(0) as u32,
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
        // `spawn_id` ist numerisch (i64); die Identität wird als Feld
        // ausgegeben, nicht durch die String-Form des Rohfehlers.
        log::error!(
            "{}",
            db_error_line("save_npc_state", &spawn_id.to_string(), db_error_class(&e))
        );
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

/// `P-18`: Persistierte Ablaufzeitpunkte der laufenden Ability-Cooldowns eines
/// Charakters laden (`ability_id` → `SystemTime`).
///
/// Die Umrechnung der persistierten Einheit (Epoch-Millisekunden) erfolgt hier
/// über `crate::combat::cooldowns::from_epoch_ms`; ein nicht darstellbarer
/// oder veralteter Zeitpunkt ergibt „bereits abgelaufen" und sperrt **keine**
/// Fähigkeit dauerhaft.
///
/// Der Fehler wird **nicht** verschluckt: der Login-Pfad behandelt ihn
/// fail-closed, weil ein stilles Behandeln als „keine Cooldowns" laufende
/// Cooldowns zurücksetzen würde.
pub async fn load_character_cooldowns(
    pool: &Pool<MySql>,
    char_id: &str,
) -> Result<std::collections::BTreeMap<String, std::time::SystemTime>, String> {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT ability_id, ready_at_ms FROM character_cooldowns WHERE char_id = ?")
            .bind(char_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Charakter-Cooldowns laden: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|(id, ready_at_ms)| (id, crate::combat::cooldowns::from_epoch_ms(ready_at_ms)))
        .collect())
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
    if let Err(e) =
        sqlx::query("UPDATE characters SET char_class = ?, faction_transition = ? WHERE id = ?")
            .bind(class.canonical_db_name())
            .bind(faction_transition)
            .bind(char_id)
            .execute(pool)
            .await
    {
        log::error!(
            "{}",
            db_error_line("save_character_class", char_id, db_error_class(&e))
        );
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
pub async fn load_item_definitions(
    pool: &Pool<MySql>,
) -> Result<Vec<crate::item::ItemDefinition>, String> {
    use crate::item::{BindingRule, ItemCategory, ItemDefinition, Rarity};

    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            String,
            String,
            i32,
            f64,
            i64,
            f64,
            Option<f64>,
            Option<i64>,
            Option<f64>,
            Option<String>,
            Option<f64>,
            Option<i32>,
            String,
        ),
    >(
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
        let attrs: Vec<(String, f64)> = sqlx::query_as(
            "SELECT attribute, bonus FROM item_definition_attributes WHERE item_id = ?",
        )
        .bind(&id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Item-Attribute laden ({id}): {e}"))?;
        let resists: Vec<(String, f64)> = sqlx::query_as(
            "SELECT resistance, bonus FROM item_definition_resistances WHERE item_id = ?",
        )
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
        if def.validate().is_err() {
            // Der Validierungsfehler wird NICHT ausgegeben: `ItemError`
            // sammelt freie Fehlertexte aus dem Content-Bestand. Es bleibt
            // die stabile Klasse.
            log::error!(
                "{}",
                db_error_line("load_item_definitions", &def.item_id, DbErrorClass::Other)
            );
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
            // `kind` ist der ROHE Datenbank-Freitext aus `loot_entries.kind`
            // und damit clientgleich kontrollierbar durch den Content-Bestand.
            // Er wird über die bereits vorhandene sichere Feldformatierung
            // `reject_field` ausgegeben (einzeilig, escaped, auf 64 Zeichen
            // gekürzt); es gibt bewusst KEINE weitere Sanitizer-Implementierung
            // in dieser Datei. Diagnosezweck und die numerischen Kennungen `id`
            // bleiben erhalten, ebenso die Validierung und der `continue`.
            log::error!(
                "Loot-Eintrag {id}: unbekannter kind '{}'; übersprungen",
                crate::security::reject_field(kind.as_str())
            );
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
async fn load_instance_opt(pool: &Pool<MySql>, uuid: &str) -> Option<crate::item::ItemInstance> {
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
    let rows: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT slot, item_uuid FROM character_inventory WHERE char_id = ? ORDER BY slot",
    )
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
    let bag_rows: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT bag_id, name, slot_count FROM character_bags WHERE char_id = ? ORDER BY bag_id",
    )
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
    let slot_rows: Vec<(i64, i64, Option<String>)> = sqlx::query_as(
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
    let buf_rows: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT slot, item_uuid FROM inventory_buffer WHERE char_id = ? ORDER BY slot",
    )
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

/// Interne Transaktionshilfe: voller Inventar-Vollwrite in eine laufende
/// Transaktion (docs/inventory_system.md §11). Alle aktuell vorhandenen
/// item_instances werden upsertet, danach alle Platzierungs-Tabellen
/// (Inventar, Rucksäcke, Equipment) komplett neu befüllt. Der Sicherheits-
/// Puffer wird NICHT persistiert (temporär, §11). Wird von `save_inventory`
/// und vom atomaren Questabschluss genutzt (Quest V1.2a.2, docs/Quest-System.
/// md §27.26).
pub(crate) async fn write_inventory(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    state: &crate::inventory::InventoryState,
) -> Result<(), String> {
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
            write_item_instance(&mut *tx, inst).await?;
        }
    }

    // --- 2. Platzierungen komplett neu schreiben (Transaktions-Vollwrite) ---
    sqlx::query("DELETE FROM character_inventory WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("Altes Inventar löschen: {e}"))?;
    sqlx::query("DELETE FROM bag_slots WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("Alte Bag-Slots löschen: {e}"))?;
    sqlx::query("DELETE FROM character_bags WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("Alte Rucksäcke löschen: {e}"))?;
    sqlx::query("DELETE FROM character_equipment WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut **tx)
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
            .execute(&mut **tx)
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
        .execute(&mut **tx)
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
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("Bag-Slot speichern: {e}"))?;
            }
        }
    }
    // Equipment-Slots.
    for (slot, inst) in &state.equipped {
        sqlx::query("INSERT INTO character_equipment (char_id, slot, item_uuid) VALUES (?, ?, ?)")
            .bind(char_id)
            .bind(slot.as_db())
            .bind(&inst.item_uuid)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("Equipment speichern: {e}"))?;
    }
    // Puffer: in V1 nicht persistiert (temporär; §11). Zeilen werden
    // beim Logout gelöscht (wipe_logout_buffer) und niemals geschrieben.
    Ok(())
}

/// Volle Inventar-Persistenz als eigener Transaktions-Vollwrite (bisheriger
/// Einzel-Save, docs/inventory_system.md §11): alle item_instances upserten,
/// danach alle Platzierungs-Tabellen komplett neu befüllen. Der Sicherheits-
/// Puffer wird NICHT persistiert (temporär, §11). Delegiert an
/// `write_inventory` innerhalb einer eigenen Transaktion.
pub async fn save_inventory(
    pool: &Pool<MySql>,
    char_id: &str,
    state: &crate::inventory::InventoryState,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Inventar-Transaktion beginnen: {e}"))?;
    write_inventory(&mut tx, char_id, state).await?;
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

// ===== Item-Instanz-Lifecycle (Migration 021, docs/inventory_system.md §18) =====
// Revisionsgebundene Instanzfinalisierung im Single-Process-Realm: Der Drain
// schreibt die Lifecycle-Metadaten eines Snapshots vollständig neu und löscht
// zulässige Instanzzeilen — in derselben Transaktion wie Inventar, Idia und
// `persist_revision` (docs/Player_Persistenz.md §30). Referenzierte oder
// widersprüchlich zugeordnete Instanzen werden NIE gelöscht; vorhandene
// Referenzorte (alle Platzierungs-/Pufferzeilen, auch fremder Charaktere)
// bleiben erhalten.

/// Interne Transaktionshilfe: Lifecycle-Metadaten eines Charakters vollständig
/// neu schreiben (Vollschreib wie die Platzierungstabellen). Nur UUIDs, die
/// im Snapshot NICHT mehr platziert sind, werden fortgeschrieben —
/// widersprüchlich zugeordnete (wieder lebendige) UUIDs entfallen ersatzlos.
async fn write_item_lifecycle_metadata(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    pending: &[crate::item_lifecycle::DetachedInstance],
) -> Result<(), String> {
    sqlx::query("DELETE FROM item_instance_finalizations WHERE char_id = ?")
        .bind(char_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("Lifecycle-Metadaten zurücksetzen {char_id}: {e}"))?;
    for d in pending {
        sqlx::query(
            "INSERT INTO item_instance_finalizations \
               (char_id, item_uuid, detached_at_revision, reason, runtime_id, recorded_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(char_id)
        .bind(&d.item_uuid)
        .bind(d.detached_at_revision)
        .bind(d.reason.as_db())
        .bind(&d.runtime_id)
        .bind(d.recorded_at_ms)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("Lifecycle-Metadaten schreiben {char_id}: {e}"))?;
    }
    Ok(())
}

/// Interne Transaktionshilfe: Ist `uuid` noch irgendwo platziert? Geprüft
/// werden alle Platzierungs-/Pufferzeilen (eigener und fremde Charaktere).
/// Die Tabellennamen stammen aus einer festen internen Liste (keine
/// Benutzereingabe). Nach dem Inventar-Vollwrite desselben Snapshots in
/// derselben Transaktion findet diese Prüfung nur noch veraltete Pufferzeilen
/// oder fremde (widersprüchliche) Zuordnungen — genau die Fälle, in denen
/// nicht gelöscht werden darf.
async fn item_uuid_is_referenced(
    tx: &mut sqlx::Transaction<'_, MySql>,
    uuid: &str,
) -> Result<bool, String> {
    for table in [
        "character_inventory",
        "bag_slots",
        "character_equipment",
        "inventory_buffer",
    ] {
        let sql = format!("SELECT 1 FROM {table} WHERE item_uuid = ? LIMIT 1");
        let hit: Option<i32> = sqlx::query_scalar(&sql)
            .bind(uuid)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("Lifecycle-Referenzprüfung ({table}): {e}"))?;
        if hit.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Interne Transaktionshilfe: Gespeicherte Lifecycle-Metadaten eines
/// Charakters in derselben Transaktion lesen (K1-Merge; Aufruf aus
/// `apply_item_lifecycle`). Zeilen mit unbekanntem Grund entfallen beim
/// Laden (konsistent zur Startup-Finalisierung, die sie nicht anfasst).
async fn load_item_lifecycle_metadata(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
) -> Result<Vec<crate::item_lifecycle::DetachedInstance>, String> {
    let rows: Vec<(String, i64, String, String, i64)> = sqlx::query_as(
        "SELECT item_uuid, detached_at_revision, reason, runtime_id, recorded_at_ms \
         FROM item_instance_finalizations WHERE char_id = ? ORDER BY item_uuid",
    )
    .bind(char_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("Lifecycle-Metadaten lesen {char_id}: {e}"))?;
    let mut out = Vec::new();
    for (uuid, revision, reason, runtime_id, recorded_at_ms) in rows {
        let Some(reason) = crate::item_lifecycle::DetachReason::from_db(&reason) else {
            continue;
        };
        out.push(crate::item_lifecycle::DetachedInstance {
            item_uuid: uuid,
            reason,
            runtime_id,
            recorded_at_ms,
            detached_at_revision: revision,
        });
    }
    Ok(out)
}

/// Interne Transaktionshilfe: Wendet die Lifecycle-Sicht EINES Snapshots an
/// (Aufruf aus `persist::apply_snapshot_to_db`, dieselbe Transaktion).
/// Altformat-Snapshots (`None`) erreichen diese Funktion nicht — sie lassen
/// Metadaten und Instanzen unberührt.
///
/// K1: Gespeicherte Pflichten werden in derselben Transaktion gelesen und mit
/// der Snapshot-Sicht zusammengeführt (`merge_pending`) — ein `Some(empty)`
/// nach Neustart löscht erhaltene Konflikt-/Finalisierungspflichten nicht.
/// Eine Wiedereinsetzung hebt nur die passende Pflicht auf; die Löschung
/// bleibt an Snapshot-Abwesenheit UND Referenzprüfung gebunden.
pub(crate) async fn apply_item_lifecycle(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    lifecycle: &crate::item_lifecycle::ItemLifecycleSnapshot,
    inventory: &crate::inventory::InventoryState,
) -> Result<(), String> {
    let placed = inventory.persistent_uuids();
    let stored = load_item_lifecycle_metadata(&mut *tx, char_id).await?;
    let merged = crate::item_lifecycle::merge_pending(&stored, &lifecycle.pending, &placed);
    write_item_lifecycle_metadata(&mut *tx, char_id, &merged).await?;
    let mut referenced = std::collections::BTreeSet::new();
    for d in &merged {
        if item_uuid_is_referenced(&mut *tx, &d.item_uuid).await? {
            // Referenziert oder widersprüchlich zugeordnet: Metadaten zur
            // erneuten Prüfung erhalten, Instanz NICHT löschen.
            referenced.insert(d.item_uuid.clone());
        }
    }
    for uuid in crate::item_lifecycle::deletable_candidates(&merged, &placed, &referenced) {
        sqlx::query("DELETE FROM item_instances WHERE item_uuid = ?")
            .bind(&uuid)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("Instanz finalisieren {uuid}: {e}"))?;
    }
    Ok(())
}

/// Ergebnis der Startup-Finalisierung (reine Zähler, keine IDs/Inhalte).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemFinalizeReport {
    pub scanned: usize,
    pub deleted: usize,
    pub kept_referenced: usize,
    pub metadata_dropped: usize,
    pub skipped_unknown_reason: usize,
}

/// Startup-Finalisierung alter Runtime-Metadaten (Aufruf aus dem Startpfad
/// NUR bei vollständig abgeschlossener Recovery — bei unvollständiger
/// Recovery keine widersprüchliche Bereinigung, keine vorzeitige Freigabe).
/// Verarbeitet ausschließlich explizit als abgekoppelt markierte Zeilen
/// (`item_instance_finalizations`) — kein Voll-Scan über `item_instances`
/// (docs/inventory_system.md §16). Je Zeile eine eigene kleine Transaktion;
/// ein Fehler bricht ab, der nächste Start versucht erneut.
pub async fn finalize_detached_item_instances(
    pool: &Pool<MySql>,
) -> Result<ItemFinalizeReport, String> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT char_id, item_uuid, reason FROM item_instance_finalizations \
         ORDER BY char_id, item_uuid",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Lifecycle-Metadaten lesen: {e}"))?;
    let mut report = ItemFinalizeReport::default();
    for (char_id, uuid, reason) in &rows {
        if uuid.trim().is_empty() {
            continue;
        }
        report.scanned += 1;
        if crate::item_lifecycle::DetachReason::from_db(reason).is_none() {
            report.skipped_unknown_reason += 1;
            continue;
        }
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| format!("Lifecycle-Finalisierung beginnen: {e}"))?;
        if item_uuid_is_referenced(&mut tx, uuid).await? {
            report.kept_referenced += 1;
            tx.commit()
                .await
                .map_err(|e| format!("Lifecycle-Finalisierung commit: {e}"))?;
            continue;
        }
        let exists: Option<i32> =
            sqlx::query_scalar("SELECT 1 FROM item_instances WHERE item_uuid = ? LIMIT 1")
                .bind(uuid)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| format!("Instanzbestand prüfen: {e}"))?;
        if exists.is_some() {
            sqlx::query("DELETE FROM item_instances WHERE item_uuid = ?")
                .bind(uuid)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("Instanz finalisieren: {e}"))?;
            report.deleted += 1;
        }
        sqlx::query("DELETE FROM item_instance_finalizations WHERE char_id = ? AND item_uuid = ?")
            .bind(char_id)
            .bind(uuid)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Lifecycle-Metadaten bereinigen: {e}"))?;
        report.metadata_dropped += 1;
        tx.commit()
            .await
            .map_err(|e| format!("Lifecycle-Finalisierung commit: {e}"))?;
    }
    Ok(report)
}

// ===== NPC-Händlerkatalog (Migration 022, docs/Handelssystem.md) =====
// Content-Schicht je RealmDB: Händlerrolle je NPC-Spawn plus Sortiment mit
// Kauf-/Verkaufspreisen (Idia, absolute Beträge). Unbegrenzter Bestand
// (keine Mengen-/Quotenspalten). Ungültige Zeilen (negative Preise,
// beidseitig NULL) werden übersprungen und protokolliert — fail-closed: kein
// Angebot ohne expliziten Preis. Zeilen mit unbekannter Item-ID bleiben
// stehen und gelten laufzeitseitig als nicht angeboten (Definition fehlt im
// RAM-Katalog, `trade::attempt_trade` lehnt dann mit `offer_unavailable` ab).

/// Lädt den Händlerkatalog (Händlerrolle + Sortiment mit Preisen).
pub async fn load_merchant_catalog(
    pool: &Pool<MySql>,
) -> Result<crate::trade::MerchantCatalog, String> {
    let spawns: Vec<i64> = sqlx::query_scalar("SELECT spawn_id FROM npc_merchants")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Händlerrollen laden: {e}"))?;
    let rows: Vec<(i64, String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT spawn_id, item_id, buy_price_idia, sell_price_idia FROM merchant_offers",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Händlerangebote laden: {e}"))?;
    let mut catalog = crate::trade::MerchantCatalog::default();
    for spawn_id in spawns {
        catalog.merchants.insert(spawn_id);
    }
    for (spawn_id, item_id, buy_price, sell_price) in rows {
        let valid = match (buy_price, sell_price) {
            (None, None) => false,
            (Some(b), _) if b < 0 => false,
            (_, Some(s)) if s < 0 => false,
            _ => true,
        };
        if !valid {
            log::warn!("Händlerangebot ohne gültigen Preis übersprungen");
            continue;
        }
        catalog.offers.insert(
            (spawn_id, item_id),
            crate::trade::MerchantOffer {
                buy_price,
                sell_price,
            },
        );
    }
    Ok(catalog)
}

/// Quest-Zeile aus der Tabelle `quests` (docs/Quest-System.md §27/Quest V1,
/// Migration 004_quests.sql). Die rohe TINYINT-Spalte `state` wird erst im
/// QuestService auf den Questzustand abgebildet (ACTIVE=1, COMPLETED=2,
/// FAILED=3). HIDDEN/AVAILABLE sind abgeleitet und werden nie persistiert.
#[derive(Debug, Clone)]
pub struct QuestRow {
    pub quest_id: String,
    pub state: i8,
    pub data: Option<String>,
}

/// Lädt die persistierten Quest-Zeilen eines Charakters (§27.13). Kehrt
/// nie mit einem Teilzustand zurück, der die Persistenz als Wahrheit
/// verfälscht; defekte/unbekannte Zahlen werden vom QuestService verworfen.
pub async fn load_quest_rows(pool: &Pool<MySql>, char_id: &str) -> Result<Vec<QuestRow>, String> {
    let rows = sqlx::query_as::<_, (String, i8, Option<String>)>(
        "SELECT quest_id, state, data FROM quests WHERE char_id = ? ORDER BY quest_id",
    )
    .bind(char_id)
    .persistent(false)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Questzustände laden (char {char_id}): {e}"))?;
    Ok(rows
        .into_iter()
        .map(|(quest_id, state, data)| QuestRow {
            quest_id,
            state,
            data,
        })
        .collect())
}

/// Interne Transaktionshilfe: Spieler-Questzustand als Upsert in eine
/// laufende Transaktion schreiben (Tabelle `quests`, Spalten state + data,
/// §27.12). Der Aufrufer stellt sicher, dass nur persistierbare Zustände
/// (ACTIVE/COMPLETED/FAILED) geschrieben werden. Wird vom bisherigen
/// Einzel-Save (`save_quest_state`) und vom atomaren Questabschluss genutzt.
pub(crate) async fn write_quest_state(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    quest_id: &str,
    state: i8,
    data: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO quests (char_id, quest_id, state, data) VALUES (?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE state = VALUES(state), data = VALUES(data)",
    )
    .bind(char_id)
    .bind(quest_id)
    .bind(state)
    .bind(data)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("Questzustand speichern (char {char_id}, quest {quest_id}): {e}"))?;
    Ok(())
}

/// Schreibt einen Spieler-Questzustand als Upsert in die Tabelle `quests`
/// (Spalten state + data, §27.12). Der Aufrufer stellt sicher, dass nur
/// persistierbare Zustände (ACTIVE/COMPLETED/FAILED) geschrieben werden.
/// Aufgerufen über `quest::QuestService::persist_state`; in V1.1 noch ohne
/// Gameplay-Aufrufer (Questdialog folgt in V1.2).
#[allow(dead_code)]
pub async fn save_quest_state(
    pool: &Pool<MySql>,
    char_id: &str,
    quest_id: &str,
    state: i8,
    data: &str,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| {
        format!("Questzustand (char {char_id}, quest {quest_id}) — Transaktion beginnen: {e}")
    })?;
    write_quest_state(&mut tx, char_id, quest_id, state, data).await?;
    tx.commit().await.map_err(|e| {
        format!("Questzustand (char {char_id}, quest {quest_id}) — Transaktion commit: {e}")
    })?;
    Ok(())
}

/// DB-seitiger Guard für den persistenten ACTIVE→COMPLETED-Übergang des
/// atomaren Questabschlusses (Quest V1.2a.2, docs/Quest-System.md §27.26
/// „Doppelabschluss-Schutz" + Auditergebnis V1.2a.1): Aktualisiert die
/// `quests`-Zeile NUR dann, wenn sie noch ACTIVE (state 1, Spalte TINYINT)
/// ist. Die Zählwerte entsprechen den Werten von `QuestState::db_value`
/// (quest.rs; den Guard-Kontrakt sichert der zugehörige quest.rs-Test).
///
/// Erwartet wird genau 1 betroffene Zeile. 0 Zeilen bedeutet, die Quest ist
/// zwischen Vorprüfung und Aufschlag nicht mehr als ACTIVE vorhanden
/// (bereits COMPLETED durch diesen oder einen konkurrierenden Realm-Prozess,
/// Doppelklick, Doppelabschluss). Der Aufrufer darf die Transaktion dann
/// NICHT committen — sie muss verworfen (rollback) werden. Liefert
/// `Ok(true)` nur bei exakt 1 betroffenen Zeile.
pub(crate) async fn guarded_complete_quest(
    tx: &mut sqlx::Transaction<'_, MySql>,
    char_id: &str,
    quest_id: &str,
    data: &str,
) -> Result<bool, String> {
    let result = sqlx::query(
        "UPDATE quests SET state = 2, data = ? \
         WHERE char_id = ? AND quest_id = ? AND state = 1",
    )
    .bind(data)
    .bind(char_id)
    .bind(quest_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("Questabschluss-Guard (char {char_id}, quest {quest_id}): {e}"))?;
    Ok(result.rows_affected() == 1)
}

// ── Tests: DB-Fehlerklassifizierung (Audit 4.5, B3) ─────────────────────
//
// Nachweis an der TATSÄCHLICH verwendeten Logformatierung (`db_error_line`),
// nicht an einer nachgebauten Hilfsfunktion. Es ist **keine** echte Datenbank
// nötig: die Klassen werden aus `sqlx::Error`-Werten gebildet, die ohne
// Verbindung konstruierbar sind, und der Rohinhalt wird über einen
// markierten String geprüft.

#[cfg(test)]
mod db_error_log_tests {
    use super::*;

    /// Ein Rohfehler mit eindeutigem Markierungsstring. Taucht dieses
    /// Markierungszeichen in einer Logzeile auf, ist Rohinhalt ausgegeben.
    const MARK: &str = "RAWLEAKMARKER";

    fn marked_driver_error() -> sqlx::Error {
        sqlx::Error::AnyDriverError(Box::new(std::io::Error::other(MARK)))
    }

    // B3, Nachweis 1: die Zeile enthält Operation, Identität und Klasse —
    // und niemals den Rohfehler, weder als Text noch als SQL/DSN-Anteil.
    #[test]
    fn db_error_line_carries_operation_id_and_class_only() {
        let line = db_error_line("save_position", "42", DbErrorClass::Driver);
        assert_eq!(
            line, "db_error operation=save_position id=42 error_class=driver",
            "{line}"
        );
        for forbidden in [MARK, "SELECT", "UPDATE", "mysql://", "://", "err="] {
            assert!(!line.contains(forbidden), "verboten {forbidden:?}: {line}");
        }
    }

    // B3, Nachweis 2: ein markierter Rohfehler darf NICHT erscheinen. Der
    // Fehler wird ausschließlich auf seine Klasse abgebildet.
    #[test]
    fn marked_raw_error_never_reaches_the_log_line() {
        let e = marked_driver_error();
        let line = db_error_line("save_position", "42", db_error_class(&e));
        assert!(!line.contains(MARK), "Rohfehler im Log: {line}");
        assert!(!line.contains("io error"), "Rohfehler-Art im Log: {line}");
        // Operation und Klasse bleiben erkennbar.
        assert!(line.contains("operation=save_position"), "{line}");
        assert!(line.contains("error_class=driver"), "{line}");
    }

    // B3, Nachweis 3: die Zuordnung ist stabil und enthält keinen
    // Treibertext. Jede Klasse hat genau eine Schreibweise.
    #[test]
    fn error_classes_are_stable_and_closed() {
        let classes = [
            (DbErrorClass::Connect, "connect"),
            (DbErrorClass::Begin, "begin"),
            (DbErrorClass::Commit, "commit"),
            (DbErrorClass::Timeout, "timeout"),
            (DbErrorClass::Driver, "driver"),
            (DbErrorClass::Other, "other"),
        ];
        for (class, want) in classes {
            assert_eq!(class.as_str(), want);
            let line = db_error_line("op", "1", class);
            assert_eq!(
                line,
                format!("db_error operation=op id=1 error_class={want}")
            );
        }
    }

    // B3, Nachweis 4: `db_error_class` bildet einen markierten Fehler auf
    // eine Klasse ab, ohne Inhalt preiszugeben, und ist für gleiche
    // Eingaben deterministisch.
    #[test]
    fn error_class_mapping_is_deterministic_and_contentless() {
        let e = marked_driver_error();
        let first = db_error_class(&e);
        let second = db_error_class(&e);
        assert_eq!(first, second, "Zuordnung muss deterministisch sein");
        // AnyDriverError wird bewusst konservativ als `driver` geführt.
        assert_eq!(first, DbErrorClass::Driver);
        // Timeout-Pfad ohne Treibertext:
        let t = sqlx::Error::PoolTimedOut;
        assert_eq!(db_error_class(&t), DbErrorClass::Timeout);
    }

    // B3, Nachweis 5: die Identität läuft durch denselben Injektionsschutz
    // wie das Ablehnungslog. Ein serverseitiger Wert kann keine Zeile
    // erzeugen und kein Feld vortäuschen.
    #[test]
    fn db_error_line_escapes_and_bounds_the_identifier() {
        let line = db_error_line("op", "a\r\nb", DbErrorClass::Other);
        assert!(!line.contains('\r') && !line.contains('\n'), "{line}");
        assert!(line.contains("id=a\\x0d\\x0ab"), "{line}");
        // Genau vier durch Leerzeichen getrennte Felder: `db_error`,
        // `operation=`, `id=` und `error_class=`. Ein Identifier mit
        // Leerzeichen oder Umbruch kann kein zusätzliches Feld erzeugen.
        assert_eq!(line.split_whitespace().count(), 4, "{line}");
        // Gegenprobe: die Leerzeichen-Variante erzeugt ebenfalls kein Feld.
        let spaced = db_error_line("op", "a b", DbErrorClass::Other);
        assert_eq!(spaced.split_whitespace().count(), 4, "{spaced}");
        assert!(spaced.contains("id=a\\x20b"), "{spaced}");

        let long = db_error_line("op", &"x".repeat(500), DbErrorClass::Other);
        let id_field = long
            .split_whitespace()
            .find(|f| f.starts_with("id="))
            .expect("id-Feld");
        assert_eq!(
            id_field.chars().count(),
            3 + crate::security::REJECT_FIELD_MAX_CHARS + 1,
            "{id_field}"
        );
        assert!(id_field.ends_with('~'), "{long}");
        assert_eq!(long.split_whitespace().count(), 4, "{long}");
    }

    // B3, Regressionsgrenze: eine normale, unauffällige Zeile bleibt exakt
    // wie erwartet lesbar. Es wird keine Diagnoseinformation entfernt, die
    // zuvor als Feld enthalten war.
    #[test]
    fn db_error_line_normal_output_is_readable() {
        assert_eq!(
            db_error_line("save_npc_state", "17", DbErrorClass::Timeout),
            "db_error operation=save_npc_state id=17 error_class=timeout"
        );
        assert_eq!(
            db_error_line("save_idia_commit", "42", DbErrorClass::Commit),
            "db_error operation=save_idia_commit id=42 error_class=commit"
        );
    }

    // ── Restbefund R-1: Freitext `loot_entries.kind` in der Ladezeile ──────
    //
    // Die Testaussage ist bewusst eng gefasst: geprueft wird die **tatsaechlich
    // verwendete Logformatierung** — also exakt das Formatargument der
    // Produktionsstelle in `load_loot_tables`, gerendert mit `reject_field`.
    //
    // Es wird **nicht** behauptet, die Produktionsstelle sei hiermit
    // ausgefuehrt worden: sie braucht einen DB-Pool. Der Nachweis, dass die
    // Stelle `reject_field` verwendet, ist ein **statischer Aufrufnachweis**
    // (Fundstelle in `load_loot_tables`), kein Testlauf. Eine kuenstliche
    // Baseline-Reproduktion wird ausdruecklich **nicht** behauptet.

    /// Rendert die Loot-Art-Zeile exakt so, wie die Produktionsstelle es tut.
    fn loot_kind_line(id: i64, kind: &str) -> String {
        format!(
            "Loot-Eintrag {id}: unbekannter kind '{}'; übersprungen",
            crate::security::reject_field(kind)
        )
    }

    /// Grundinvariante: die Zeile bleibt EINZeilig, ohne Steuerzeichen, und
    /// behaelt ihre feste Feldstruktur. Der Freitextwert darf kein
    /// zusaetzliches Feld erzeugen: die Zahl der Leerzeichen-getrennten
    /// Felder muss unabhaengig vom Wert immer dieselbe sein (hier sechs:
    /// `Loot-Eintrag`, `<id>:`, `unbekannter`, `kind`, `'<wert>';`,
    /// `uebersprungen`).
    fn assert_loot_kind_line_single_field(id: i64, kind: &str) -> String {
        let line = loot_kind_line(id, kind);
        for bad in ['\n', '\r'] {
            assert!(!line.contains(bad), "Zeilenumbruch in {line:?}");
        }
        for bad in ['\u{0}', '\u{7}', '\u{1b}', '\u{7f}'] {
            assert!(!line.contains(bad), "Steuerzeichen in {line:?}");
        }
        assert_eq!(
            line.split_whitespace().count(),
            6,
            "Feldzahl geaendert: {line:?}"
        );
        assert!(line.starts_with(&format!("Loot-Eintrag {id}: unbekannter kind '")));
        assert!(line.ends_with("'; übersprungen"), "{line:?}");
        line
    }

    // R-1, Nachweis 1: CR/LF im Freitext erzeugen KEINE zweite Logzeile.
    #[test]
    fn loot_kind_value_escapes_crlf_and_stays_one_line() {
        let line = assert_loot_kind_line_single_field(
            7,
            "item\r\nFAKE db_error operation=logout id=1 error_class=driver",
        );
        assert!(line.contains("\\x0d\\x0a"), "CR/LF nicht escaped: {line:?}");
        // Der vorgetaeuschte Inhalt ist noch lesbar, aber harmlos als Text:
        // auch seine Leerzeichen sind escaped, sodass er kein Feld erzeugt.
        assert!(line.contains("FAKE\\x20db_error"), "{line:?}");
        assert!(
            !line.contains(" FAKE"),
            "un-escapetes Feld im Log: {line:?}"
        );
    }

    // R-1, Nachweis 2: Leerzeichen und Backslash werden escaped, sodass der
    // Wert kein zusaetzliches Feld erzeugen und keine Escape-Sequente
    // vortaeuschen kann.
    #[test]
    fn loot_kind_value_escapes_space_and_backslash() {
        let line = assert_loot_kind_line_single_field(7, "a b c");
        assert!(line.contains("a\\x20b\\x20c"), "{line:?}");

        let line = assert_loot_kind_line_single_field(7, "a\\x0db");
        // Der echte Backslash wird doppelt escaped; die vorgetaeuschte
        // Steuerzeichen-Darstellung bleibt deshalb als Text erkennbar.
        assert!(line.contains("a\\\\x0db"), "{line:?}");
        assert!(!line.contains('\u{0}'), "{line:?}");
    }

    // R-1, Nachweis 3: ein ueberlanger Unicode-Freitext wird zeichenweise auf
    // 64 Zeichen gekuerzt, die Zeile bleibt einzeilig und gueltiges UTF-8.
    #[test]
    fn loot_kind_value_bounds_long_unicode_without_splitting_a_char() {
        let raw = "ü".repeat(crate::security::REJECT_FIELD_MAX_CHARS + 40);
        let line = assert_loot_kind_line_single_field(7, &raw);

        // Der geklammerte Wert ist der Teil zwischen dem ersten und dem letzten
        // einfachen Anfuehrungszeichen.
        let value = line
            .split_once("kind '")
            .and_then(|(_, rest)| rest.rsplit_once("';"))
            .map(|(v, _)| v)
            .expect("Wert zwischen den Anfuehrungszeichen");
        assert_eq!(
            value.chars().count(),
            crate::security::REJECT_FIELD_MAX_CHARS + 1,
            "Wert nicht auf 64 Zeichen + Marker begrenzt: {value:?}"
        );
        assert!(value.ends_with('~'), "Marker fehlt: {value:?}");
        assert_eq!(
            value.chars().filter(|c| *c == 'ü').count(),
            crate::security::REJECT_FIELD_MAX_CHARS,
            "Mehrbytezeichen mittig abgeschnitten: {value:?}"
        );
    }

    // R-1, Regressionsgrenze: der NORMALE Fall bleibt unveraendert lesbar. Ein
    // unbekannter, aber harmloser Wert erscheint weiterhin im Klartext; die
    // Validierung selbst wird davon nicht beruehrt.
    #[test]
    fn loot_kind_line_normal_output_is_unchanged() {
        assert_eq!(
            loot_kind_line(3, "bogus"),
            "Loot-Eintrag 3: unbekannter kind 'bogus'; übersprungen"
        );
        // Ein realer Wert wird weiterhin akzeptiert, der unbekannte weiterhin
        // abgelehnt — die Zeile entsteht dann gar nicht erst.
        assert!(crate::loot::LootKind::from_db("item").is_some());
        assert!(crate::loot::LootKind::from_db("bogus").is_none());
    }
}
