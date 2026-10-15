extends Reference
# Wunsch -> Bau-Spezifikation (Godot 3.6). Zwei Wege:
# 1) Ollama (lokal, plain HTTP, konfigurierbar): Die KI uebersetzt den
#    Wunsch in Spec-JSON. Die Antwort wird strikt validiert (fail-closed);
#    ungueltig -> Rueckfall auf Weg 2, niemals Backen mit Ratespiel.
# 2) Deterministischer Stichwort-Parser (deutsch): immer verfuegbar,
#    offline testbar, dokumentiertes Verhalten.
# Die KI generiert KEINE Meshes — sie konfiguriert den prozeduralen
# Generator (builder.gd). Was der Generator nicht kennt, kann die KI
# nicht herbeizaubern (Schema ist der Vertrag).

const PRESETS = {
	"human": {"skin": [0.85, 0.7, 0.55], "height": 1.0, "bulk": 1.0},
	"ork": {"skin": [0.35, 0.6, 0.3], "height": 1.05, "bulk": 1.25},
	"elf": {"skin": [0.92, 0.8, 0.65], "height": 1.05, "bulk": 0.85},
	"dwarf": {"skin": [0.8, 0.62, 0.48], "height": 0.85, "bulk": 1.2},
}

const CLIPS = ["walk", "idle", "attack"]
const WEAPONS = ["none", "sword", "axe", "bow", "staff"]
const HELMETS = ["none", "cap", "helm", "hood"]

func default_spec():
	return {
		"version": 1,
		"body": {"preset": "human", "height": 1.0, "bulk": 1.0,
			"skin": [0.85, 0.7, 0.55], "cloth": [0.3, 0.35, 0.5],
			"trim": [0.7, 0.6, 0.3]},
		"equipment": {"weapon": "none", "shield": false,
			"helmet": "none", "quiver": false},
		"anim": {"clip": "walk", "frames": 8},
	}

# ---------- Validierung (fail-closed) ----------

func _is_num(x):
	return typeof(x) == TYPE_INT or typeof(x) == TYPE_REAL

func _is_color(c):
	if typeof(c) != TYPE_ARRAY or c.size() != 3:
		return false
	for v in c:
		if not _is_num(v) or v < 0.0 or v > 1.0:
			return false
	return true

func validate_spec(spec):
	# Gibt "" bei gueltig zurueck, sonst einen stabilen Grund.
	if typeof(spec) != TYPE_DICTIONARY:
		return "not_an_object"
	if int(spec.get("version", -1)) != 1:
		return "bad_version"
	var keys = spec.keys()
	keys.sort()
	if keys != ["anim", "body", "equipment", "version"]:
		return "bad_keys"
	var body = spec["body"]
	if typeof(body) != TYPE_DICTIONARY:
		return "bad_body"
	if not PRESETS.has(str(body.get("preset", ""))):
		return "bad_preset"
	if not _is_num(body.get("height", -1)) or body["height"] < 0.8 or body["height"] > 1.3:
		return "bad_height"
	if not _is_num(body.get("bulk", -1)) or body["bulk"] < 0.8 or body["bulk"] > 1.4:
		return "bad_bulk"
	for k in ["skin", "cloth", "trim"]:
		if not _is_color(body.get(k, null)):
			return "bad_color"
	var eq = spec["equipment"]
	if typeof(eq) != TYPE_DICTIONARY:
		return "bad_equipment"
	if not (str(eq.get("weapon", "")) in WEAPONS):
		return "bad_weapon"
	if typeof(eq.get("shield", null)) != TYPE_BOOL:
		return "bad_shield"
	if not (str(eq.get("helmet", "")) in HELMETS):
		return "bad_helmet"
	if typeof(eq.get("quiver", null)) != TYPE_BOOL:
		return "bad_quiver"
	var anim = spec["anim"]
	if typeof(anim) != TYPE_DICTIONARY:
		return "bad_anim"
	if not (str(anim.get("clip", "")) in CLIPS):
		return "bad_clip"
	if not _is_num(anim.get("frames", -1)) or int(anim["frames"]) < 1 or int(anim["frames"]) > 16:
		return "bad_frames"
	return ""

# ---------- Stichwort-Parser (deterministisch) ----------

