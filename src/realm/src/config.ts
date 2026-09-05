// config.ts — Konfiguration aus src/realm/config.env laden (Realm-/World-Server)
// DREI getrennte Datenbankverbindungen dieses Servers (je DB: eigene
// Konfiguration, eigener technischer DB-Benutzer, eigener Connection-Pool —
// siehe db/pool.ts): character, world_data, realm_state.
// Es gibt keine WORLD_DB_*-Sammelkonfiguration und keinen Legacy-Fallback auf DB_*.
// Fehlt eine notwendige DB-Konfiguration, wird beim Init des jeweiligen Pools
// mit klarer Fehlermeldung abgebrochen; es wird nie stillschweigend eine
// andere Datenbank verwendet.
// Die AUTH-DB gehoert NICHT zu diesem Server: Der Realm-/World-Server besitzt
// keine AUTH_DB_*-Zugangsdaten und keinen direkten auth-DB-Zugriff. Fuer
// Auth-Funktionen (Handoff-/Session-Validierung, Account-Permissionen,
// World-Server-Authentifizierung, Heartbeat, Elternkontrolle) verwendet er
// ausschliesslich die Auth-API des separaten Auth-/API-Services
// (Berechtigung u. a.: handoff.validate, session.validate,
// account.permissions, parental.status, parental.pin; z. B.
// docs/Auth_API_Architektur.md Abschnitt 8/10,
// docs/Login_Realm_Architektur.md Abschnitt 6/7). Die Schnittstelle bildet
// das Auth-API-Modul authapi.ts ab; dieses Config bzw. der
// Realm-/World-Server liefert und speichert keinerlei AUTH-Zugangsdaten.
// Ist AUTHAPI_URL leer, ist die Auth-Anbindung (inkl. Elternkontrolle)
// bewusst deaktiviert (Entwicklung/Testprototyp).
import fs from 'fs';
import path from 'path';

export interface Env {
  [k: string]: string;
}

export interface DbConfig {
  host: string;
  port: number;
  user: string;
  password: string;
  database: string;
}

export function loadEnv(p: string): Env {
  const out: Env = {};
  if (!fs.existsSync(p)) return out;
  for (const line of fs.readFileSync(p, 'utf8').split('\n')) {
    const t = line.trim();
    if (!t || t.startsWith('#')) continue;
    const eq = t.indexOf('=');
    if (eq === -1) continue;
    out[t.slice(0, eq).trim()] = t.slice(eq + 1).trim();
  }
  return out;
}

const env = loadEnv(path.join(__dirname, '..', 'config.env'));
const num = (k: string, d: number) => {
  const v = Number(env[k]);
  return Number.isFinite(v) && v > 0 ? v : d;
};

// Liest eine DB-Konfiguration aus einem Env-Block (z. B. AUTH_DB_*).
function dbConfig(p: string, dName: string): DbConfig {
  return {
    host: env[p + '_HOST'] || '',
    port: num(p + '_PORT', 3306),
    user: env[p + '_USER'] || '',
    password: env[p + '_PASSWORD'] || '',
    database: env[p + '_NAME'] || dName
  };
}

export const config = {
  wsPort: num('PORT_WS', 3001),
  healthPort: num('PORT_HTTP', 3002),
  tickInterval: num('TICK_MS', 100),
  aofbRadius: num('AOFB_RADIUS', 20),
  renderCap: num('RENDER_CAP_DEFAULT', 64),
  ollama: {
    url: env['OLLAMA_URL'] || 'http://192.168.1.32:11434',
    model: env['OLLAMA_MODEL'] || '',
    modelQuality: env['OLLAMA_MODEL_QUALITY'] || '',
    timeoutMs: num('OLLAMA_TIMEOUT_MS', 15000),
    numCtx: num('OLLAMA_NUM_CTX', 8192),
    temperature: Number(env['OLLAMA_TEMPERATURE'] || 0.8),
    topP: Number(env['OLLAMA_TOP_P'] || 0.95),
    topK: Number(env['OLLAMA_TOP_K'] || 64),
    temperatureQuality: Number(env['OLLAMA_TEMP_QUALITY'] || 1.0),
    fallback: env['OLLAMA_FALLBACK'] === '1'
  },
  // Auth-API des separaten Auth-/API-Services (eigene Service-Credential
  // dieses Realms; leere URL = Auth-Anbindung deaktiviert).
  authApi: {
    url: (env['AUTHAPI_URL'] || '').replace(/\/$/, ''),
    serviceId: env['AUTHAPI_SERVICE_ID'] || '',
    secret: env['AUTHAPI_SERVICE_SECRET'] || ''
  },
  // character-DB: persoenliche Charakterdaten und Fortschritt (eigene Verbindung)
  characterDb: dbConfig('CHARACTER_DB', 'character'),
  // world_data-DB: statische globale Weltdaten (eigene Verbindung, zumeist lesend)
  worldDataDb: dbConfig('WORLD_DATA_DB', 'world_data'),
  // realm_state-DB: persistenter Zustand eines konkreten Realms (realm_state_<realm>)
  realmStateDb: dbConfig('REALM_STATE_DB', 'realm_state_de1'),
  // SQL-Migrationen: je DB eigenes Verzeichnis (Override, sonst
  // <serverroot>/db/<bereich>/migrations, siehe db/migrations.ts).
  // ALLOW_DESTRUCTIVE_MIGRATIONS=1 gibt als destruktiv markierte
  // Migrationen (`-- destructive: ...`) frei und darf nur im
  // Realm-Update-Ablauf NACH erfolgtem Backup gesetzt werden
  // (siehe docs/Deployment_Betriebsarchitektur.md).
  migrations: {
    allowDestructive: env['ALLOW_DESTRUCTIVE_MIGRATIONS'] === '1',
    characterDir: env['CHARACTER_MIGRATIONS_DIR'] || '',
    worldDataDir: env['WORLD_DATA_MIGRATIONS_DIR'] || '',
    realmStateDir: env['REALM_STATE_MIGRATIONS_DIR'] || ''
  }
};
