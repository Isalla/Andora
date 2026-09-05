// db/worldData.ts — world_data-DB: eigener Pool, statische globale Weltdaten
// (Definitionen: Iteme, Monster, NPC, Regionen, Loot, Spawnregeln, ...).
// "world_data beschreibt, was in Andora existieren kann."
// Getrennt von auth, character und realm_state (eigene WORLD_DATA_DB_*-Konfiguration,
// eigener Pool). Im Server-Betrieb wird world_data nur gelesen; Aenderungen
// erfolgen kontrolliert ueber migrations/ und seed/ (Deployment/Content-Updates).
//
// ÜBERGANGSSTAND (Alt-Architektur): Eine zentrale world_data-Datenbank ist
// NICHT Zielarchitektur — statische Definitionen gehören realmbezogen in
// realm_state_<realm> (siehe docs/Datenbank_Architektur.md). Dieser Pool
// (derzeit ohne fachliche Queries, nur verbunden) darf nicht ausgebaut
// werden und entfällt mit dem Rust-Realm (src/realm-rs).
import { Pool } from 'mysql2/promise';
import { config } from '../config';
import { DbPoolHandle, openPool, closePool, getPool } from './pool';

const worldDataHandle: DbPoolHandle = { pool: null, initializing: null };

/** Gibt an, ob der world_data-DB-Pool initialisiert ist. */
export function worldDataDbReady(): boolean {
  return worldDataHandle.pool !== null;
}

/** Initialisiert den world_data-DB-Pool und prueft die Verbindung.
 * Abbruch mit klarer Fehlermeldung bei unvollstaendiger WORLD_DATA_DB_*-Konfiguration. */
export function initWorldDataDb(): Promise<void> {
  return openPool('WORLD_DATA_DB', config.worldDataDb, worldDataHandle);
}

/** Liefert den world_data-DB-Pool (nur nach initWorldDataDb). */
export function getWorldDataPool(): Pool {
  return getPool('WORLD_DATA_DB', worldDataHandle);
}

/** Schliesst den world_data-DB-Pool. */
export async function closeWorldDataDb(): Promise<void> {
  await closePool(worldDataHandle);
}
