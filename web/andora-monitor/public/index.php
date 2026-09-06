<?php
/**
 * index.php — Front-Controller für das Andora-Monitoring-Panel
 * Einzige Entry Point für alle Requests (GET /, /api/* etc.)
 * Wird vom PHP-Built-In-Server (php -S) oder von Apache/Nginx als Router genutzt.
 *
 * Verhalten entspricht dem Node-Original (server.js), ist aber
 * fehlertolerant (fail-closed), token-geschützt und IPv6-kompatibel.
 */

// Bootstrap: config + Libs laden
$ROOT = __DIR__ . '/../';
require_once $ROOT . 'lib/config.php';
$cfg = panel_config();
require_once $ROOT . 'lib/envconfig.php';

// Fehlerbehandlung: alle Outputs als JSON, keine rohen Meldungen an Client
// Warnings/Notices werden geloggt und nicht als 500 beantwortet (z.B. @-unterdrückte
// Verbindungsfehler -> fail-closed offline). Uncaught Exceptions -> JSON 500.
set_error_handler(function ($severity, $message, $file, $line) {
    error_log('PHP-Monitor-Fehler: ' . $message . ' in ' . $file . ':' . $line);
    return true;
});
set_exception_handler(function (\Throwable $e) {
    error_log('PHP-Monitor-Exception: ' . $e->getMessage() . ' in ' . $e->getFile() . ':' . $e->getLine());
    http_response_code(500);
    header('Content-Type: application/json; charset=utf-8');
    echo json_encode(['ok' => false, 'error' => 'Interner Fehler']);
    exit(1);
});

// Token-Prüfung: Header 'x-api-token' (case-insensitive) oder Query-Param 'token'
function token_ok(string $header, string $query, string $cfgToken): bool {
    if (!$cfgToken) return true; // Kein Token gesetzt -> offen
    $h = trim($header);
    $q = trim($query);
    return hash_equals($cfgToken, $h) || hash_equals($cfgToken, $q);
}

// Request-URI parsen
$uri = $_SERVER['REQUEST_URI'] ?? '/';
$path = parse_url($uri, PHP_URL_PATH) ?? '/';
$query = parse_url($uri, PHP_URL_QUERY) ?? '';
$qsArr = [];
if ($query) parse_str($query, $qsArr);
$tokenFromQs = $qsArr['token'] ?? '';

// Header holen (HTTP-Header-Namen sind case-insensitive in $_SERVER)
$headers = getallheaders();
$headerToken = '';
foreach ($headers as $name => $value) {
    if (strtolower($name) === 'x-api-token') {
        $headerToken = $value;
        break;
    }
}

// Request-Methode
$method = $_SERVER['REQUEST_METHOD'] ?? 'GET';

// Antwort-Hilfen
function send_json(int $code, array $payload): void {
    header('Content-Type: application/json; charset=utf-8');
    http_response_code($code);
    echo json_encode($payload, JSON_UNESCAPED_UNICODE);
    exit();
}

// ---------- Routing ----------
$tokenOk = token_ok($headerToken, $tokenFromQs, $cfg['token']);

// Token-Schutz für alle API-Routen (fail-closed)
$isApi = str_starts_with($path, '/api/');
if ($cfg['token'] && $isApi && !$tokenOk) {
    send_json(401, ['ok' => false, 'error' => 'Token erforderlich']);
    exit();
}

