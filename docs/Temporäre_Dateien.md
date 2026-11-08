## Temporäre Dateien

Für **dauerhafte Projektdateien, Toolchains, persistente Caches und bewusst
über Arbeitsschritte/Sessions erhaltene lokale Hilfsmittel** ist
ausschließlich der projektinterne Ordner

`.tmp`

(relativ zum Projektroot, dem Root des aktuell geöffneten Andora-Repositories)
zu verwenden.

Dauerhafte Projektdateien dürfen den Projektroot nicht verlassen und
dürfen nicht in systemweiten temporären Verzeichnissen abgelegt werden.

### Kurzlebige Arbeits- und Zwischendaten

Sämtliche temporären Arbeits- und Zwischendaten liegen ausschließlich unter
`<Projektroot>/.tmp/`, bei Bedarf in Zweck-Unterverzeichnissen. Systemweite
Verzeichnisse wie `/tmp`, `/var/tmp` oder `/tmp/opencode` sind keine
Schreibziele. Die Regel gilt für Dateien, die ein Auftrag oder ein Werkzeug
bewusst anlegt. Wählt ein Werkzeug seinen Temp-Pfad selbst, ist `TMPDIR` auf
`<Projektroot>/.tmp/tmp` zu setzen; schreibt es trotzdem außerhalb, ist das
ein Befund und zu melden, kein stillschweigend hingenommener Zustand.

Grund: Das System liegt auf einer SD, der Projektroot auf einer HDD.
Systemweite Verzeichnisse können auf der SD liegen (`/`, `/var/tmp`);
Schreiblast auf der SD ist zu vermeiden, weil ihre Lebensdauer das Risiko
trägt. `/tmp` ist auf diesem Host ein tmpfs und damit flüchtig: Nach einem
Neustart existieren dort keine Artefakte mehr. Prüfnachweise, die über einen
Auftrag hinaus belegt bleiben sollen, gehören deshalb in `.tmp/` auf der HDD.
Zweiter Grund: Projektgrenze.

Grund der `TMPDIR`-Vorgabe ist die Projektgrenze, nicht die SD-Schonung:
`/tmp` ist ein tmpfs. Ein großer Build schreibt außerdem gegen die Größe
dieses tmpfs; meldet ein Lauf „No space left on device", obwohl auf der
HDD Platz ist, ist das die tmpfs-Grenze und kein Plattenproblem.

Es gelten zwei Grenzen:

* **Keine Schreibvorgänge außerhalb des Projektroots.**
* **Atomare projektinterne Schreibvorgänge** bleiben an ihrem jeweiligen Ort:
  temporäre Dateien, die Teil einer sicheren Rename-/Persistenzlogik sind,
  werden weiterhin dort erzeugt, wo der atomare Vorgang stattfindet
  (z. B. die `.tmp-*`-Write-Dateien der Coordinator-Queue unter
  `COORDINATOR_DATA_DIR`). Ein Umweg über `/tmp` würde die atomic-rename-
  Garantie auf demselben Dateisystem brechen.

Projekt-Toolchains (`.tmp/go`, `.tmp/rust`, Mod-/Build-Caches) liegen
dauerhaft im Projekt.

Systemweite Installationen oder sonstige persistente Änderungen außerhalb
des Projektroots bleiben weiterhin zustimmungspflichtig (siehe
`OpenCode_Session_Pflege.md`).

### Einschlägige Fälle im Projekt

Dies gilt insbesondere für:

* temporäre Arbeitsdateien und Zwischenstände
* generierte temporäre Konfigurationen
* Build-/Hilfsdateien, sofern deren Speicherort steuerbar ist
* temporäre Queue- und Recovery-Dateien (zwingend projektintern, siehe oben)

→ Diese liegen in `.tmp/` (bei Bedarf mit passenden Unterverzeichnissen)
bzw. am Ort des jeweiligen atomaren Schreibvorgangs, niemals in
systemweiten temporären Verzeichnissen.

## Projekt-Toolchains in `.tmp/`

Die Build- und Test-Toolchains des Projekts liegen projektintern unter `.tmp/` (relativ zum Projektroot). Die Coding-KI verwendet diese Toolchains für Bauen und Testen und installiert keine systemweiten Toolchains.

| Toolchain | Speicherort (relativ zum Projektroot) |
|---|---|
| Go | `.tmp/go/go-toolchain/bin/go` |
| Go-Mod-Cache | `.tmp/go-mod-cache` |
| Go-Build-Cache | `.tmp/go-build-cache` |
| Rust (rustup + rustc + cargo) | `.tmp/rust` (bin: `.tmp/rust/bin`) und `.tmp/rustup` (rustup-Instanz) |

Rust-Toolchain aktivieren (relativ zum Projektroot):

```sh
source .tmp/rust/env.sh
mkdir -p "$(pwd)/.tmp/tmp"
export TMPDIR="$(pwd)/.tmp/tmp"
```

Alternativ explizit (ohne Source-Datei):

```sh
export RUSTUP_HOME="$(pwd)/.tmp/rustup"
export CARGO_HOME="$(pwd)/.tmp/rust"
export PATH="$(pwd)/.tmp/rust/bin:$PATH"
mkdir -p "$(pwd)/.tmp/tmp"
export TMPDIR="$(pwd)/.tmp/tmp"
```

Bauen/Testen des Realm-Server (Rust) mit der Projekt-Toolchain:

```sh
source .tmp/rust/env.sh
mkdir -p "$(pwd)/.tmp/tmp"
export TMPDIR="$(pwd)/.tmp/tmp"
cargo build --manifest-path src/realm-rs/Cargo.toml
cargo test  --manifest-path src/realm-rs/Cargo.toml
```

Die Rust-Toolchain umfasst `rustc`, `cargo` und `rustup` im Profil `minimal` (Toolchain `stable-aarch64-unknown-linux-gnu`). Neuinstallation bei Bedarf:

```sh
mkdir -p "$(pwd)/.tmp/tmp"
export TMPDIR="$(pwd)/.tmp/tmp"
CARGO_HOME="$(pwd)/.tmp/rust" RUSTUP_HOME="$(pwd)/.tmp/rustup" \
  .tmp/rustup/rustup-init -y --profile minimal
```
