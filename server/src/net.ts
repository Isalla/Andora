// net.ts — WebSocket-Server + Message-Dispatcher
import WebSocket from 'ws';
import { config } from './config';
import { C2S } from './protocol';
import { bySocket, players } from './world';
import { savePosition } from './db/character';
import { handleHello } from './handlers/hello';
import { handleHeartbeat } from './handlers/heartbeat';
import { handleMove } from './handlers/move';
import { handleChat } from './handlers/chat';
import type { NetMsg } from './types';

let wss: WebSocket.Server;

export function initWebSocket(): WebSocket.Server {
  wss = new WebSocket.Server({ port: config.wsPort });

  wss.on('connection', (ws) => {
    console.log('client connected');

    ws.on('message', (raw) => {
      let msg: NetMsg;
      try {
        msg = JSON.parse(raw.toString());
      } catch {
        return; // invalid JSON ignorieren, kein Crash
      }
      switch (msg.type) {
        case C2S.HELLO:      handleHello(ws, msg).catch(e => console.error('HELLO', e)); break;
        case C2S.HEARTBEAT:  handleHeartbeat(ws, msg); break;
        case C2S.MOVE:       handleMove(ws, msg); break;
        case C2S.CHAT:       handleChat(ws, msg); break;
        // M5: C2S.ATTACK  → handlers/battle.ts (noch stub-free)
        // M6: C2S.NPC_TALK → handlers/npc.ts
        // M8: C2S.AUCTION_* → handlers/auction.ts
        default:
          console.log('unknown type', msg.type);
      }
    });

    ws.on('close', async () => {
      const me = bySocket.get(ws);
      if (!me) return;
      await savePosition(me.id, me.x, me.y);
      const despawn = JSON.stringify({ seq: 0, type: 3 /* S2C.DESPAWN */, data: { id: me.id } });
      for (const q of players.values()) {
        if (q.ws === ws) continue;
        if (q.entities.delete(me.id)) q.ws.send(despawn);
      }
      players.delete(me.id);
      bySocket.delete(ws);
      console.log('client disconnected:', me.id);
    });

    ws.on('error', (e) => console.error('ws error:', e.message));
  });

  console.log(`WebSocket on ${config.wsPort}`);
  return wss;
}
