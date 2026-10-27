class_name Protocol
# Message-IDs des Realm-Protokolls (Godot-3.x-Seite). MUSS dieselben
# numerischen IDs wie src/realm-rs/src/protocol.rs verwenden.

const C2S = {
	"HELLO": 1, "MOVE": 2, "ATTACK": 3, "PICKUP": 4, "CHAT": 5,
	"NPC_TALK": 6, "AUCTION_LIST": 7, "AUCTION_BID": 8, "AUCTION_BUY": 9,
	"HEARTBEAT": 10, "PARENTAL": 11, "ABILITY": 12,
	"GROUP_INVITE": 13, "GROUP_INVITE_REACT": 14, "GROUP_SUGGEST": 15,
	"GROUP_SUGGEST_DECIDE": 16, "GROUP_LEAVE": 17, "GROUP_KICK": 18,
	"GROUP_TRANSFER": 19, "SPEND_ATTRIBUTE": 20, "PLAYER_TRADE": 21,
	"INVENTORY": 22
}
# NPC_TALK (6) Händler-Payload (docs/Handelssystem.md §13):
# {npc_id, action, item_id?, item_uuid?, history_id?, count?} mit
# action = open/buy/sell/buyback. Buyback nur per history_id (volle Einträge).
# Zusätzlich Questangebot/-annahme (Quest-System.md §27.27):
# {npc_id, action: quest_offers/quest_accept, quest_id?}; nur Kennungen/Absicht.

