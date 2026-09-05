// db/migrations.ts — Automatische, versionsbasierte SQL-Migrationen der
// eigenen Realm-Datenbanken (character, world_data, realm_state_<realm>).
//
// ÜBERGANGSSTAND (Alt-Architektur-Annahme): Dieser Runner migriert alle
// drei Pools des Übergangsstands. Zielarchitektur ist genau EINE
// Realm-Datenbank (realm_state_<realm>); der Rust-Realm (src/realm-rs)
// migriert nur diese. Die character-/world_data-Pfade hier dürfen nur als
// dokumentierter Übergang bestehen und werden mit dem TS-Realm entfernt.
//
// Das ist die TypeScript-Entsprechung des Auth-Mechanismus aus src/api
// (applyAuthMigrations, db_version-Tabelle, NNN_name.sql-Dateien) und kein
// paralleles System: gleiches Dateiformat, gleiche db_version-Struktur,
// gleiche Reihenfolge- und Abbruchregeln.
//
// Ablauf pro Datenbank (eigene db_version-Tabelle in der jeweiligen DB,
// keine zentrale globale Steuerung — jeder Realm verwaltet ausschliesslich
// seine eigenen DBs):
//
//   Pool verbunden
//         ↓
//   db_version pruefen/erzeugen
//         ↓
//   angewendete Versionen laden
//         ↓
//   vorhandene Migrationen pruefen (lückenlos, eindeutig, passend zum Stand)
//         ↓
//   fehlende Migrationen numerisch aufsteigend anwenden
//         ↓
//   jede erfolgreiche Migration in db_version eintragen
//
// Regeln:
// * Bereits eingetragene Migrationen werden nie erneut ausgeführt.
// * Unbekannte/mehrdeutige Dateinamen, doppelte Versionsnummern, Lücken in
//   der Nummerierung oder ein db_version-Stand ohne passende Datei brechen
//   den Start mit klarer Fehlermeldung ab (nichts wird stillschweigend
//   übersprungen).
// * Jede Migration läuft in einer Transaktion (Statements + db_version-
//   Eintrag) auf einer eigenen Verbindung aus dem Pool. Hinweis: MariaDB
//   führt bei DDL (CREATE/ALTER/DROP) einen impliziten Commit aus; DDL ist
//   dort nicht rückrollbar. Das entspricht der Vorgabe „transaktional,
//   soweit Datenbank und Migrationsschritt dies zulassen". Die Dateien
//   verwenden deshalb IF NOT EXISTS/IF EXISTS, damit ein abgebrochener
//   Lauf beim nächsten Start sauber fortgesetzt werden kann.
// * USE-Anweisungen in Migrationsdateien werden ignoriert: Die Verbindung
//   liegt bereits auf der konfigurierten Datenbank (z. B. realm_state_de1),
//   während Dateien den kanonischen Namen (z. B. USE realm_state;) tragen.
//   Ein Ausführen von USE würde auf die falsche DB zeigen oder an den
//   eingeschränkten Realm-Rechten scheitern.
// * Dateien mit einer Kopfzeile `-- destructive: <Grund>` sind destruktiv
//   oder nicht rückwärtskompatibel und laufen nur, wenn in config.env
//   ALLOW_DESTRUCTIVE_MIGRATIONS=1 gesetzt ist. Diese Freigabe darf nur im
//   Realm-Update-Ablauf nach erfolgtem Backup gesetzt werden (siehe
//   docs/Deployment_Betriebsarchitektur.md, Abschnitt Realm-Updates).
// * Schlägt eine Migration fehl, wirft der Runner: main.ts fängt ab und
//   beendet den Prozess mit Exit 1, BEVOR Health/WebSocket starten — es
//   werden keine Spieler zugelassen.
import fs from 'fs';
import path from 'path';
import { Pool } from 'mysql2/promise';
import { config } from '../config';

const DB_VERSION_DDL = `CREATE TABLE IF NOT EXISTS db_version (
  version INT NOT NULL PRIMARY KEY,
  migration VARCHAR(255) NOT NULL,
  applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
)`;

/** Eine erkannte Migrationsdatei (Nummer = Reihenfolge, Tag = Dateiname-Rest). */
export interface MigrationFile {
  num: number;
  tag: string;
  file: string;
}

