# Qwen3 Coder — Aufgaben-Übergabe (Andora MMO)

Du arbeitest als Coding-Agent am Projekt **Andora MMO**.

Implementiere ausschließlich die unten definierten Tasks und beachte die bestehenden Projektstrukturen.

---

# 1. Allgemeine Arbeitsregeln

## Bestehenden Code zuerst analysieren

Bevor du eine Aufgabe implementierst:

1. Bestehende Projektstruktur analysieren.
2. Relevante Dateien lesen.
3. Vorhandene Systeme, Klassen und Funktionen identifizieren.
4. Bestehende Komponenten bevorzugt erweitern.
5. Keine parallelen Systeme bauen, wenn bereits geeignete Strukturen existieren.
6. Erst danach implementieren.

Bestehende funktionierende Architektur nicht unnötig umbauen.

---

# 2. Projektgrenze

Du arbeitest ausschließlich innerhalb des aktuellen Projektverzeichnisses.

Keine Dateien außerhalb des Projekts erstellen, verändern, löschen oder überschreiben.

Insbesondere NICHT direkt verändern:

```text
/etc/systemd/
/etc/systemd/user/
/etc/sudoers
/etc/sudoers.d/
/usr/
/opt/
```

Keine Systemkonfiguration auf dem Entwicklungsrechner installieren.

Benötigte Deployment-Dateien werden innerhalb des Projekts vorbereitet:

```text
deploy/
├── systemd/
├── sudoers/
└── README.md
```

Die Installation auf dem Produktionsserver erfolgt später manuell.

---

# 3. Modularität

Keine großen monolithischen Dateien erstellen.

Insbesondere nicht sämtliche Logik in:

```text
main.ts
world.ts
npc.ts
npcManager.ts
server.ts
```

unterbringen.

Regel:

**Eine Datei = eine klar abgegrenzte Verantwortung.**

Wenn eine Datei mehrere unabhängige Systeme enthält oder unnötig groß wird, muss die Funktionalität in passende Module aufgeteilt werden.

Keine künstliche Aufteilung in viele winzige Dateien.

Vor größeren Implementierungen kurz festlegen:

```text
Datei/Modul -> Verantwortung
```

`main.ts` dient primär zum Starten und Verbinden der Systeme und soll keine große Sammlung von Spiellogik werden.

---

# 4. Server

Technologie:

```text
Node.js 20
TypeScript
Projekt: src/realm/
```

Start:

```bash
npm i
npm run dev
```

`npm run dev` führt TypeScript-Kompilierung und Serverstart aus.

Datenbank:

```text
MariaDB 10
Database: andora
mysql2 Promise API
```

Konfiguration:

```text
src/realm/config.env
```

wird erstellt aus:

```text
src/realm/config.env.example
```

Datenbankschema:

```text
src/realm/db/
```

TypeScript-Regeln:

* `strict`
* kein `any`
* JSDoc-Kommentar für exportierte Funktionen
* Fehler kontrolliert behandeln
* keine neuen Dependencies ohne Nachfrage
* keine blockierenden Operationen im 10-Hz-Welttick

---

# 5. Client

Technologie:

```text
Godot 3.5
GDScript 2
Projekt: src/client/
```

WICHTIG:

**Keine Godot-4-APIs verwenden.**

Insbesondere:

```text
CharacterBody2D
```

existiert für diesen Client nicht.

Für Spielerbewegung Godot-3.5-kompatible APIs verwenden, insbesondere:

```text
KinematicBody2D
```

Vor Verwendung einer API prüfen, ob sie mit Godot 3.5 kompatibel ist.

GDScript:

* Godot-3-Syntax
* Tabs zur Einrückung
* keine Kommentare außer `TODO`
* Pi 4 als Zielhardware berücksichtigen

---

# 6. Internationalisierung

Autoload:

```text
I18n
```

Keine deutschen oder englischen UI-/Spieltexte hardcoden.

Verwendung:

```text
I18n.t("key", {"name": value})
```

Sprachdateien:

