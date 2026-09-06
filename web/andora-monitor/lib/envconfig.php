<?php
// lib/envconfig.php — Lese-/Schreib-Logik für config.env (Whitelist-basiert)
// Verfugbar via require_once im Front-Controller

/**
 * Lies die config.env Datei (Key=Value, # Kommentare).
 */
function read_env_file(string $path): array {
    $out = [];
    if (!file_exists($path)) return $out;
    foreach (file($path, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES) as $line) {
        $t = trim($line);
        if (!$t || strpos($t, '#') === 0) continue;
        $eq = strpos($t, '=');
        if ($eq === false) continue;
        $key = trim(substr($t, 0, $eq));
        $val = trim(substr($t, $eq + 1));
        $out[$key] = $val;
    }
    return $out;
}

/** Antwortet für GET /api/config (nur sichtbare/Whitelist-Keys). */
function getConfigView(array $cfg): array {
    $env = read_env_file($cfg['serverConfigPath']);
    $visible = [];
    foreach ($cfg['configVisible'] as $k) {
        $visible[$k] = $env[$k] ?? '';
    }
    return $visible;
}

/**
 * Validiert geanderte Werte (serverseitig, hart).
 * Returns: { ok, error, changes } — changes nur wenn ok.
 */
function validateChanges(array $changes): array {
    $out = [];
    if (empty($changes)) {
        return ['ok' => true, 'error' => null, 'changes' => []];
    }
    foreach ($changes as $key => $raw) {
        if (!in_array($key, CONFIG_WHITELIST, true)) {
            return ['ok' => false, 'error' => 'Key nicht erlaubt: ' . $key, 'changes' => null];
        }
        $v = is_string($raw) ? $raw : (is_int($raw) ? (string)$raw : (is_float($raw) ? (string)$raw : ''));
        $v = trim($v);
        $err = validateValue($key, $v);
        if ($err !== null) return ['ok' => false, 'error' => $err . ' (' . $key . ')', 'changes' => null];
        $out[$key] = $v;
    }
    return ['ok' => true, 'error' => null, 'changes' => $out];
}

function validateValue(string $key, string $v): ?string {
    switch ($key) {
        case 'PORT_WS':
        case 'PORT_HTTP':
            if (!preg_match('/^\d+$/', $v) || +$v < 1 || +$v > 65535) return 'Port muss eine Zahl 1-65535 sein';
            return null;
        case 'TICK_MS':
            if (!preg_match('/^\d+$/', $v) || +$v < 16 || +$v > 1000) return 'TICK_MS muss zwischen 16 und 1000 ms sein';
            return null;
        case 'AOFB_RADIUS':
            if (!preg_match('/^\d+$/', $v) || +$v < 1 || +$v > 1000) return 'AOFB_RADIUS muss zwischen 1 und 1000 m sein';
            return null;
        case 'RENDER_CAP_DEFAULT':
            if (!preg_match('/^\d+$/', $v) || +$v < 1 || +$v > 512) return 'RENDER_CAP_DEFAULT muss zwischen 1 und 512 sein';
            return null;
        case 'OLLAMA_TIMEOUT_MS':
            if (!preg_match('/^\d+$/', $v) || +$v < 1000 || +$v > 120000) return 'OLLAMA_TIMEOUT_MS muss zwischen 1000 und 120000 ms sein';
            return null;
        case 'OLLAMA_NUM_CTX':
            if (!preg_match('/^\d+$/', $v) || +$v < 256 || +$v > 32768) return 'OLLAMA_NUM_CTX muss zwischen 256 und 32768 sein';
            return null;
        case 'OLLAMA_TOP_K':
            if (!preg_match('/^\d+$/', $v) || +$v < 1 || +$v > 256) return 'OLLAMA_TOP_K muss zwischen 1 und 256 sein';
            return null;
        case 'OLLAMA_TEMPERATURE': {
            $n = (float)$v;
            if (!is_finite($n) || $n < 0 || $n > 2) return 'OLLAMA_TEMPERATURE muss 0-2 sein';
            return null;
        }
        case 'OLLAMA_TOP_P': {
            $n = (float)$v;
            if (!is_finite($n) || $n <= 0 || $n > 1) return 'OLLAMA_TOP_P muss 0<x<=1 sein';
            return null;
        }
        case 'OLLAMA_TEMP_QUALITY': {
            $n = (float)$v;
            if (!is_finite($n) || $n < 0 || $n > 2) return 'OLLAMA_TEMP_QUALITY muss 0-2 sein';
            return null;
        }
        case 'OLLAMA_FALLBACK':
            if ($v !== '0' && $v !== '1') return "OLLAMA_FALLBACK muss '0' oder '1' sein";
            return null;
        case 'OLLAMA_URL':
            if (!preg_match('#^https?://.+#i', $v)) return 'OLLAMA_URL muss eine http(s)-URL sein';
            return null;
        case 'OLLAMA_MODEL':
        case 'OLLAMA_MODEL_QUALITY':
            // leere Zeile erlaubt = aus; sonst freier String (kein Secret)
            return null;
        case 'WS_BIND_HOST':
        case 'HEALTH_BIND_HOST':
            // Bind-Host: leere Zeile oder gueltige IP/Literal erlaubt
            if ($v !== '' && !filter_var($v, FILTER_VALIDATE_IP)) return 'Ungueltiger Bind-Host';
            return null;
        default:
            return 'Nicht erlaubter Key: ' . $key;
    }
}

/**
 * Schreibt geaenderte, validierte Values in config.env.
 * Kommentare und nicht-Whitelist-Keys bleiben unangetastet.
 */
function writeChanges(string $path, array $changes): array {
    if (!file_exists($path)) return ['ok' => false, 'error' => 'Config-Datei nicht vorhanden', 'written' => 0];
    $lines = file($path, FILE_IGNORE_NEW_LINES);
    $set = array_keys($changes);
    $present = [];
    $out = [];
    foreach ($lines as $line) {
        $t = trim($line);
        if (!$t || strpos($t, '#') === 0) { $out[] = $line; continue; }
        $eq = strpos($t, '=');
        if ($eq === false) { $out[] = $line; continue; }
        $key = trim(substr($t, 0, $eq));
        if (in_array($key, $set, true)) {
            $present[] = $key;
            $out[] = $key . '=' . $changes[$key];
        } else {
            $out[] = $line;
        }
    }
    foreach ($changes as $key => $val) {
        if (!in_array($key, $present, true)) $out[] = $key . '=' . $val;
    }
    $written = count($changes);
    $newContent = implode("\n", $out) . "\n";
    if (file_put_contents($path, $newContent) !== false) {
        return ['ok' => true, 'written' => $written];
    }
    return ['ok' => false, 'error' => 'Schreiben der Config-Datei fehlgeschlagen', 'written' => 0];
}

