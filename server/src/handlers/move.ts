// handlers/move.ts — MOVE: Position auf Server validiert (Speed-Cap)
import WebSocket from 'ws';
import { config } from '../config';
import { bySocket } from '../world';
import type { NetMsg } from '../types';

export function handleMove(ws: WebSocket, msg: NetMsg): void {
  const me = bySocket.get(ws);
  if (!me) return;
  const dir = msg.data?.dir;
  const dx = Array.isArray(dir) ? Number(dir[0]) : Number(msg.data?.x) || 0;
  const dy = Array.isArray(dir) ? Number(dir[1]) : Number(msg.data?.y) || 0;
  const len = Math.hypot(dx, dy) || 1;
  // Speed-Cap: max 210 m/s, skaliert auf Tick-Intervall -> keine Teleports
  const maxStep = 210 * (config.tickInterval / 1000);
  const step = Math.min(len, maxStep);
  me.x += (dx / len) * step;
  me.y += (dy / len) * step;
}