const S2C = {
	"WELCOME": 1, "SPAWN": 2, "DESPAWN": 3, "STATE": 4, "DAMAGE": 5,
	"KILL": 6, "LOOT": 7, "NPC_TEXT": 8, "CHAT": 9, "LEVELUP": 10,
	"SYNC": 11, "PERFGO": 12,
	"PARENTAL_STATUS": 13, "PARENTAL_BLOCKED": 14, "PARENTAL_RESULT": 15,
	"ABILITY": 16, "EFFECT": 17,
	"GROUP_INFO": 18, "GROUP_INVITE_S2C": 19, "GROUP_TOAST": 20,
	"ATTRIBUTE_RESULT": 21, "PLAYER_TRADE": 22, "INVENTORY": 23,
	"VITALS": 24, "INVENTORY_RESULT": 25,
	"WEATHER": 26
}
# WELCOME (1) Payload: {you:{id,name,x,y,mana,max_mana,level}, mailboxes:[...]}.
# `mailboxes` ist ein ADDITIVES Feld (Realm Migration 028, M2): reine
# Positionsangaben {id,x,y} in Metern aus der RealmDB, einmalig beim
# Charaktereinstieg; bei leerer Tabelle leere Liste. Kein neuer Frame, keine
# neue Protokoll-ID, kein Decoder-Zwang: fehlendes oder leeres Feld laesst den
# lokalen DECO_MAILBOXES-Fallback des Testclients unveraendert gelten.
# Serverautoritaet unveraendert — keine Reichweiten-, Bestands- oder
# Berechtigungspruefung; Mail-Inhalte kommen weiterhin nur ueber die Mail-API.
# WEATHER (26) Serverautoritatives Wetter (docs/Weltzeit_und_Wettersystem.md):
# Push (seq 0) to all online players on join (after WELCOME) and on change.
# {weather: sunny|rain|fog|storm|snow|cloudy, intensity, sight_factor,
#  season: spring|summer|autumn|winter, revision}. The client only REPRESENTS it
# (visuals + sight-limited rendering); it never decides it.
# Gruppensystem V1 (docs/Gruppensystem.md §§1–9, server: group.rs/handlers.rs):
# C2S 13 GROUP_INVITE {target_id}; 14 GROUP_INVITE_REACT {group_id, accept};
# 15 GROUP_SUGGEST {target_id}; 16 GROUP_SUGGEST_DECIDE {target_id, accept};
# 17 GROUP_LEAVE {}; 18 GROUP_KICK {target_id}; 19 GROUP_TRANSFER {target_id}.
# S2C 18 GROUP_INFO {group_id, leader_id, members:[{id,name,class,level,hp,
# max_hp,mp,max_mp,online,is_leader,in_range,effects}]}; 19 GROUP_INVITE_S2C
# {group_id, from_id, from_name}; 20 GROUP_TOAST {text, kind} (kind "group",
# self-confirm "left"/"kicked"). Chat channel "group": server routes to
# group members distance-independent (GroupManager), never radius-broadcast.
# NPC_TEXT (8) Händlerantwort (docs/Handelssystem.md §13):
# Erfolg {ok: true, action, npc_id, merchant_name, idia, history[]} plus je
# Aktion offers/item_id/item_uuid/count/total_price; Ablehnung
# {ok: false, action?, reason} mit stabilem reason.
# Questantwort: {ok, action, npc_id, quests?, quest_id?, state?, reason?}.
# quests: [{quest_id, title_key, description_key, state: AVAILABLE, can_accept}].
# ACTIVE erst nach bestätigter Persistenz und RAM-Übernahme; seq Korrelation.
# Abgabe-Vorbereitung: NPC_TALK {npc_id, action: quest_turn_ins/quest_turn_in,
# quest_id?}. quest_turn_ins liefert ACTIVE-Killquests mit objectives_met,
# rewards{exp,idia}, can_turn_in:false und reason:quest_turn_in_unavailable.
# Bis zur Persistenzvertragsfreigabe kein mutierender Abschluss/COMPLETED.
# PLAYER_TRADE (21) Spielerhandel (docs/Handelssystem.md §15):
# VITALS (24) eigene Spielerwerte {mana, max_mana, level}, nur Eigentümer:
# Initialwerte in WELCOME (you), danach Push nur bei tatsächlicher Änderung
# (Fähigkeitsverbrauch, Regeneration, Stufenaufstieg). Kein HP (STATE),
# keine Fremdwerte.
# {action, dialog_id?, target_id?, items?, idia?, version?} mit
# action = request/accept/decline/offer/confirm/cancel. items ist eine Liste
# aus {item_uuid, count} (nur eigene Instanz-UUIDs), idia >= 0, version nur
# bei confirm (muss der aktuellen Angebotsversion entsprechen).
# PLAYER_TRADE (22) Antwort: {ok, action?, event?, reason?, dialog_id?,
# dialog?, commit_id?, idia?}; seq wird nur zurückgespiegelt.
# INVENTORY (23) Eigentümer-Sicht (Server-Push): {ok, idia, base_slots,
# slots: [{slot, item}], bags: [{bag_id, name, slot_count, slots}],
# equipment: {Slot: {...}}, loot_id?, reason?}. item je Instanz
# {item_uuid, item_id, name, count, ...}. Push beim Einstieg (nach WELCOME)
# und nach bestätigten Mutationen (Pickup, NPC-Kauf/-Verkauf/-Buyback,
# Spielerhandel-Commit, Inventaraktionen). Pickup antwortet immer (Erfolg
# oder stabile Ablehnung: unknown_loot/not_claimed/out_of_range/
# trade_commit_pending).
# INVENTORY (22) C2S-Absicht (docs/inventory_system.md §19):
# {action, item_uuid?, target_area?, target_slot?, target_bag?, count?,
# slot?} mit action = move/equip/unequip/use. Nur Absichten (Instanz-UUID,
# Zielslot, ggf. Menge); alle Regeln prüft der Server.
# INVENTORY_RESULT (25) Antwort: {ok, action?, reason?, item_uuid?, slot?,
# count?} mit stabilem reason; Erfolg pusht danach INVENTORY (seq 0).

static func encode(seq: int, msg_type: int, data) -> String:
	return to_json({"seq": seq, "type": msg_type, "data": data})

static func decode(text: String) -> Dictionary:
	# Godot 3.6: JSON ist nicht instanziierbar (kein JSON.new());
	# statisches JSON.parse liefert ein Ergebnisobjekt mit .error/.result.
	var r = JSON.parse(text)
	if r.error != OK:
		return {}
	if typeof(r.result) == TYPE_DICTIONARY:
		return r.result
	return {}

# ---------- Quest-Protokoll V1.3 (docs/Quest-Protokoll-V1.3.md) ----------
# C2S (Godot->Rust) ueber NPC_TALK (6); S2C (Rust->Godot) als Payload in
# NPC_TEXT (8). Der Client sendet nur Absichten; die Server allein entscheidet
# (Validationspfad, InventoryState.try_remove, guarded_complete_quest,
# spool::publish_quest_completion). Alle Decoders are seq-Korrelation-tolerant:
# they recognise a NPC_TEXT/NPC_TALK-Frame by type+action, otherwise {}.

