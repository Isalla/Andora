// protocol — Nachrichten-IDs des Realm-Protokolls.
// Quelle der Wahrheit: shared/protocol.js (Client-Referenz, Godot in
// shared/protocol.gd gespiegelt). IDs müssen dort IDENTISCH bleiben;
// bei Änderungen hier UND dort anpassen.

/// Client → Server. Vollständige ID-Liste (auch künftige Typen):
/// IDs müssen shared/protocol.js entsprechen, kein Eintrag entfernen.
///
#[allow(dead_code)]
pub mod c2s {
    pub const HELLO: i64 = 1; // {session_id?, handoff_token?, char_id, lang}
    pub const MOVE: i64 = 2; // {dir:[x,y], seq} oder {x, y}
    pub const ATTACK: i64 = 3; // {target_id} start / {stop: true} beenden (Combat V1)
    pub const PICKUP: i64 = 4; // {item_id} (künftig)
    pub const CHAT: i64 = 5; // {channel, text}
    pub const NPC_TALK: i64 = 6; // {npc_id, text} (künftig)
    pub const AUCTION_LIST: i64 = 7; // (künftig)
    pub const AUCTION_BID: i64 = 8; // (künftig)
    pub const AUCTION_BUY: i64 = 9; // (künftig)
    pub const HEARTBEAT: i64 = 10;
    pub const PARENTAL: i64 = 11; // {action, pin}
}

/// Server → Client. Vollständige ID-Liste (auch künftige Typen):
/// IDs müssen shared/protocol.js entsprechen, kein Eintrag entfernen.
///
#[allow(dead_code)]
pub mod s2c {
    pub const WELCOME: i64 = 1; // {you:{id,name,x,y}}
    pub const SPAWN: i64 = 2; // {id, kind, x, y, face}; kind player|npc; npc: extra {status, aggro, claimed, name}
    pub const DESPAWN: i64 = 3; // {id}
    pub const STATE: i64 = 4; // {id, x, y, face}; Spieler + {hp, max_hp}; NPC zusätzlich {kind, status, aggro, claimed}
    pub const DAMAGE: i64 = 5; // {id, amount, from_id, hit} (hit: miss/dodge/parry/block/normal/crit)
    pub const KILL: i64 = 6; // {id, killer_id}
    pub const LOOT: i64 = 7; // (künftig)
    pub const NPC_TEXT: i64 = 8; // (künftig)
    pub const CHAT: i64 = 9; // {from, channel, text}
    pub const LEVELUP: i64 = 10; // (künftig)
    pub const SYNC: i64 = 11; // {ack_seq}
    pub const PERFGO: i64 = 12; // (künftig)
    pub const PARENTAL_STATUS: i64 = 13;
    pub const PARENTAL_BLOCKED: i64 = 14; // {reason}
    pub const PARENTAL_RESULT: i64 = 15; // {ok, reason?, unlocked?, ...}
}

/// Drahtformat einer Nachricht: {seq, type, data} als JSON-Frame.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Frame {
    #[serde(default)]
    pub seq: i64,
    #[serde(rename = "type")]
    pub msg_type: i64,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl Frame {
    pub fn new(seq: i64, msg_type: i64, data: serde_json::Value) -> Self {
        Self {
            seq,
            msg_type,
            data,
        }
    }

    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"seq":0,"type":0,"data":{}}"#.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // IDs müssen shared/protocol.js entsprechen (Quelle der Wahrheit).
    #[test]
    fn ids_match_shared_protocol() {
        assert_eq!(c2s::HELLO, 1);
        assert_eq!(c2s::MOVE, 2);
        assert_eq!(c2s::CHAT, 5);
        assert_eq!(c2s::HEARTBEAT, 10);
        assert_eq!(c2s::PARENTAL, 11);
        assert_eq!(s2c::WELCOME, 1);
        assert_eq!(s2c::SPAWN, 2);
        assert_eq!(s2c::DESPAWN, 3);
        assert_eq!(s2c::STATE, 4);
        assert_eq!(s2c::CHAT, 9);
        assert_eq!(s2c::SYNC, 11);
        assert_eq!(s2c::PARENTAL_STATUS, 13);
        assert_eq!(s2c::PARENTAL_BLOCKED, 14);
        assert_eq!(s2c::PARENTAL_RESULT, 15);
    }

    #[test]
    fn frame_roundtrip() {
        let f = Frame::new(7, s2c::SYNC, serde_json::json!({"ack_seq": 7}));
        let s = f.encode();
        let back: Frame = serde_json::from_str(&s).unwrap();
        assert_eq!(back.seq, 7);
        assert_eq!(back.msg_type, s2c::SYNC);
    }
}
