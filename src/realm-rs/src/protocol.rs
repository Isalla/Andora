// protocol — Nachrichten-IDs des Realm-Protokolls.
// Hier ist die Quelle der Wahrheit der Serverseite. Der Godot-Client
// (shared/protocol.gd) MUSS dieselben numerischen IDs verwenden; keine
// JavaScript-Protokollquelle mehr. Bei Änderungen hier UND dort anpassen.

/// Client → Server. Vollständige ID-Liste (auch künftige Typen):
/// IDs müssen shared/protocol.gd (Godot-Client) entsprechen, kein Eintrag entfernen.
///
#[allow(dead_code)]
pub mod c2s {
    pub const HELLO: i64 = 1; // {session_id?, handoff_token?, char_id, lang}
    pub const MOVE: i64 = 2; // {dir:[x,y], seq} oder {x, y}
    pub const ATTACK: i64 = 3; // {target_id} start / {stop: true} beenden (Combat V1)
    pub const PICKUP: i64 = 4; // {loot_id} (Loot System V1)
    pub const CHAT: i64 = 5; // {channel, text}
    pub const NPC_TALK: i64 = 6; // {npc_id, text} (künftig)
    pub const AUCTION_LIST: i64 = 7; // (künftig)
    pub const AUCTION_BID: i64 = 8; // (künftig)
    pub const AUCTION_BUY: i64 = 9; // (künftig)
    pub const HEARTBEAT: i64 = 10;
    pub const PARENTAL: i64 = 11; // {action, pin}
    pub const ABILITY: i64 = 12; // {ability_id, target_id?, x?, y?} (Combat V3)
    pub const GROUP_INVITE: i64 = 13; // {target_id}
    pub const GROUP_INVITE_REACT: i64 = 14; // {group_id, accept: bool}
    pub const GROUP_SUGGEST: i64 = 15; // {target_id}
    pub const GROUP_SUGGEST_DECIDE: i64 = 16; // {target_id, accept: bool}
    pub const GROUP_LEAVE: i64 = 17;
    pub const GROUP_KICK: i64 = 18; // {target_id}
    pub const GROUP_TRANSFER: i64 = 19; // {target_id}
}

/// Server → Client. Vollständige ID-Liste (auch künftige Typen):
/// IDs müssen shared/protocol.gd (Godot-Client) entsprechen, kein Eintrag entfernen.
///
#[allow(dead_code)]
pub mod s2c {
    pub const WELCOME: i64 = 1; // {you:{id,name,x,y}}
    pub const SPAWN: i64 = 2; // {id, kind, x, y, face}; kind player|npc; npc: extra {status, aggro, claimed, name}
    pub const DESPAWN: i64 = 3; // {id}
    pub const STATE: i64 = 4; // {id, x, y, face}; Spieler + {hp, max_hp}; NPC zusätzlich {kind, status, aggro, claimed}
    pub const DAMAGE: i64 = 5; // {id, amount, from_id, hit} (hit: miss/dodge/parry/block/normal/crit)
    pub const KILL: i64 = 6; // {id, killer_id}
    pub const LOOT: i64 = 7; // {id, kind, x, y, claimed[, item_id, count | gold]} (Loot System V1)
    pub const NPC_TEXT: i64 = 8; // (künftig)
    pub const CHAT: i64 = 9; // {from, channel, text}
    pub const LEVELUP: i64 = 10; // (künftig)
    pub const SYNC: i64 = 11; // {ack_seq}
    pub const PERFGO: i64 = 12; // (künftig)
    pub const PARENTAL_STATUS: i64 = 13;
    pub const PARENTAL_BLOCKED: i64 = 14; // {reason}
    pub const PARENTAL_RESULT: i64 = 15; // {ok, reason?, unlocked?, ...}
    pub const ABILITY: i64 = 16; // {caster_id, ability_id, outcome, reason?, target_id?} (Combat V3)
    pub const EFFECT: i64 = 17; // {entity_id, action, effect_id, group, kind, duration_left_ms} (Combat V3)
    pub const GROUP_INFO: i64 = 18; // {group_id, leader_id, members:[{id,name,class,level,hp,max_hp,mp,max_mp,online,in_range,effects}]}
    pub const GROUP_INVITE_S2C: i64 = 19; // {group_id, from_id}
    pub const GROUP_TOAST: i64 = 20; // {text, kind}
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

    // IDs müssen shared/protocol.gd (Godot-Client) entsprechen.
    #[test]
    fn ids_match_shared_protocol() {
        assert_eq!(c2s::HELLO, 1);
        assert_eq!(c2s::MOVE, 2);
        assert_eq!(c2s::CHAT, 5);
        assert_eq!(c2s::HEARTBEAT, 10);
        assert_eq!(c2s::PARENTAL, 11);
        assert_eq!(c2s::ABILITY, 12);
        assert_eq!(s2c::WELCOME, 1);
        assert_eq!(s2c::SPAWN, 2);
        assert_eq!(s2c::DESPAWN, 3);
        assert_eq!(s2c::STATE, 4);
        assert_eq!(s2c::CHAT, 9);
        assert_eq!(s2c::SYNC, 11);
        assert_eq!(s2c::PARENTAL_STATUS, 13);
        assert_eq!(s2c::PARENTAL_BLOCKED, 14);
        assert_eq!(s2c::PARENTAL_RESULT, 15);
        assert_eq!(s2c::ABILITY, 16);
        assert_eq!(s2c::EFFECT, 17);
        assert_eq!(s2c::GROUP_INFO, 18);
        assert_eq!(s2c::GROUP_INVITE_S2C, 19);
        assert_eq!(s2c::GROUP_TOAST, 20);
        assert_eq!(c2s::GROUP_INVITE, 13);
        assert_eq!(c2s::GROUP_INVITE_REACT, 14);
        assert_eq!(c2s::GROUP_SUGGEST, 15);
        assert_eq!(c2s::GROUP_SUGGEST_DECIDE, 16);
        assert_eq!(c2s::GROUP_LEAVE, 17);
        assert_eq!(c2s::GROUP_KICK, 18);
        assert_eq!(c2s::GROUP_TRANSFER, 19);
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
