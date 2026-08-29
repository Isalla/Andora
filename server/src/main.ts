#!/usr/bin/env node

/**
 * Server-Kern für Andora MMO
 * Task M2: Implementierung des Server-Kerns
 */

import { createServer } from 'http';
import WebSocket from 'ws';
import { Pool } from 'mysql2/promise';
import fs from 'fs';
import path from 'path';

// Protokoll-Definitionen
const C2S = {
  HELLO: 1,
  HEARTBEAT: 2,
  MOVE: 3
};

const S2C = {
  WELCOME: 1,
  SYNC: 2,
  STATE: 3,
  DESPAWN: 4,
  KILL: 5
};

// Konfiguration laden
const config = {
  port: 3001,
  db: {
    host: 'localhost',
    user: 'andora',
    password: 'andora',
    database: 'andora'
  },
  aofbRadius: 20, // m
  tickInterval: 100, // ms
  healthPort: 3002
};

// Globale Variablen
let playerPool: Pool;
let wss: WebSocket.Server;
let players: Map<string, any> = new Map();
let worldTickTimer: NodeJS.Timeout;

/**
 * Initialisiert die Datenbankverbindung
 */
async function initDatabase(): Promise<void> {
  try {
    playerPool = new Pool(config.db);
    console.log('Database connection established');
  } catch (error) {
    console.error('Failed to connect to database:', error);
    process.exit(1);
  }
}

/**
 * Lädt einen Charakter aus der Datenbank
 */
async function loadCharacter(playerId: string): Promise<any> {
  try {
    const [rows] = await playerPool.execute(
      'SELECT id, name, x, y FROM players WHERE id = ?',
      [playerId]
    );
    
    if (rows.length > 0) {
      return rows[0];
    }
    
    // Falls Spieler nicht existiert, erstelle neuen
    const [insertResult] = await playerPool.execute(
      'INSERT INTO players (id, name, x, y) VALUES (?, ?, 0, 0)',
      [playerId, `Player${playerId}`]
    );
    
    return {
      id: playerId,
      name: `Player${playerId}`,
      x: 0,
      y: 0
    };
  } catch (error) {
    console.error('Error loading character:', error);
    throw error;
  }
}

/**
 * Speichert die Position eines Spielers
 */
async function savePlayerPosition(playerId: string, x: number, y: number): Promise<void> {
  try {
    await playerPool.execute(
      'UPDATE players SET x = ?, y = ? WHERE id = ?',
      [x, y, playerId]
    );
  } catch (error) {
    console.error('Error saving player position:', error);
  }
}

/**
 * Welt-Tick-Logik für alle Spieler
 */
async function worldTick(): Promise<void> {
  try {
    // Für jeden verbundenen Spieler
    for (const [playerId, player] of players.entries()) {
      // AOFB-Filter (Area of Focus Broadcast) - nur in 20m Radius senden
      // Hier würde die Logik für die Broadcast-Filterung eingefügt
      
      // Sende STATE an alle Players im Radius
      const stateMsg = {
        seq: player.seq,
        type: S2C.STATE,
        data: {
          playerId,
          x: player.x,
          y: player.y
        }
      };
      
      // Broadcasten an Clients im 20m Radius
      // (Diese Logik müsste implementiert werden, basierend auf Position)
    }
  } catch (error) {
    console.error('Error in world tick:', error);
  }
}

/**
 * WebSocket-Handler für eingehende Nachrichten
 */
function handleWebSocketMessage(ws: WebSocket, data: string): void {
  try {
    const message = JSON.parse(data);
    
    switch (message.type) {
      case C2S.HELLO:
        // Lade Spieler aus DB und sende WELCOME
        loadCharacter(message.playerId)
          .then(character => {
            const welcomeMsg = {
              seq: message.seq,
              type: S2C.WELCOME,
              data: {
                you: character
              }
            };
            ws.send(JSON.stringify(welcomeMsg));
            
            // Spieler in Liste aufnehmen
            players.set(character.id, {
              id: character.id,
              ws: ws,
              seq: message.seq,
              x: character.x,
              y: character.y,
              lastActivity: Date.now()
            });
          })
          .catch(error => {
            console.error('Error handling HELLO:', error);
            // Fehlermeldung an Client senden
          });
        break;
        
      case C2S.HEARTBEAT:
        // ACK zurücksenden
        const ackMsg = {
          seq: message.seq,
          type: S2C.SYNC,
          data: {
            ack_seq: message.seq
          }
        };
        ws.send(JSON.stringify(ackMsg));
        
        // Aktualisiere letzte Aktivität
        if (players.has(message.playerId)) {
          const player = players.get(message.playerId);
          player.lastActivity = Date.now();
        }
        break;
        
      case C2S.MOVE:
        // Position aktualisieren und Broadcasten
        if (players.has(message.playerId)) {
          const player = players.get(message.playerId);
          player.x = message.x;
          player.y = message.y;
          
          // Speichere Position in DB
          savePlayerPosition(message.playerId, message.x, message.y);
        }
        break;
        
      default:
        console.log('Unknown message type:', message.type);
    }
  } catch (error) {
    console.error('Error parsing message:', error);
    // Fehlerhandling: kein Crash bei invalid JSON -> log + ignorieren
  }
}

/**
 * Initialisiert den WebSocket-Server
 */
function initWebSocketServer(): void {
  wss = new WebSocket.Server({ port: config.port });
  
  wss.on('connection', (ws) => {
    console.log('New client connected');
    
    ws.on('message', (data) => {
      handleWebSocketMessage(ws, data.toString());
    });
    
    ws.on('close', () => {
      console.log('Client disconnected');
      
      // Entferne Spieler aus Liste
      for (const [playerId, player] of players.entries()) {
        if (player.ws === ws) {
          // Speichere letzte Position vor dem Disconnect
          savePlayerPosition(playerId, player.x, player.y);
          
          // Sende DESPAWN an andere Spieler
          const despawnMsg = {
            seq: 0,
            type: S2C.DESPAWN,
            data: {
              playerId
            }
          };
          
          players.delete(playerId);
          break;
        }
      }
    });
    
    ws.on('error', (error) => {
      console.error('WebSocket error:', error);
    });
  });
  
  console.log(`WebSocket server listening on port ${config.port}`);
}

/**
 * Health-Endpoint für Statusabfragen
 */
function setupHealthEndpoint(): void {
  const httpServer = createServer((req, res) => {
    if (req.url === '/health' && req.method === 'GET') {
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ ok: true }));
    } else {
      res.writeHead(404);
      res.end();
    }
  });
  
  httpServer.listen(config.healthPort, () => {
    console.log(`Health server listening on port ${config.healthPort}`);
  });
}

/**
 * Hauptfunktion
 */
async function main(): Promise<void> {
  try {
    // Initialisiere Datenbank
    await initDatabase();
    
    // Initialisiere Server
    initWebSocketServer();
    setupHealthEndpoint();
    
    // Starte Welt-Tick
    worldTickTimer = setInterval(worldTick, config.tickInterval);
    
    console.log('Server started successfully');
  } catch (error) {
    console.error('Failed to start server:', error);
    process.exit(1);
  }
}

// Starte den Server
main().catch(console.error);