// item — Item System V1 (Realm-autoritativ, technische Grundlage).
//
// Grundprinzip (docs/item_properties.md, docs/Crafting.md):
// - Statische Item-Definitionen tragen die BASISWERTE (item_definitions,
//   Migration 014). Stabile ID = item_id.
// - Item-Instanzen sind konkrete individuelle Exemplare (item_instances,
//   Migration 015) mit eigener item_uuid und referenzieren ihre item_id.
//   Sie speichern den Basiswert NICHT als zweite Kopie, sondern nur eine
//   klar getrennte Modifier-Struktur. Effektiver Wert =
//   Basiswert + Instanz-Modifikation.
// - 1 Item oder 1 Stack belegt genau 1 Slot. Keine Itemgröße, kein
//   Multi-Slot-Item. max_stack ist Eigenschaft der Definition; ein Stack
//   ist EIN Datensatz, keine UUID pro einzelner Einheit.
// - Gewicht: bei stackbaren Items als VOLLER Stack definiert; Teilstack =
//   full_stack_weight * count / max_stack. Nicht-stackbare gecraftete Items
//   erhalten einmalig beim Crafting 75 % der Gesamtmasse der verbrauchten
//   Materialien (weight_modifier).
// - Numerische Qualität (Materialqualität, Crafting 75/100/125 %) und
//   Seltenheit (Common..Legendary) sind zwei getrennte Systeme.
// - Haltbarkeit: durability_current/max; current = 0 ist DEFEKT (nicht
//   zerstört, Stats inaktiv, is_broken()/stats_active()).
//
// Inventory-/Loot-/Crafting-Ausführung sind NICHT Teil dieses Moduls
// (Folgeaufträge).
#![allow(dead_code)]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::class::ClassStatus;

/// Die sieben Grundattribute (kanonische Schlüssel, konsistent zur
/// characters-Tabelle: strength/constitution/dexterity/intelligence/wisdom/
/// luck/endurance, docs/Attribute_und_Regeneration.md §1).
pub const ATTRIBUTES: [&str; 7] = [
    "strength",
    "constitution",
    "dexterity",
    "intelligence",
    "wisdom",
    "luck",
    "endurance",
];

/// Bekannte (erweiterbare) Kategorien. Unbekannte Content-Werte werden beim
/// Laden abgelehnt, bis die Kategorie explizit ergänzt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemCategory {
    Weapon,
    Armor,
    Accessory,
    RawMaterial,
    Food,
    Drink,
    Potion,
    Recipe,
    QuestItem,
}

impl ItemCategory {
    pub fn from_db(s: &str) -> Option<ItemCategory> {
        match s.trim().to_lowercase().as_str() {
            "weapon" => Some(ItemCategory::Weapon),
            "armor" => Some(ItemCategory::Armor),
            "accessory" => Some(ItemCategory::Accessory),
            "raw_material" => Some(ItemCategory::RawMaterial),
            "food" => Some(ItemCategory::Food),
            "drink" => Some(ItemCategory::Drink),
            "potion" => Some(ItemCategory::Potion),
            "recipe" => Some(ItemCategory::Recipe),
            "quest_item" => Some(ItemCategory::QuestItem),
            _ => None,
        }
    }

    pub fn as_db(self) -> &'static str {
        match self {
            ItemCategory::Weapon => "weapon",
            ItemCategory::Armor => "armor",
            ItemCategory::Accessory => "accessory",
            ItemCategory::RawMaterial => "raw_material",
            ItemCategory::Food => "food",
            ItemCategory::Drink => "drink",
            ItemCategory::Potion => "potion",
            ItemCategory::Recipe => "recipe",
            ItemCategory::QuestItem => "quest_item",
        }
    }

    /// Standard-max_stack je Kategorie (pro Definition überschreibbar):
    /// Potion 20, Food/Drink 50, RawMaterial 100, Equipment 1 (sonst 1).
    pub fn default_max_stack(self) -> i64 {
        match self {
            ItemCategory::Potion => 20,
            ItemCategory::Food | ItemCategory::Drink => 50,
            ItemCategory::RawMaterial => 100,
            _ => 1,
        }
    }
}

