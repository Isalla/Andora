# Andora – zstd-Komprimierung (Referenzanalyse)

## Status

**Referenz- / Konzeptdokument – zstd ist NICHT implementiert.**

Dieses Dokument analysiert die vendodierte zstd-Quelle in `deps/zstd/` und bereitet die spätere Entscheidung vor, **ob und wie** zstd in Andora eingesetzt wird. Es enthält Vorschläge, trifft aber **keine** verbindliche technische Entscheidung.

Kennzeichnungen:

* **Fakt:** aus der zstd-Quelle bzw. dem Repository belegt.
* **Vorschlag:** mögliche Einsatzart, noch nicht entschieden.
* **Offen:** Entscheidung erst bei der Implementierung.

---

## 1. Überblick und Repo-Lage

**Fakt:** `deps/zstd/` enthält die offizielle, seit dem Commit `d405f8f` (**zstd 1.5.7**) byte-genau vendodierte Referenzquelle von Zstandard. Gemäß `deps/README.md` ist zstd dort **„nur vendoriert — nicht eingebaut oder ins Projekt eingebunden"**.

| Feld | Wert |
|---|---|
| Komponente | Zstandard (libzstd) |
| Version | 1.5.7 (`ZSTD_VERSION_MAJOR 1`, `MINOR 5`, `RELEASE 7`) |
| Offizielle Herkunft | `https://github.com/facebook/zstd/releases/tag/v1.5.7` |
| SHA-256 | `eb33e51f49a15e023950cd7825ca74a4a2b43db8354825ac24fc1b7ee09e6fa3` |
| Lizenz | Dual `BSD-2-Clause` (`deps/zstd/LICENSE`) / `GPL-2.0` (`deps/zstd/COPYING`) |
| Status | nur vendoriert — **nicht** eingebaut |

Quellgröße `deps/zstd/` ≈ 10 MB (inkl. Tests, Build-Systemen, Programmen). Für die eigentliche Bibliothek relevant ist das Verzeichnis `deps/zstd/lib/`.

zstd wird derzeit **von keinem** Service (`src/`) referenziert (Suche über Code und Doku ohne Treffer). Der Einsatz ist ein reines Zukunfts-/Entscheidungsthema.

---

## 2. Lizenz (Fakt)

* Dual-Lizenz: **BSD-2-Clause** (`LICENSE`) **oder** **GPL-2.0** (`COPYING`); der Nutzer wählt eine der beiden.
* Für den Einbau ist BSD-2-Clause die praxisübliche Wahl (kompatibel mit kommerzieller, proprietärer und Open-Source-Nutzung). Ein einbettendes Projekt muss den BSD-/Copyright-Hinweis im Quelltext erhalten (bei Statik-/C-Runtime-Einbindung üblich: Copyright-Zusatz in der jeweiligen `CARGO`-`NOTICE`/README bzw. Build-Datei).
* **Offen:** konkrete Lizenz-Umsetzung beim Einbau (z. B. Vermerk in `deps/README.md` bzw. Projekt-README/`cargo`-Metadaten).

---

## 3. Quell- und Modulstruktur (`deps/zstd/lib/`)

**Fakt:** Die Bibliothek ist modular aufgebaut. Das Build-README (`deps/zstd/lib/README.md`) beschreibt die Abhängigkeiten explizit:

