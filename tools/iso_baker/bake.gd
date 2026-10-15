extends SceneTree
# Iso-Baker (Godot 3.6, headless Batch): Wunsch/Spec -> 3D-Figur ->
# Iso-Sprites (8 Richtungen x Frames) + Atlas-JSON + Spritesheet.
# Aufruf (projektlokal, mit Anzeige, z. B. Xvfb):
#   godot --no-window --audio-driver Dummy --path . \
#     --script res://tools/iso_baker/bake.gd -- \
#     --wish "Ork mit Axt" --out .tmp/iso_bake --size 128
# Args (--key=value): wish, spec_json, spec_file, out (.tmp/iso_bake),
#   size (128), dirs (1/4/8), tilt (30), name, no_llm,
#   llm_backend (ollama|openai, Default ollama),
#   llm_url (Default ollama: http://localhost:11434; openai: Pflicht),
#   llm_model (Pflicht bei LLM-Nutzung; Aliase ollama_url/ollama_model),
#   ollama_timeout_ms (10000).
# Exit: 0 ok, 2 Eingabe/Spec ungueltig, 3 Bake-Fehler.

var _llm_done = false
var _llm_text = ""

func args():
	# Beide Formen: --key=value und --key value (naechstes Token, wenn es
	# kein --Schalter ist). Engine-Args ohne -- werden ignoriert.
	var out = {}
	var raw = OS.get_cmdline_args()
	var i = 0
	while i < raw.size():
		var a = str(raw[i])
		if a.begins_with("--") and a.find("=") > 2:
			out[a.substr(2, a.find("=") - 2).replace("-", "_")] = a.substr(a.find("=") + 1)
		elif a.begins_with("--") and a.length() > 2 and i + 1 < raw.size() \
			and not str(raw[i + 1]).begins_with("--"):
			out[a.substr(2).replace("-", "_")] = str(raw[i + 1])
			i += 1
		elif a == "--no_llm" or a == "--no-llm":
			out["no_llm"] = "1"
		i += 1
	return out

func slug(s):
	var r = ""
	for i in range(str(s).length()):
		var ch = str(s).substr(i, 1).to_lower()
		if (ch >= "a" and ch <= "z") or (ch >= "0" and ch <= "9"):
			r += ch
		elif r != "" and not r.ends_with("_"):
			r += "_"
	while r.ends_with("_"):
		r = r.left(r.length() - 1)
	if r == "":
		r = "figur"
	return r.left(32)

func fail(code, msg):
	printerr("BAKE FAIL: ", msg)
	quit(code)

func _init():
	call_deferred("run")