/// Die fünf Seltenheitsstufen (docs/item_properties.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub fn from_db(s: &str) -> Option<Rarity> {
        match s.trim().to_lowercase().as_str() {
            "common" | "gewoehnlich" | "gewöhnlich" => Some(Rarity::Common),
            "uncommon" | "ungewöhnlich" | "ungewoehnlich" => Some(Rarity::Uncommon),
            "rare" | "selten" => Some(Rarity::Rare),
            "epic" | "episch" => Some(Rarity::Epic),
            "legendary" | "legendär" | "legendaer" => Some(Rarity::Legendary),
            _ => None,
        }
    }

    pub fn as_db(self) -> &'static str {
        match self {
            Rarity::Common => "common",
            Rarity::Uncommon => "uncommon",
            Rarity::Rare => "rare",
            Rarity::Epic => "epic",
            Rarity::Legendary => "legendary",
        }
    }
}

/// Bindungsregel einer DEFINITION.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingRule {
    /// Frei handelbar.
    Tradeable,
    /// Bindet beim Aufnehmen an den Charakter.
    BindOnPickup,
    /// Bindet beim Anlegen an den Charakter.
    BindOnEquip,
}

impl BindingRule {
    pub fn from_db(s: &str) -> Option<BindingRule> {
        match s.trim().to_lowercase().as_str() {
            "tradeable" | "free" | "none" => Some(BindingRule::Tradeable),
            "bind_on_pickup" | "bop" => Some(BindingRule::BindOnPickup),
            "bind_on_equip" | "boe" => Some(BindingRule::BindOnEquip),
            _ => None,
        }
    }

    pub fn as_db(self) -> &'static str {
        match self {
            BindingRule::Tradeable => "tradeable",
            BindingRule::BindOnPickup => "bind_on_pickup",
            BindingRule::BindOnEquip => "bind_on_equip",
        }
    }
}

/// Bindungszustand einer INSTANZ (inkl. bereits charaktergebunden).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingState {
    /// Frei handelbar.
    Tradeable,
    /// Durch Pickup charaktergebunden.
    BindOnPickup,
    /// Durch Equip charaktergebunden.
    BindOnEquip,
    /// Bereits charaktergebunden (z. B. Quest-/Boss-Item).
    Bound,
}

impl BindingState {
    pub fn from_db(s: &str) -> Option<BindingState> {
        match s.trim().to_lowercase().as_str() {
            "tradeable" | "free" | "none" => Some(BindingState::Tradeable),
            "bind_on_pickup" | "bop" => Some(BindingState::BindOnPickup),
            "bind_on_equip" | "boe" => Some(BindingState::BindOnEquip),
            "bound" => Some(BindingState::Bound),
            _ => None,
        }
    }

    pub fn as_db(self) -> &'static str {
        match self {
            BindingState::Tradeable => "tradeable",
            BindingState::BindOnPickup => "bind_on_pickup",
            BindingState::BindOnEquip => "bind_on_equip",
            BindingState::Bound => "bound",
        }
    }

    /// Charaktergebunden? (frei handelbar ist das einzige nicht-gebundene).
    pub fn is_bound(self) -> bool {
        !matches!(self, BindingState::Tradeable)
    }
}

/// Statische Item-Definition (item_definitions, Migration 014).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDefinition {
    /// Stabile, eindeutige Item-ID der Definition.
    pub item_id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: ItemCategory,
    pub rarity: Rarity,
    pub item_level: i64,
    /// Numerische Basis-/Materialqualität (Rohstoffqualität, meist 100).
    pub base_quality: f64,
    /// 1 Item oder 1 Stack = 1 Slot. >= 1.
    pub max_stack: i64,
    /// Gewicht eines VOLLEN Stacks (bei max_stack = 1: Itemgewicht).
    pub weight: f64,
    // --- Waffe (Kategorie weapon) ---
    pub weapon_type: Option<String>,
    pub base_damage: Option<f64>,
    /// Zeit zwischen automatischen Grundangriffen (ms), > 0.
    pub duration_ms: Option<i64>,
    pub range: Option<f64>,
    // --- Rüstung (Kategorie armor) ---
    pub armor_value: Option<f64>,
    // --- Equip-Voraussetzungen (keine Attributanforderungen) ---
    pub min_level: Option<i64>,
    pub allowed_classes: Vec<ClassStatus>,
    pub binding_rule: BindingRule,
    /// Basis-Attributboni (7 Attribute, erweiterbar).
    pub attribute_bonuses: BTreeMap<String, f64>,
    /// Basis-Resistenzen (erweiterbar, z. B. fire/frost/poison).
    pub resistances: BTreeMap<String, f64>,
}