/** Zerlegt NNN_name.sql in Nummer und Tag (Split am ERSTEN Unterstrich,
 * konsistent zum Auth-Runner; der Rest darf weitere Unterstriche enthalten).
 * Wirft bei ungültigem Namen (kein stillschweigendes Überspringen). */
export function parseMigrationName(file: string): MigrationFile {
  const m = /^(\d+)_([A-Za-z0-9][A-Za-z0-9_-]*)\.sql$/.exec(file);
  if (!m) {
    throw new Error(
      `Ungültiger Migrationsdateiname ${JSON.stringify(file)}: ` +
      `erwartet wird NNN_name.sql mit numerischem Präfix`
    );
  }
  return { num: parseInt(m[1], 10), tag: m[2], file };
}

/** Teilt einen Migrations-Body in einzelne Statements (Dateien dürfen
 * mehrere enthalten, z. B. Tabelle plus Index). Zeilen mit "--" sind
 * Kommentare. USE-Anweisungen werden herausgefiltert (siehe Kopfkommentar).
 * Leere Reste ohne ";" am Dateiende werden mitgenommen. */
export function splitStatements(body: string): string[] {
  const stmts: string[] = [];
  let cur = '';
  for (const line of body.split('\n')) {
    const trimmed = line.trim();
    if (trimmed === '' || trimmed.startsWith('--')) continue;
    cur = cur === '' ? trimmed : cur + ' ' + trimmed;
    if (cur.endsWith(';')) {
      const stmt = cur.slice(0, -1).trim();
      if (stmt !== '' && !/^USE\s+\S+$/i.test(stmt)) stmts.push(stmt);
      cur = '';
    }
  }
  const rest = cur.trim().replace(/;$/, '').trim();
  if (rest !== '' && !/^USE\s+\S+$/i.test(rest)) stmts.push(rest);
  return stmts;
}

/** True, wenn die Datei als destruktiv/nicht rückwärtskompatibel markiert
 * ist (Kopfzeile `-- destructive: <Grund>`). */
export function isDestructive(body: string): boolean {
  return body.split('\n').some((l) => /^\s*--\s*destructive\s*:/i.test(l));
}

/** Ermittelt das Migrationsverzeichnis eines DB-Bereichs: Override aus
 * config.env (z. B. REALM_STATE_MIGRATIONS_DIR), sonst
 * <serverroot>/db/<bereich>/migrations neben build/ (gilt für dev und für
 * /opt/andora/server, das das komplette src/realm-Verzeichnis enthält). */
export function migrationsDir(area: string, override: string): string {
  if (override) return override;
  return path.join(__dirname, '..', '..', 'db', area, 'migrations');
}

/** Wendet alle noch fehlenden Migrationen auf genau einer Datenbank an.
 * areaLabel dient nur der Protokollierung (z. B. 'REALM_STATE_DB').
 * Wirft bei jedem Problem; der Aufrufer (main.ts) bricht dann den Start ab. */
