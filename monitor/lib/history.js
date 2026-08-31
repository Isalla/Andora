// lib/history.js — kurzer Metrik-Verlauf im Arbeitsspeicher (Ring-Buffer)
//
// Bewusst klein gehalten: ~360 Punkte à 4-5 Werte. Für die erste Implementierung
// reicht In-Memory; kein SQLite, kein zusätzliches Monitoring-System.

const CAP = 360; // 1 Punkte pro 10 s -> 1 Stunde Verlauf

const buf = [];

/** Fügt einen Zeitpunkt ein (ts in ms). Alte Punkte fallen raus. */
function push(point) {
  buf.push(point);
  while (buf.length > CAP) buf.shift();
}

/** Liefert die gespeicherten Punkte (alt -> neu). */
function list() {
  return buf.slice();
}

module.exports = { push, list, CAP };
