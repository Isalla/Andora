// db.ts — MariaDB-Pool + Charakter-Load/Save
import { createPool, Pool } from 'mysql2/promise';
import { config } from './config';

let pool: Pool;

export function getPool(): Pool {
  if (!pool) {
    pool = createPool(config.db as any);
  }
  return pool;
}

export async function initDatabase(): Promise<void> {
  await getPool().query('SELECT 1');
  console.log('Database pool ok');
}

export interface Character {
  id: number | string;
  name: string;
  x: number;
  y: number;
}

/** Lädt einen Charakter; erzeugt ihn bei Bedarf (Zone 0, Spawn 0,0). */
export async function loadCharacter(charId: string): Promise<Character> {
  const p = getPool();
  const [rows] = await p.execute<any>(
    'SELECT id, name, pos_x, pos_y FROM characters WHERE id = ?',
    [charId]
  ) as [any[], any];
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

export async function savePosition(charId: string | number, x: number, y: number): Promise<void> {
  try {
    await getPool().execute(
      'UPDATE characters SET pos_x = ?, pos_y = ? WHERE id = ?',
      [x, y, charId]
    );
  } catch (e) {
    console.error('savePosition:', e);
  }
}

export async function closeDatabase(): Promise<void> {
  if (pool) await pool.end();
}
