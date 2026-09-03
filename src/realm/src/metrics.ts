// metrics.ts — interne Server-Metriken für den /status-Endpoint
// (nur Node-Stdlib, keine neuen Dependencies)

export interface TickStat {
  lastMs: number;
  maxMs: number;
  avgMs: number;
  count: number;
}

const ticks: TickStat = { lastMs: 0, maxMs: 0, avgMs: 0, count: 0 };

/** Baut die Dauer eines worldTick (ms) in die Statistik ein. */
export function recordTick(durationMs: number): void {
  ticks.lastMs = durationMs;
  if (durationMs > ticks.maxMs) ticks.maxMs = durationMs;
  ticks.avgMs = (ticks.avgMs * ticks.count + durationMs) / (ticks.count + 1);
  ticks.count += 1;
}

/** Aktueller Zustand der Tick-Statistik. */
export function tickStat(): TickStat {
  return { lastMs: ticks.lastMs, maxMs: ticks.maxMs, avgMs: ticks.avgMs, count: ticks.count };
}

/** CPU-Sekunden (user + system) seit Prozessstart, gerundet. */
export function processCpuSeconds(): number {
  const u = process.cpuUsage();
  return (u.user + u.system) / 1e6;
}

/** Heap-Größe in kB. */
export function heapKb(): number {
  return process.memoryUsage().heapUsed / 1024;
}

/** Residenter Prozess-Speicher (RSS) in MB. */
export function rssMb(): number {
  return process.memoryUsage().rss / 1048576;
}