```text
i18n/de.json
i18n/en.json
```

Jeder neue Key muss in BEIDEN Dateien vorhanden sein.

Sprache muss zur Laufzeit ohne Neustart umschaltbar bleiben.

---

# 7. Netzwerkprotokoll

Gemeinsame Definitionen:

```text
shared/protocol.js
shared/protocol.gd
```

Message-IDs müssen identisch bleiben.

Neue Message:

**immer in beiden Dateien ergänzen.**

WebSocket-Framing:

```json
{
  "seq": 1,
  "type": 1,
  "data": {}
}
```

Transport:

```text
WebSocket Text Frame
```

Keine unnötig großen JSON-Strukturen übertragen.

Kleine Control-/Entity-Payloads möglichst unter ca. 200 Byte halten.

`S2C.STATE` darf jedoch nicht durch eine starre 200-Byte-Grenze unbrauchbar werden.

Größere State-Updates:

* kompakt strukturieren
* AOFB anwenden
* bei Bedarf auf mehrere Nachrichten verteilen
* niemals unkontrolliert große Frames erzeugen

---

# 8. Performance-Ziele

Zielclient:

```text
Raspberry Pi 4
RAM: 8 GB
```

Zusätzlich berücksichtigen:

```text
4-GB-Pi -> RENDER_CAP = 48
8-GB-Pi -> RENDER_CAP = 64
```

Server:

```text
Tickrate: 10 Hz
Tick: 100 ms
AOFB Radius: 20 m
```

Performance ist Bestandteil der Architektur.

Keine unnötige Verarbeitung für Entities durchführen, die für einen Client nicht relevant sind.

---

# Task M2 — Server-Kern

Implementiere die noch offenen Teile des Server-Kerns.

Bestehende Implementierung zuerst analysieren und geeignete Komponenten wiederverwenden.

## Netzwerk

WebSocket-Server:

```text
Port 3001
```

MariaDB-Pool über:

```text
mysql2/promise
```

## HELLO

Bei:

```text
C2S.HELLO
```

Charakter aus der Datenbank laden.

Antwort:

```text
S2C.WELCOME
```

mit:

```text
data.you
```

## HEARTBEAT

Bei:

```text
C2S.HEARTBEAT
```

antworten:

```text
S2C.SYNC
{
  ack_seq
}
```

## Bewegung

Der Gameserver ist autoritativ.

Der Client darf nicht einfach eine beliebige Position vorgeben.

`C2S.MOVE` übermittelt Bewegungsabsicht bzw. zulässige Bewegungsdaten.

Der Server:

1. validiert Eingaben
2. berechnet bzw. validiert Bewegung
3. aktualisiert die autoritative Position
4. verwendet diese Position für weitere Berechnungen

Manipulierte Clients dürfen sich nicht durch beliebige Koordinaten teleportieren.

## Welt-Tick

Alle:

```text
100 ms
```

Für jeden verbundenen Spieler:

1. Eingaben verarbeiten
2. Position aktualisieren
3. AOFB-Filter anwenden
4. relevante Entities bestimmen
5. `S2C.STATE` senden

Nur Entities innerhalb:

```text
20 m
```

berücksichtigen.

## Disconnect

Bei Disconnect:

1. Spieler aus aktiver Welt entfernen
2. `S2C.DESPAWN` an relevante Clients senden
3. Position in MariaDB speichern

## Fehlerhandling

Ungültiges JSON:

```text
log
ignore
continue
```

Der Gameserver darf dadurch nicht abstürzen.

---

# Health-/Status-Server

HTTP-Port:

```text
3002
```

Bestehenden Health-Server bevorzugt erweitern.

Mindestens:

```text
GET /health
```

Antwort:

```json
{"ok":true}
```

Verifikation:

```bash
curl -s localhost:3002/health
```

Falls bereits `/status` existiert oder im Monitoring-System vorgesehen ist, diesen Endpoint modular erweitern und nicht parallel neu implementieren.

---

# Task M3 — Godot-Client

Pfad:

```text
src/client/
```

