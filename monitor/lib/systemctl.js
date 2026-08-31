// lib/systemctl.js — Verwaltung des Gameserver-Dienstes via sudo/systemctl
//
// Der Panel-Prozess läuft NICHT als root. Start/Stop/Restart laufen ausschließlich
// über `sudo -n` (non-interaktiv) auf die in deploy/sudoers/andora-monitor
// definierte, beschränkte Regel. Es wird NIEMALS ein beliebiger Befehl ausgeführt.

const { execFile } = require('child_process');
const config = require('../config');

const ALLOWED_ACTIONS = ['is-active', 'start', 'stop', 'restart', 'status'];

function sudo(args, timeoutMs = 10000) {
  return new Promise((resolve, reject) => {
    execFile('sudo', ['-n', ...args], { timeout: timeoutMs, shell: false }, (err, stdout, stderr) => {
      if (err) {
        const msg = (stderr ? String(stderr).trim() : '') || err.message;
        reject(new Error('systemctl action fehlgeschlagen: ' + msg));
        return;
      }
      resolve({ stdout: String(stdout), stderr: String(stderr) });
    });
  });
}

async function runSystemctl(action, unit) {
  const a = ALLOWED_ACTIONS.includes(action) ? action : null;
  if (!a) throw new Error('Ungültige Aktion: ' + action);
  const u = typeof unit === 'string' && unit.length > 0 ? unit : config.gameServerService;
  if (!u.endsWith('.service')) throw new Error('Ungültige Unit: ' + u);
  // is-active darf ohne sudo abgefragt werden; Control-Operationen nur via sudo.
  if (a === 'is-active') {
    return new Promise((resolve, reject) => {
      execFile('systemctl', [a, u], { timeout: 10000, shell: false }, (err, stdout, stderr) => {
        if (err) {
          const msg = (stderr ? String(stderr).trim() : '') || err.message;
          reject(new Error(msg));
          return;
        }
        resolve({ stdout: String(stdout), stderr: String(stderr) });
      });
    });
  }
  return sudo(['systemctl', a, u], 10000);
}

/** Liefert true, wenn der Dienst aktuell aktiv ist. */
async function isServiceActive() {
  try {
    const out = await runSystemctl('is-active', config.gameServerService);
    const state = out.stdout.trim().split('\n')[0].trim();
    return state === 'active' || state === 'activating' || state === 'reloading';
  } catch {
    return false;
  }
}

/** Kurzer Status-String (active/inactive/failed/unknown). */
async function serviceState() {
  try {
    const out = await runSystemctl('is-active', config.gameServerService);
    return out.stdout.trim().split('\n')[0].trim() || 'unknown';
  } catch {
    return 'unknown';
  }
}

/**
 * Prüft, ob die sudo-Regel für den Panel-User installiert ist.
 * Wichtig für die Deploy-Anleitung: ohne die Datei in /etc/sudoers.d
 * funktionieren Start/Stop/Restart nicht.
 */
function sudoRuleInstalled() {
  try {
    const fs = require('fs');
    return fs.existsSync(config.sudoersPath);
  } catch {
    return false;
  }
}

module.exports = { runSystemctl, isServiceActive, serviceState, sudoRuleInstalled };