impl Default for ItemDefinition {
    fn default() -> ItemDefinition {
        ItemDefinition::new("", "", ItemCategory::QuestItem)
    }
}

impl ItemDefinition {
    pub fn new(item_id: &str, name: &str, category: ItemCategory) -> ItemDefinition {
        ItemDefinition {
            item_id: item_id.to_string(),
            name: name.to_string(),
            description: None,
            category,
            rarity: Rarity::Common,
            item_level: 1,
            base_quality: 100.0,
            max_stack: category.default_max_stack(),
            weight: 0.0,
            weapon_type: None,
            base_damage: None,
            duration_ms: None,
            range: None,
            armor_value: None,
            min_level: None,
            allowed_classes: Vec::new(),
            binding_rule: BindingRule::Tradeable,
            attribute_bonuses: BTreeMap::new(),
            resistances: BTreeMap::new(),
        }
    }

    /// Zentraler Validierung der Definition. Offensichtlich ungültige Werte
    /// werden abgelehnt (docs/item_properties.md, Auftrag Item System V1 §17).
    /// Balancewerte werden NICHT erfunden.
    pub fn validate(&self) -> Result<(), ItemError> {
        let mut errors = Vec::new();
        if self.item_id.trim().is_empty() {
            errors.push("item_id leer".to_string());
        }
        if self.name.trim().is_empty() {
            errors.push("name leer".to_string());
        }
        if self.item_level < 0 {
            errors.push("item_level negativ".to_string());
        }
        if self.max_stack < 1 {
            errors.push("max_stack < 1".to_string());
        }
        if self.weight < 0.0 {
            errors.push("weight negativ".to_string());
        }
        if self.base_quality < 0.0 {
            errors.push("base_quality negativ".to_string());
        }
        if let Some(d) = self.duration_ms {
            if d < 0 {
                errors.push("duration_ms negativ".to_string());
            }
            if self.category != ItemCategory::Weapon {
                errors.push("duration_ms nur für Kategorie weapon".to_string());
            }
        }
        if let Some(bd) = self.base_damage {
            if bd < 0.0 {
                errors.push("base_damage negativ".to_string());
            }
            if self.category != ItemCategory::Weapon {
                errors.push("base_damage nur für Kategorie weapon".to_string());
            }
        }
        if self.weapon_type.is_some() && self.category != ItemCategory::Weapon {
            errors.push("weapon_type nur für Kategorie weapon".to_string());
        }
        if self.range.is_some() && self.category != ItemCategory::Weapon {
            errors.push("range nur für Kategorie weapon".to_string());
        }
        if let Some(a) = self.armor_value {
            if a < 0.0 {
                errors.push("armor_value negativ".to_string());
            }
        }
        if let Some(min_lvl) = self.min_level {
            if min_lvl < 0 {
                errors.push("min_level negativ".to_string());
            }
        }
        if !self.attribute_bonuses.keys().all(|k| is_valid_attribute(k)) {
            errors.push("unbekanntes Attribut in attribute_bonuses".to_string());
        }
        if !self.resistances.keys().all(|k| is_valid_resistance(k)) {
            errors.push("unbekannte Resistenz (key ungültig)".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ItemError { errors })
        }
    }

    /// Ist das Item stackbar (max_stack > 1)?
    pub fn is_stackable(&self) -> bool {
        self.max_stack > 1
    }

    /// Teilstack-Gewicht: full_stack_weight * count / max_stack.
    pub fn stack_weight(&self, count: i64) -> f64 {
        let cnt = count.clamp(0, self.max_stack);
        self.weight * cnt as f64 / self.max_stack as f64
    }

    /// Equip-Voraussetzungen: Level + erlaubte Klassen (keine Attribute).
    /// Leere allowed_classes = keine Klassenbeschränkung.
    pub fn can_equip(&self, level: i64, class: ClassStatus) -> bool {
        if let Some(min_lvl) = self.min_level {
            if level < min_lvl {
                return false;
            }
        }
        self.allowed_classes.is_empty() || self.allowed_classes.contains(&class)
    }
}

fn is_valid_attribute(key: &str) -> bool {
    ATTRIBUTES.contains(&key.trim().to_lowercase().as_str()) || {
        let lower = key.trim().to_lowercase();
        matches!(
            lower.as_str(),
            "kraft" | "konstitution" | "geschicklichkeit" | "intelligenz" | "weisheit" | "glück"
                | "glueck" | "ausdauer"
        )
    }
}

