extends Reference
# Prozeduraler Figuren-Generator (Godot 3.6): Baut aus einer validierten
# Spec (wish_to_spec.gd) einen einfachen 3D-Humanoiden mit Pivots,
# Ausruestungs-Anbauten und Posen (walk/idle/attack). Platzhalter-Niveau
# (Bau-Kasten + Kugelkopf), aber masshaltig, symmetrisch und mit stabiler
# Registrierung — ausreichend als Bake-Vorlage und als Vorschau, was die
# KI aus einem Wunsch gemacht hat.

func _mat(color):
	var m = SpatialMaterial.new()
	m.albedo_color = color
	m.roughness = 0.9
	return m

func _box(parent, size, pos, mat):
	var mi = MeshInstance.new()
	var mesh = CubeMesh.new()
	mesh.size = size
	mi.mesh = mesh
	mi.position = pos
	if mat != null:
		mi.material_override = mat
	parent.add_child(mi)
	return mi

func _ball(parent, radius, height, pos, mat):
	var mi = MeshInstance.new()
	var mesh = SphereMesh.new()
	mesh.radius = radius
	mesh.height = height
	mi.mesh = mesh
	mi.position = pos
	if mat != null:
		mi.material_override = mat
	parent.add_child(mi)
	return mi

func _pole(parent, radius, height, pos, mat):
	var mi = MeshInstance.new()
	var mesh = CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = height
	mi.mesh = mesh
	mi.position = pos
	if mat != null:
		mi.material_override = mat
	parent.add_child(mi)
	return mi

func _col(arr):
	return Color(arr[0], arr[1], arr[2], 1)

func build(spec):
	# Gibt {root, rig} zurueck. Nimmt nur validierte Specs (Baker prueft).
	var body = spec["body"]
	var eq = spec["equipment"]
	var h = float(body["height"])
	var b = float(body["bulk"])
	var skin = _mat(_col(body["skin"]))
	var cloth = _mat(_col(body["cloth"]))
	var trim = _mat(_col(body["trim"]))
	var root = Spatial.new()
	root.name = "figure"
	var rig = {}
	var leg_h = 0.80 * h
	var torso_h = 0.62 * h
	var torso_w = 0.42 * b
	# Beine (Pivot an der Huefte).
	for side in [-1.0, 1.0]:
		var hip = Spatial.new()
		hip.position = Vector3(side * 0.11 * b, leg_h, 0)
		root.add_child(hip)
		_box(hip, Vector3(0.15 * b, leg_h, 0.17 * b),
			Vector3(0, -leg_h / 2.0, 0), cloth)
		_box(hip, Vector3(0.16 * b, 0.10, 0.24 * b),
			Vector3(0, -leg_h + 0.05, 0.03), trim)
		if side < 0.0:
			rig["legL"] = hip
		else:
			rig["legR"] = hip
	# Torso.
	var torso = Spatial.new()
	torso.position = Vector3(0, leg_h, 0)
	root.add_child(torso)
	_box(torso, Vector3(torso_w, torso_h, 0.26 * b),
		Vector3(0, torso_h / 2.0, 0), cloth)
	_box(torso, Vector3(torso_w + 0.02, 0.08, 0.28 * b),
		Vector3(0, 0.10, 0), trim)
	rig["torso"] = torso
	var shoulder_y = torso_h - 0.08
	# Arme (Pivot an der Schulter).
	for side in [-1.0, 1.0]:
		var sh = Spatial.new()
		sh.position = Vector3(side * (torso_w / 2.0 + 0.07 * b), shoulder_y, 0)
		torso.add_child(sh)
		_box(sh, Vector3(0.13 * b, 0.55 * h, 0.15 * b),
			Vector3(0, -0.27 * h, 0), cloth)
		_ball(sh, 0.07 * b, 0.14 * b, Vector3(0, -0.56 * h, 0), skin)
		if side < 0.0:
			rig["armL"] = sh
		else:
			rig["armR"] = sh
	# Kopf.
	var neck = Spatial.new()
	neck.position = Vector3(0, torso_h + 0.02, 0)
	torso.add_child(neck)
	_ball(neck, 0.15 * h, 0.30 * h, Vector3(0, 0.15 * h, 0), skin)
	rig["head"] = neck
	_build_helmet(str(eq["helmet"]), neck, h, b, trim, cloth)
	# Haende-Anker (Waffe rechts, Schild links).
	var handR = Spatial.new()
	handR.position = Vector3(0, -0.58 * h, 0.02)
	rig["armR"].add_child(handR)
	rig["handR"] = handR
	var handL = Spatial.new()
	handL.position = Vector3(0, -0.58 * h, 0.02)
	rig["armL"].add_child(handL)
	rig["handL"] = handL
	_build_weapon(str(eq["weapon"]), handR, h, trim, cloth)
	if bool(eq["shield"]):
		_build_shield(handL, h, trim, cloth)
	if bool(eq["quiver"]):
		_box(torso, Vector3(0.12, 0.5 * h, 0.12),
			Vector3(0, 0.35 * h, -0.20 * b), trim)
	root.set_meta("base_y", 0.0)
	return {"root": root, "rig": rig}

