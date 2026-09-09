# Andora – Lokalisierung (i18n)

**Zweck:** Verbindliche Grundlage der Andora-Lokalisierung. Ergänzt die
i18n-Grundaussagen in `docs/architecture.md` (§ i18n) und bereitet das spätere
Lokalisierungs-/Terminologieglossar vor.

## Unterstützte Locales

| Locale | Sprache | Datei |
|---|---|---|
| `de` | Deutsch | `i18n/de.json` |
| `en` | Englisch (Master) | `i18n/en.json` |
| `zh-Hans` | 简体中文 (vereinfachtes Chinesisch) | `i18n/zh-Hans.json` |
| `zh-Hant` | 繁體中文 (traditionelles Chinesisch) | `i18n/zh-Hant.json` |

- Locale-Bezeichnungen verwenden die standardisierte BCP-47-Schreibweise
  (`zh-Hans` / `zh-Hant` für die Skriptvariante des Chinesischen).
- Pro Locale existiert genau eine Datei `i18n/<locale>.json`.
- `en.json` ist der **englische Mastertext**; neue Texte werden zuerst dort,
  anschließend in den übrigen Locales angelegt.
- Fallback-Reihenfolge bei fehlendem Key: gewähltes Locale → `en` → Key-Name.

## Regeln

- **Keine hart codierten Clienttexte.** Alle vom Spieler sichtbaren Texte
  laufen über `t("key", args)`/`localizedMessage(...)` (Godot-Autoload
  `I18n` bzw. `src/realm/i18n.js`). Client UND Server laden dieselben
  `i18n/*.json`-Dateien.
- **zh-Hans und zh-Hant sind getrennte Lokalisierungen**, keine automatische
  Schriftkonvertierung. Beide werden eigenständig gepflegt.
- **Eigennamen und Andora-Fantasybegriffe** werden nicht eigenmächtig
  sinngemäß umbenannt, solange keine verbindliche Glossarentscheidung vorliegt
  (siehe unten).

## Terminologie-Grundlage (Glossar-Vorbereitung)

Dieser Abschnitt ist die Startbasis für das zukünftige
Lokalisierungs-/Terminologieglossar. Solange ein Begriff nicht verbindlich
festgelegt ist, gilt:

- **Eigennamen** (z. B. `Andora`) bleiben in allen Locales identisch.
- **Noch nicht festgelegte Fantasy-/Systembegriffe** werden über die Locales
  hinweg konsistent behandelt und nicht frei neu übersetzt oder umbenannt.

| Begriff | Ort/Wirkung | Status |
|---|---|---|
| `Andora` | Spieltitel / Weltname | Eigennamen – in allen Locales unverändert (`Andora`) |
| Raid / 团队副本 | Systembegriff (de/en: `Raid`, zh-Hans/Hant: 团队副本) | vorläufige Übersetzung, Glossar-Entscheidung offen |
| Boss / 首领 | Systembegriff (de/en: `Boss`, zh-Hans/Hant: 首领) | vorläufige Übersetzung, Glossar-Entscheidung offen |
| Quest / 任务 | Systembegriff (de: `Aufgabe`, en: `Quest`, zh: 任务) | vorläufige Übersetzung, Glossar-Entscheidung offen |
| 其他 noch nicht festgelegte Fantasy-Namen (Orte, Völker, NPCs) | erscheinen bisher nicht in `i18n/*.json` | offen – vor Einführung ins Glossar nicht frei übersetzen |

> Das Glossar wird später zentral dokumentiert (siehe
> `docs/README.md` / Terminologie). Neue Texte, die noch nicht festgelegte
> Fantasy-Begriffe enthalten, werden vor der Aufnahme in die Lokalisierung
> zunächst zurückgestellt oder – falls technisch nötig – konsistent mit
> diesem Absatz behandelt.