export async function applyMigrations(
  pool: Pool,
  areaLabel: string,
  dbName: string,
  dir: string
): Promise<void> {
  let entries: string[];
  try {
    entries = fs.readdirSync(dir);
  } catch (e) {
    throw new Error(
      `[migration:${areaLabel}] Migrationsverzeichnis nicht lesbar (${dir}): ${e}. ` +
      `Abbruch — Migrationen dürfen nicht stillschweigend übersprungen werden.`
    );
  }

  const files: MigrationFile[] = entries
    .filter((n) => n.endsWith('.sql'))
    .map(parseMigrationName)
    .sort((a, b) => a.num - b.num);

  for (let i = 1; i < files.length; i++) {
    if (files[i].num === files[i - 1].num) {
      throw new Error(
        `[migration:${areaLabel}] Doppelte Migrationsnummer ${files[i].num} ` +
        `(${files[i - 1].file} vs. ${files[i].file}): Abbruch.`
      );
    }
  }
  // Lückenlose Nummerierung ab 1: Eine fehlende Datei (z. B. nur 001 und 003
  // vorhanden) bräche sonst unbemerkt die Reihenfolge — lieber laut abbrechen.
  for (let i = 0; i < files.length; i++) {
    if (files[i].num !== i + 1) {
      throw new Error(
        `[migration:${areaLabel}] Lücke in der Migrationsnummerierung: ` +
        `erwartet ${i + 1}, gefunden ${files[i].num} (${files[i].file}). Abbruch.`
      );
    }
  }

  const conn = await pool.getConnection();
  try {
    await conn.query(DB_VERSION_DDL);
    const [rows] = await conn.query('SELECT version, migration FROM db_version');
    const applied = new Map<number, string>();
    for (const r of rows as { version: number; migration: string }[]) {
      applied.set(Number(r.version), String(r.migration));
    }

    // Eingetragene Versionen ohne passende Datei = Schema-Drift (z. B.
    // gelöschte Datei oder Downgrade): laut abbrechen statt ignorieren.
    for (const [v, tag] of applied) {
      const f = files.find((x) => x.num === v);
      if (!f) {
        throw new Error(
          `[migration:${areaLabel}] db_version enthält Version ${v} (${tag}), ` +
          `aber keine passende Datei liegt in ${dir}: Abbruch.`
        );
      }
      if (f.tag !== tag) {
        throw new Error(
          `[migration:${areaLabel}] db_version Version ${v} ist als ${JSON.stringify(tag)} ` +
          `eingetragen, die Datei heißt ${f.file}: Abbruch (angewendete ` +
          `Migrationen dürfen nicht verändert/umbenannt werden).`
        );
      }
    }

    const pending = files.filter((f) => !applied.has(f.num));
    const current = files.length === 0 ? 0 : Math.max(...[...applied.keys()], 0);
    if (pending.length === 0) {
      console.log(
        `[migration:${areaLabel}] ${dbName}: Schema aktuell (Version ${current}, ` +
        `${files.length} Migrationen geprüft, keine ausstehend).`
      );
      return;
    }
    console.log(
      `[migration:${areaLabel}] ${dbName}: Version ${current}, ` +
      `${pending.length} ausstehende Migration(en) werden angewendet ...`
    );

    for (const f of pending) {
      const body = fs.readFileSync(path.join(dir, f.file), 'utf8');
      if (isDestructive(body) && !config.migrations.allowDestructive) {
        throw new Error(
          `[migration:${areaLabel}] ${f.file} ist als destruktiv markiert, aber ` +
          `ALLOW_DESTRUCTIVE_MIGRATIONS=1 ist nicht gesetzt. Destruktive Migrationen ` +
          `dürfen nur im Realm-Update-Ablauf NACH erfolgtem Backup freigegeben werden ` +
          `(siehe docs/Deployment_Betriebsarchitektur.md). Abbruch.`
        );
      }
      const stmts = splitStatements(body);
      if (stmts.length === 0) {
        throw new Error(
          `[migration:${areaLabel}] ${f.file} enthält keine ausführbaren Statements: Abbruch.`
        );
      }
      await conn.beginTransaction();
      try {
        for (let i = 0; i < stmts.length; i++) {
          try {
            await conn.query(stmts[i]);
          } catch (e) {
            throw new Error(
              `Statement ${i + 1}/${stmts.length} fehlgeschlagen: ${e}`
            );
          }
        }
        await conn.query(
          'INSERT INTO db_version (version, migration) VALUES (?, ?)',
          [f.num, f.tag]
        );
        await conn.commit();
      } catch (e) {
        try {
          await conn.rollback();
        } catch {
          // Rollback-Fehler nicht maskieren: Der Originalfehler zählt.
        }
        throw new Error(
          `[migration:${areaLabel}] ${f.file} (Version ${f.num}) fehlgeschlagen: ${e}. ` +
          `Start wird abgebrochen, keine Spieler zugelassen. ` +
          `(Hinweis: MariaDB führt DDL implizit aus — ggf. Backup-Regeln aus ` +
          `docs/Deployment_Betriebsarchitektur.md anwenden.)`
        );
      }
      console.log(`[migration:${areaLabel}] ${dbName}: ${f.file} angewendet.`);
    }
    console.log(
      `[migration:${areaLabel}] ${dbName}: Schema jetzt auf Version ` +
      `${pending[pending.length - 1].num} (${pending.length} angewendet).`
    );
  } finally {
    conn.release();
  }
}
