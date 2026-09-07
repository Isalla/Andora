// SHARED PROTOCOL — einzige Quelle für Message-Typen und IDs.
// Wird vom Node-Server (direct import) UND auf dem Godot-Client (gespiegelt in shared/protocol.gd) referenziert.
// Regelt: ID = integer, payload = JS-Objekt/Dictionary. Encoding über Socket: JSON-Frame
// {seq, type, data}. JSON hier ist OK, da Payloads klein sind (Positions-Pakete ~60 Byte).

export const C2S = {
  HELLO: 1,        // {session_id, handoff_token, char_id, lang}
  MOVE: 2,         // {dir:[x,y], seq}
  ATTACK: 3,       // {target_id} start / {stop: true} beenden (Combat V1)
  PICKUP: 4,       // {item_id}
  CHAT: 5,         // {channel, text}
  NPC_TALK: 6,     // {npc_id, text}
  AUCTION_LIST: 7, // {item_id, count, buyout, bid}
  AUCTION_BID: 8,  // {auction_id, amount}
  AUCTION_BUY: 9,  // {auction_id}
  HEARTBEAT: 10,
  PARENTAL: 11,  // {action, pin}  (In-Game-Elternpanel: extend/unlock_chat/unlock_voice)
}

export const S2C = {
  WELCOME: 1,   // {server_tick, time, you:{...}}
  SPAWN: 2,     // {id, kind, x, y, face, extra}
  DESPAWN: 3,   // {id}
  STATE: 4,     // {id, x, y, face, anim, hp, max_hp}
  DAMAGE: 5,    // {id, amount, from_id, hit} (hit: miss/dodge/parry/block/normal/crit)
  KILL: 6,      // {id, killer_id}
  LOOT: 7,      // {item_id, x, y}
  NPC_TEXT: 8,  // {npc_id, text}       (Text = i18n-KEY oder freier Text)
  CHAT: 9,      // {from, channel, text}
  LEVELUP: 10,  // {level, hp, mana}
  SYNC: 11,     // {ack_seq}            (Server bestätigt letzten Client-seq)
  PERFGO: 12,   // {level}              (0=volle Qual., 1=ohne Particles, 2=minimal)
  PARENTAL_STATUS: 13,  // {remaining_seconds, blocked, buffer_until, warning, chat_allowed, voice_allowed, extended_used_today}
  PARENTAL_BLOCKED: 14, // {reason}     (blocked / buffer_expired -> Logout)
  PARENTAL_RESULT: 15,  // {ok, reason?, unlocked?, remaining_seconds?} (Antwort aufs Elternpanel)
}

// Renderer-Auswahl auf dem Client, wenn RENDER_CAP überschritten:
// 1 eigenes Char > 2 Gilden-Mitglieder > 3 Combat-Ziele > 4 Distanz (nächste zuerst)
