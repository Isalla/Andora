// db/auth.ts — AUTH-DB: eigener Pool fuer Account/Auth/Session/Realm-Registrierung.
// Getrennt von character, world_data und realm_state (eigene Module, eigene Pools,
// eigene AUTH_DB_*-Konfiguration). Access/Geheimnisse bleiben exclusive hier.
import { Pool } from 'mysql2/promise';
import { config } from '../config';
import { DbPoolHandle, openPool, closePool, getPool } from './pool';

const authHandle: DbPoolHandle = { pool: null, initializing: null };

/** Gibt an, ob der AUTH-DB-Pool initialisiert ist. */
export function authDbReady(): boolean {
  return authHandle.pool !== null;
}

/** Initialisiert den AUTH-DB-Pool und prueft die Verbindung.
 * Abbruch mit klarer Fehlermeldung bei unvollstaendiger AUTH_DB_*-Konfiguration. */
export function initAuthDb(): Promise<void> {
  return openPool('AUTH_DB', config.authDb, authHandle);
}

/** Liefert den AUTH-DB-Pool (nur nach initAuthDb). */
export function getAuthPool(): Pool {
  return getPool('AUTH_DB', authHandle);
}

/** Schliesst den AUTH-DB-Pool. */
export async function closeAuthDb(): Promise<void> {
  await closePool(authHandle);
}
