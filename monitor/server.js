#!/usr/bin/env node
// server.js — Unabhängiges Monitoring-/Admin-Panel (eigener Node-Prozess)
//
// Läuft getrennt vom Gameserver: ist selbst dann erreichbar, wenn der
// Gameserver down ist (Status rot + Neustart-Button angezeigt).
//
// Sicherheit:
//  - bindet standard nur auf 127.0.0.1 (ANDORA_MONITOR_BIND/PORT konfigurierbar)
//  - optional Token (ANDORA_MONITOR_TOKEN): bei gesetztem Token muss jeder
//    Request davon betroffen sein (Header 'x-api-token' oder ?token=)
//  - keine Secrets an den Client (nur Whitelist-Keys)
//  - nur feste Aktionen via systemctl (start/stop/restart), nie Shell-Frei
//  - gefährliche Aktionen (restart/stop) erst nach explizit Bestätigung

const http = require('http');
const fs = require('fs');
const path = require('path');
const config = require('./config');
const { push, list } = require('./lib/history');
const { runSystemctl, isServiceActive, serviceState, sudoRuleInstalled } = require('./lib/systemctl');
const { getConfigView, validateChanges, writeChanges } = require('./lib/envconfig');

const STATUS_POLL_MS = 5000;

/** Holt den Gameserver-Status (http GET mit Timeout). null = nicht erreichbar. */
function fetchGameStatus(timeoutMs = 2500) {
  return new Promise((resolve) => {
    const url = new URL('/status', config.gameServerUrl);
    const req = http.get(url, { timeout: timeoutMs }, (res) => {
      let body = '';
      res.on('data', (c) => { body += c; });
      res.on('end', () => {
        try { resolve(JSON.parse(body)); } catch { resolve(null); }
      });
    });
    req.on('timeout', () => { req.destroy(); resolve(null); });
    req.on('error', () => resolve(null));
  });
}

/** Zeichnet den aktuellen Zustand in den Verlauf (In-Memory, Ring-Buffer). */
async function recordHistoryPoint() {
  const status = await fetchGameStatus();
  const point = {
    ts: Date.now(),
    online: status ? !!status.ok : false,
    players: status ? status.players : 0,
    cpu_percent: status ? status.cpu_percent : 0,
    heap_mb: status ? status.heap_mb : 0,
    tick_avg_ms: status ? status.tick.avg_ms : 0,
    tick_max_ms: status ? status.tick.max_ms : 0,
    loop_lag_ms: status ? status.event_loop.max_lag_ms : 0
  };
  push(point);
}

function sendJson(res, code, obj) {
  res.writeHead(code, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify(obj));
}

function sendHtml(res, code, html) {
  res.writeHead(code, { 'Content-Type': 'text/html; charset=utf-8' });
  res.end(html);
}

function readBody(req, limit = 32768) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    req.on('data', (c) => {
      size += c.length;
      if (size > limit) { reject(new Error('Body zu groß')); req.destroy(); return; }
      chunks.push(c);
    });
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

function tokenOk(req) {
  if (!config.token) return true;
  const h = req.headers['x-api-token'];
  const u = new URL(req.url, 'http://x');
  const q = u.searchParams.get('token');
  return h === config.token || q === config.token;
}

/** Liest das Dashboard-HTML (monitor/public/index.html) ein. */
function dashboardHtml() {
  const p = path.join(__dirname, 'public', 'index.html');
  return fs.readFileSync(p, 'utf8');
}

const server = http.createServer(async (req, res) => {
  if (!tokenOk(req)) {
    sendJson(res, 401, { ok: false, error: 'Token erforderlich' });
    return;
  }

  const url = new URL(req.url, 'http://x');
  const pathname = url.pathname;

  try {
    if (pathname === '/' && req.method === 'GET') {
      sendHtml(res, 200, dashboardHtml());
      return;
    }

    if (pathname === '/api/status' && req.method === 'GET') {
      const [status, active, state, sudoOk] = await Promise.all([
        fetchGameStatus(),
        isServiceActive(),
        serviceState(),
        Promise.resolve(sudoRuleInstalled())
      ]);
      sendJson(res, 200, {
        ok: true,
        panel: {
          game_server_service: config.gameServerService,
          sudo_rule_installed: sudoOk,
          token_required: !!config.token
        },
        systemd: { state, active },
        game_server: status ? status : { offline: true }
      });
      return;
    }

    if (pathname === '/api/history' && req.method === 'GET') {
      sendJson(res, 200, { ok: true, points: list() });
      return;
    }

    if (pathname === '/api/control' && req.method === 'POST') {
      const body = JSON.parse((await readBody(req)) || '{}');
      const action = String(body.action || '');
      const confirm = body.confirm === true;
      if (!['start', 'stop', 'restart'].includes(action)) {
        sendJson(res, 400, { ok: false, error: 'Aktion ungültig (start/stop/restart)' });
        return;
      }
      if (!confirm) {
        // Gefährliche Aktionen erst nach explizit Bestätigung im UI
        sendJson(res, 400, { ok: false, error: 'Bestätigung fehlt: confirm=true setzen' });
        return;
      }
      const out = await runSystemctl(action, config.gameServerService);
      sendJson(res, 200, { ok: true, action, output: out.stdout.trim() });
      return;
    }

    if (pathname === '/api/config' && req.method === 'GET') {
      sendJson(res, 200, { ok: true, config: getConfigView() });
      return;
    }

    if (pathname === '/api/config' && req.method === 'POST') {
      const body = JSON.parse((await readBody(req)) || '{}');
      const check = validateChanges(body.changes || {});
      if (!check.ok) {
        sendJson(res, 400, { ok: false, error: check.error });
        return;
      }
      const written = writeChanges(check.changes);
      sendJson(res, 200, {
        ok: true,
        written: written,
        note: 'Config gespeichert. Neueste Werte greifen beim nächsten Neustart.'
      });
      return;
    }

    sendJson(res, 404, { ok: false, error: 'Not found' });
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    sendJson(res, 500, { ok: false, error: msg });
  }
});

server.listen(config.bindPort, config.bindHost, () => {
  console.log('Monitoring-Panel auf http://' + config.bindHost + ':' + config.bindPort +
    ' (Token: ' + (config.token ? 'erforderlich' : 'aus') + ')');
});

setInterval(recordHistoryPoint, STATUS_POLL_MS);
process.on('unhandledRejection', (e) => {
  console.error('unhandledRejection:', e && e.message ? e.message : e);
});
