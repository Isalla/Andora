// world.ts — Spieler-Registry + AOFB (worldTick, spawn/despawn)
import WebSocket from 'ws';
import { config } from './config';
import { C2S, S2C } from './protocol';
import { recordTick } from './metrics';

/** Spieler keyed by id (char_id) */
export const players = new Map<string, any>();
/** Spieler keyed by ws-Instanz — sicherer Sender-Lookup */
export const bySocket = new Map<WebSocket, any>();

function dist(a: any, b: any): number {
  const dx = a.x - b.x, dy = a.y - b.y;
  return Math.sqrt(dx * dx + dy * dy);
}

/** Sendet SPAWN + STATE an o (falls p noch nicht sichtbar). */
export function ensureVisible(o: any, p: any): void {
  if (!o.entities.has(p.id)) {
    o.ws.send(JSON.stringify({
      seq: 0, type: S2C.SPAWN,
      data: { id: p.id, kind: 'player', x: p.x, y: p.y, face: p.face ?? 0 }
    }));
    o.entities.add(p.id);
  }
  o.ws.send(JSON.stringify({
    seq: 0, type: S2C.STATE,
    data: { id: p.id, x: p.x, y: p.y, face: p.face ?? 0 }
  }));
}

/** Welt-Tick: AOFB-Broadcast (SPAWN/STATE/DESPAWN) für alle Spieler. */
export function worldTick(): void {
  const t0 = process.hrtime.bigint();
  const list = [...players.values()];
  for (const q of list) {
    const nowVisible = new Set<string>();
    for (const p of list) {
      if (p.id === q.id) continue;
      if (dist(p, q) > config.aofbRadius) continue;
      nowVisible.add(p.id);
      ensureVisible(q, p);
    }
    for (const eid of [...q.entities]) {
      if (!nowVisible.has(eid)) {
        q.ws.send(JSON.stringify({ seq: 0, type: S2C.DESPAWN, data: { id: eid } }));
        q.entities.delete(eid);
      }
    }
    for (const id of nowVisible) q.entities.add(id);
  }
  recordTick(Number(process.hrtime.bigint() - t0) / 1e6);
}
