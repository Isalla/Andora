#!/usr/bin/env node
// main.ts — Einstiegspunkt Andora-Server (dünn, nur Verkabelung)
import { config } from './config';
import { initCharacterDb, closeCharacterDb } from './db/character';
import { initWorldDataDb, closeWorldDataDb } from './db/worldData';
import { initRealmStateDb, closeRealmStateDb } from './db/realmState';
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
