## Temporäre Dateien

Für sämtliche temporären Dateien des Projekts ist ausschließlich der vorhandene projektinterne Ordner

`.tmp`

(relativ zum Projektroot, dem Root des aktuell geöffneten Andora-Repositories) zu verwenden.

Systemweite temporäre Verzeichnisse wie `/tmp`, `/var/tmp` oder vergleichbare Verzeichnisse außerhalb des Projekts dürfen nicht verwendet werden.

Dies gilt insbesondere für:

* temporäre Arbeitsdateien
* Zwischenstände
* atomare Schreibvorgänge
* generierte temporäre Konfigurationen
* Build-/Hilfsdateien, sofern deren Speicherort steuerbar ist
* temporäre Queue- und Recovery-Dateien

Bei Bedarf sind innerhalb von `.tmp/` geeignete Unterverzeichnisse anzulegen.

Temporäre Dateien dürfen den Projektroot nicht verlassen.

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
```

Alternativ explizit (ohne Source-Datei):

```sh
export RUSTUP_HOME="$(pwd)/.tmp/rustup"
export CARGO_HOME="$(pwd)/.tmp/rust"
export PATH="$(pwd)/.tmp/rust/bin:$PATH"
```

Bauen/Testen des Realm-Server (Rust) mit der Projekt-Toolchain:

```sh
source .tmp/rust/env.sh
cargo build --manifest-path src/realm-rs/Cargo.toml
cargo test  --manifest-path src/realm-rs/Cargo.toml
```

Die Rust-Toolchain umfasst `rustc`, `cargo` und `rustup` im Profil `minimal` (Toolchain `stable-aarch64-unknown-linux-gnu`). Neuinstallation bei Bedarf:

```sh
CARGO_HOME="$(pwd)/.tmp/rust" RUSTUP_HOME="$(pwd)/.tmp/rustup" \
  .tmp/rustup/rustup-init -y --profile minimal
```