func run():
	var a = args()
	var out_base = str(a.get("out", ".tmp/iso_bake"))
	var size = int(a.get("size", 128))
	var dirs = int(a.get("dirs", 8))
	var tilt = float(a.get("tilt", 30.0))
	if not (dirs in [1, 4, 8]) or size < 32 or size > 512:
		fail(2, "dirs in {1,4,8}, size 32..512")
		return
	var Wishes = load("res://tools/iso_baker/wish_to_spec.gd").new()
	# Spec aufloesen: spec_json > spec_file > wish (+LLM) > Demo.
	var spec = {}
	var source = "demo"
	if a.has("spec_json"):
		var r = JSON.parse(str(a["spec_json"]))
		if r.error != OK or typeof(r.result) != TYPE_DICTIONARY:
			fail(2, "spec_json unlesbar")
			return
		spec = r.result
		source = "spec_json"
	elif a.has("spec_file"):
		var f = File.new()
		if f.open(str(a["spec_file"]), File.READ) != OK:
			fail(2, "spec_file unlesbar")
			return
		var r = JSON.parse(f.get_as_text())
		f.close()
		if r.error != OK or typeof(r.result) != TYPE_DICTIONARY:
			fail(2, "spec_file kein JSON-Objekt")
			return
		spec = r.result
		source = "spec_file"
	else:
		var wish = str(a.get("wish", "Mensch"))
		if not a.has("no_llm"):
			var backend = str(a.get("llm_backend", "ollama"))
			if not (backend in ["ollama", "openai"]):
				fail(2, "llm_backend in {ollama,openai}")
				return
			var url = str(a.get("llm_url", a.get("ollama_url", "")))
			if url == "":
				if backend == "ollama":
					url = "http://localhost:11434"
				else:
					fail(2, "llm_url fuer openai explizit angeben (kein Port geraten)")
					return
			var llm_model = str(a.get("llm_model", a.get("ollama_model", "")))
			if llm_model == "":
				fail(2, "llm_model explizit angeben (kein Modell geraten)")
				return
			var llm = yield(_try_llm(Wishes, wish, backend, url, llm_model,
				int(a.get("ollama_timeout_ms", 10000))), "completed")
			if typeof(llm) == TYPE_DICTIONARY and bool(llm.get("ok", false)):
				spec = llm["spec"]
				source = "llm"
			else:
				spec = Wishes.wish_to_spec_local(wish)
				source = "keyword"
		else:
			spec = Wishes.wish_to_spec_local(wish)
			source = "keyword"
		if source == "demo":
			source = "keyword"
	var reason = Wishes.validate_spec(spec)
	if reason != "":
		fail(2, "spec ungueltig: " + reason)
		return
	var name = slug(str(a.get("name", "fig_" + slug(str(a.get("wish", "mensch"))))))
	var ok = yield(_bake_all(spec, source, str(a.get("wish", "")), name,
		out_base, size, dirs, tilt), "completed")
	if not ok:
		fail(3, "bake abgebrochen")
		return
	print("BAKE OK: ", name, " (spec: ", source, ")")
	quit(0)

func _try_llm(wishes, wish, backend, url, model, timeout_ms):
	# Gibt {ok, spec} oder {ok:false} zurueck (niemals Crash, niemals
	# ungepruefte Spec). Backend "ollama" (nativ) oder "openai"
	# (kompatibel, z. B. Strata auf localhost).
	var path = "/api/generate"
	var body = wishes.llm_request_body(wish, model)
	if backend == "openai":
		path = "/v1/chat/completions"
		body = wishes.openai_request_body(wish, model)
	var req = HTTPRequest.new()
	get_root().add_child(req)
	req.connect("request_completed", self, "_on_llm_done")
	_llm_done = false
	_llm_text = ""
	var err = req.request(url + path,
		["Content-Type: application/json"], false,
		HTTPClient.METHOD_POST, body)
	if err != OK:
		req.queue_free()
		return {"ok": false}
	var waited = 0
	while not _llm_done and waited < timeout_ms:
		yield(self, "idle_frame")
		waited += 1000 / 30
	req.queue_free()
	if not _llm_done:
		return {"ok": false}
	if backend == "openai":
		var r = JSON.parse(_llm_text)
		if r.error != OK:
			return {"ok": false}
		return wishes.openai_parse_response(r.result)
	return wishes.llm_parse_response_text(_llm_text)

func _on_llm_done(result, code, headers, body):
	_llm_done = true
	if result == HTTPRequest.RESULT_SUCCESS and code == 200:
		_llm_text = body.get_string_from_utf8()

func _aabb(node):
	var first = true
	var box = AABB()
	var stack = [node]
	while not stack.empty():
		var n = stack.pop_back()
		if n is MeshInstance:
			var local = n.get_aabb()
			var world = n.global_transform.xform(local)
			if first:
				box = world
				first = false
			else:
				box = box.merge(world)
		for c in n.get_children():
			stack.append(c)
	return box

