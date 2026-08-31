// handlers/chat.ts — CHAT: an Spieler im AOFB-Radius senden
import WebSocket from 'ws';
import { config } from '../config';
import { S2C } from '../protocol';
import { players, bySocket } from '../world';
import type { NetMsg } from '../types';

export function handleChat(ws: WebSocket, msg: NetMsg): void {
  const me = bySocket.get(ws);
  if (!me) return;
  const text = String(msg.data?.text || '').slice(0, 240);
  if (!text) return;
  const channel = String(msg.data?.channel || 'local');
  const payload = JSON.stringify({
    seq: 0, type: S2C.CHAT, data: { from: me.name, channel, text }
  });
  for (const o of players.values()) {
    const d = Math.hypot(o.x - me.x, o.y - me.y);
    if (d > config.aofbRadius) continue;
    o.ws.send(payload);
  }
  ws.send(payload);
}