// Route nach Pfad und Methode
switch ($path) {
    case '/':
        // Dashboard HTML (statisch, Token ggf. prüfen)
        if (!$tokenOk) {
            send_json(401, ['ok' => false, 'error' => 'Token erforderlich']);
        }
        // index.html einlesen und ausliefern
        $htmlPath = $ROOT . 'public/index.html';
        if (!file_exists($htmlPath)) {
            send_json(404, ['ok' => false, 'error' => 'Dashboard nicht gefunden']);
        }
        $html = file_get_contents($htmlPath);
        header('Content-Type: text/html; charset=utf-8');
        echo $html;
        exit();

    case '/api/status':
        // Game-Server-Status abfragen (via stream_socket_client)
        // Fail-closed: Verbindungsfehler -> offline
        $status = fetch_game_status($cfg['gameServerUrl'], 2500);
        $players = fetch_players_list($cfg['gameServerUrl'], 2500);
        
        // History-Punkt mit echten Werten des Serverstatus aufnehmen (5-Sekunden-Drosselung)
        $point = [
            'ts' => microtime(true) * 1000,
            'online' => !empty($status) && ($status['ok'] ?? false),
            'players' => is_numeric($status['players'] ?? null) ? (int)$status['players'] : 0,
            'cpu_percent' => is_numeric($status['cpu_percent'] ?? null) ? (float)$status['cpu_percent'] : 0,
            'heap_mb' => is_numeric($status['heap_mb'] ?? null) ? (float)$status['heap_mb'] : 0,
            'tick_avg_ms' => is_numeric($status['tick']['avg_ms'] ?? null) ? (float)$status['tick']['avg_ms'] : 0,
            'tick_max_ms' => is_numeric($status['tick']['max_ms'] ?? null) ? (float)$status['tick']['max_ms'] : 0,
            'loop_lag_ms' => is_numeric($status['event_loop']['max_lag_ms'] ?? null) ? (float)$status['event_loop']['max_lag_ms'] : 0,
        ];
        // History-Punkt mit 5-Sekunden-Drosselung atomar aufnehmen
        write_history_point($ROOT, $point, 5000);
        
        // Systemd-Status prüfen (ohne sudo, falls is-active)
        $isActive = check_service_active($cfg['gameServerService']);
        $sudoOk = check_sudo_rule_installed($cfg['sudoersPath']);
        
        send_json(200, [
            'ok' => true,
            'panel' => [
                'game_server_service' => $cfg['gameServerService'],
                'sudo_rule_installed' => $sudoOk,
                'token_required' => !!$cfg['token']
            ],
            'systemd' => [
                'state' => $isActive ? 'active' : 'inactive',
                'active' => $isActive
            ],
            'game_server' => $status ?? ['offline' => true],
            'player_list' => $players
        ]);
        exit();

    case '/api/history':
        // Verlauf der letzten Punkte zurückliefern
        $points = read_history($ROOT);
        // Nur die letzten ~360 Punkte (CAP) behalten; Client filtert auf 10 Min.
        send_json(200, ['ok' => true, 'points' => $points]);
        exit();

    case '/api/control':
        // Steuerung: start/stop/restart (erfordert confirm=true)
        if ($method !== 'POST') {
            send_json(404, ['ok' => false, 'error' => 'Not found']); // Node-Verhalten
        }
        // Rohdaten lesen (Body)
        $body = '';
        if (!empty($_SERVER['CONTENT_LENGTH'])) {
            $body = file_get_contents('php://input');
        }
        $data = [];
        if (!empty($body)) {
            $data = json_decode($body, true) ?? [];
        }
        $action = trim(strtolower($data['action'] ?? ''));
        $confirm = !empty($data['confirm']) && $data['confirm'] === true;

        $allowed = ['start', 'stop', 'restart'];
        if (!in_array($action, $allowed) || !$confirm) {
            send_json(400, ['ok' => false, 'error' => 'Ungültige Aktion oder Bestätigung fehlt (start/stop/restart, confirm=true)']);
        }
        // systemctl ausführen (sudo -n)
        $out = run_systemctl($action, $cfg['gameServerService']);
        if ($out['exitCode'] !== 0) {
            error_log('systemctl ' . $action . ' fehlgeschlagen, exitCode=' . $out['exitCode'] . ' stderr=' . $out['stderr']);
            send_json(500, ['ok' => false, 'error' => 'Aktion fehlgeschlagen']);
            exit();
        }
        $output = trim($out['stdout'] ?? '');
        send_json(200, ['ok' => true, 'action' => $action, 'output' => $output]);
        exit();

    case '/api/config':
        // Config-Abfrage oder -Änderung
        if ($method === 'GET') {
            // Nur sichtbare/Whitelist-Keys zurückliefern
            $visible = getConfigView($cfg);
            send_json(200, ['ok' => true, 'config' => $visible]);
        } elseif ($method === 'POST') {
            // Body lesen und validieren
            $body = '';
            if (!empty($_SERVER['CONTENT_LENGTH'])) {
                $body = file_get_contents('php://input');
            }
            $data = json_decode($body, true);
            if (!is_array($data)) {
                send_json(400, ['ok' => false, 'error' => 'Ungültiges JSON']);
            }
            $changes = $data['changes'] ?? [];
            if (!is_array($changes)) {
                send_json(400, ['ok' => false, 'error' => 'Ungültiges Changes-Format']);
            }
            // Validierung (hard)
            $check = validateChanges($changes);
            if (!$check['ok']) {
                send_json(400, ['ok' => false, 'error' => $check['error']]);
            }
            // Änderungen schreiben (atomic)
            $written = writeChanges($cfg['serverConfigPath'], $check['changes']);
            if (!$written['ok']) {
                send_json(500, ['ok' => false, 'error' => $written['error']]);
            }
            send_json(200, ['ok' => true, 'written' => $written['written'], 'note' => 'Config gespeichert. Neueste Werte greifen beim nächsten Neustart.']);
        } else {
            send_json(404, ['ok' => false, 'error' => 'Not found']);
        }
        exit();

    default:
        // Unbekannter Pfad -> 404 JSON (entspricht Node-Verhalten)
        send_json(404, ['ok' => false, 'error' => 'Not found']);
        exit();
}