func wish_to_spec_local(wish):
	var spec = default_spec()
	var body = spec["body"]
	var eq = spec["equipment"]
	var anim = spec["anim"]
	var hay = " " + str(wish).to_lower().replace("-", " ") + " "
	# Rasse (erste Nennung gewinnt).
	for p in [["ork", "ork"], ["orc", "ork"], ["elf", "elf"],
			["zwerg", "dwarf"], ["dwarf", "dwarf"], ["mensch", "human"],
			["human", "human"]]:
		if hay.find(" " + p[0]) >= 0 or hay.find(p[0] + " ") >= 0 or hay.find(" " + p[0] + " ") >= 0:
			body["preset"] = p[1]
			var pre = PRESETS[p[1]]
			body["skin"] = pre["skin"]
			body["height"] = pre["height"]
			body["bulk"] = pre["bulk"]
			break
	# Farben (explizit schlaegt Preset).
	var colors = {
		"rot": [0.65, 0.15, 0.15], "red": [0.65, 0.15, 0.15],
		"blau": [0.2, 0.3, 0.65], "blue": [0.2, 0.3, 0.65],
		"gruen": [0.25, 0.5, 0.25], "grün": [0.25, 0.5, 0.25], "green": [0.25, 0.5, 0.25],
		"schwarz": [0.12, 0.12, 0.14], "black": [0.12, 0.12, 0.14],
		"weiss": [0.85, 0.85, 0.88], "weiß": [0.85, 0.85, 0.88],
		"gold": [0.8, 0.62, 0.2], "silber": [0.7, 0.72, 0.78],
		"braun": [0.4, 0.28, 0.16], "brown": [0.4, 0.28, 0.16],
	}
	for w in colors.keys():
		if hay.find(w) >= 0:
			if w in ["gold", "silber"]:
				body["trim"] = colors[w]
			else:
				body["cloth"] = colors[w]
	# Ausruestung.
	if hay.find("axt") >= 0 or hay.find("axe") >= 0:
		eq["weapon"] = "axe"
	elif hay.find("schwert") >= 0 or hay.find("sword") >= 0:
		eq["weapon"] = "sword"
	elif hay.find("bogen") >= 0 or hay.find("bow") >= 0:
		eq["weapon"] = "bow"
		eq["quiver"] = true
	elif hay.find("stab") >= 0 or hay.find("staff") >= 0:
		eq["weapon"] = "staff"
	if hay.find("schild") >= 0 or hay.find("shield") >= 0:
		eq["shield"] = true
	if hay.find("helm") >= 0 or hay.find("helmet") >= 0:
		eq["helmet"] = "helm"
	elif hay.find("kappe") >= 0 or hay.find("cap") >= 0:
		eq["helmet"] = "cap"
	elif hay.find("kapuze") >= 0 or hay.find("hood") >= 0:
		eq["helmet"] = "hood"
	# Animation.
	if hay.find("angriff") >= 0 or hay.find("attack") >= 0 or hay.find("kampf") >= 0:
		anim["clip"] = "attack"
		anim["frames"] = 6
	elif hay.find("steh") >= 0 or hay.find("idle") >= 0 or hay.find("wache") >= 0:
		anim["clip"] = "idle"
		anim["frames"] = 4
	else:
		anim["clip"] = "walk"
		anim["frames"] = 8
	return spec

# ---------- Ollama (Godot-3.6-HTTP, mit Timeout, fail-closed) ----------

func llm_prompt(wish):
	return "Du bist ein Figurengenerator-Uebersetzer. Antworte NUR mit JSON, " \
		+ "kein anderer Text. Schema (exakt diese Schluessel, keine weiteren): " \
		+ '{"version":1,"body":{"preset":"human|ork|elf|dwarf","height":0.8-1.3,' \
		+ '"bulk":0.8-1.4,"skin":[r,g,b],"cloth":[r,g,b],"trim":[r,g,b]},' \
		+ '"equipment":{"weapon":"none|sword|axe|bow|staff","shield":bool,' \
		+ '"helmet":"none|cap|helm|hood","quiver":bool},' \
		+ '"anim":{"clip":"walk|idle|attack","frames":1-16}}. ' \
		+ "Farben als 0.0-1.0. Wunsch: " + str(wish)

func extract_json(text):
	var s = str(text)
	var a = s.find("{")
	var b = s.rfind("}")
	if a < 0 or b < 0 or b <= a:
		return {}
	var r = JSON.parse(s.substr(a, b - a + 1))
	if r.error != OK or typeof(r.result) != TYPE_DICTIONARY:
		return {}
	return r.result

func llm_request_body(wish, model):
	# Ollama-Backend (nativ /api/generate). Spec-Aufrufe immer ohne
	# Thinking (Modell denkt per Default; Karte: Instruct-Modus ohne
	# Thinking, niedrige Temperatur, deterministisches JSON).
	return to_json({"model": str(model), "prompt": llm_prompt(wish),
		"stream": false, "think": false,
		"options": {"temperature": 0.2, "top_p": 0.8}})

func openai_request_body(wish, model):
	# OpenAI-kompatibles Backend (z. B. Strata auf localhost):
	# /v1/chat/completions. Kein "think"-Schalter (providerneutral) —
	# stattdessen explizite JSON-Anweisung + niedrige Temperatur; die
	# Antwort wird ohnehin strikt validiert (fail-closed).
	return to_json({"model": str(model),
		"messages": [
			{"role": "system", "content": "Du antwortest NUR mit einem "
				+ "einzigen JSON-Objekt nach dem bekannten Schema, ohne "
				+ "Erklaerungen, ohne Markdown, ohne Denkspuren."},
			{"role": "user", "content": llm_prompt(wish)}],
		"temperature": 0.2, "top_p": 0.8, "stream": false})

func openai_parse_response(body):
	# body: geparstes JSON von /v1/chat/completions. Gibt {ok, spec?/reason?}.
	if typeof(body) != TYPE_DICTIONARY:
		return {"ok": false, "reason": "bad_envelope"}
	var choices = body.get("choices", [])
	if typeof(choices) != TYPE_ARRAY or choices.empty():
		return {"ok": false, "reason": "no_choices"}
	var first = choices[0]
	if typeof(first) != TYPE_DICTIONARY:
		return {"ok": false, "reason": "bad_choice"}
	var msg = first.get("message", {})
	if typeof(msg) != TYPE_DICTIONARY:
		return {"ok": false, "reason": "bad_message"}
	return llm_parse_response_text(str(msg.get("content", "")))

func llm_parse_response_text(text):
	# Extrahiert JSON, validiert strikt. Gueltig -> {ok:true, spec};
	# sonst {ok:false, reason} (Antwort nie ungeprueft verwenden).
	var d = extract_json(text)
	if d.empty():
		return {"ok": false, "reason": "no_json"}
	var reason = validate_spec(d)
	if reason != "":
		return {"ok": false, "reason": reason}
	return {"ok": true, "spec": d}