# C2S_quest_turn_in (NPC_TALK 6): {npc_id, action:'quest_turn_in', quest_id,
# npc_content_id?, item_uuids?:Array<String>}. npc_id = NPC-Instanz-ID (Server
# interaction); npc_content_id = stabile Content-ID (V1.3-Absicht, Server
# vergleich Npc.content_id); item_uuids = Absicht-List (Server entscheidet).
static func encode_quest_turn_in_frame(seq: int, npc_id: String, quest_id: String, npc_content_id: String = "", item_uuids: Array = []) -> String:
	var data = {
		"npc_id": npc_id,
		"action": "quest_turn_in",
		"quest_id": quest_id,
	}
	if npc_content_id != "":
		(data)["npc_content_id"] = npc_content_id
	if not item_uuids.empty():
		(data)["item_uuids"] = item_uuids
	return encode(seq, C2S["NPC_TALK"], data)

# C2S_quest_accept (NPC_TALK 6): {npc_id, action:'quest_accept', quest_id}.
static func encode_quest_accept_frame(seq: int, npc_id: String, quest_id: String) -> String:
		return encode(seq, C2S["NPC_TALK"], {
			"npc_id": npc_id,
			"action": "quest_accept",
			"quest_id": quest_id,
		})

# decode_quest_offer_frame: S2C NPC_TEXT (8) action 'quest_offers' ->
# quest_offer-Payload (Array {quest_id,title_key,description_key,state,can_accept}).
# Non-matching -> [].
static func decode_quest_offer_frame(frame: Dictionary) -> Array:
		if int(frame.get("type", 0)) != S2C["NPC_TEXT"]:
			return []
		var d = frame.get("data", {})
		if typeof(d) != TYPE_DICTIONARY:
			return []
		if str(d.get("action", "")) != "quest_offers":
			return []
		var qs = d.get("quests", [])
		return qs if typeof(qs) == TYPE_ARRAY else []

# decode_quest_turn_in_frame: C2S NPC_TALK (6) turn-in -> {npc_id, action,
# quest_id, item_uuids}. Validate type+action+non-empty npc_id/quest_id.
# Invalid -> {}.
static func decode_quest_turn_in_frame(frame: Dictionary) -> Dictionary:
		if int(frame.get("type", 0)) != C2S["NPC_TALK"]:
			return {}
		var d = frame.get("data", {})
		if typeof(d) != TYPE_DICTIONARY:
			return {}
		if str(d.get("action", "")) != "quest_turn_in":
			return {}
		var npc_id = str(d.get("npc_id", ""))
		var quest_id = str(d.get("quest_id", ""))
		if npc_id == "" or quest_id == "":
			return {}
		var out = {
			"npc_id": npc_id,
			"action": "quest_turn_in",
			"quest_id": quest_id,
		}
		var iu = d.get("item_uuids", [])
		(out)["item_uuids"] = iu if typeof(iu) == TYPE_ARRAY else []
		if d.has("npc_content_id"):
			(out)["npc_content_id"] = str(d.get("npc_content_id", ""))
		return out

# decode_quest_text_frame: S2C NPC_TEXT (8) action 'quest_turn_in' ->
# quest_turn_in_result-Payload {ok, state?, rewards?, receipt_id?, reason?,
# error?}. rewards carries the V1 fields exp/idia (V1.3 alias: experience=exp,
# gold=idia; items still open, see Quest-System.md 27.26). Non-matching -> {}.
static func decode_quest_text_frame(frame: Dictionary) -> Dictionary:
		if int(frame.get("type", 0)) != S2C["NPC_TEXT"]:
			return {}
		var d = frame.get("data", {})
		if typeof(d) != TYPE_DICTIONARY:
			return {}
		if str(d.get("action", "")) != "quest_turn_in":
			return {}
		var out = {
			"ok": bool(d.get("ok", false)),
			"quest_id": str(d.get("quest_id", "")),
		}
		if bool(d.get("ok", false)):
			(out)["state"] = str(d.get("state", ""))
			(out)["rewards"] = d.get("rewards", {})
			if d.has("receipt_id"):
			(out)["receipt_id"] = str(d.get("receipt_id", ""))
		else:
			(out)["reason"] = str(d.get("reason", ""))
			(out)["error"] = str(d.get("error", d.get("reason", "")))
		return out
