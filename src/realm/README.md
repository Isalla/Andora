# Realm-Server (Node.js/TypeScript) — ÜBERGANGSSTAND

Dieser Server ist der **Übergangsstand** der Zielkette
`Auth/API → Login → Realm` (siehe `docs/architecture.md`,
`docs/Login_Realm_Architektur.md`).

**Zielimplementierung ist der Rust-Realm (`src/realm-rs`).**
Bestehende Logik hier (Auth/API-Anbindung, Session-Bindung,
Elternkontrolle, Chat-Gating, Protokoll, Health/Status) dient als
Referenz und wird schrittweise nach Rust migriert.

## Bewusste Alt-Architektur-Annahmen (nur Übergang, nicht ausbauen)

- Drei DB-Pools (`character`, `world_data`, `realm_state`) statt genau
  einer Realm-Datenbank. `world_data` ist bereits ohne fachliche Queries;
  eine zentrale `world_data`-Datenbank ist **keine** Zielarchitektur.
- Einstieg per Session statt per Handoff (Zielkette: Login stellt
  Handoff aus, Realm verbraucht ihn via `/handoff/validate`).
- Keine eigene Realm-ID-Prüfung bei der Spielerübergabe.

Der Migrationsrunner (`src/db/migrations.ts`) wendet die Migrationen
aller drei Pools an (je eigene `db_version`); auch das ist Übergang —
der Rust-Runner migriert nur `realm_state_<realm>`.

## Bauen/Starten (solange Übergang aktiv)

```bash
npm run build   # tsc → build/
npm start       # node build/main.js (config.env daneben)
```

Migrationen laufen vor Health/WebSocket; Fehler → Exit 1.