| Verzeichnis | Inhalt | Abhängigkeit | Nötig für |
|---|---|---|---|
| `lib/common` | gemeinsame Basis (Mem-Zugriff, FSE/HUF-Basis, XXH64, Thread-Pool `pool.c`, `threading.c`) | **immer** erforderlich | jede Variante |
| `lib/compress` | Kompressions-Encoder (5 Strategien: fast → greedy → lazy → lazy2 → btlazy2 → opt, Long-Distance-Matcher, MT-Kompression `zstdmt_compress.c`) | nur `common` | Kompression |
| `lib/decompress` | Dekodierer inkl. Huffman-AMD64-Zusatz (`huf_decompress_amd64.S`) und DDict | nur `common` | Dekompression |
| `lib/dictBuilder` | Wörterbuch-Training (`zdict.h`, cover/fastcover) | `common` + `compress` | Trainierte Wörterbücher |
| `lib/legacy` | Legacy-Dekoder v0.1–v0.7 (`ZSTD_LEGACY_SUPPORT`, Standard: 5 → „≥ v0.5.0") | `common` + `decompress` | alte Frames |
| `lib/deprecated` | veraltete „zbuff"-Streaming-APIs | `common` + … | **nicht** für neuen Code |

Wichtige Eigenschaften:

* **Kompression und Dekompression sind unabhängig** – es kann ein reiner Decoder- oder reiner Encoder-Build gebaut werden (`ZSTD_LIB_COMPRESSION=0` bzw. `ZSTD_LIB_DECOMPRESSION=0`).
* `lib/legacy` und `lib/deprecated` sind für neue Verwendung **verzichtbar**.
* Die **Dateien in `deps/` bleiben unverändert** (`deps/README.md`: jede Änderung/Austausch ist ein eigener bewusster Schritt). Ein Einbau baut die Quellen **aus** `deps/` ab, ohne sie zu modifizieren.

---

## 4. API-Oberfläche (Fakt)

Kern-API im Header `deps/zstd/lib/zstd.h` (stabil, ~3.200 Zeilen):

* **Einfache API (One-Shot):** `ZSTD_compress()` / `ZSTD_decompress()` mit vorab bekannten Größen, `ZSTD_compressBound()` für Worst-Case-Buffer.
* **CCtx-/DCtx-API (wiederverwendbare Kontexte):** `ZSTD_compressCCtx()`, `ZSTD_decompressDCtx()`, Parameter via `ZSTD_CCtx_setParameter()`/`ZSTD_DCtx_setParameter()`.
* **Streaming-API:** `ZSTD_compressStream()` / `ZSTD_decompressStream()` (unbekannte Eingabegrößen; für Netzwerk-/Dauerstrom-Fälle).
* **Hilfe-Funktionen:** `ZSTD_getFrameContentSize()`, `ZSTD_findFrameCompressedSize()`, `ZSTD_isError()`.
* **Fehlerbehandlung:** `ZSTD_isError()` ist versionsübergreifend korrekt; `ZSTD_ErrorCode` in `zstd_errors.h` – **nur Werte < 100 sind stabil** (u. a. `corruption_detected`=20, `checksum_wrong`=22, `parameter_outOfBound`=42, `dstSize_tooSmall`=70, `memory_allocation`=64). Experimentelle API nur mit `ZSTD_STATIC_LINKING_ONLY` und **nur statisch**.
* **Wörterbuch-API:** `deps/zstd/lib/zdict.h` – `ZDICT_trainFromBuffer()` (Training aus Beispiel-Daten, empfohlen: mehrere Tausend Samples, Gesamtmenge ~100× Zielgröße), für Wiederverwendung von Struktur in kleinen Nachrichten sinnvoll.

Relevante Parameter (Auswahl):

| Parameter | Wert | Bedeutung |
|---|---|---|
| `ZSTD_c_compressionLevel` | 100 | Standard-Presets (negativ = schnell, bis 22 = stark) |
| `ZSTD_c_windowLog` | 101 | max. Rückwärts-Suchdistanz (pot. 2 hoch n) – Haupt-Ressourcenfaktor |
| `ZSTD_c_contentSizeFlag` | 200 | Inhaltsgröße im Frame-Header (Standard: 1) |
| `ZSTD_c_checksumFlag` | 201 | 32-Bit-Checksumme am Frame-Ende (Standard: 0; deaktiviert = kleiner, aktiv = klassisch Fehler-Detektion) |
| `ZSTD_c_dictIDFlag` | 202 | Dict-ID im Header (Standard: 1) |
| `ZSTD_c_nbWorkers` | 400 | parallele Threads (`ZSTD_MULTITHREAD` + pthread) |

---

## 5. Datenformat – Grundlagen (Fakt)

* Frame-Magie: `0xFD2FB528` (Kleinendian), Dict-Magie `0xEC30A437`, Skippable-Frames 0x184D2A50–5F.
* `ZSTD_compressBound(srcSize) = srcSize + (srcSize >> 8) + Rand` (≤ 128 KB) – einfache Worst-Case-Formel für Buffer-Allokation.
* Kompression lohnt i. d. R. ab einigen hundert Byte; sehr kleine Nachrichten gewinnen wenig.
* Frame-Header kann die Originalgröße enthalten (`contentSizeFlag`), wodurch `ZSTD_getFrameContentSize()` die Zielgröße vorab liefert (hilfreich gegen Speicher-Überschätzung, aber **Angebot ist bei fremden Daten nicht vertrauenswürdig** – siehe Sicherheit).

---

## 6. Build- / Integrationsoptionen (Fakt + Vorschlag)

zstd bringt mehrere offizielle Integrationswege mit:

1. **Makefile** (`deps/zstd/lib/Makefile`): statisch+shared, `lib-mt`/`lib-nomt`, `make install`, `.pc`-Datei (pkg-config).
2. **Modular / Minifizierung:** `ZSTD_LIB_COMPRESSION=0`, `ZSTD_LIB_DECOMPRESSION=0`, `ZSTD_LIB_DICTBUILDER=0`, `ZSTD_LIB_DEPRECATED=0`, `ZSTD_LIB_MINIFY=1`, `HUF_FORCE_DECOMPRESS_X1`, `ZSTD_FORCE_DECOMPRESS_SEQUENCES_SHORT` → kleinster Dekoder (Referenz: ~26 kB WebAssembly laut offiziellem README, nativ etwas mehr). Strategie-Ausschluss `ZSTD_LIB_EXCLUDE_COMPRESSORS_DFAST_AND_UP` u. ä. verkleinert nur den Encoder.
3. **Single-File-Amalgamierung** (`deps/zstd/build/single_file_libs/combine.py`): erzeugt `zstddeclib.c` (nur Decoder) bzw. `zstd.c` (voll, ~1,2 MB) als **eine** Quelldatei – sehr einfache Einbindung ohne weiteres Build-System.
4. **CMake/Meson/Buck** in `deps/zstd/build/` (für andere Build-Systeme).

**Vorschläge für Andora (Einbindung „aus dem vendodierten Quellbaum", keine Entscheidung):**

* **Rust-Realm:** Quellen aus `deps/zstd/lib/{common,compress,decompress}` über einen Cargo `build.rs` (z. B. `cc`-Crate) statisch kompilieren, nur die benötigten Funktionssätze; oder die Single-File-Amalgamierung (Decoder-Option) einbinden. Damit bleibt das „Referenzquelle"-Prinzip aus `deps/README.md` gewahrt: `deps/` bleibt unmodifiziert, gebaut wird abgeleitet davon.
* **Alternative:** externe Rust-Crate (z. B. `zstd`) statt Eigen-Build – dies umginge jedoch das vendodierte `deps/` und ist daher **kein** vorausgewählter Weg (offen).
* **Go-Services (api/login/coordinator/agent):** bei Bedarf eigene Go-zstd-Bindung oder Port – erst entscheiden, wenn ein Go-Service tatsächlich komprimieren soll.
* **Offen:** Es wird **nicht** festgelegt, welche Variante (Make/cc-Crate/Amalgamierung), welche Module (Decode-only vs. encode+decode), welche Minifizierungs-Stufe und ob Multithreading (`nbWorkers`) nötig wird.

---

## 7. Performance- und Ressourcen-Charakteristik (Fakt + Hinweis)

* Kompression ist deutlich teurer als Dekompression; die **Dekompression ist sehr schnell** und ressourcenarm (CPU-Zustand-gepuffert).
* Dominanter Ressourcenfaktor ist `windowLog` (Search-Window-Größe), nicht der Level – wichtig für Zielplattform **Raspberry Pi 4** (Performance-Ziel 60 FPS, siehe `Clientdarstellung_und_Performance.md`); komprimierende Nebenläufigkeit sollte hier limitiert/gepuffert werden.
* Decoder allein ist klein genug für Embedded-Ziellasten; Full-Lib ist ebenfalls unkritisch, wenn nur moderate Level eingesetzt werden.

---

## 8. Mögliche Einsatzszenarien in Andora (Vorschläge)

Alle Punkte sind **Vorschläge/Kandidaten** – es werden keine Protokolle, kein Schema, keine Systeme verändert. Bei jedem Szenario: Nutzen ↔ Kosten ↔ Risiken.

### 8.1 Realm-WebSocket-Nachrichten (Netzwerk)
* **Kontext:** Despot-Server spricht JSON-Frames `{seq, type, data}` (siehe `src/realm-rs/src/protocol.rs`, `net.rs`). Kleine Nachrichten (MOVE, DAMAGE) lohnen kaum; größere Blöcke (`GROUP_INFO`, spätere World-/State-Syncs, Cutscene-Daten) könnten komprimiert werden.
* **Vorschlag:** Nur **größere** Frames oder gesammelte Sync-Batches per zstd (Streaming-API), Einzel-Frames unverändert lassen; `checksumFlag` könnte Frames automatisch absichern.
* **Risiko/Bedingung:** Jede Änderung am Drahtformat ist eine **Protokolländerung** und muss mit Godot-Client (`shared/protocol.gd`) abgestimmt werden; Eigenschaft s. „KEINE Protokolländerungen" – daher **bewusst offen**, nicht jetzt entscheiden.

### 8.2 Persistente Daten / MariaDB
* **Kontext:** MariaDB bleibt für persistenten Runtime-/Spielerzustand zuständig (keine Arbeitsoberfläche, siehe `Content-Studio.md`).
* **Vorschlag:** Kein zstd im App-Code für normale Spielzustände (dort schlanke JSON-Typen). Allenfalls später für **große statische/Content-Blobs** oder Archiv-/Backup-Formate in Betracht ziehen.
* **Bedingung:** Blob-Format/Migration wäre eine Schema-/Datengrundlage-Änderung → nur nach separater Entscheidung.

### 8.3 Content-/Lua-/Studio-Dateien
* **Kontext:** Content entsteht als kontrollierte, versionierbare Definitionen (Lua/JSON, siehe `Lua-Scripting-System.md`, `Content-Studio.md`).
* **Vorschlag:** zstd für **statische Content-Pakete** (z. B. zusammengefasste Karten-/Regions-Basen, vorübersetze Kataloge) als Dateiformat-Kandidat – einsetzbar, ohne Gameplay-/Protokolllogik zu berühren.
* **Offen:** Dateiformat-Entscheidung (Container, Dictionary-Nutzung) erst in der Einführungsphase.

### 8.4 Service-Nachrichten und lokale Dateien (Go-Seite)
* **Kontext:** Coordinator nutzt dateibasierte Queue/Recovery-Dateien; Agent/Panel übertragen Manifeste/Updates (`Deployment_Betriebsarchitektur.md`).
* **Vorschlag:** Für **große lokale Anhänge/Queue-Blobs oder Update-Artefakte** zstd als optionales Format – einfacher Gewinn ohne Netzprotokoll-Relevanz.
* **Offen:** konkreter Einsatzort.

### 8.5 Client-Assets (Godot)
* **Vorschlag (klar als Option):** zstd-komprimierte Static-Assets/Gebäudedaten, wenn ein Datei-Format im Client etabliert wird. Kein aktueller Auftrag.

---

## 9. Sicherheits- und Betriebsaspekte (Fakt)

* **Nicht-vertrauenswürdige Eingaben:** Frame-Header-Angaben (Originalgröße) können manipuliert sein – beim Dekomprimieren fremder Daten immer eigene Limits anwenden (`ZSTD_getFrameContentSize()` liefert die *angegebene*, nicht eine geprüfte Größe; Empfehlung in `zstd.h` note 5: Rückgabe gegen app-seitige Limits prüfen).
* `ZSTD_isError()` sollte nach jeder Dekompressions-API geprüft werden (klassische Fehlerkette); stabile Codes nur < 100.
* Multithreading (`nbWorkers`) benötigt `ZSTD_MULTITHREAD` + `-pthread` beim Linken – sonst Fehlverhalten beim Build.
* Dedup/(`dictionary_corrupted`/`dictionary_wrong`) bei Dict-Einsatz.
* Performance-Budget: komprimierende Threads nicht unbegrenzt auf Pi-4-lastigen Rechnern zulassen (siehe §7).

---

## 10. Entscheidungskatalog (offen)

Beim späteren Einbau sind mindestens zu klären (nicht heute):

1. **Einsatzort/-art:** Netzwerk-Frames, Content-Pakete, Queue-/Update-Dateien, Assets; bzw. mehrere?
2. **Richtung:** nur Dekompression, oder auch Kompression?
3. **Modul-/Build-Variante:** Make / cc-Crate / Single-File; `ZSTD_LIB_*`-Selektion; Minifizierung.
4. **Parameter:** Default-Level, `windowLog`, `checksumFlag`, `contentSizeFlag`, `dictIDFlag`.
5. **Wörterbücher:** ja/nein; wenn ja: trainierte Dicts für welche Datenklassen; Pflege/Zyklus.
6. **Multithreading** (`nbWorkers`) in welchen Services.
7. **Typgrößen-/Buffer-Strategien** und Limits für fremde Eingaben.
8. **Format-/Versions-Stabilität** für gespeicherte, komprimierte Daten (zstd-Backward-Kompatibilität von Frames wird zstd-seitig erhalten, aber eigene Wrapper sind selber zu versionieren).

---

## 11. Referenzen

* `deps/zstd/README.md`, `deps/zstd/lib/README.md` (Build/Module), `deps/zstd/lib/zstd.h`, `lib/zstd_errors.h`, `lib/zdict.h`
* `deps/zstd/build/single_file_libs/README.md` (Amalgamierung: `zstddeclib.c`, `zstd.c`; Generierung über `combine.py`)
* `deps/README.md` (Referenzquelle-Regel; zstd 1.5.7 Eintrag)
* `src/realm-rs/src/protocol.rs`, `src/realm-rs/src/net.rs` (WebSocket-JSON-Frames, reine Referenz für mögliche Einsatzorte)
* `Lua-Scripting-System.md`, `Content-Studio.md` (Content-/Datei-Kontext)
* `Clientdarstellung_und_Performance.md` (Performance-Zielgerät)
* `Deployment_Betriebsarchitektur.md` (Update-/Manifest-Kontext)