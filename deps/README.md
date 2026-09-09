## Vendorierte Upstream-Quellen

`deps/` enthält die offiziellen, **unveränderten** Upstream-Quellverzeichnisse
zuständiger Bibliotheken, die das Projekt als feste, nachvollziehbare
Referenzquellen führt. Die Inhalte sind bytegenau aus den offiziellen
Release-Tarballs extrahiert und ohne eigene Modifikation abgelegt.

Die Tarballs selbst bleiben unter `.tmp/downloads/` (nicht Teil des
Projektverzeichnisses, siehe `docs/Temporäre_Dateien.md`); `deps/` enthält
ausschließlich die entpackten Quellverzeichnisse und dieses README.

### Lua 5.5.1

| Feld | Wert |
|---|---|
| Projektname | Lua |
| Version | 5.5.1 |
| Offizielle Herkunft | `https://www.lua.org/ftp/lua-5.5.1.tar.gz` |
| SHA-256 | `1c4b4068d67061f2a2231ad2b5422e77acea1487ea9890f6320af614f4373dce` |
| Lizenz | Lua-Lizenz (MIT-artig), siehe `deps/lua/COPYRIGHT` und `deps/lua/doc/readme.html` |
| Zweck in Andora | Fest veranordnete offizielle Referenzquelle der Lua-Bibliothek (Skript-/Interpreterumgebung) |
| Status | nur vendoriert — **nicht** eingebaut oder ins Projekt eingebunden |

Lage: `deps/lua/` (Wurzel des entpackten Quellverzeichnisses `lua-5.5.1/`).

### zstd 1.5.7

| Feld | Wert |
|---|---|
| Projektname | zstd |
| Version | 1.5.7 |
| Offizielle Herkunft | `https://github.com/facebook/zstd/releases/tag/v1.5.7` (Asset `zstd-1.5.7.tar.gz`) |
| SHA-256 | `eb33e51f49a15e023950cd7825ca74a4a2b43db8354825ac24fc1b7ee09e6fa3` |
| Lizenz | Dual License BSD-2-Clause / GPL-2.0 (siehe `deps/zstd/COPYING` und `deps/zstd/LICENSE`) |
| Zweck in Andora | Fest veranordnete offizielle Referenzquelle der zstd-Kompressionsbibliothek (libzstd, Frameformat) |
| Status | nur vendoriert — **nicht** eingebaut oder ins Projekt eingebunden |

Lage: `deps/zstd/` (Wurzel des entpackten Quellverzeichnisses `zstd-1.5.7/`).

### Versionswahl und Aktualisierungsregel

- Es existierte vor diesem Schritt **keine** im Projekt gepinnte Lua- oder
  zstd-Version (keine Angabe in Doku, Builds oder Konfig). Die Versionen
  wurden daher bewusst auf die jeweils aktuelle stabile Release-Ebene der
  genannten Linien festgelegt: **Lua 5.5.1** und **zstd 1.5.7**.
- Die Upstream-Quellen bleiben dauerhaft unverändert. Jede Änderung in
  `deps/` (Austausch, Versionssprong, Patches) ist ein eigener, bewusster
  Schritt mit Update dieses READMEs (Version, Herkunft, SHA-256).
- Automatische Aktualisierung findet nicht statt. Ein neuer offizieller
  Release wird nur nach Entscheidung und mit dokumentierter
  Prüfsummenverifizierung übernommen.
