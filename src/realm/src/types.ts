// types.ts — Gemeinsame Typen (Server-Spielzustand)
import WebSocket from 'ws';

/** Autoritativer Spieler-State auf dem Server. */
export interface Player {
  id: string;
  name: string;
  ws: WebSocket;
  x: number;
  y: number;
  face: number;
  /** Ping in ms, wenn der Client ihn mitsendet (HEARTBEAT data.ping_ms); 0 = unbekannt. */
  pingMs: number;
  /** Zone/Gebiet (zone_id); 0 = Standardzone. */
  zoneId: number;
  hp: number;
  maxHp: number;
  lang: string;
  /** Auth-API-Account (0 = unbekannt/Dev ohne Auth-API). */
  accountId: number;
  /** Login-Session ('' = keine); Basis fuer Elternkontroll-Polling. */
  sessionId: string;
  entities: Set<string>;        // IDs, die dieser Spieler gerade sieht
  lastActivity: number;
  pendingMove: { dx: number; dy: number } | null;
  lastSavedTick: number;
}

export interface NetMsg {
  seq: number;
  type: number;
  data: any;
}
