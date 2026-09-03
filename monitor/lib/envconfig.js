// lib/envconfig.js — Lese-/Schreib-Logik für src/realm/config.env (Whitelist-basiert)
const fs = require('fs');
const path = require('path');
const config = require('../config');

/** Parses src/realm/config.env wie der Server-Satz (Key=Value, # Kommentare). */
function readEnvFile(p) {
  const out = {};
  if (!fs.existsSync(p)) return out;
  for (const line of fs.readFileSync(p, 'utf8').split('\n')) {
    const t = line.trim();
    if (!t || t.startsWith('#')) continue;
    const eq = t.indexOf('=');
    if (eq === -1) continue;
    out[t.slice(0, eq).trim()] = t.slice(eq + 1).trim();
  }
  return out;
}

/** Antwortet für GET /api/config (nur sichtbare/Whitelist-Keys). */
function getConfigView() {
  const env = readEnvFile(config.serverConfigPath);
  const visible = {};
  for (const k of config.configVisible) {
    visible[k] = (k in env) ? env[k] : '';
  }
  return visible;
}

/**
 * Validiert geänderte Werte (serverseitig, hart).
 * Returns: { ok, error, changes } — changes nur wenn ok.
 */
function validateChanges(changes) {
  const out = {};
  if (typeof changes !== 'object' || changes === null || Array.isArray(changes)) {
    return { ok: false, error: 'Ungültiges Changes-Format' };
  }
  for (const [key, raw] of Object.entries(changes)) {
    if (!config.configWhitelist.includes(key)) {
      return { ok: false, error: 'Key nicht erlaub: ' + key };
    }
    const v = String(raw === undefined || raw === null ? '' : raw).trim();
    const err = validateValue(key, v);
    if (err) return { ok: false, error: err + ' (' + key + ')' };
    out[key] = v;
  }
  return { ok: true, error: null, changes: out };
}

function validateValue(key, v) {
  switch (key) {
    case 'PORT_WS':
    case 'PORT_HTTP':
      if (!/^\d+$/.test(v) || +v < 1 || +v > 65535) return 'Port muss eine Zahl 1-65535 sein';
      return null;
    case 'TICK_MS':
      if (!/^\d+$/.test(v) || +v < 16 || +v > 1000) return 'TICK_MS muss zwischen 16 und 1000 ms sein';
      return null;
    case 'AOFB_RADIUS':
      if (!/^\d+$/.test(v) || +v < 1 || +v > 1000) return 'AOFB_RADIUS muss zwischen 1 und 1000 m sein';
      return null;
    case 'RENDER_CAP_DEFAULT':
      if (!/^\d+$/.test(v) || +v < 1 || +v > 512) return 'RENDER_CAP_DEFAULT muss zwischen 1 und 512 sein';
      return null;
    case 'OLLAMA_TIMEOUT_MS':
      if (!/^\d+$/.test(v) || +v < 1000 || +v > 120000) return 'OLLAMA_TIMEOUT_MS muss zwischen 1000 und 120000 ms sein';
      return null;
    case 'OLLAMA_NUM_CTX':
      if (!/^\d+$/.test(v) || +v < 256 || +v > 32768) return 'OLLAMA_NUM_CTX muss zwischen 256 und 32768 sein';
      return null;
    case 'OLLAMA_TOP_K':
      if (!/^\d+$/.test(v) || +v < 1 || +v > 256) return 'OLLAMA_TOP_K muss zwischen 1 und 256 sein';
      return null;
    case 'OLLAMA_TEMPERATURE': {
      const n = Number(v);
      if (!Number.isFinite(n) || n < 0 || n > 2) return 'OLLAMA_TEMPERATURE muss 0-2 sein';
      return null;
    }
    case 'OLLAMA_TOP_P': {
      const n = Number(v);
      if (!Number.isFinite(n) || n <= 0 || n > 1) return 'OLLAMA_TOP_P muss 0<x<=1 sein';
      return null;
    }
    case 'OLLAMA_TEMP_QUALITY': {
      const n = Number(v);
      if (!Number.isFinite(n) || n < 0 || n > 2) return 'OLLAMA_TEMP_QUALITY muss 0-2 sein';
      return null;
    }
    case 'OLLAMA_FALLBACK':
      if (v !== '0' && v !== '1') return "OLLAMA_FALLBACK muss '0' oder '1' sein";
      return null;
    case 'OLLAMA_URL':
      if (!/^https?:\/\/.+/i.test(v)) return 'OLLAMA_URL muss eine http(s)-URL sein';
      return null;
    case 'OLLAMA_MODEL':
    case 'OLLAMA_MODEL_QUALITY':
      // leere Zeile erlaubt = aus; sonst freier String (kein Secret)
      return null;
    default:
      return 'Nicht erlaubter Key: ' + key;
  }
}

/**
 * Schreibt geänderte, validierte Values in src/realm/config.env.
 * Kommentare und nicht-Whitelist-Keys (DB*, Passwörter …) bleiben unangetastet.
 */
function writeChanges(changes) {
  const p = config.serverConfigPath;
  if (!fs.existsSync(p)) throw new Error('src/realm/config.env existiert nicht');
  const lines = fs.readFileSync(p, 'utf8').split('\n');
  const set = new Set(Object.keys(changes));
  const present = new Set();
  const out = lines.map((line) => {
    const t = line.trim();
    if (!t || t.startsWith('#')) return line;
    const eq = t.indexOf('=');
    if (eq === -1) return line;
    const key = t.slice(0, eq).trim();
    if (set.has(key)) {
      present.add(key);
      return key + '=' + changes[key];
    }
    return line;
  });
  for (const key of Object.keys(changes)) {
    if (!present.has(key)) out.push(key + '=' + changes[key]);
  }
  fs.writeFileSync(p, out.join('\n') + (out[out.length - 1].endsWith('\n') ? '' : '\n'));
  return Object.keys(changes);
}

module.exports = { readEnvFile, getConfigView, validateChanges, writeChanges };
