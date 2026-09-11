<?php
// config.php — Panel-Konfiguration (Umgebungsvariablen, sensible Defaults)
// Kompatibel zu Node-Überwachungs-Logik; keine Secrets im Repository.

function andora_env_str(string $name, string $default): string {
    $v = getenv($name);
    return $v === false || $v === '' ? $default : $v;
}

function andora_env_int(string $name, int $default): int {
    $v = getenv($name);
    if ($v === false) return $default;
    $n = filter_var($v, FILTER_VALIDATE_INT);
    return $n === false ? $default : $n;
}

function andora_env_bool(string $name, bool $default): bool {
    $v = andora_env_str($name, '');
    return strtolower($v) === 'true' || $v === '1';
}

// Whitelist für editierbare config.env-Keys (Key-Strings, case-sensitive)
const CONFIG_WHITELIST = [
    'PORT_WS', 'PORT_HTTP',
    'WS_BIND_HOST', 'HEALTH_BIND_HOST',
    'TICK_MS', 'AOFB_RADIUS', 'RENDER_CAP_DEFAULT',
    'OLLAMA_URL', 'OLLAMA_MODEL', 'OLLAMA_MODEL_QUALITY',
    'OLLAMA_TIMEOUT_MS', 'OLLAMA_NUM_CTX',
    'OLLAMA_TEMPERATURE', 'OLLAMA_TOP_P', 'OLLAMA_TOP_K',
    'OLLAMA_FALLBACK', 'OLLAMA_TEMP_QUALITY'
];

// Sichtbare Keys (gleiche Whitelist, keine DB-Passwörter etc.)
const CONFIG_VISIBLE = [...CONFIG_WHITELIST];

// Projekt-Root ableiten (falls web/andora-monitor/webroot, sonst /)
$projectRoot = dirname(__DIR__, 2); // von web/andora-monitor nach root (repositorium)
$defaultServerConfig = $projectRoot . '/src/realm-rs/config.env';

// Config-Path kann per ANDORA_SERVER_CONFIG überschrieben werden
$serverConfigPath = andora_env_str('ANDORA_SERVER_CONFIG', $defaultServerConfig);
if (!file_exists($serverConfigPath)) {
    $serverConfigPath = $defaultServerConfig;
}

// Falls Datei nicht existiert, erzeugbare Basis-Struktur werfen (GET /api/config gibt dann leeres Array)
if (!file_exists($serverConfigPath)) {
    // Kein Fehler werfen, nur für GET /api/config sichtbar machen.
    // Schreiben geschieht via validateChanges + writeChanges prüfen wir erst bei POST.
}

// Hilfreich: URL-Host klammern für IPv6-Literale (kompatibel zu src/login/urlutil.go / src/realm/src/bind.ts)
// Erkennt einen unbracketen IPv6-Literal im Host-Teil von URLs (host:port -> [host]:port)
function bracket_url_host(string $raw): string {
    $schemeEnd = strpos($raw, '://');
    if ($schemeEnd === false) return $raw;
    $rest = substr($raw, $schemeEnd + 3);
    // Authority abtrennen (bis / ? #)
    $authEnd = strcspn($rest, '/?#'); // Position des ersten /, ?, #
    $auth = substr($rest, 0, $authEnd);
    $tail = substr($rest, $authEnd);
    if ($auth === '' || $auth[0] === '[' || strpos($auth, ':') === false) return $raw;
    // Letztes ':' trennt Host von Port
    $li = strrpos($auth, ':');
    if ($li > 0) {
        $hostPart = substr($auth, 0, $li);
        $portPart = substr($auth, $li + 1);
        if ($portPart !== '' && preg_match('/^\d+$/', $portPart) && is_ipv6_literal($hostPart)) {
            return substr($raw, 0, $schemeEnd + 3) . '[' . $hostPart . ']' . ':' . $portPart . $tail;
        }
    }
    if (is_ipv6_literal($auth)) {
        return substr($raw, 0, $schemeEnd + 3) . '[' . $auth . ']' . $tail;
    }
    return $raw;
}

