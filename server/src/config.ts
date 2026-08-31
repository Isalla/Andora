// config.ts — Konfiguration aus server/config.env laden
import fs from 'fs';
import path from 'path';

export interface Env {
  [k: string]: string;
}

export function loadEnv(p: string): Env {
  const out: Env = {};
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

const env = loadEnv(path.join(__dirname, '..', 'config.env'));
const num = (k: string, d: number) => {
  const v = Number(env[k]);
  return Number.isFinite(v) && v > 0 ? v : d;
};

export const config = {
  wsPort: num('PORT_WS', 3001),
  healthPort: num('PORT_HTTP', 3002),
  tickInterval: num('TICK_MS', 100),
  aofbRadius: num('AOFB_RADIUS', 20),
  renderCap: num('RENDER_CAP_DEFAULT', 64),
  ollama: {
    url: env['OLLAMA_URL'] || 'http://192.168.1.32:11434',
    model: env['OLLAMA_MODEL'] || '',
    modelQuality: env['OLLAMA_MODEL_QUALITY'] || '',
    timeoutMs: num('OLLAMA_TIMEOUT_MS', 15000),
    numCtx: num('OLLAMA_NUM_CTX', 8192),
    temperature: Number(env['OLLAMA_TEMPERATURE'] || 0.8),
    topP: Number(env['OLLAMA_TOP_P'] || 0.95),
    topK: Number(env['OLLAMA_TOP_K'] || 64),
    temperatureQuality: Number(env['OLLAMA_TEMP_QUALITY'] || 1.0),
    fallback: env['OLLAMA_FALLBACK'] === '1'
  },
  db: {
    host: env['DB_HOST'] || 'localhost',
    port: num('DB_PORT', 3306),
    user: env['DB_USER'] || 'andora',
    password: env['DB_PASS'] || '',
    database: env['DB_NAME'] || 'andora'
  }
};