// ---------- Hilfsfunktionen (am Dateiende platziert, damit Switch sie nutzen kann) ----------

/**
 * Holt per HTTP GET ein JSON vom Gameserver (Socket-basiert, IPv6-fähig).
 * Liefert assoziatives Array oder null bei Fehler.
 */
function http_get_json(string $url, int $timeoutMs = 2500): ?array {
    $parsed = parse_url($url);
    if (!$parsed || !isset($parsed['host'], $parsed['port'])) return null;
    $host = $parsed['host'];
    $port = $parsed['port'];
    $reqPath = $parsed['path'] ?? '/';

    // Für IPv6 im Stream Kontext ggf. Klammern sicherstellen
    if (strpos($host, ':') !== false && $host[0] !== '[') {
        $host = '[' . $host . ']';
    }

    $fp = @stream_socket_client("tcp://$host:$port", $errno, $errstr, $timeoutMs / 1000, STREAM_CLIENT_CONNECT);
    if (!$fp) return null;

    // Request senden
    $req = "GET $reqPath HTTP/1.1\r\nHost: $host:$port\r\nConnection: close\r\n\r\n";
    if (@fwrite($fp, $req) === false) {
        @fclose($fp);
        return null;
    }

    // Antwort lesen (bis Ende der Header + body)
    $response = '';
    $start = microtime(true);
    while (!feof($fp) && (microtime(true) - $start) < ($timeoutMs / 1000) + 2) {
        $chunk = @fread($fp, 8192);
        if ($chunk === '') break;
        $response .= $chunk;
    }
    @fclose($fp);

    // Nur den Body nach den Headern extrahieren
    $pos = strpos($response, "\r\n\r\n");
    if ($pos === false) return null;
    $body = substr($response, $pos + 4);

    // JSON decodieren
    $data = json_decode($body, true);
    if (json_last_error() !== JSON_ERROR_NONE) return null;
    if (!isset($data['ok'])) return null;
    return $data;
}

/**
 * Holt den Gameserver-Status via GET /status.
 * Liefert assoziatives Array oder null bei Fehler.
 */
function fetch_game_status(string $url, int $timeoutMs = 2500): ?array {
    return http_get_json(rtrim($url, '/') . '/status', $timeoutMs);
}

/**
 * Holt die aktive Spielerliste via GET /players.
 * Liefert immer ein Array (leer bei Fehler/offline).
 */
function fetch_players_list(string $url, int $timeoutMs = 2500): array {
    $data = http_get_json(rtrim($url, '/') . '/players', $timeoutMs);
    $players = $data['players'] ?? [];
    return is_array($players) ? $players : [];
}

/**
 * Prüft, ob der Dienst aktuell aktiv ist (systemctl is-active, kein sudo).
 */
