<?php
// lib/agent.php — HTTP-Client für den Andora-Agent (src/agent)
// Versionierte Management-API (Token-Auth via X-Andora-Token)
// Stream-Socket-Basis (IPv4/IPv6), konsistent zum Panel (http_get_json).

/**
 * Agent-Konfiguration aus panel_config() extrahieren.
 */
function agent_config(array $cfg): array {
    $a = $cfg['agent'] ?? [];
    return [
        'url' => trim((string)($a['url'] ?? '')),
        'token' => trim((string)($a['token'] ?? '')),
        'timeoutMs' => (int)($a['timeoutMs'] ?? 2500),
        'serviceKey' => trim((string)($a['serviceKey'] ?? 'realm')),
    ];
}

/**
 * Agent aktiv, wenn ANDORA_AGENT_URL gesetzt.
 */
function agent_enabled(array $cfg): bool {
    $a = agent_config($cfg);
    return $a['url'] !== '';
}

/**
 * Generischer HTTP-Request gegen den Agent.
 * GET/POST, X-Andora-Token Header, JSON-Antwort, fail-closed.
 * Returns ['ok' => bool, 'http' => int|null, 'data' => array|null, 'error' => string|null].
 */
function agent_request(string $method, array $acfg, string $path): array {
    $url = rtrim($acfg['url'], '/') . $path;
    $parsed = parse_url($url);
    if (!$parsed || !isset($parsed['host'], $parsed['port'])) {
        return ['ok' => false, 'http' => null, 'data' => null, 'error' => 'Ungültige Agent-URL'];
    }
    $host = $parsed['host'];
    $port = $parsed['port'];
    $reqPath = ($parsed['path'] ?? '/') . (isset($parsed['query']) ? '?' . $parsed['query'] : '');

    // IPv6-Literale klammern (kompatibel zu http_get_json)
    if (strpos($host, ':') !== false && $host[0] !== '[') {
        $host = '[' . $host . ']';
    }

    $fp = @stream_socket_client("tcp://$host:$port", $errno, $errstr, $acfg['timeoutMs'] / 1000, STREAM_CLIENT_CONNECT);
    if (!$fp) {
        return ['ok' => false, 'http' => null, 'data' => null, 'error' => 'Agent nicht erreichbar: ' . $errstr];
    }

    $body = '';
    $req = "$method $reqPath HTTP/1.1\r\nHost: $host:$port\r\nConnection: close\r\n";
    if ($acfg['token'] !== '') {
        $req .= 'X-Andora-Token: ' . $acfg['token'] . "\r\n";
    }
    if ($method === 'POST') {
        $body = '{}';
        $req .= "Content-Type: application/json\r\nContent-Length: " . strlen($body) . "\r\n";
    }
    $req .= "\r\n" . $body;
    if (@fwrite($fp, $req) === false) {
        @fclose($fp);
        return ['ok' => false, 'http' => null, 'data' => null, 'error' => 'Agent-Anfrage fehlgeschlagen'];
    }

    $response = '';
    $start = microtime(true);
    while (!feof($fp) && (microtime(true) - $start) < ($acfg['timeoutMs'] / 1000) + 2) {
        $chunk = @fread($fp, 8192);
        if ($chunk === '') break;
        $response .= $chunk;
    }
    @fclose($fp);

    // Statuscode aus Statuszeile
    $code = null;
    $pos = strpos($response, "\r\n");
    if ($pos !== false && preg_match('#^HTTP/[\d.]+ (\d{3})#', substr($response, 0, $pos), $m)) {
        $code = (int)$m[1];
    }
    // Body nach Headern
    $body = '';
    $hb = strpos($response, "\r\n\r\n");
    if ($hb !== false) {
        $body = substr($response, $hb + 4);
    }

    $data = json_decode($body, true);
    if (json_last_error() !== JSON_ERROR_NONE || !is_array($data)) {
        $data = null;
    }
    if ($code === null || $code >= 400) {
        return ['ok' => false, 'http' => $code, 'data' => $data, 'error' => $code === null ? 'Keine gültige HTTP-Antwort' : 'Agent: HTTP ' . $code];
    }
    return ['ok' => true, 'http' => $code, 'data' => $data, 'error' => null];
}

/**
 * Agent-Detailstatus für den konfigurierten Service-Key.
 * Returns normiertes Array (connected=false bei Fehler, fail-closed).
 */
function agent_service_status(array $cfg): array {
    $acfg = agent_config($cfg);
    $r = agent_request('GET', $acfg, '/api/v1/services/' . urlencode($acfg['serviceKey']));
    if (!$r['ok']) {
        return ['connected' => false, 'key' => $acfg['serviceKey'], 'error' => $r['error'] ?? 'Agent offline'];
    }
    $d = $r['data'] ?? [];
    return [
        'connected' => true,
        'key' => $d['key'] ?? $acfg['serviceKey'],
        'unit' => $d['unit'] ?? null,
        'state' => $d['state'] ?? 'unknown',
        'active' => !empty($d['active']),
        'healthy' => !empty($d['healthy']),
        'version' => $d['version'] ?? 'unknown',
        'health_url' => $d['health_url'] ?? null,
    ];
}

/**
 * Führt eine Agent-Aktion aus (start/stop/restart).
 * Returns ['ok', 'action', 'state', 'active', 'key', 'unit', 'error'].
 */
function agent_service_action(array $cfg, string $action): array {
    $acfg = agent_config($cfg);
    if (!in_array($action, ['start', 'stop', 'restart'], true)) {
        return ['ok' => false, 'action' => $action, 'error' => 'Ungültige Aktion'];
    }
    $r = agent_request('POST', $acfg, '/api/v1/services/' . urlencode($acfg['serviceKey']) . '/' . $action);
    if (!$r['ok']) {
        return ['ok' => false, 'action' => $action, 'error' => $r['error'] ?? ('Agent: HTTP ' . $r['http']), 'key' => $acfg['serviceKey']];
    }
    $d = $r['data'] ?? [];
    $state = $d['state'] ?? 'unknown';
    return [
        'ok' => true,
        'action' => $action,
        'state' => $state,
        'active' => $state === 'active',
        'key' => $d['key'] ?? $acfg['serviceKey'],
        'unit' => $d['unit'] ?? null,
        'error' => null,
    ];
}

/**
 * Agent-Übersichtsliste (alle verwalteten Dienste) oder fail-closed leer.
 * Returns ['connected' => bool, 'services' => array, 'error' => string|null].
 */
function agent_service_list(array $cfg): array {
    $acfg = agent_config($cfg);
    $r = agent_request('GET', $acfg, '/api/v1/services');
    if (!$r['ok']) {
        return ['connected' => false, 'services' => [], 'error' => $r['error'] ?? 'Agent offline'];
    }
    $services = $r['data']['services'] ?? [];
    return ['connected' => true, 'services' => is_array($services) ? $services : [], 'error' => null];
}