// Hilfreich: echte IPv6-Literal erkennen (FILTER_FLAG_IPV6 ist strenger als Node-RegEx)
function is_ipv6_literal(string $s): bool {
    if (!preg_match('/^[0-9a-f:]+$/i', $s)) return false;
    return filter_var($s, FILTER_VALIDATE_IP, FILTER_FLAG_IPV6) !== false;
}

// Konfigurations-Array zurückliefern
function panel_config(): array {
    global $projectRoot, $serverConfigPath;
    static $cache = null;
    if ($cache !== null) return $cache;

    $token = andora_env_str('ANDORA_MONITOR_TOKEN', '');
    $bindHost = andora_env_str('ANDORA_MONITOR_BIND', '127.0.0.1');
    $bindPort = andora_env_int('ANDORA_MONITOR_PORT', 3003);
    $gameServerUrl = andora_env_str('ANDORA_GAME_SERVER_URL', 'http://127.0.0.1:3002');
    $gameServerService = andora_env_str('ANDORA_SERVICE_NAME', 'andora-realm.service');
    $sudoUser = andora_env_str('ANDORA_SUDO_USER', getenv('USER') ?: 'pi');
    $sudoUnit = andora_env_str('ANDORA_SUDO_UNIT', 'andora-monitor');
    $sudoDir = andora_env_str('ANDORA_SUDO_DIR', '/etc/sudoers.d');

    // Andora-Agent (Management-API, src/agent); leer = deaktiviert (Legacy-Fallback)
    $agentUrl = andora_env_str('ANDORA_AGENT_URL', '');
    $agentToken = andora_env_str('ANDORA_AGENT_TOKEN', '');
    $agentServiceKey = andora_env_str('ANDORA_AGENT_SERVICE_KEY', 'realm');
    $agentTimeoutMs = andora_env_int('ANDORA_AGENT_TIMEOUT_MS', 2500);

    // Listen-Pläne aus Bind-Host ableiten (entsprechend Node listenPlans)
    $listenPlans = listen_plans($bindHost);

    $cfg = [
        'projectRoot' => $projectRoot,
        'serverConfigPath' => $serverConfigPath,
        'gameServerUrl' => bracket_url_host($gameServerUrl),
        'gameServerService' => $gameServerService,
        'bindHost' => $bindHost,
        'bindPort' => $bindPort,
        'token' => $token,
        'listenPlans' => $listenPlans,
        'configWhitelist' => CONFIG_WHITELIST,
        'configVisible' => CONFIG_VISIBLE,
        'agent' => [
            'url' => $agentUrl,
            'token' => $agentToken,
            'serviceKey' => $agentServiceKey,
            'timeoutMs' => $agentTimeoutMs,
        ],
        'sudo' => [
            'user' => $sudoUser,
            'unit' => $sudoUnit,
            'dir' => $sudoDir,
        ],
        'sudoersPath' => $sudoDir . '/' . $sudoUnit,
    ];
    $cache = $cfg;
    return $cfg;
}

// Liefert die Listener-Pläne basierend auf Bind-Host (analog listenPlans aus Node)
function listen_plans(string $host): array {
    $h = trim(strtolower($host));
    switch ($h) {
        case '':
        case 'auto':
            return [['host' => '127.0.0.1']];
        case 'ipv4':
        case '4':
            return [['host' => '0.0.0.0']];
        case 'ipv6':
        case '6':
            return [['host' => '[::]', 'ipv6Only' => true]];
        case 'dual':
        case 'both':
            return [['host' => '0.0.0.0'], ['host' => '[::]', 'ipv6Only' => true]];
        default:
            // Literal oder Hostname; IPv6 Literal ggf. klammern prüfen
            $h = trim($host);
            if (preg_match('/^\[.*\]$/', $h)) {
                // Bereits geklammert: Host als IPv6 Literal behandeln
                return [['host' => $h, 'ipv6Only' => true]];
            }
            if (is_ipv6_literal(preg_replace('/^\[/', '', preg_replace('/\]$/', '', $h)))) {
                return [['host' => $h, 'ipv6Only' => true]];
            }
            return [['host' => $h]];
    }
}

