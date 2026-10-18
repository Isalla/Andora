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
	"VITALS": 24, "INVENTORY_RESULT": 25
}
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