func _bake_all(spec, source, wish, name, out_base, size, dirs, tilt):
	var Builder = load("res://tools/iso_baker/builder.gd").new()
	var built = Builder.build(spec)
	var root3d = built["root"]
	var vp = Viewport.new()
	vp.size = Vector2(size, size)
	vp.transparent_bg = true
	vp.own_world = true
	vp.render_target_update_mode = Viewport.UPDATE_ALWAYS
	get_root().add_child(vp)
	var env = WorldEnvironment.new()
	var e = Environment.new()
	e.background_mode = Environment.BG_COLOR
	e.background_color = Color(0, 0, 0, 0)
	env.environment = e
	vp.add_child(env)
	# Studio-Licht ohne Ambient-Enum (3.6 stellt AMBIENT_SOURCE_COLOR nicht
	# als Konstante bereit): Haupt- + Fuell-Licht, keine Schatten.
	var sun = DirectionalLight.new()
	sun.rotation_degrees = Vector3(-50, -30, 0)
	sun.light_energy = 1.1
	sun.shadow_enabled = false
	vp.add_child(sun)
	var fill = DirectionalLight.new()
	fill.rotation_degrees = Vector3(-20, 150, 0)
	fill.light_energy = 0.45
	fill.shadow_enabled = false
	vp.add_child(fill)
	var table = Spatial.new()
	vp.add_child(table)
	table.add_child(root3d)
	var cam = Camera.new()
	cam.projection = Camera.PROJECTION_ORTHOGONAL
	vp.add_child(cam)
	cam.current = true
	# Einrahmen: Ziel = AABB-Mitte, Groesse aus Hoehe + Reserve.
	yield(self, "idle_frame")
	yield(self, "idle_frame")
	var box = _aabb(root3d)
	if box.size.y <= 0.0:
		return false
	var dist = 10.0
	var center = box.position + box.size / 2.0
	cam.size = box.size.y * 1.3
	var tilt_r = deg2rad(tilt)
	var yaw_r = deg2rad(45.0)
	var off = Vector3(cos(yaw_r) * cos(tilt_r), sin(tilt_r), sin(yaw_r) * cos(tilt_r))
	cam.translation = center + off * dist
	cam.look_at(center, Vector3.UP)
	var clip = str(spec["anim"]["clip"])
	var frames = int(spec["anim"]["frames"])
	var dir = Directory.new()
	var folder = out_base + "/" + name
	if dir.make_dir_recursive(folder) != OK:
		return false
	var step = 8 / dirs
	var files = []
	for d in range(dirs):
		table.rotation.y = deg2rad(float(d * step) * 45.0)
		for f in range(frames):
			Builder.apply_pose(built, clip, float(f) / float(maxi(frames, 1)))
			yield(self, "idle_frame")
			yield(self, "idle_frame")
			yield(self, "idle_frame")
			var img = vp.get_texture().get_data()
			if img == null or img.get_width() != size or img.get_height() != size:
				return false
			img.flip_y()
			var path = folder + "/d%02d_f%02d.png" % [d, f]
			if img.save_png(path) != OK:
				return false
			files.append(path)
	# Spritesheet + Atlas.
	var sheet = Image.new()
	sheet.create(size * frames, size * dirs, false, Image.FORMAT_RGBA8)
	for d in range(dirs):
		for f in range(frames):
			var im = Image.new()
			if im.load(files[d * frames + f]) != OK:
				return false
			sheet.blit_rect(im, Rect2(0, 0, size, size), Vector2(f * size, d * size))
	if sheet.save_png(folder + "/sheet.png") != OK:
		return false
	var atlas = {
		"name": name, "wish": wish, "spec_source": source, "spec": spec,
		"size": size, "dirs": dirs, "tilt_deg": tilt, "clip": clip,
		"frames": frames, "anchor": [0.5, 1.0], "sheet": "sheet.png",
	}
	var jf = File.new()
	if jf.open(folder + "/atlas.json", File.WRITE) != OK:
		return false
	jf.store_string(to_json(atlas))
	jf.close()
	return true

func maxi(a, b):
	return a if a > b else b