fn is_valid_resistance(key: &str) -> bool {
    !key.trim().is_empty()
        && key
            .trim()
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '_' || c == '-')
}

/// Modifier-Struktur einer individuellen INSTANZ. Effektiver Wert =
/// Basiswert (Definition) + Modifikation. Basiswerte werden NIE als
/// zweite Kopie auf der Instanz gespeichert.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemModifiers {
    pub damage_modifier: f64,
    pub armor_modifier: f64,
    pub weight_modifier: f64,
    pub quality_modifier: f64,
    pub attribute_modifiers: BTreeMap<String, f64>,
    pub resistance_modifiers: BTreeMap<String, f64>,
}

impl ItemModifiers {
    pub fn validate(&self) -> Result<(), ItemError> {
        let mut errors = Vec::new();
        if !self.attribute_modifiers.keys().all(|k| is_valid_attribute(k)) {
            errors.push("unbekanntes Attribut in attribute_modifiers".to_string());
        }
        if !self
            .resistance_modifiers
            .keys()
            .all(|k| is_valid_resistance(k))
        {
            errors.push("unbekannte Resistenz in resistance_modifiers".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ItemError { errors })
        }
    }
}

/// Individuelle Item-Instanz (item_instances, Migration 015).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemInstance {
    /// Eindeutige UUID dieses konkreten Exemplars.
    pub item_uuid: String,
    /// Referenz auf die statische Definition.
    pub item_id: String,
    /// Stackgröße (1..=max_stack der Definition). Ein Stack = EIN Datensatz.
    pub count: i64,
    /// Haltbarkeit; current = 0 ist defekt (nicht zerstört, Stats inaktiv).
    pub durability_current: Option<i64>,
    pub durability_max: Option<i64>,
    pub binding: BindingState,
    /// Hersteller-/Creator-Referenz.
    pub creator_id: Option<i64>,
    /// Dynamische Modifikatoren gegenüber der Definition.
    pub modifiers: ItemModifiers,
}

impl ItemInstance {
    pub fn new(item_uuid: &str, item_id: &str, modifiers: ItemModifiers) -> ItemInstance {
        ItemInstance {
            item_uuid: item_uuid.to_string(),
            item_id: item_id.to_string(),
            count: 1,
            durability_current: None,
            durability_max: None,
            binding: BindingState::Tradeable,
            creator_id: None,
            modifiers,
        }
    }

    /// Validierung der Instanz gegen ihre Definition.
    pub fn validate(&self, def: &ItemDefinition) -> Result<(), ItemError> {
        let mut errors = Vec::new();
        if self.item_uuid.trim().is_empty() {
            errors.push("item_uuid leer".to_string());
        }
        if self.item_id != def.item_id {
            errors.push(format!(
                "item_id/Definition-Mismatch: {} != {}",
                self.item_id, def.item_id
            ));
        }
        if self.count < 1 {
            errors.push("count < 1".to_string());
        }
        if self.count > def.max_stack {
            errors.push(format!("count {} > max_stack {}", self.count, def.max_stack));
        }
        match (self.durability_current, self.durability_max) {
            (Some(cur), Some(max)) => {
                if max <= 0 {
                    errors.push("durability_max <= 0".to_string());
                }
                if cur < 0 {
                    errors.push("durability_current negativ".to_string());
                }
                if cur > max {
                    errors.push("durability_current > durability_max".to_string());
                }
            }
            (Some(_), None) => errors.push("durability_current ohne durability_max".to_string()),
            (None, Some(_)) => errors.push("durability_max ohne durability_current".to_string()),
            (None, None) => {}
        }
        if let Err(e) = self.modifiers.validate() {
            errors.extend(e.errors);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ItemError { errors })
        }
    }

    /// Defekt? (durability_current == 0, nur bei vorhandener Haltbarkeit).
    pub fn is_broken(&self) -> bool {
        self.durability_current == Some(0)
    }

    /// Gameplay-Stats aktiv? (nicht defekt; ohne Haltbarkeit immer aktiv).
    pub fn stats_active(&self) -> bool {
        !self.is_broken()
    }

    /// Charaktergebunden?
    pub fn is_bound(&self) -> bool {
        self.binding.is_bound()
    }

    /// Frei handelbar?
    pub fn can_trade(&self) -> bool {
        !self.binding.is_bound()
    }

    // --- Effektive Werte: Basiswert (Definition) + Modifikator ---

    pub fn effective_damage(&self, def: &ItemDefinition) -> f64 {
        (def.base_damage.unwrap_or(0.0) + self.modifiers.damage_modifier).max(0.0)
    }

    pub fn effective_armor(&self, def: &ItemDefinition) -> f64 {
        (def.armor_value.unwrap_or(0.0) + self.modifiers.armor_modifier).max(0.0)
    }

    pub fn effective_numeric_quality(&self, def: &ItemDefinition) -> f64 {
        (def.base_quality + self.modifiers.quality_modifier).max(0.0)
    }

    /// Effektives Gewicht dieses Stacks (Teilstack-Regel inkl. Modifikator).
    pub fn effective_weight(&self, def: &ItemDefinition) -> f64 {
        let full = (def.weight + self.modifiers.weight_modifier).max(0.0);
        let cnt = self.count.clamp(0, def.max_stack);
        full * cnt as f64 / def.max_stack as f64
    }

    pub fn effective_attribute_bonus(&self, def: &ItemDefinition, attribute: &str) -> f64 {
        def.attribute_bonuses.get(attribute).copied().unwrap_or(0.0)
            + self
                .modifiers
                .attribute_modifiers
                .get(attribute)
                .copied()
                .unwrap_or(0.0)
    }

    pub fn effective_resistance(&self, def: &ItemDefinition, resistance: &str) -> f64 {
        def.resistances.get(resistance).copied().unwrap_or(0.0)
            + self
                .modifiers
                .resistance_modifiers
                .get(resistance)
                .copied()
                .unwrap_or(0.0)
    }
}

