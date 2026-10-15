# Iso-Baker — Wunsch → 3D-Figur → 2D-Iso-Sprites

Werkzeug (Godot 3.6, Teststand-Niveau): Aus einem Satz („Ork mit Axt“)
wird eine Bau-Spezifikation, daraus eine einfache 3D-Figur, daraus
Iso-Sprites (Richtungen × Frames) mit Atlas-JSON und Spritesheet.

## Architektur (maßgeblich)

KI generiert **keine Meshes**. Sie übersetzt Wunsch → Spec-JSON; alles
andere ist deterministisch und validiert:

`Wunsch → [Ollama | Stichwort-Parser] → Spec → Validator (fail-closed)
→ Generator → Baker → PNGs + atlas.json + sheet.png`

- Ungültige Specs werden **nie** gebacken (stabile Gründe, Stichwort-Rückfall).
- `spec_source` (`llm`/`keyword`/`spec_json`/`spec_file`) steht im Atlas —
  damit ist pro Modell messbar, ob es liefert.

## Benutzung (projektlokal, mit Anzeige, z. B. projektlokalem Xvfb)

```sh
godot --no-window --audio-driver Dummy --path . \
  --script res://tools/iso_baker/bake.gd -- \
  --wish "Ork mit Axt" --out .tmp/iso_bake --size 128
```

Args (`--key=value` oder `--key value`): `wish`, `spec_json`, `spec_file`,
`out` (Default `.tmp/iso_bake`), `size` (32–512), `dirs` (1/4/8),
`tilt` (Iso-Neigung, Default 30), `name`, `no_llm`,
`llm_backend` (`ollama`|OpenAI-kompatibel `openai`, Default `ollama`),
`llm_url` (Default Ollama `http://localhost:11434`; bei `openai`
Pflicht — kein Port geraten), `llm_model` (Pflicht bei LLM-Nutzung;
Aliase `ollama_url`/`ollama_model`), `ollama_timeout_ms` (10000).
Exit: 0 ok, 2 Eingabe/Spec ungültig, 3 Bake-Fehler.

Beispiel Strata (OpenAI-kompatibel, eigener Port einsetzen):

```sh
godot --no-window --audio-driver Dummy --path . \
  --script res://tools/iso_baker/bake.gd -- \
  --wish "Ork mit Axt" --llm-backend openai \
  --llm-url http://localhost:8080/v1 --llm-model DEIN-MODELLNAME
```

## Modell-Hinweis (Qwen3.8-Flash-Next, lokal via Ollama)

- Basis: `Qwen/Qwen3.8-Flash-Next` (125B MoE, 6B aktiv; Multimodal;
  IFBench 81.3; Lizenz qwen-community-1.0).
- Komprimiert: `ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-Coder-GGUF`
  (50 % Experten entfernt + Rest 3,5 bpw; **kein 1-Bit-Modell** — 1,89 bpw
  ist die effektive Rate über den Originalstand; 29,6 GB resident,
  N-Gram-Tabelle optional von Platte; Karte nennt Apache-2.0, Gewichte
  erben die Basis-Lizenz). Ollama: `ollama run
  hf.co/ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-Coder-GGUF:IQ1_M`.
- Spec-Aufrufe laufen ohne Thinking, Temperatur 0,2 (deterministisches
  JSON). Benchmarks der Karte gelten für xhigh-Reasoning und damit
  **nicht** für diesen Modus — Eignung misst der `spec_source`-Anteil,
  nicht das Datenblatt.
- Backends: Ollama nativ (`/api/generate`, Projekt-Standard) oder
  OpenAI-kompatibel (`/v1/chat/completions`, z. B. Strata); Antwort
  beiderseits strikt validiert, Provider schreibt nie direkt.
- Betrieb: Dev-Maschine mit ≥32 GB RAM (Modell resident), Generierung
  offline; der Pi spielt nur die gebackenen Sprites ab. Kein Betrieb
  seitens dieses Jobs eingerichtet (keine Installation, kein Start).

## Schema (`spec_schema.json`, Version 1)

`body`: `preset` (human/ork/elf/dwarf), `height` 0,8–1,3, `bulk`
0,8–1,4, `skin`/`cloth`/`trim` als 0,0–1,0-RGB. `equipment`: `weapon`
(none/sword/axe/bow/staff), `shield` bool, `helmet` (none/cap/helm/hood),
`quiver` bool. `anim`: `clip` (walk/idle/attack), `frames` 1–16.
Unbekannte Schlüssel → Ablehnung.

## Grenzen (ausdrücklich)

- Platzhalter-Ästhetik (Kasten + Kugelkopf), kein Final-Art.
- Kein glTF-Laufzeitimport (Godot 3.6 braucht dafür Editor-Import);
  externe Modelle nur als ladbare Szene per `--scene`? **Nicht
  implementiert** — folgt bei Bedarf, nichts erfunden.
- Feste Leinwand, Füße als Anker `[0.5, 1.0]`; Walk-Phasen sind
  symmetrisch (5/8 unterscheidbar bei 8 Frames).
- Live-Ollama hier nicht erreichbar gewesen (Probe localhost:11434
  negativ); Pfad implementiert, dokumentiert und ohne Netz testbar.
