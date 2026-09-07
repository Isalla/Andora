# src/coordinator — Coordinator-Service

Der Coordinator ist der Schritt `Realm → Coordinator → Ollama → Realm`
(siehe `docs/Coordinator.md`, `docs/ai_system.md`): Realms reichen KI-Jobs
ein, der Coordinator wartet eine persistente, priorisierte Warteschlange
ab, spricht ausschließlich mit Ollama und liefert das Ergebnis (oder einen
klaren Fehlercode) an das Realm zurück.

Der Coordinator hält bewusst **keine** Datenbankzugangsdaten und **kein**
Spielweltwissen (`docs/Coordinator.md` §2). Sein einziger Zustand ist die
dateibasierte Queue unter `COORDINATOR_DATA_DIR`:

```
data/queue/jobs/<timestamp>_<type>-<id>.json   ein Job = eine Datei
data/queue/queue.json                          Reihenfolge (rekonstruierbar)
data/queue/done/                               abgeschlossene/fehlgeschlagene Jobs
data/queue/quarantine/                         defekte/fehlende Dateien (zur Diagnose)
data/queue/jobs/.tmp-*                         atomare Writes (nur projektintern)
```

## Grundprinzipien (Kurzform)

- Eine einzige KI-Schnittstelle (Ollama); keine Realm–Ollama-Direktverbindung
  (§3). Fehler lösen nie neue KI-Arbeit aus (§13).
- Kern wartet nie auf die KI: Annahme → sofortige Antwort `QUEUED`, dann
  asynchron verarbeiten (§2/3).
- Prioritäten konfigurierbar (§5), gleicher Rang = Eingangsreihenfolge.
- Pro-Spieler-Cooldown (§6), Eingabe-/Ausgaberegeln zentral (§7/9/10),
  Kontextbudget mit reserviertem Antwortraum (§8), maximal 5
  Korrektur-/Retry-Versuche (§11/12).
- Crash-Konsistenz: Queue und Job-Dateien are genau eine Quelle der Wahrheit,
  atomare Renames, Wiederaufsetzen beim Start (§14–18), beschädigte Dateien
  quarrantiniert (§18).
- Ergebnis-Zustellung per signiertem Callback (wenn `REALM_<name>_RESULT_URL`
  gesetzt) oder Polling `GET /v1/jobs/{id}` (§18/24–26).

## Endpunkte

| Methode | Pfad              | Authz | Zweck |
|---|---|---|---|
| GET     | `/health`         | offen | Liveness |
| GET     | `/status`         | offen | Queue-/KI-Status (Operatoren) |
| POST    | `/v1/jobs`        | `coordinator.jobs.submit` | Job einreichen → `QUEUED`/Ablehnung |
| GET     | `/v1/jobs/{id}`   | `coordinator.jobs.query` | Existent? Status/Resultat/Fehlercode |

Signatur/Authz wie in `docs/Auth_API_Architektur.md` §10 (`X-Andora-Service`,
`X-Andora-Timestamp`, `X-Andora-Signature` = HMAC-SHA256 über
`METHOD\nPATH\nRAW_QUERY\nTIMESTAMP\nSHA256(BODY)`).

## Bauen/Testen (projektinterne Go-Toolchain, kein System-Go nötig)

```bash
../../.tmp/go/go-toolchain/bin/go build ./...
../../.tmp/go/go-toolchain/bin/go test  ./...
../../.tmp/go/go-toolchain/bin/go vet   ./...
```

## Inhaltsregeln / Sprachfilter (§9/10)

Deny-Wort-Regeln liegen sprachdateibasiert in ``COORDINATOR_FILTER_DIR``
(Standard ``filters``) mit **einer Datei pro Sprache und Richtung**:

```
filters/en.input.txt    englische Masterliste  (Input, von uns gepflegt)
filters/en.output.txt   englische Masterliste  (Output)
filters/de.input.txt    deutsche Ergänzungen   (Input)
filters/de.output.txt   deutsche Ergänzungen   (Output)
```

- Beim Start werden **alle vorhandenen Sprachdateien gemeinsam** geladen und
  gelten für **alle** Jobs — unabhängig von der Client-/Spielersprache
  (Sprachwechsel kann Filter nicht umgehen).
- Input und Output bleiben getrennt (``*.input.txt`` vs. ``*.output.txt``).
- ``INPUT_DENY_WORDS``/``OUTPUT_DENY_WORDS`` bleiben als zusätzliche
  Betreiber-Einträge erhalten und verschmelzen mit den Dateiregeln.
- Jede Zeile = ein Regelwort bzw. eine Phrase (niedrig geschrieben);
  ``#``-Zeilen sind Kommentare. Matching ist Wort-genau (kein Teilstring),
  Unicode-fähig und case-insensitiv.
- Fehlende/leere Filterdateien sind erlaubt (Regeln bleiben leer).
- **Neue Sprache:** ``<lang>.input.txt`` + ``<lang>.output.txt`` anlegen und
  (später) aus der englischen Masterliste übersetzen — keine Codeänderung.

## Betrieb

```bash
./coordinator [config.env]     # oder COORDINATOR_CONFIG=<pfad>
```

Konfigurationsvorbild: `config.env.example` → nach `config.env` neben dem
Binary kopieren. Systemd-Vorlage: `deploy/systemd/andora-coordinator.service`.

## Verhalten (nur Punkte, die sich aus dem Betrieb ergeben)

- `OLLAMA_URL` in der Konfiguration ist **Pflicht**; fehlt die Modellzeile
  oder eine Realm-Registrierung, startet der Dienst nicht (fail-closed).
- Für jedes Realm braucht es `REALM_<name>_ID` + das passende
  `SERVICE_<name>_ID/_SECRET/_PERMISSIONS`-Tupel.
- Cooldown-Fehler antworten mit HTTP 429 + `Retry-After` (Sekunden bis zur
  nächsten erlaubten Anfrage), `QUEUE_FULL` mit HTTP 503.
- Wenn `REALM_<name>_RESULT_URL` gesetzt ist, muss der Realm dieses Endstück
  verifizieren können (Koordinator-Identität, siehe
  `COORDINATOR_SERVICE_ID/_SECRET`); andernfalls pollt er selbst. Der
  Beispielwert ist ein Deployment-Platzhalter — die tatsächliche
  Callback-Adresse legt der Betreiber je Realm fest.