Alle Implementierungen müssen Godot-3.5-kompatibel sein.

## net_client.gd

```text
extends Node
```

Verwendet die mit Godot 3.5 kompatible WebSocket-API.

Aufgaben:

* connect
* C2S-Nachrichten senden
* Send-Queue
* Empfang
* Decode über `Protocol.decode()`
* kontrolliertes Fehlerhandling

---

# interp.gd

Interpolation Buffer:

```text
max 16 Einträge
```

Rendering:

```text
120 ms in Vergangenheit
```

Bereitstellen:

```text
draw_pos()
```

---

# local_player.gd

Godot 3.5:

```text
KinematicBody2D
```

NICHT:

```text
CharacterBody2D
```

Aufgaben:

* Input lesen
* MOVE senden
* 1-Frame Client Prediction
* Server-Reconciliation

Bei:

```text
S2C.SYNC
```

Serverzustand berücksichtigen.

Abweichungen über:

```text
150 ms
```

weich korrigieren.

Server bleibt autoritativ.

---

# entity_view.gd

Remote Entities:

* Interpolation
* Position
* Sichtbarkeit
* Animation nur bei sichtbaren Entities

Keine unnötigen Animationen außerhalb des sichtbaren Bereichs.

---

# render_cap.gd

Sichtbare Entities priorisieren:

```text
1. own
2. guild
3. target
4. distance
```

Maximal:

```text
8GB Pi: 64
4GB Pi: 48
```

anzeigen.

Rest:

```text
hide()
```

---

# perf_mode.gd

Rolling-FPS-Messung.

Bei:

```text
FPS < 15
```

Performance-Level erhöhen.

`S2C.PERFGO`:

```text
0
1
2
```

abarbeiten.

Stufen können reduzieren:

* Partikel
* Animationen
* Chunks

Keine Godot-4-APIs verwenden.

---

# Task M6 — Ollama NPC AI

Datei:

```text
src/realm/src/ai/ollama.ts
```

Falls das AI-System wächst, weitere klar abgegrenzte Dateien unter:

```text
src/realm/src/ai/
```

verwenden.

Nicht sämtliche zukünftige NPC-AI-Logik in `ollama.ts` unterbringen.

## Modelle

Aus:

```text
config.env
```

Standardmodell:

```text
OLLAMA_MODEL
```

Qualitätsmodell:

```text
OLLAMA_MODEL_QUALITY
```

Das Qualitätsmodell wird nur für:

* Bosse
* Raidbosse
* ausdrücklich hochwertige KI-Situationen

verwendet.

Ist der Wert leer:

```text
Quality Model disabled
```

---

# Ollama Request

Endpoint:

```text
POST {OLLAMA_URL}/api/generate
```

Body sinngemäß:

```json
{
  "model": "...",
  "prompt": "...",
  "stream": false,
  "options": {
    "num_ctx": 0,
    "temperature": 0,
    "top_p": 0,
    "top_k": 0
  }
}
```

Prompt:

```text
Rolle: <npc>.
Antworte maximal 2 Sätze.
Sprache: <lang>.
```

Keine Sprache fest auf Deutsch codieren, wenn der Spieler eine andere aktive Sprache besitzt.

---

# Ollama-Konfiguration

Folgende Werte immer aus der Konfiguration übernehmen:

```text
num_ctx
temperature
top_p
top_k
```

`num_ctx` IMMER explizit setzen.

Für normale NPCs bewusst klein halten, um VRAM zu sparen.

Für Qualitätsmodell:

```text
OLLAMA_TEMP_QUALITY
```

anstatt der normalen Temperatur verwenden.

---

# Ollama Timeout

Timeout:

```text
OLLAMA_TIMEOUT_MS
```

Bei Fehler und:

```text
OLLAMA_FALLBACK=1
```

Fallback über bestehendes i18n-System:

```text
t(lang, "npc_greeting")
```

Keine hardcodierten deutschen Fallback-Texte.

---

# Rate Limit

Pro Spieler:

