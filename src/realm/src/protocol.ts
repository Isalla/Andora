// SERVER-SEITIGES PROTOTOKOLL (CommonJS-kompatibel, via tsc).
// WICHTIG: IDs hier und in shared/protocol.js (Client-Referenz)
// und shared/protocol.gd (Godot) müßen IDENTISCH bleiben.
// Quelle der Wahrheit: shared/protocol.js — bei Änderungen hier anpassen!

export const C2S = {
  HELLO: 1, MOVE: 2, ATTACK: 3, PICKUP: 4, CHAT: 5,
  NPC_TALK: 6, AUCTION_LIST: 7, AUCTION_BID: 8, AUCTION_BUY: 9,
  HEARTBEAT: 10
};

export const S2C = {
  WELCOME: 1, SPAWN: 2, DESPAWN: 3, STATE: 4, DAMAGE: 5,
  KILL: 6, LOOT: 7, NPC_TEXT: 8, CHAT: 9, LEVELUP: 10,
  SYNC: 11, PERFGO: 12
};
