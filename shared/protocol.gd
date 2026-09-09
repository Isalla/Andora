class_name Protocol
# Spiegelt shared/protocol.js (Message-IDs). Beides MUSS identisch bleiben. (Godot 3.x)

const C2S = {
	"HELLO": 1, "MOVE": 2, "ATTACK": 3, "PICKUP": 4, "CHAT": 5,
	"NPC_TALK": 6, "AUCTION_LIST": 7, "AUCTION_BID": 8, "AUCTION_BUY": 9,
	"HEARTBEAT": 10, "PARENTAL": 11, "ABILITY": 12
}

const S2C = {
	"WELCOME": 1, "SPAWN": 2, "DESPAWN": 3, "STATE": 4, "DAMAGE": 5,
	"KILL": 6, "LOOT": 7, "NPC_TEXT": 8, "CHAT": 9, "LEVELUP": 10,
	"SYNC": 11, "PERFGO": 12,
	"PARENTAL_STATUS": 13, "PARENTAL_BLOCKED": 14, "PARENTAL_RESULT": 15,
	"ABILITY": 16, "EFFECT": 17
}

static func encode(seq: int, msg_type: int, data) -> String:
	return JSON.stringify({"seq": seq, "type": msg_type, "data": data})

static func decode(text: String) -> Dictionary:
	var j = JSON.new()
	if j.parse(text) != OK:
		return {}
	return j.data