func _build_helmet(kind, neck, h, b, trim, cloth):
	if kind == "cap":
		_ball(neck, 0.16 * h, 0.16 * h, Vector3(0, 0.26 * h, 0), cloth)
	elif kind == "helm":
		_box(neck, Vector3(0.32 * h, 0.22 * h, 0.32 * h),
			Vector3(0, 0.20 * h, 0), trim)
		_box(neck, Vector3(0.06 * h, 0.10 * h, 0.02),
			Vector3(0, 0.16 * h, 0.16 * h), cloth)
	elif kind == "hood":
		_ball(neck, 0.19 * h, 0.30 * h, Vector3(0, 0.20 * h, -0.02), cloth)

func _build_weapon(kind, hand, h, trim, cloth):
	if kind == "sword":
		_box(hand, Vector3(0.05, 0.62 * h, 0.10),
			Vector3(0, -0.95 * h, 0), trim)
		_box(hand, Vector3(0.16, 0.05, 0.14), Vector3(0, -0.62 * h, 0), cloth)
	elif kind == "axe":
		_pole(hand, 0.03, 0.70 * h, Vector3(0, -0.95 * h, 0), cloth)
		_box(hand, Vector3(0.06, 0.20 * h, 0.22),
			Vector3(0, -1.25 * h, 0.08), trim)
	elif kind == "bow":
		_pole(hand, 0.025, 0.80 * h, Vector3(0, -0.95 * h, 0), cloth)
		_box(hand, Vector3(0.05, 0.10, 0.05), Vector3(0, -0.60 * h, 0), trim)
		_box(hand, Vector3(0.05, 0.10, 0.05), Vector3(0, -1.30 * h, 0), trim)
	elif kind == "staff":
		_pole(hand, 0.03, 1.10 * h, Vector3(0, -1.05 * h, 0), cloth)
		_ball(hand, 0.07, 0.14, Vector3(0, -1.62 * h, 0), trim)

func _build_shield(hand, h, trim, cloth):
	_box(hand, Vector3(0.06, 0.45 * h, 0.35 * h),
		Vector3(0, -0.55 * h, 0), cloth)
	_ball(hand, 0.07, 0.14, Vector3(-0.05, -0.55 * h, 0), trim)

# ---------- Posen (phase 0..1) ----------

func apply_pose(built, clip, phase):
	var rig = built["rig"]
	var root = built["root"]
	var p = fposmod(float(phase), 1.0)
	if clip == "walk":
		var s = sin(p * TAU) * 0.55
		rig["armL"].rotation.x = s
		rig["armR"].rotation.x = -s
		rig["legL"].rotation.x = -s * 0.9
		rig["legR"].rotation.x = s * 0.9
		root.position.y = abs(cos(p * TAU)) * 0.05
	elif clip == "attack":
		var r = sin(p * PI)
		rig["armR"].rotation.x = -2.0 * r
		rig["armL"].rotation.x = 0.3 * r
		rig["torso"].rotation.y = 0.3 * r
		root.position.y = 0.0
	else:
		var b = sin(p * TAU) * 0.03
		rig["armL"].rotation.x = b
		rig["armR"].rotation.x = -b
		rig["legL"].rotation.x = 0.0
		rig["legR"].rotation.x = 0.0
		rig["torso"].rotation.y = 0.0
		root.position.y = abs(b) * 0.4
