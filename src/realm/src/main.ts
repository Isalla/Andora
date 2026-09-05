#!/usr/bin/env node
// main.ts — Einstiegspunkt Andora-Server (dünn, nur Verkabelung)
//
// ÜBERGANGSSTAND: Dieser Node.js/TypeScript-Server ist der Übergangsstand
// (siehe docs/architecture.md). Zielimplementierung ist der Rust-Realm
// (src/realm-rs). Bestehende Logik dient dort als Referenz und wird
// schrittweise migriert; dieser Code bleibt bis dahin lauffähig.
import { config } from './config';
import { initCharacterDb, closeCharacterDb, getCharacterPool } from './db/character';
import { initWorldDataDb, closeWorldDataDb, getWorldDataPool } from './db/worldData';
import { initRealmStateDb, closeRealmStateDb, getRealmStatePool } from './db/realmState';
import { applyMigrations, migrationsDir } from './db/migrations';
import { setupHealth } from './health';
import { initWebSocket } from './net';
import { startParentalPoller, stopParentalPoller } from './parental';
import { worldTick } from './world';

async function main(): Promise<void> {
  // Drei getrennte Datenbankverbindungen, je eigener Pool:
  // character, world_data, realm_state (realm_state_<realm>).
  // Der Realm-/World-Server besitzt KEINEN direkten auth-DB-Zugriff;
  // Auth-Funktionen laufen ausschließlich über die Auth-API des
  // Auth-/API-Service (Go, src/api).
  // Fehlt eine Konfiguration, abbrechen mit klarer Fehlermeldung
  // (keine stillschweigende Fallback-DB, keine Sammelverbindung).
  await initCharacterDb();
  await initWorldDataDb();
  await initRealmStateDb();
  // Automatische, versionsbasierte Migrationen der drei eigenen DBs
  // (je DB eigene db_version-Historie, keine zentrale Steuerung).
  // Läuft VOR Health/WebSocket: Schlägt eine Migration fehl, bricht der
  // catch-Handler unten den Start mit Exit 1 ab — keine Spieler.
  await applyMigrations(
    getCharacterPool(), 'CHARACTER_DB', config.characterDb.database,
    migrationsDir('character', config.migrations.characterDir)
  );
  await applyMigrations(
    getWorldDataPool(), 'WORLD_DATA_DB', config.worldDataDb.database,
    migrationsDir('world_data', config.migrations.worldDataDir)
  );
  await applyMigrations(
    getRealmStatePool(), 'REALM_STATE_DB', config.realmStateDb.database,
    migrationsDir('realm_state', config.migrations.realmStateDir)
  );
  setupHealth();
  initWebSocket();
  // Elternkontrolle: Status-Polling pro beaufsichtigtem Spieler (~10 s).
  startParentalPoller();

  const tick = setInterval(worldTick, config.tickInterval);

  const shutdown = async (sig: string) => {
    console.log(sig, '-> shutting down');
    stopParentalPoller();
    clearInterval(tick);
    await closeCharacterDb();
    await closeWorldDataDb();
    await closeRealmStateDb();
    process.exit(0);
  };
  process.on('SIGINT', () => shutdown('SIGINT'));
  process.on('SIGTERM', () => shutdown('SIGTERM'));

  console.log('Server started (tick', config.tickInterval + 'ms, AOFB', config.aofbRadius + 'm)');
}

main().catch((e) => {
  console.error('Startup failed:', e);
  process.exit(1);
});
