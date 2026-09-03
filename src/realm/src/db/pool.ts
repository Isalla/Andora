// db/pool.ts — Gemeinsamer Pool-Helper fuer die drei getrennten Andora-DBs.
// Jede DB (character, world_data, realm_state) besitzt eigene Konfiguration
// und wird ueber diesen Helper zu genau EINEM eigenen Connection-Pool verbunden.
// Die auth-DB ist KEIN Teil dieses Servers: kein Pool, keine AUTH_DB_*-Config
// (der Auth-/API-Service ist einziger direkter auth-DB-Zugriff, src/api).
// Kein Pool wird von mehreren DB-Bereichen gemeinsam verwendet.
// Fehlt eine notwendige DB_*-Konfiguration, wird mit klarer Fehlermeldung
// abgebrochen; es wird niemals stillschweigend eine andere Datenbank verwendet.
import { createPool, Pool } from 'mysql2/promise';
import type { DbConfig } from '../config';

/** Zustand eines einzelnen DB-Pools (exakt eine Datenbank). */
export interface DbPoolHandle {
  pool: Pool | null;
  initializing: Promise<void> | null;
}

/** Erzeugt den Pool fuer eine einzelne DB und prueft die Verbindung.
 * prefix = Env-Prvaefix der DB (z. B. 'CHARACTER_DB'). */
export async function openPool(prefix: string, cfg: DbConfig, handle: DbPoolHandle): Promise<void> {
  if (handle.pool) return;
  if (handle.initializing) return handle.initializing;
  const missing: string[] = [];
  if (!cfg.host) missing.push(prefix + '_HOST');
  if (!cfg.user) missing.push(prefix + '_USER');
  if (!cfg.database) missing.push(prefix + '_NAME');
  if (missing.length > 0) {
    throw new Error('Fehlende ' + prefix + '-DB-Konfiguration: ' + missing.join(', '));
  }
  handle.initializing = (async () => {
    const pool = createPool({
      host: cfg.host,
      port: cfg.port,
      user: cfg.user,
      password: cfg.password,
      database: cfg.database
    });
    await pool.query('SELECT 1');
    handle.pool = pool;
  })();
  try {
    await handle.initializing;
  } catch (e) {
    handle.initializing = null;
    handle.pool = null;
    throw e;
  }
}

/** Liefert den Pool der DB (nur nach openPool). */
export function getPool(prefix: string, handle: DbPoolHandle): Pool {
  if (!handle.pool) throw new Error(prefix + '-DB nicht initialisiert');
  return handle.pool;
}

/** Schliesst den Pool der DB. */
export async function closePool(handle: DbPoolHandle): Promise<void> {
  if (handle.pool) {
    await handle.pool.end();
    handle.pool = null;
  }
  handle.initializing = null;
}
