# Dokumentation der Gegenstandseigenschaften

## Überblick
Dieses Dokument beschreibt alle Eigenschaften und Merkmale der Gegenstände im Inventarsystem des Spiels.

## Grundlegende Gegenstandseigenschaften

### Grundattribute
- **name**: Eindeutige Kennung des Gegenstands
- **type**: Kategorie des Gegenstands (Waffe, Rüstung, Zubehör, Verbrauchsgegenstand, Handwerksmaterial)
- **quality**: numerische Qualität (getrennt von Seltenheit; siehe Abschnitt „Gegenstandssystem V1")
- **weight**: Gegenstandsgewicht, das die Bewegung und Tragfähigkeit des Spielers beeinflusst
- **description**: Ausführliche Beschreibung des Zwecks und der Eigenschaften des Gegenstands

### Qualität und Seltenheit (V1 – verbindlich)

Numerische Qualität und Seltenheit sind **getrennte Eigenschaften** (siehe Abschnitt „Gegenstandssystem V1 – Implementierungsstand").

Die nachfolgend beschriebene ehemalige Verknüpfung von Quality-Stufen mit Rarity-Farben (Quality 1 Gray … 5 Purple) und die alten %igen Stat-Boni je Quality-Stufe sind **veraltet** und wurden durch die V1-Regelung ersetzt:

* **Seltenheiten** (5 Stufen): Common / Uncommon / Rare / Epic / Legendary – rein kategorial.
* **Numerische Quality** (0–100, f64): gewichtete Materialqualität, CraftQuality-Faktoren.

### Minimalstufen und Tier-Einteilung (veraltet)

Die ehemalige Zuordnung von Quality-Stufen zu minimalen Levelanforderungen
(Common Level 1 … Legendary Level 30) und die ehemalige Equipment-Tier-Einteilung
(Tier 0 Level 1–10 … Tier 11 Level 111+) sind veraltet und wurden aus
`inventory_system.md` entfernt. Minimalstufen sind jetzt pro Item-Definition
einzeln hinterlegt (`item_definitions.min_level`). Tier- und
Progressionsgrenzen sind Content-/Progressionsfragen (siehe `Tier-Progression.md`).

## Ausrüstungsspezifische Eigenschaften


### Waffen
- **damage**: Durch die Waffe vorgegebener Grundschaden (siehe Kampfsystem.md Abschnitt 6; Grundschaden bestimmt die Schadenshöhe eines Treffers)
- **duration**: Zeit zwischen automatischen Grundangriffen (siehe Kampfsystem.md Abschnitt 3 und 6; entspricht der Angriffsgeschwindigkeit bzw. der Zeit zwischen zwei Grundangriffen)
- **range**: Angriffsreichweite in Kacheln
- **durability**: Maximale Anzahl an Benutzungen bis zum Defekt
- **enchantments**: Besondere Effekte oder Boni

### Rüstung
- **defense**: Durch die Ausrüstung bereitgestellter Rüstungswert; der gesamte relevante Rüstungswert wird in eine prozentuale physische Schadensreduktion umgerechnet (siehe Kampfsystem.md Abschnitt 7). Die genaue Umrechnungsformel und die klassenspezifische maximale Reduktion sind Werte der Spielbalance.
- **resistance**: Widerstand gegen bestimmte Schadensarten
- **movement_speed**: Einfluss auf die Bewegungsgeschwindigkeit des Spielers
- **weight_reduction**: Verringert die gesamte Gewichtsbelastung durch Gegenstände

### Zubehör
- **stat_bonus**: Änderung des primären Attributs
- **special_ability**: Einzigartige passive oder aktive Fähigkeiten

### Ausrüstungs-Achsen: `equip_subtype` und `handling` (Zielregel – Umsetzung folgt)

`item_definitions` erhält zwei zusätzliche, erweiterbare Eigenschaften:

| Achse | Feld | Beispielwerte | Zweck |
|---|---|---|---|
| Kategorie | `category` (bestehend) | weapon, armor, accessory, … | Grobkategorie |
| Subtyp | `equip_subtype` (neu) | sword, dagger, axe, staff, bow, shield, helm, chest, belt, ring, lantern … | slot-relevante Art |
| Handhabung | `handling` (neu) | one_handed, two_handed, off_hand, ranged, none | wie gehalten |

**Slot-Zulässigkeit** ergibt sich aus `equip_subtype` + `handling` nach
`inventory_system.md` §7.1 — als **zentral hinterlegte Regel**, nicht als festes
Slot-Einzelwert-Feld pro Item.

- **Erweiterbarkeitsanforderung:** Die Zuordnung Subtyp + Handhabung → Slot und
  die Kombinationsregeln sind so zu implementieren, dass ein **neuer Subtyp mit
  einem vorhandenen Handhabungsmodell ohne Änderung des Rust-Kerns oder der
  Kampflogik** ergänzt werden kann (Content-/Konfigurationsdaten, keine
  verstreuten Subtyp-Abfragen in der Kampf-/Equip-Logik). Eine **neue
  Handhabungs- oder Kampfmechanik darf dagegen eine Codeänderung erfordern**.
- **Drei Tragbarkeitsklassen:**
  - *Ausrüstungsgegenstände* (weapon/armor/accessory): Slot nach §7.1.
  - *Funktionsgegenstände* (Food/Drink): Slots 20/21 für Auto-Consumable.
  - *Nicht ausrüstbare Gegenstände* (RawMaterial, Potion, QuestItem): kein Slot.
  `handling = none` bedeutet daher **nicht** „kein Slot"; entscheidend ist die
  Kombination aus category + equip_subtype gemäß §7.1.
- `weapon_type` (bestehend) bleibt ein reiner Content-Wert für Schaden und
  Darstellung; für die Slot-Ableitung wird **ausschließlich** `equip_subtype`
  (zusammen mit `handling`) herangezogen. Ob `weapon_type` und `equip_subtype`
  später zusammengeführt werden, ist eine **separate Content-Entscheidung** und
  keine Voraussetzung dieser Regel. **Schild:** `category armor` +
  `equip_subtype shield` + `handling off_hand` (kein WeaponType).
- `slot_type` (alt, nur „Zubehör"): durch `equip_subtype` + `handling`
  **abgelöst**. Das Konzept war **nie implementiert** – weder eine
  `slot_type`-Spalte in `item_definitions` (Migration 014) noch ein Feld in
  `ItemDefinition`. Eine eigene `slot_type`-Spalte entsteht nicht; weil es nie
  Daten darin gab, ist dafür **keine Datenmigration** erforderlich.
- Ungültige Kombination (Subtyp ohne passendes Handling) ist Definitionsfehler →
  **fail-closed** `wrong_slot`, nicht ausrüstbar.

**Kanonische Werte:** Die Achsenlisten sind erweiterbar, die verbindliche
Schreibweise je Slot steht ausschließlich in der Zuordnungstabelle
`inventory_system.md` §7.1 (Kopfschutz = `helm`). Neue Subtypen werden dort und
im Content ebenso kleingeschrieben und ohne Leerzeichen eingetragen.

**Status / Bestand:**

- `category` existiert (`VARCHAR(32)`, enum `ItemCategory` mit neun Kategorien);
  für die Slot-Ableitung ist `category` nur die Tragbarkeitsklasse.
- `weapon_type` existiert als `Option<String>` mit der bestehenden
  Definitionsregel „nur für Kategorie weapon" (`ItemDefinition::validate()`) und
  wird über `load_item_definitions` geladen. Bisher steckte die Handhabung
  mit im Wert (Test-Definition `one_hand_sword`) – das übernimmt künftig
  `handling`; `weapon_type` wird für die Slot-Ableitung **nicht** gelesen und
  bleibt für Schaden/Darstellung erhalten. Eine spätere Zusammenführung von
  `weapon_type` und `equip_subtype` ist eine separate Content-Entscheidung.
- `slot_type` war nie Datenbankfeld oder Code-Feld, sondern nur eine
  Doku-Angabe beim Zubehör; der Eintrag ist damit gegenstandslos und **keine
  Datenmigration** nötig.
- `equip_subtype` und `handling` sind **noch nicht vorhanden** (keine Spalte in
  `item_definitions`/Migration 014, kein Feld in `ItemDefinition`, keine Prüfung
  in `try_equip`). Die Umsetzung ist additiv: zwei Spalten,
  serde-Default-Felder, eine zentrale Regelstelle für Zuordnung und
  Kombination, unbekannte oder fehlerhafte Achsenwerte werden fail-closed mit
  `wrong_slot` abgelehnt. Details der Slot- und Kombinationsregeln:
  `inventory_system.md` §7.1 und §7.2.

### Attributboni auf Ausrüstung
Waffen, Rüstung und Zubehör können die sieben Grundattribute (Kraft, Konstitution, Geschicklichkeit, Intelligenz, Weisheit, Glück, Ausdauer) erhöhen. Ausrüstung soll im späteren Spiel einen großen Teil des tatsächlichen Attributwachstums und der Feinabstimmung ermöglichen: Die Rasse bleibt die Grundprägung, während der Spieler seinen Charakter durch Ausrüstung stark beeinflussen kann. Hergestellte Gegenstände können je nach Handwerks-/Qualitätssystem andere oder zusätzliche Attributwerte erhalten. Ob Attributboni langfristig ausschließlich ganzzahlig bleiben, soll durch Spieltests entschieden werden. Details und aktuelle Werte der Spielbalance: `Attribute_und_Regeneration.md`.

## Lebenszyklus von Gegenständen

### Erwerb
- Monsterbeute (Qualität abhängig von der Monsterstufe)
- Questbelohnungen
- Handwerksergebnisse
- Händlerkäufe (ausschließlich Händler im Spiel, die Spielwährung annehmen; keine Echtgeld-Käufe, siehe `Monetarisierung_und_Donations.md`)

### Aufbewahrung
- Inventarplätze (1 Gegenstand / 1 Stapel = 1 Platz; keine Gegenstandsgröße)
- Ausrüstbare Rucksäcke / Taschen (eigene Inventarbereiche, siehe `inventory_system.md`)
- Ausrüstungsplätze

### Verwendung
- Anlegen am Charakter
- Verbrauchen von Verbrauchsgegenständen
- Verkaufen oder Handeln
- Handwerk mit Materialien

### Bindung

Gegenstände können als handelbar oder charaktergebunden definiert werden.

Charaktergebundene Gegenstände können nicht an andere Spieler weitergegeben, verkauft oder über das Auktionshaus gehandelt werden.

Die Bindung wird insbesondere für besonders wertvolle Raid-, Boss-, Quest- oder Eventgegenstände verwendet, deren Wert aus einer besonderen spielerischen Leistung entstehen soll.

Rohstoffe, hergestellte Gegenstände und andere für die Spielerwirtschaft vorgesehene Gegenstände bleiben grundsätzlich handelbar.

Welche Gegenstände gebunden sind, wird über die jeweilige Gegenstandsdefinition festgelegt.

## Gegenstandssystem V1 – Implementierungsstand (Migrationen 014/015)

### Datenmodell

Das Item-System trennt **statische Definitionen** (Basiswerte, Content) von **individuellen Instanzen** (Zustand, Modifier). Beide liegen in der Realm-Datenbank:

- **`item_definitions`**: Vorlagen für jeden Item-Typ (id, name, description, category, rarity, item_level, base_quality, max_stack, weight, weapon_type, base_damage, duration_ms, range, armor_value, min_level, binding_rule)
- **`item_instances`**: Individuelle Objekte (item_uuid, item_id → Definition, count, durability_current/max, binding, creator_id)
- **Modifier-Satelliten**: `item_instance_modifiers` (damage/armor/weight/quality_modifier), `item_instance_attribute_modifiers`, `item_instance_resistance_modifiers`
- **Definition-Satelliten**: `item_definition_classes` (erlaubte Klassen), `item_definition_attributes` (Attributboni), `item_definition_resistances` (Resistenzboni)

Effektiver Wert = Basiswert (Definition) + Modifier (Instanz). Das Ändern von Basiswerten in der Definition beeinflusst alle betroffenen Instanzen.

### Kategorien und Stack-Regeln

| Kategorie | `max_stack`-Default | Gewicht |
|---|---|---|
| Potion | 20 | pro Stk (volles Stack-Gewicht) |
| Food/Drink | 50 | pro Stk |
| RawMaterial | 100 | pro Stk |
| Weapon/Armor | 1 | pro Stück |
| QuestItem/Accessory | 1 | pro Stück |

1 Item/Stack = 1 Inventar-Slot. Keine Item-Größe und kein Multi-Slot.

### Gewichtsberechnung

- **Stapelbar**: Stapelgewicht = `full_stack_weight` (unabhängig von `count` im Inventar)
- **Teilstapel**: `full_stack_weight × count / max_stack`
- **Hergestellt, nicht stapelbar**: `75 %` der verbrauchten Materialmasse (beim Handwerk berechnet, als `weight` in die Instanz geschrieben)

`full_stack_weight` = Gewicht einer Definition (`item_definitions.weight`), nicht veränderlich pro Instanz.

### Seltenheiten (5 Stufen, numerisch)

`Common` / `Uncommon` / `Rare` / `Epic` / `Legendary` – rein kategorial, kein numerischer Wert. Seltenheit erzwingt keine Bindung und keinen Itemschaden. Drop-Raten sind Balance-Werte, nicht im Code verankert.

### Numerische Qualität (separat von Seltenheit)

Die Quality-Metriken für Crafting:

- `weighted_material_quality = Σ(q × a) / Σ(a)` (q = Materialqualität 0–100, a = Menge)
- `CraftQuality::Inferior` = 0.75, `Normal` = 1.0, `Superior` = 1.25
- Rare-Material: `rare_material_quality = Σ(q × a × 2) / Σ(a)`
- Intern f64; finale Werte werden in der Instanz gespeichert (nicht gerundet)

### Bindung (4 Zustände)

| Zustand | Erlaubnis |
|---|---|
| `Tradeable` | Frei handelbar |
| `BindOnPickup` | Sofort gebunden beim Aufheben |
| `BindOnEquip` | Gebunden beim ersten Tragen |
| `Bound` | Nicht handelbar (Quest/Event) |

Bindungsregel wird in der Definition gesetzt; Seltenheit erzwingt keine Änderung.

### Haltbarkeit

`durability_current: Option<i64>` / `durability_max: Option<i64>` (None = unbegrenzt):
- `is_broken()` = `current == Some(0)` → Stats inaktiv
- `stats_active()` = nicht defekt (kein current=0)
- Defekt zerstört das Item **nicht**; es bleibt im Inventar

### Voraussetzungen zum Anlegen

- `min_level: Option<i64>` (Minimal-Level des Charakters)
- `allowed_classes: Vec<ClassStatus>` (leer = alle Klassen; DB via `item_definition_classes`)
- **Keine Attributanforderungen** (V1 bewusst offen)

### Waffen-Spezifika (Datenfelder, keine Balance-Formel)

`weapon_type: Option<String>` – WeaponTypes werden aus dem Content geladen, nicht im Code-hardcoded. `Shield` ist **nicht** als WeaponType dokumentiert und in V1 nicht vorgesehen: Ein Schild ist Rüstung mit `equip_subtype shield` + `handling off_hand`.

`weapon_type` bleibt ein reiner Content-Wert für Schaden und Darstellung und wird für die Slot-Ableitung **nicht** gelesen; für die Slot-Ableitung ist allein `equip_subtype` (mit `handling`) maßgeblich. Die Handhabung kommt aus `handling`, nicht aus dem WeaponType-Namen (siehe „Ausrüstungs-Achsen"). Eine spätere Zusammenführung von `weapon_type` und `equip_subtype` ist eine separate Content-Entscheidung und kein Bestandteil der Slot-Regel.

### Rüstungs-Spezifika

`armor_value: Option<f64>` – Basis-Rüstungswert. Skalierung/Reduktion ist Balancing, nicht Teil von V1-Code.

### Attribut- und Resistenz-Boni

Definitionen: `item_definition_attributes` (bonus) und `item_definition_resistances` (bonus)
Instanzen: `item_instance_attribute_modifiers` (modifier) und `item_instance_resistance_modifiers` (modifier)

Effektiv = Definition-Bonus + Instanz-Modifier. Attributnamen werden bei Validierung akzeptiert: kraft, konstitution, geschicklichkeit, intelligenz, weisheit, glueck/glück, ausdauer.

### Bindungsregeln

Die vier Bindungszustände werden in `binding_rule` der Definition gesetzt. Beim Aufheben/Anlegen:
- `BindOnPickup`: Instanz wird sofort `Bound`
- `BindOnEquip`: Instanz wird beim ersten `equip`-Event `Bound`
- `Tradeable`/`Bound`: keine Änderung

### Status

**Eingebaut**, aber ohne Inventory/Crafting/Loot. Die Datenstruktur steht vollständig:
- `validate()` prüft Instanzen- und Definitions-Konsistenz
- `load_item_definitions` lädt beim Realm-Start in die Shared-World
- `load_item_instance`/`save_item_instance` sind Hook-Funktionen für Inventory V1
- Keine aktiven Spielereffekte (kein Loot, kein Inventar-Slots, kein Crafting)
