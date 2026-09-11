# Item Properties Documentation

## Overview
This document describes all properties and characteristics of items in the game inventory system.

## Basic Item Properties

### Core Attributes
- **name**: Unique identifier for the item
- **type**: Category of the item (weapon, armor, accessory, consumable, crafting material)
- **quality**: Quality level (1-5) determining stats and rarity
- **size**: Storage space required in inventory (measured in slots)
- **weight**: Item weight affecting player movement and carrying capacity
- **description**: Detailed description of the item's purpose and properties

### Quality Levels
- **Quality 1 (Gray/Poor)**: Basic functionality, lowest stat bonuses
- **Quality 2 (Green/Common)**: Standard stats and attributes
- **Quality 3 (Blue/Uncommon)**: Enhanced stats with moderate bonuses
- **Quality 4 (Yellow/Rare)**: Significant stat improvements
- **Quality 5 (Orange/Epic)**: Powerful abilities with high bonuses
- **Quality 6 (Purple/Legendary)**: Maximum stat bonuses and exceptional abilities

### Rarity System
- **Common**: 70% chance of appearing
- **Uncommon**: 20% chance of appearing  
- **Rare**: 7% chance of appearing
- **Epic**: 2.5% chance of appearing
- **Legendary**: 0.5% chance of appearing

## Equipment-Specific Properties

### Weapons
- **damage**: Base damage, specified by the weapon (siehe Kampfsystem.md Abschnitt 6; Grundschaden bestimmt die Schadenshöhe eines Treffers)
- **duration**: Time between automatic basic attacks (siehe Kampfsystem.md Abschnitt 3 und 6; entspricht der Angriffsgeschwindigkeit bzw. der Zeit zwischen zwei Grundangriffen)
- **range**: Attack range in tiles
- **durability**: Maximum uses before breaking
- **enchantments**: Special effects or bonuses

### Armor
- **defense**: Armor value provided by the equipment; the total relevant armor value is converted into a percentage-based physical damage reduction (siehe Kampfsystem.md Abschnitt 7). The exact conversion formula and the class-specific maximum reduction are balancing values.
- **resistance**: Resistance to specific damage types
- **movement_speed**: Effect on player movement speed
- **weight_reduction**: Reduces overall item weight burden

### Accessories
- **stat_bonus**: Primary stat modification
- **special_ability**: Unique passive or active abilities
- **slot_type**: Type of equipment slot required

### Attribute Bonuses on Equipment
Weapons, armor, and accessories can increase the seven basic attributes (Kraft, Konstitution, Geschicklichkeit, Intelligenz, Weisheit, Glück, Ausdauer). Equipment is intended to provide a large part of the actual attribute growth and fine-tuning in the later game: the race remains the base imprinting, while the player can strongly influence his character through equipment. Crafted items may receive different or additional attribute values depending on the crafting/quality system. Whether attribute bonuses remain exclusively integer-valued in the long term is to be decided through playtesting. Details and the current balance values: `Attribute_und_Regeneration.md`.

## Item Lifecycle

### Acquisition
- Monster drops (quality based on monster level)
- Quest rewards
- Crafting results
- Shop purchases (ausschließlich Ingame-Händler, die Ingame-Währung annehmen; keine Echtgeld-Käufe, siehe `Monetarisierung_und_Donations.md`)

### Storage
- Inventory slots
- Backpack expansion tiers
- Equipment slots
- Crafting material storage

### Usage
- Equipping to character
- Consuming consumable items
- Selling or trading
- Crafting with materials

### Bindung

Gegenstände können als handelbar oder charaktergebunden definiert werden.

Charaktergebundene Gegenstände können nicht an andere Spieler weitergegeben, verkauft oder über das Auktionshaus gehandelt werden.

Die Bindung wird insbesondere für besonders wertvolle Raid-, Boss-, Quest- oder Eventgegenstände verwendet, deren Wert aus einer besonderen spielerischen Leistung entstehen soll.

Rohstoffe, hergestellte Gegenstände und andere für die Spielerwirtschaft vorgesehene Gegenstände bleiben grundsätzlich handelbar.

Welche Gegenstände gebunden sind, wird über die jeweilige Gegenstandsdefinition festgelegt.

## Item System V1 – Implementierungsstand (Migrationen 014/015)

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

- **Stackable**: Stack-Gewicht = `full_stack_weight` (unabhängig von `count` im Inventar)
- **Partial**: `full_stack_weight × count / max_stack`
- **Crafted Non-Stackable**: `75 %` der verbrauchten Materialmasse (berechnet beim Crafting, als `weight` in die Instanz geschrieben)

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
| `BindOnPickup` | Sofort bound beim Aufheben |
| `BindOnEquip` | Bound beim ersten Tragen |
| `Bound` | Nicht handelbar (Quest/Event) |

Bindungsregel wird in der Definition gesetzt; Seltenheit erzwingt keine Änderung.

### Haltbarkeit (Durability)

`durability_current: Option<i64>` / `durability_max: Option<i64>` (None = unbegrenzt):
- `is_broken()` = `current == Some(0)` → Stats inaktiv
- `stats_active()` = nicht defekt (kein current=0)
- Defekt zerstört das Item **nicht**; es bleibt im Inventar

### Equip-Voraussetzungen

- `min_level: Option<i64>` (Minimal-Level des Charakters)
- `allowed_classes: Vec<ClassStatus>` (leer = alle Klassen; DB via `item_definition_classes`)
- **Keine Attributanforderungen** (V1 bewusst offen)

### Waffen-Spezifika (Datenfelder, keine Balance-Formel)

`weapon_type: Option<String>` – WeaponTypes werden aus dem Content geladen, nicht im Code-hardcoded. `Shield` ist **nicht** als WeaponType dokumentiert und in V1 nicht vorgesehen.

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