```text
maximal 1 NPC-Antwort / 2 Sekunden
```

Beispiel:

```text
Map<playerId, timestamp>
```

Boss-/Raidphase darf das Qualitätsmodell verwenden.

---

# Kritische Performance-Regel für Ollama

**Der 10-Hz-Welttick darf niemals auf Ollama warten.**

Insbesondere verboten:

```text
worldTick()
  -> await ollama()
```

KI-Anfragen müssen außerhalb des Tick-Pfades asynchron verarbeitet werden.

Prinzip:

```text
World Event
    ↓
AI Job erzeugen
    ↓
World Tick läuft weiter
    ↓
Ollama verarbeitet Anfrage
    ↓
Resultat kommt später zurück
    ↓
Resultat validieren
    ↓
als Event/Aktion übernehmen
```

Ollama-Ausfall darf:

* Tickrate nicht blockieren
* Gameserver nicht stoppen
* Spielerbewegung nicht verzögern
* NPC-Grundlogik nicht unbrauchbar machen

---

# Zukünftige NPC-AI-Architektur

Ollama ist nicht die Wahrheit der Spielwelt.

Der Gameserver besitzt und kontrolliert:

* Position
* NPC-State
* Beziehungen
* Wissen
* Inventar
* Verfügbarkeit
* Reise
* Raid-Zugehörigkeit
* erlaubte Aktionen

Ollama darf später verwendet werden für:

* Dialog
* Persönlichkeit
* Reaktion auf bekannte Informationen
* Auswahl zwischen ausdrücklich erlaubten Aktionen

Ollama darf keine Weltfakten erfinden und keine direkten unvalidierten Änderungen am Weltzustand durchführen.

---

# Verifikation Server

Prüfen:

```bash
curl -s localhost:3002/health
```

Erwartet:

```json
{"ok":true}
```

Zusätzlich TypeScript vollständig kompilieren.

Keine neuen TypeScript-Fehler akzeptieren.

---

# Verifikation Client

Mit zwei Client-Instanzen testen:

1. Beide verbinden sich.
2. Beide sehen sich.
3. Bewegung wird synchronisiert.
4. Interpolation funktioniert.
5. Disconnect erzeugt DESPAWN.
6. Test-Tod eines Spielers erzeugt `S2C.KILL`.
7. Beide Clients verarbeiten das Ereignis korrekt.

---

# Verifikation i18n

Während der Laufzeit:

```text
Deutsch -> Englisch
Englisch -> Deutsch
```

umschalten.

Kein Neustart erforderlich.

Neue Texte müssen in:

```text
i18n/de.json
i18n/en.json
```

vorhanden sein.

---

# Vorgehensweise

Für jeden größeren Task:

1. Relevanten bestehenden Code analysieren.
2. Wiederverwendbare Komponenten bestimmen.
3. Geplante Module und Verantwortlichkeiten festlegen.
4. Keine unnötigen Parallelstrukturen erzeugen.
5. Implementieren.
6. TypeScript/GDScript auf Versionskompatibilität prüfen.
7. Tests durchführen.
8. Fehler korrigieren.
9. Dokumentation aktualisieren.
10. Am Ende kurz auflisten:

* neue Dateien
* geänderte Dateien
* durchgeführte Tests
* offene Probleme

Nicht wegen kleiner Unsicherheiten die komplette Aufgabe abbrechen.

Bei einer Architekturentscheidung, die bestehende Daten oder größere Teile des Projekts gefährden könnte, zuerst nachfragen.

---

# Grundprinzipien

**Serverautorität vor Clientvertrauen.**

**Bestehende Architektur erweitern statt duplizieren.**

**Eine Datei = eine Verantwortung.**

**Keine Systemdateien außerhalb des Projekts verändern.**

**Godot 3.5 bedeutet keine Godot-4-APIs.**

**Ollama darf niemals den 10-Hz-Tick blockieren.**

**Spielweltzustand kommt vom Server, nicht vom LLM.**

**Pi-Performance ist eine Designanforderung, keine spätere Optimierung.**
