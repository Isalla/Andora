// handlers/hello.ts — HELLO: Charakter laden, registrieren, Nachbarn Spawn
import WebSocket from 'ws';
import { S2C } from '../protocol';
import { loadCharacter } from '../db';
import { players, bySocket, ensureVisible } from '../world';
import { config } from '../config';
import type { NetMsg, Player } from '../types';

export async function handleHello(ws: WebSocket, msg: NetMsg): Promise<void> {
  const charId = String(msg.data?.char_id ?? '');
  if (!charId) return;
  const lang = String(msg.data?.lang ?? 'de');

  const c = await loadCharacter(charId);
  const me: Player = {
    id: String(c.id),
    name: c.name,
    ws,
    x: c.x, y: c.y,
    face: 0,
    pingMs: 0, zoneId: 0,
    hp: 100, maxHp: 100,
    lang,
    entities: new Set<string>(),
    lastActivity: Date.now(),
    pendingMove: null,
    lastSavedTick: 0
  };
  players.set(me.id, me);
  bySocket.set(ws, me);

  // WELCOME an den Spieler selbst
  ws.send(JSON.stringify({
    seq: msg.seq, type: S2C.WELCOME,
    data: { you: { id: me.id, name: me.name, x: me.x, y: me.y } }
  }));

  // Gebiets-Kollegen: Spieler spawnen (SPAWN + STATE) und umgekehrt
  for (const q of players.values()) {
    if (q.ws === ws) continue;
    if (Math.hypot(q.x - me.x, q.y - me.y) > config.aofbRadius) continue;
    ensureVisible(q, me);
    q.entities.add(me.id);
  }
}
