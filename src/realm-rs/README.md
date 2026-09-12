# Realm-Server (Rust)

Zielimplementierung des Andora-Realmservers (`Auth/API → Login → Realm`,
siehe `docs/architecture.md`, `docs/Login_Realm_Architektur.md`).
Portiert nach dem Node.js/TypeScript-Übergangsstand (`src/realm`), ohne
dessen Alt-Architektur-Annahmen.

## Unterschiede zum Übergangsstand (bewusst)

- **Genau eine Datenbank**: `realm_state_<realm>` (Charaktere + Zustand).
  Keine `character`-/`world_data`-Pools; Migrationen in `migrations/`
  (001–008, aus den Übergangs-Verzeichnissen zusammengeführt; 008 = Combat V1).
- **Einstieg per Handoff (fail-closed)**: `HELLO` braucht `handoff_token`
  UND `session_id`. Der `handoff_token` ist einmalig, realm-gebunden
  und wird via `/handoff/validate` verbraucht; die `session_id` muss
  gültig sein und denselben Account tragen wie der Handoff
  (`session.account_id == handoff.account_id`). Jeder Fehlschlag
  (fehlender/abgelaufener/verbrauchter Handoff, Realm-Mismatch,
  fehlende/ungültige/fremde Session, Auth-API unerreikbaar) lässt die
  Verbindung schließen. Der Übergangsstand prüfte nur die Session.
- **Eigene Realm-ID** (`REALM_ID`): Handoffs fremder Realms werden
  abgelehnt.

Erhalten: fail-closed Einstieg (`handoff_token` + `session_id`,
Account-Gleichheit), Elternkontrolle (Status/PIN/Extension,
10-s-Poll, Force-Logout), Chat-Gating (240 Zeichen, AOFB-Broadcast),
Protokoll-IDs (`src/protocol.rs`, clientseitig gespiegelt in
`shared/protocol.gd` — dieselben numerischen IDs), Health/Status (`/health`,
`/status`, `/players`), Speed-Cap (210 m/s), Migrationen mit
`db_version` + Destruktiv-Sperre.

## Bauen/Testen (projektinterne Rust-Toolchain, kein System-Rust nötig)

Die Rust-Toolchain des Projekts liegt unter `.tmp/` des Projektroots
(relativ, Details: `docs/Temporäre_Dateien.md`):

```bash
source .tmp/rust/env.sh
cargo build --manifest-path src/realm-rs/Cargo.toml
cargo test  --manifest-path src/realm-rs/Cargo.toml
```

## Betrieb

`andora-realm [config.env]` (Pfad auch per `REALM_CONFIG`). Migrationen
laufen vor Health/WebSocket; Fehler → Exit 1, keine Spieler.
`migrations/` liegt neben dem Binary (Override:
`REALM_STATE_MIGRATIONS_DIR`).

## Bewusste Folgeschritte (kein Bestandteil dieses Stands)

- **Combat**: Vertikaler Schnitt V1 ist eingebaut (`ATTACK` start/stop,
  Auto-Grundangriff, Trefferauflösung, Rüstung/Klassen-Caps, Tod/KILL —
  vorläufige Balancingwerte via `COMBAT_*`-Config). Als Nächstes: Fähigkeiten
  (`skill_id`), Gegner/NPC-Angriffe, Loot.
- **Loot/Auktion/NPC-Handler** (`PICKUP`, `NPC_TALK`, `AUCTION_*` —
  Protokoll-IDs bereits reserviert, Dispatcher meldet `unknown type`).
- Coordinator-Anbindung (`OLLAMA_URL`/`RENDER_CAP_DEFAULT` sind
  konfiguriert, aber noch nicht verdrahtet; Fallback-Regeln aus
  `docs/Coordinator.md` gelten dann).
- Charaktertransfer zwischen Realms (kontrollierte DB-zu-DB-Migration).
- Voice-Service (späterer Release) ist ein separater Dienst.
