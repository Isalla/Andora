// db/character.ts — character-DB: eigener Pool, persoenliche Charakterdaten und
// persoenlicher Fortschritt. Getrennt von auth, world_data und realm_state
// (eigene CHARACTER_DB_*-Konfiguration, eigener Pool).
// Access-/Geheimnisse werden hier NICHT gelesen oder gespeichert.
//
// ÜBERGANGSSTAND (Alt-Architektur): Zielarchitektur ist genau EINE
// Realm-Datenbank (realm_state_<realm>) ohne separate character-DB
// (siehe docs/Datenbank_Architektur.md). Diese Anbindung bleibt nur
// bestehen, bis Charakterdaten/ Realm-Zustand im Rust-Realm
// (src/realm-rs) zusammengeführt sind.
import { Pool } from 'mysql2/promise';
import { config } from '../config';
import { DbPoolHandle, openPool, closePool, getPool } from './pool';

const characterHandle: DbPoolHandle = { pool: null, initializing: null };

/** Gibt an, ob der character-DB-Pool initialisiert ist. */
export function characterDbReady(): boolean {
  return characterHandle.pool !== null;
}

/** Initialisiert den character-DB-Pool und prueft die Verbindung.
 * Abbruch mit klarer Fehlermeldung bei unvollstaendiger CHARACTER_DB_*-Konfiguration. */
export function initCharacterDb(): Promise<void> {
  return openPool('CHARACTER_DB', config.characterDb, characterHandle);
}

/** Liefert den character-DB-Pool (nur nach initCharacterDb). */
export function getCharacterPool(): Pool {
  return getPool('CHARACTER_DB', characterHandle);
}

/** Schliesst den character-DB-Pool. */
export async function closeCharacterDb(): Promise<void> {
  await closePool(characterHandle);
}

export interface Character {
  id: number | string;
  name: string;
  x: number;
  y: number;
}

/** Laedt einen Charakter; erzeugt ihn bei Bedarf (Testprototyp, Zone 0, Spawn 0,0). */
export async function loadCharacter(charId: string): Promise<Character> {
  const p = getCharacterPool();
  const result = await p.execute(
    'SELECT id, name, pos_x, pos_y FROM characters WHERE id = ?',
    [charId]
  );
  const rows = result[0] as unknown as {
    id: number | string;
    name: string;
    pos_x: number;
    pos_y: number;
  }[];
  if (rows.length > 0) {
    const c = rows[0];
    return { id: c.id, name: c.name, x: c.pos_x, y: c.pos_y };
  }
  await p.execute(
    "INSERT INTO characters (name, race, char_class) VALUES (?, 'Mensch', 'Warrior')",
    [charId]
  );
  return { id: charId, name: charId, x: 0, y: 0 };
}

/** Speichert die Position eines Charakters in der character-DB. */
export async function savePosition(charId: string | number, x: number, y: number): Promise<void> {
  try {
    await getCharacterPool().execute(
      'UPDATE characters SET pos_x = ?, pos_y = ? WHERE id = ?',
      [x, y, charId]
    );
  } catch (e) {
    console.error('savePosition:', e);
  }
}
