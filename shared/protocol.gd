class_name Protocol
# Message-IDs des Realm-Protokolls (Godot-3.x-Seite). MUSS dieselben
# numerischen IDs wie src/realm-rs/src/protocol.rs verwenden.

const C2S = {
	"HELLO": 1, "MOVE": 2, "ATTACK": 3, "PICKUP": 4, "CHAT": 5,
	"NPC_TALK": 6, "AUCTION_LIST": 7, "AUCTION_BID": 8, "AUCTION_BUY": 9,
	"HEARTBEAT": 10, "PARENTAL": 11, "ABILITY": 12,
	"GROUP_INVITE": 13, "GROUP_INVITE_REACT": 14, "GROUP_SUGGEST": 15,
	"GROUP_SUGGEST_DECIDE": 16, "GROUP_LEAVE": 17, "GROUP_KICK": 18,
	"GROUP_TRANSFER": 19, "SPEND_ATTRIBUTE": 20, "PLAYER_TRADE": 21
}
# NPC_TALK (6) Händler-Payload (docs/Handelssystem.md §13):
# {npc_id, action, item_id?, item_uuid?, history_id?, count?} mit
# action = open/buy/sell/buyback. Buyback nur per history_id (volle Einträge).

const S2C = {
	"WELCOME": 1, "SPAWN": 2, "DESPAWN": 3, "STATE": 4, "DAMAGE": 5,
	"KILL": 6, "LOOT": 7, "NPC_TEXT": 8, "CHAT": 9, "LEVELUP": 10,
	"SYNC": 11, "PERFGO": 12,
	"PARENTAL_STATUS": 13, "PARENTAL_BLOCKED": 14, "PARENTAL_RESULT": 15,
	"ABILITY": 16, "EFFECT": 17,
	"GROUP_INFO": 18, "GROUP_INVITE_S2C": 19, "GROUP_TOAST": 20,
	"ATTRIBUTE_RESULT": 21, "PLAYER_TRADE": 22
}
# NPC_TEXT (8) Händlerantwort (docs/Handelssystem.md §13):
# Erfolg {ok: true, action, npc_id, merchant_name, idia, history[]} plus je
# Aktion offers/item_id/item_uuid/count/total_price; Ablehnung
# {ok: false, action?, reason} mit stabilem reason.
# PLAYER_TRADE (21) Spielerhandel (docs/Handelssystem.md §15):
# {action, dialog_id?, target_id?, items?, idia?, version?} mit
# action = request/accept/decline/offer/confirm/cancel. items ist eine Liste
# aus {item_uuid, count} (nur eigene Instanz-UUIDs), idia >= 0, version nur
# bei confirm (muss der aktuellen Angebotsversion entsprechen).
# PLAYER_TRADE (22) Antwort: {ok, action?, event?, reason?, dialog_id?,
# dialog?, commit_id?, idia?}; seq wird nur zurückgespiegelt.

static func encode(seq: int, msg_type: int, data) -> String:
	return JSON.stringify({"seq": seq, "type": msg_type, "data": data})

static func decode(text: String) -> Dictionary:
	var j = JSON.new()
	if j.parse(text) != OK:
		return {}
	return j.data
