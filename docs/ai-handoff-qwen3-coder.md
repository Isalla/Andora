# Qwen3 Coder — Aufgaben-Übergabe (Andora MMO)

Du schreibst NUR den Code für die Tasks unten. Regeln:

- Server: Node.js 20 + TypeScript, Projekt in `server/`
  - `npm i` dann `npm run dev` (tsc + start)
  - DB-Konfig kommt aus `server/config.env` (erstellt aus `config.env.example`)
  - Tabellen: `server/db/schema.sql` (MariaDB 10, DB `andora`)
- Client: Godot 3.5 (NICHT Godot 4!). GDScript 2. i18n = Autoload `I18n`.
  - Niemals deutsche Texte hardcoden: `I18n.t("key", {"name": x})`
  - Keys: `i18n/de.json` + `i18n/en.json` (ergänzen: Key in BEIDE Dateien)
- Protokoll: `shared/protocol.js` (Node) + `shared/protocol.gd` (Godot)
  - IDs müssen identisch bleiben. Neue Message -> in BEIDE Dateien.
  - Framing: `{"seq": int, "type": int, "data": {...}}` per WebSocket-Text-Frame
- Pi 4 (8GB) ist Ziel-Client: 10 Hz Server-Tick, AOFB Radius 20m,
  RENDER_CAP=64 (4GB: 48), kein JSON-Payload > 200 Byte pro Paket

## Task M2: Server-Kern `server/src/main.ts`
Implementiere (TODO im main.ts):
1. ws-Server Port 3001, MariaDB-Pool (mysql2, promise)
2. C2S.HELLO -> Lade Charakter aus DB, sende S2C.WELCOME (data.you)
3. C2S.HEARTBEAT -> S2C.SYNC {ack_seq}
4. Welt-Tick 100 ms (setInterval): für jeden verbundenen Player
   C2S.MOVE einarbeiten -> Position, AOFB-Filter, Broadcast S2C.STATE (nur
   Clients im 20m-Radius)
5. Bei Disconnect: S2C.DESPAWN + Save (pos) nach DB
6. Fehlerhandling: kein Crash bei invalid JSON -> log + ignorieren

## Task M3: Godot-Client `src/client/`
1. `net_client.gd` (extends Node, WebSocketPeer): connect(), send(C2S.*),
   Queue, Decode via `Protocol.decode()`
2. `interp.gd`: Buffer (max 16 Einträge, 120 ms in Vergangenheit) + draw_pos()
3. `local_player.gd` (CharacterBody2D): Input -> MOVE-Paket, 1-Frame-Prediktion,
   Reconciliation bei S2C.SYNC (Difference ueber 150 ms weich addieren)
4. `entity_view.gd`: remote Entities, Interpolation, Animation nur bei Sicht
5. `render_cap.gd`: sortiert sichtbare Entities (Prio: own > guild > target >
   distanz), zeigt max. RENDER_CAP, Rest -> hide()
6. `perf_mode.gd`: FPS-Messung (Rolling), <15 FPS -> Level aufschrauben,
   S2C.PERFGO abarbeiten (0/1/2: Particles/Animationen/Chunks)

## Verifikation
- Server: `curl -s localhost:3001/health` -> {"ok":true} (add health-Endpoint via http-Modul, Port 3002)
- Client: 2 Pi/Window-Instanzen, beide sehen sich, 1 Player waehrend
  des Tests stirbt (Test-Command in DB), beide sehen S2C.KILL
- i18n: Sprache de<->en zur Laufzeit umschaltbar ohne Restart

## Stil
- TypeScript: strict, keine any, JSDoc-Kommentar pro exportierter Funktion
- GDScript: Godot-3-Syntax, Tab-Einrueckung, keine Kommentare ausser TODO
- Keine neuen Abhaengigkeiten ohne Nachfrage

## Task M6: Ollama-NPC-AI `server/src/ai/ollama.ts`
- Zwei Modelles aus config.env: OLLAMA_MODEL (schnell, Default) und
  OLLAMA_MODEL_QUALITY (nur Boss/Raid, leer->aus).
- POST {OLLAMA_URL}/api/generate, body:
  {model, prompt: "Rolle: <npc>. Antworte max 2 Saetze. Sprache: de",
   stream:false,
   options:{num_ctx, temperature, top_p, top_k}}   // alle aus config.env
   // WICHTIG: 1) num_ctx immer setzen (klein halten = VRAM).
   //          2) temperature/top_p/top_k IMMER mitschicken (siehe unten).
   //          3) Boss/Qualitaets: OLLAMA_TEMP_QUALITY statt TEMPERATURE.
- Timeout OLLAMA_TIMEOUT_MS, OLLAMA_FALLBACK=1 -> bei Fehler:
  server/i18n.js t(lang,"npc_greeting")
- Rate-Limit: max 1 NPC-Antwort pro 2 s pro Spieler (Map key=playerId,
  ts). Boss-Phase: QUALITaET-Modell.
- AUFgabe: 10 Hz-Tick muss dadurch NICHT langsamer werden -> Promise/
  asynchron, nie blocking im Tick.
