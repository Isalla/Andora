// events.ts — Event-Loop-Verzögerung (Lag-Diagnose)
//
// Gleitendes Zeitfenster: Ein Timer soll exakt alle INTERVAL_MS ausführen.
// Jede Ausführung misst, wie weit der tatsächliche Zeitpunkt vom geplanten
// (letzter Zeitpunkt + Intervall) abweicht. Ergebnis in einem Ringpuffer
// (WINDOW Werte). Alte Ausreißer fallen damit automatisch aus dem Fenster.

const INTERVAL_MS = 1000;
const WINDOW = 60; // 60 Werte x 1 s = 1 Minute

const ring: number[] = [];
let lastTime: number | null = null;
let started = false;

/** Startet die Messung (idempotent). Kein Einfluss auf den Welttick. */
export function startEventLoopLagTracking(): void {
  if (started) return;
  started = true;
  lastTime = performance.now();
  setInterval(() => {
    const now = performance.now();
    const expected = (lastTime ?? now) + INTERVAL_MS;
    // frühere Ausführung (Timer-Jitter) zählt nicht als negatives Lag
    const lag = Math.max(0, now - expected);
    ring.push(lag);
    if (ring.length > WINDOW) ring.shift();
    lastTime = now;
  }, INTERVAL_MS);
}

/** Statistik des gleitenden Fensters (ms, gerundet auf 2 Nachkommastellen). */
export function loopLagStats(): { current: number; avg: number; max: number; samples: number } {
  if (ring.length === 0) return { current: 0, avg: 0, max: 0, samples: 0 };
  const sum = ring.reduce((a, b) => a + b, 0);
  let max = 0;
  for (const v of ring) if (v > max) max = v;
  return {
    current: Math.round(ring[ring.length - 1] * 100) / 100,
    avg: Math.round((sum / ring.length) * 100) / 100,
    max: Math.round(max * 100) / 100,
    samples: ring.length
  };
}