/// Validierungsfehler mit menschenlesbarer Detailliste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ItemError {
    pub errors: Vec<String>,
}

impl std::fmt::Display for ItemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.errors.join("; "))
    }
}

impl std::error::Error for ItemError {}

// ===== Numerische Qualität / Crafting-Werte (reine, testbare Funktionen) =====

/// Gewichtete Materialqualität:
/// weighted = sum(material_quality * amount_used) / total_amount_used.
/// Dezimalpräzision wird intern erhalten (keine Rundung).
pub fn weighted_material_quality(materials: &[(f64, i64)]) -> f64 {
    let total: i64 = materials.iter().map(|(_, amount)| (*amount).max(0)).sum();
    if total == 0 {
        return 0.0;
    }
    let weighted: f64 = materials
        .iter()
        .map(|(quality, amount)| quality * (*amount).max(0) as f64)
        .sum();
    weighted / total as f64
}

/// Crafting-Stufen-Multiplikatoren: minderwertig 75 %, normal 100 %,
/// meisterhaft 125 %.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftQuality {
    Inferior,
    Normal,
    Masterwork,
}

impl CraftQuality {
    pub fn multiplier(self) -> f64 {
        match self {
            CraftQuality::Inferior => 0.75,
            CraftQuality::Normal => 1.0,
            CraftQuality::Masterwork => 1.25,
        }
    }

    /// final_numeric_quality = weighted_material_quality * multiplier.
    pub fn final_quality(self, weighted: f64) -> f64 {
        weighted * self.multiplier()
    }
}

/// Seltene Variante eines Rohstoffs: 2 × Materialqualität des normalen Materials.
pub fn rare_material_quality(normal_quality: f64) -> f64 {
    normal_quality * 2.0
}

/// Masse eines tatsächlich verbrauchten Material-Teilstacks:
/// full_stack_weight * count / max_stack (Gewicht = voller Stack).
pub fn consumed_material_weight(full_stack_weight: f64, max_stack: i64, count: i64) -> f64 {
    if max_stack <= 0 {
        return 0.0;
    }
    let cnt = count.clamp(0, max_stack);
    full_stack_weight * cnt as f64 / max_stack as f64
}

