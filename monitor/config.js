// config.js — Panel-Konfiguration (Umgebungsvariablen, sensible Defaults)
const path = require('path');

function envInt(name, d) {
  const v = process.env[name];
  if (v === undefined) return d;
  const n = parseInt(v, 10);
  return Number.isFinite(n) ? n : d;
}
function envStr(name, d) {
  const v = process.env[name];
  return v === undefined || v === '' ? d : v;
}

// Klammert einen ungeklammerten IPv6-Literal im URL-Host (host:port ->
// [host]:port), kompatibel zu src/login/urlutil.go / src/realm/src/bind.ts.
function bracketUrlHost(raw) {
  const schemeEnd = raw.indexOf('://');
  if (schemeEnd === -1) return raw;
  const rest = raw.slice(schemeEnd + 3);
  let authEnd = rest.length;
  const cut = rest.search(/[/?#]/);
  if (cut !== -1) authEnd = cut;
  const auth = rest.slice(0, authEnd);
  if (!auth || auth.startsWith('[') || !auth.includes(':')) return raw;
  const li = auth.lastIndexOf(':');
  if (li > 0) {
    const hostPart = auth.slice(0, li);
    const portPart = auth.slice(li + 1);
    if (portPart && /^\d+$/.test(portPart) && /^[0-9a-f:]+$/i.test(hostPart) && hostPart.includes(':')) {
      return `${raw.slice(0, schemeEnd + 3)}[${hostPart}]:${portPart}${rest.slice(authEnd)}`;
    }
  }
  if (/^[0-9a-f:]+$/i.test(auth) && auth.includes(':')) {
    return `${raw.slice(0, schemeEnd + 3)}[${auth}]${rest.slice(authEnd)}`;
  }
  return raw;
}

/** Whitelisted, über das Panel editierbare config.env-Keys. */
const CONFIG_WHITELIST = [
  'PORT_WS', 'PORT_HTTP',
  'WS_BIND_HOST', 'HEALTH_BIND_HOST',
  'TICK_MS', 'AOFB_RADIUS', 'RENDER_CAP_DEFAULT',
  'OLLAMA_URL', 'OLLAMA_MODEL', 'OLLAMA_MODEL_QUALITY',
  'OLLAMA_TIMEOUT_MS', 'OLLAMA_NUM_CTX',
  'OLLAMA_TEMPERATURE', 'OLLAMA_TOP_P', 'OLLAMA_TOP_K',
  'OLLAMA_FALLBACK', 'OLLAMA_TEMP_QUALITY'
];

/** Keys, die im Panel sichtbar sein dürfen (rohe Werte). DB* und Passwörter NICHT. */
const CONFIG_VISIBLE = [...CONFIG_WHITELIST];

const projectRoot = path.resolve(__dirname, '..');

module.exports = {
  projectRoot,
  serverConfigPath: path.join(projectRoot, 'src', 'realm', 'config.env'),
  serverDir: path.join(projectRoot, 'src', 'realm'),
  gameServerUrl: bracketUrlHost(envStr('ANDORA_GAME_SERVER_URL', 'http://127.0.0.1:3002')),
  gameServerService: envStr('ANDORA_SERVICE_NAME', 'andora-server.service'),
  bindHost: envStr('ANDORA_MONITOR_BIND', '127.0.0.1'),
  bindPort: envInt('ANDORA_MONITOR_PORT', 3003),
  token: envStr('ANDORA_MONITOR_TOKEN', ''),
  sudo: {
    user: envStr('ANDORA_SUDO_USER', process.env.USER || 'pi'),
    unit: envStr('ANDORA_SUDO_UNIT', 'andora-sudo-group'),
    dir: envStr('ANDORA_SUDO_DIR', '/etc/sudoers.d')
  },
  sudoersPath: path.join(envStr('ANDORA_SUDO_DIR', '/etc/sudoers.d'),
    envStr('ANDORA_SUDO_UNIT', 'andora-sudo-group')),
  configWhitelist: CONFIG_WHITELIST,
  configVisible: CONFIG_VISIBLE
};
