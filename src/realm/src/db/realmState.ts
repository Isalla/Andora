// db/realmState.ts — realm_state-DB: eigener Pool, persistenter Zustand eines
// konkreten Realms (Gilden, Gildenstaedte, Herrschaft, Wirtschaft, Auktionen,
// Weltfortschritt, persistente NPC-Zustaende, Event-Zustaende, ...).
// "realm_state beschreibt, was in diesem Realm passiert ist."
// Jeder Realm hat seine eigene realm_state_<realm>-Datenbank (z. B. realm_state_de1);
// eigenstaendige DB, eigener Realm-State-DB-Benutzer, eigener Pool.
// Getrennt von auth, character und world_data (eigene REALM_STATE_DB_*-Konfiguration).
// Kein Zugriff auf andere realm_state_<realm>-Datenbanken, keine Access-Geheimnisse.
import { Pool } from 'mysql2/promise';
import { config } from '../config';
import { DbPoolHandle, openPool, closePool, getPool } from './pool';

const realmStateHandle: DbPoolHandle = { pool: null, initializing: null };

/** Gibt an, ob der realm_state-DB-Pool initialisiert ist. */
export function realmStateDbReady(): boolean {
  return realmStateHandle.pool !== null;
}

/** Initialisiert den realm_state-DB-Pool und prueft die Verbindung.
 * Abbruch mit klarer Fehlermeldung bei unvollstaendiger REALM_STATE_DB_*-Konfiguration. */
export function initRealmStateDb(): Promise<void> {
  return openPool('REALM_STATE_DB', config.realmStateDb, realmStateHandle);
}

/** Liefert den realm_state-DB-Pool (nur nach initRealmStateDb). */
export function getRealmStatePool(): Pool {
  return getPool('REALM_STATE_DB', realmStateHandle);
}

/** Schliesst den realm_state-DB-Pool. */
export async function closeRealmStateDb(): Promise<void> {
  await closePool(realmStateHandle);
}
