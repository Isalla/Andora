// handlers/heartbeat.ts — HEARTBEAT → SYNC-ACK
import WebSocket from 'ws';
import { S2C } from '../protocol';
import { bySocket } from '../world';
import type { NetMsg } from '../types';

export function handleHeartbeat(ws: WebSocket, msg: NetMsg): void {
  const me = bySocket.get(ws);
  if (me) {
    me.lastActivity = Date.now();
    // Optionales Ping-Feld des Clients (Fehlerdiagnose, kein Protokoll-Change)
    const ping = Number(msg.data?.ping_ms);
    if (Number.isFinite(ping) && ping >= 0 && ping < 10000) me.pingMs = Math.round(ping);
  }
  ws.send(JSON.stringify({
    seq: msg.seq, type: S2C.SYNC, data: { ack_seq: msg.seq }
  }));
}
