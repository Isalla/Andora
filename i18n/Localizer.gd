extends Node
# Localizer (Godot 3) — Autoload als "I18n".
# Lädt i18n/<lang>.json, t(key, args) ersetzt {name} etc.
# Fallback: en, dann der Key selbst.

signal language_changed(lang)

var current = "de"
var tables = {}

func _ready():
	_detect_system_lang()
	load_table(current)

func _detect_system_lang():
	var loc = (OS.get_system_language() if OS.has_method("get_system_language") else "de")
	# Godot 3: OS.get_locale_x11() liefert z.B. "de_DE.UTF-8"
	var x = OS.get_locale_x11()
	var code = x.get_slice("_", 0).get_slice(".", 0)
	if code in ["en", "de", "fr", "es", "it"]:
		if has_table(code):
			current = code
	elif code == "zh":
		# Chinesisch: Region entscheidet zwischen zh-Hans und zh-Hant
		var region = x.get_slice("_", 1).get_slice(".", 0)
		var mapped = "zh-Hans"
		if region in ["TW", "HK", "MO"]:
			mapped = "zh-Hant"
		if has_table(mapped):
			current = mapped

func has_table(code):
	return File.new().file_exists("res://i18n/" + code + ".json")

func load_table(code):
	var f = File.new()
	if f.file_exists("res://i18n/" + code + ".json"):
		var txt = f.get_as_text()
		var j = JSON.new()
		if j.parse(txt) == OK:
			tables[code] = j.data

func set_language(code):
	if has_table(code):
		load_table(code)
		current = code
		emit_signal("language_changed", code)
		return true
	return false

func t(key, args = {}):
	if current in tables and tables[current].has(key):
		return _fill(tables[current][key], args)
	if "en" in tables and tables["en"].has(key):
		return _fill(tables["en"][key], args)
	return key

func _fill(text, args):
	var out = text
	for k in args:
		out = out.replace("{" + str(k) + "}", str(args[k]))
	return out