function check_service_active(string $unit): bool {
    // unit Namen validieren (nur alphanumerisch und .service)
    if (!preg_match('/^[A-Za-z0-9._-]+\.service$/', $unit)) return false;
    $out = exec("systemctl is-active $unit 2>/dev/null");
    return trim($out) === 'active';
}

/**
 * Prüft, ob die sudo-Regel für das Panel installiert ist.
 */
function check_sudo_rule_installed(string $path): bool {
    return is_file($path);
}

/**
 * Führt eine systemctl-Aktion via sudo -n aus (fail-closed).
 * Liefert ['stdout', 'stderr', 'exitCode'].
 */
function run_systemctl(string $action, string $unit): array {
    $allowed = ['is-active', 'start', 'stop', 'restart', 'status'];
    if (!in_array($action, $allowed)) {
        return ['stdout' => '', 'stderr' => 'Ungültige Aktion', 'exitCode' => 1];
    }
    // unit auf .service-Endung prüfen
    if (!preg_match('/^[A-Za-z0-9._-]+\.service$/', $unit)) {
        return ['stdout' => '', 'stderr' => 'Ungültige Unit', 'exitCode' => 1];
    }
    // sudo -n systemctl <action> <unit>
    $cmd = ['sudo', '-n', 'systemctl', $action, $unit];
    $out = proc_open($cmd, [
        0 => ['pipe', 'r'],
        1 => ['pipe', 'w'],
        2 => ['pipe', 'w']
    ], $pipes);
    if (!is_resource($out)) return ['stdout' => '', 'stderr' => 'proc_open fehlgeschlagen', 'exitCode' => 1];

    $stdout = stream_get_contents($pipes[1]);
    $stderr = stream_get_contents($pipes[2]);
    fclose($pipes[0]);
    fclose($pipes[1]);
    fclose($pipes[2]);
    $code = proc_close($out);

    return ['stdout' => trim($stdout), 'stderr' => trim($stderr), 'exitCode' => $code];
}

/**
 * Liest die gespeicherten History-Punkte aus der Daten-Datei (JSON, Ring-Buffer).
 */
function read_history(string $root): array {
    $dataDir = $root . '/data';
    $histFile = $dataDir . '/history.json';
    if (!file_exists($histFile)) return [];
    $json = @file_get_contents($histFile);
    if ($json === false) return [];
    $data = json_decode($json, true);
    if (json_last_error() !== JSON_ERROR_NONE) return [];
    // Auf max. CAP=360 Punkte beschneiden (alteste zuerst entfernen, da Array alt->neu)
    $cap = 360;
    $points = array_slice($data, -$cap);
    return $points;
}

/**
 * Schreibt einen neuen History-Punkt atomar in die Datei (mit flock
 * und 5-Sekunden-Drosselung). Throttle und CAP werden unter demselben
 * Lock geprüft, damit keine zwei gleichzeitigen Requests denselben
 * Zeitpunkt doppelt eintragen.
 */
function write_history_point(string $root, array $point, float $throttleMs = 5000, int $cap = 360): void {
    $dataDir = $root . '/data';
    $histFile = $dataDir . '/history.json';
    if (!is_dir($dataDir)) @mkdir($dataDir, 0700, true);
    $fh = @fopen($histFile, 'c+');
    if (!$fh) return;
    if (flock($fh, LOCK_EX)) {
        $existing = [];
        if (filesize($histFile) > 0) {
            $existing = json_decode(file_get_contents($histFile), true) ?? [];
        }
        if (!is_array($existing)) $existing = [];
        $lastTs = $existing ? (float)end($existing)['ts'] : 0;
        $nowMs = microtime(true) * 1000;
        if ($nowMs - $lastTs >= $throttleMs || empty($existing)) {
            $existing[] = $point;
            if (count($existing) > $cap) $existing = array_slice($existing, -$cap);
            ftruncate($fh, 0);
            rewind($fh);
            fwrite($fh, json_encode($existing, JSON_UNESCAPED_UNICODE));
        }
        flock($fh, LOCK_UN);
    }
    fclose($fh);
}