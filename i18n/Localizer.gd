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
	# Godot 3.6: weder get_system_language noch get_locale_x11 existieren;
	# OS.get_locale() liefert z.B. "de_DE".
	var x = OS.get_locale() if OS.has_method("get_locale") else "de"
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
	if f.open("res://i18n/" + code + ".json", File.READ) == OK:
		var txt = f.get_as_text()
		f.close()
		var r = JSON.parse(txt)
		if r.error == OK and typeof(r.result) == TYPE_DICTIONARY:
			tables[code] = r.result

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
