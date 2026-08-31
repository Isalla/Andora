#!/usr/bin/env node
// main.ts — Einstiegspunkt Andora-Server (dünn, nur Verkabelung)
import { config } from './config';
import { initDatabase, closeDatabase } from './db';
import { setupHealth } from './health';
import { initWebSocket } from './net';
import { worldTick } from './world';

async function main(): Promise<void> {
  await initDatabase();
  setupHealth();
  initWebSocket();

  const tick = setInterval(worldTick, config.tickInterval);

  const shutdown = async (sig: string) => {
    console.log(sig, '-> shutting down');
    clearInterval(tick);
    await closeDatabase();
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
