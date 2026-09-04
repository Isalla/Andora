// handlers/hello.ts — HELLO: Session validieren, Charakter laden,
// registrieren, Elternkontrolle anhaengen, Nachbarn Spawn
import WebSocket from 'ws';
import { S2C } from '../protocol';
import { loadCharacter } from '../db/character';
import { players, bySocket, ensureVisible } from '../world';
import { config } from '../config';
import { authApiEnabled, validateSession } from '../authapi';
import { attachParental } from '../parental';
import type { NetMsg, Player } from '../types';

export async function handleHello(ws: WebSocket, msg: NetMsg): Promise<void> {
  const charId = String(msg.data?.char_id ?? '');
  if (!charId) return;
  const lang = String(msg.data?.lang ?? 'de');
  const sessionId = String(msg.data?.session_id ?? '');

  // Session-Bindung, sobald die Auth-API konfiguriert ist: ohne gueltige
  // Session kein Einstieg (sonst waere die Elternkontrolle umgehbar).
  let accountId = 0;
  if (authApiEnabled()) {
    if (!sessionId) {
      ws.close();
      return;
    }
    try {
      const v = await validateSession(sessionId);
      if (!v.valid || !v.account_id) {
        ws.close();
        return;
      }
      accountId = v.account_id;
    } catch (e) {
      console.error('HELLO session validate:', (e as Error).message);
      ws.close();
      return;
    }
  }

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
    accountId,
    sessionId,
    entities: new Set<string>(),
    lastActivity: Date.now(),
    pendingMove: null,
    lastSavedTick: 0
  };
  players.set(me.id, me);
  bySocket.set(ws, me);

  // Elternkontrolle: BLOCKED am Login -> Einstieg verweigert (WELCOME
  // wird dann nie gesendet; die Verbindung ist bereits geschlossen).
  const parental = await attachParental(me, accountId, sessionId);
  if (!parental.ok) {
    players.delete(me.id);
    bySocket.delete(ws);
    ws.close();
    return;
  }

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