/// Endgewicht nicht-stackbarer gecrafteter Items:
/// 75 % der Gesamtmasse der tatsächlich verbrauchten Materialien.
/// Wird EINMAL beim (späteren) Crafting berechnet und auf der Instanz
/// (weight_modifier) gespeichert.
pub fn crafted_item_weight(consumed_masses: &[f64]) -> f64 {
    0.75 * consumed_masses.iter().sum::<f64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weapon_def() -> ItemDefinition {
        ItemDefinition {
            item_id: "eisenschwert".to_string(),
            name: "Eisenschwert".to_string(),
            category: ItemCategory::Weapon,
            item_level: 1,
            base_quality: 100.0,
            max_stack: 1,
            weight: 3.0,
            weapon_type: Some("one_hand_sword".to_string()),
            base_damage: Some(10.0),
            duration_ms: Some(2500),
            range: Some(2.0),
            ..Default::default()
        }
    }

    #[test]
    fn default_max_stack_per_category() {
        assert_eq!(ItemCategory::Potion.default_max_stack(), 20);
        assert_eq!(ItemCategory::Food.default_max_stack(), 50);
        assert_eq!(ItemCategory::Drink.default_max_stack(), 50);
        assert_eq!(ItemCategory::RawMaterial.default_max_stack(), 100);
        assert_eq!(ItemCategory::Weapon.default_max_stack(), 1);
        assert_eq!(ItemCategory::Armor.default_max_stack(), 1);
        // definition may override defaults
        let mut def = ItemDefinition::new("x", "X", ItemCategory::RawMaterial);
        def.max_stack = 5;
        assert_eq!(def.max_stack, 5);
    }

    #[test]
    fn definition_validation_rejects_invalid() {
        let def = weapon_def();
        assert!(def.validate().is_ok());

        let mut bad = weapon_def();
        bad.max_stack = 0;
        assert!(bad.validate().is_err());

        let mut bad = weapon_def();
        bad.item_level = -1;
        assert!(bad.validate().is_err());

        let mut bad = weapon_def();
        bad.duration_ms = Some(-100);
        assert!(bad.validate().is_err());

        let mut bad = weapon_def();
        bad.category = ItemCategory::Potion;
        assert!(bad.validate().is_err()); // weapon values on non-weapon
    }

    #[test]
    fn instance_validation_rejects_invalid() {
        let def = weapon_def();
        let ok = ItemInstance::new("u1", "eisenschwert", ItemModifiers::default());
        assert!(ok.validate(&def).is_ok());

        let mut bad = ItemInstance::new("u2", "eisenschwert", ItemModifiers::default());
        bad.durability_max = Some(100);
        bad.durability_current = Some(50);
        assert!(bad.validate(&def).is_ok());

        bad.durability_current = Some(101);
        assert!(bad.validate(&def).is_err()); // current > max

        bad.durability_current = Some(-1);
        assert!(bad.validate(&def).is_err()); // negative

        let bad2 = ItemInstance::new("u3", "fremd", ItemModifiers::default());
        assert!(bad2.validate(&def).is_err()); // item_id mismatch
    }

    #[test]
    fn stack_and_partial_stack_weight() {
        let mut def = ItemDefinition::new("holz", "Holz", ItemCategory::RawMaterial);
        def.max_stack = 100;
        def.weight = 50.0; // voller Stack = 50
        assert_eq!(def.stack_weight(25), 12.5);
        assert_eq!(def.stack_weight(100), 50.0);
        assert_eq!(def.stack_weight(0), 0.0);
        assert_eq!(def.stack_weight(999), 50.0); // über max clampiert

        // max_stack = 1 -> full weight = item weight
        let sword = weapon_def();
        assert_eq!(sword.stack_weight(1), 3.0);
    }

    #[test]
    fn weighted_material_quality_and_craft_result() {
        // (10er-Material x3, 30er-Material x1) -> (30+30)/4 = 15
        let w = weighted_material_quality(&[(10.0, 3), (30.0, 1)]);
        assert!((w - 15.0).abs() < 1e-9);

        // Prezision intern erhalten
        let w = weighted_material_quality(&[(1.0, 3), (2.0, 1)]);
        assert!((w - 1.25).abs() < 1e-9);

        assert_eq!(CraftQuality::Inferior.multiplier(), 0.75);
        assert_eq!(CraftQuality::Normal.multiplier(), 1.0);
        assert_eq!(CraftQuality::Masterwork.multiplier(), 1.25);

        // final = weighted * multiplier
        assert_eq!(CraftQuality::Inferior.final_quality(100.0), 75.0);
        assert_eq!(CraftQuality::Normal.final_quality(100.0), 100.0);
        assert_eq!(CraftQuality::Masterwork.final_quality(100.0), 125.0);
    }

    #[test]
    fn rare_material_is_double_quality() {
        assert_eq!(rare_material_quality(100.0), 200.0);
        assert_eq!(rare_material_quality(10.0), 20.0);
    }

    #[test]
    fn crafted_non_stackable_weight() {
        // zwei Material-Teilstacks: Holz (voller Stack 50, 25 Stück -> 12.5)
        // und Erz (voller Stack 20, 10 Stück -> 2)
        let m1 = consumed_material_weight(50.0, 100, 25); // 12.5
        let m2 = consumed_material_weight(20.0, 10, 10); // 20*10/10=20? max_stack 10 -> 20
        // rekalkulieren: 20*10/10 = 20 (voller stack), korrekt
        let mass = crafted_item_weight(&[m1, m2]);
        // erwartet Konsum-Masse 12.5 + 20 = 32.5 -> 0.75*32.5 = 24.375
        assert!((mass - 24.375).abs() < 1e-9);
    }

    #[test]
    fn durability_broken_and_stats_active() {
        let mut inst = ItemInstance::new("u", "eisenschwert", ItemModifiers::default());
        inst.durability_max = Some(100);
        inst.durability_current = Some(100);
        assert!(!inst.is_broken());
        assert!(inst.stats_active());

        inst.durability_current = Some(0);
        assert!(inst.is_broken());
        assert!(!inst.stats_active());

        // ohne Haltbarkeit immer aktiv
        let no_durability = ItemInstance::new("u2", "eisenschwert", ItemModifiers::default());
        assert!(!no_durability.is_broken());
        assert!(no_durability.stats_active());
    }

    #[test]
    fn attributes_and_resistances_data() {
        let def = weapon_def();
        let modifiers = ItemModifiers {
            attribute_modifiers: [("strength".to_string(), 2.0)].into_iter().collect(),
            resistance_modifiers: [("fire".to_string(), 5.0)].into_iter().collect(),
            ..Default::default()
        };
        let inst = ItemInstance::new("u", "eisenschwert", modifiers);
        assert_eq!(inst.effective_attribute_bonus(&def, "strength"), 2.0);
        assert_eq!(inst.effective_resistance(&def, "fire"), 5.0);
        assert_eq!(inst.effective_attribute_bonus(&def, "wisdom"), 0.0);
    }

    #[test]
    fn binding_states() {
        assert!(!BindingState::Tradeable.is_bound());
        assert!(BindingState::BindOnPickup.is_bound());
        assert!(BindingState::BindOnEquip.is_bound());
        assert!(BindingState::Bound.is_bound());
        assert!(BindingRule::from_db("boe").is_some());
        assert!(BindingState::from_db("bound").is_some());
        assert!(BindingState::from_db("unsinn").is_none());
    }

    #[test]
    fn equip_prerequisites_use_class_system() {
        let mut def = weapon_def();
        def.allowed_classes = vec![ClassStatus::Fighter];
        def.min_level = Some(5);
        assert!(!def.can_equip(4, ClassStatus::Fighter)); // zu geringes Level
        assert!(def.can_equip(5, ClassStatus::Fighter));
        assert!(!def.can_equip(5, ClassStatus::Mage)); // falsche Klasse

        def.allowed_classes = vec![];
        assert!(def.can_equip(5, ClassStatus::Mage)); // keine Beschränkung
    }

    #[test]
    fn definition_vs_instance() {
        let mut def = weapon_def();
        def.base_damage = Some(10.0);
        let mods = ItemModifiers {
            damage_modifier: 3.0,
            ..Default::default()
        };
        let inst = ItemInstance::new("uuid-1", "eisenschwert", mods);
        assert_eq!(inst.item_uuid, "uuid-1");
        assert_eq!(inst.item_id, def.item_id);
        // gleiche Definition, individuelle Werte
        assert_eq!(inst.effective_damage(&def), 13.0);
        let inst2 = ItemInstance::new("uuid-2", "eisenschwert", ItemModifiers::default());
        assert_eq!(inst2.effective_damage(&def), 10.0);
    }

    #[test]
    fn quality_modifier_represents_craft_result() {
        let mut def = weapon_def();
        def.base_quality = 100.0;
        // Crafting: weighted 110, Masterwork -> 137.5
        let weighted = weighted_material_quality(&[(100.0, 1), (120.0, 1)]);
        let final_q = CraftQuality::Masterwork.final_quality(weighted);
        let inst = ItemInstance::new(
            "u",
            "eisenschwert",
            ItemModifiers {
                quality_modifier: final_q - def.base_quality,
                ..Default::default()
            },
        );
        assert_eq!(inst.effective_numeric_quality(&def), final_q);
    }
}