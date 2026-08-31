// health.ts — HTTP-Status-Server (Port PORT_HTTP): /health + /status
import { createServer, IncomingMessage, ServerResponse } from 'http';
import { config } from './config';
import { players } from './world';
import { tickStat, processCpuSeconds, heapKb, rssMb } from './metrics';
import { startEventLoopLagTracking, loopLagStats } from './events';

const LOOP_INTERVAL_MS = 1000;
const LOOP_WINDOW_SAMPLES = 60;
const MAX_PLAYERS = 50; // Obergrenze / max. erwartete Spielerzahl

interface PlayerView {
  id: string;
  name: string;
  zone_id: number;
  ping_ms: number;
  last_activity: number;
  visible_entities: number;
  seconds_inactive: number;
}

function buildStatus(): Record<string, unknown> {
  const now = Date.now();
  const cpuTotal = processCpuSeconds();
  const uptime = process.uptime();
  const zoneCounts: Record<number, number> = {};
  const playersView: PlayerView[] = [];
  const cpuPercent = uptime > 0 ? Math.min(100, (cpuTotal / uptime) * 100) : 0;
  for (const p of players.values()) {
    zoneCounts[p.zoneId] = (zoneCounts[p.zoneId] || 0) + 1;
    playersView.push({
      id: p.id,
      name: p.name,
      zone_id: p.zoneId,
      ping_ms: p.pingMs,
      last_activity: p.lastActivity,
      visible_entities: p.entities.size,
      seconds_inactive: Math.max(0, Math.round((now - p.lastActivity) / 1000))
    });
  }
  return {
    ok: true,
    server_up: true,
    uptime_s: Math.round(uptime),
    players: playersView.length,
    max_players: MAX_PLAYERS,
    zones: zoneCounts,
    zone_count: Object.keys(zoneCounts).length,
    npc_active: 0,
    instances: 1,
    tick: {
      interval_ms: config.tickInterval,
      last_ms: Math.round(tickStat().lastMs * 100) / 100,
      max_ms: Math.round(tickStat().maxMs * 100) / 100,
      avg_ms: Math.round(tickStat().avgMs * 100) / 100,
      count: tickStat().count
    },
    cpu_percent: Math.round(cpuPercent * 10) / 10,
    heap_mb: Math.round(heapKb() / 1024),
    rss_mb: Math.round(rssMb() * 10) / 10,
    event_loop: {
      interval_ms: LOOP_INTERVAL_MS,
      window_samples: LOOP_WINDOW_SAMPLES,
      current_lag_ms: loopLagStats().current,
      avg_lag_ms: loopLagStats().avg,
      // max_lag_ms = Maximum im gleitenden Fenster (kompatibel mit alter API)
      max_lag_ms: loopLagStats().max
    },
    ts: now
  };
}

function send(res: ServerResponse, code: number, body: unknown): void {
  res.writeHead(code, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify(body));
}

export function setupHealth(): void {
  startEventLoopLagTracking();

  const httpServer = createServer((req: IncomingMessage, res: ServerResponse) => {
    if (req.method !== 'GET') {
      send(res, 405, { ok: false, error: 'Method not allowed' });
      return;
    }
    if (req.url === '/health') {
      send(res, 200, { ok: true, uptime_s: Math.round(process.uptime()) });
      return;
    }
    if (req.url === '/status') {
      try {
        send(res, 200, buildStatus());
      } catch (e) {
        send(res, 500, { ok: false, error: String(e) });
      }
      return;
    }
    if (req.url === '/players') {
      send(res, 200, {
        players: Array.from(players.values()).map((p) => ({
          id: p.id, name: p.name, zone_id: p.zoneId,
          ping_ms: p.pingMs, last_activity: p.lastActivity
        }))
      });
      return;
    }
    send(res, 404, { ok: false, error: 'Not found' });
  });
  httpServer.listen(config.healthPort, () => {
    console.log(`Health server on ${config.healthPort} (/health, /status, /players)`);
  });
}
