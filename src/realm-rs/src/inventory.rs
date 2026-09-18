// inventory — Inventory System V1 (docs/inventory_system.md).
//
// Realm-autoritative Verwaltung von Grundinventar, ausrüstbaren
// Rucksäcken/Taschen, Equipment-/Funktionsslots und dem temporären
// Sicherheits-Puffer. Reine Zustandslogik ohne DB-I/O (Persistierung über
// db.rs).
//
// Verbindliche Regeln (docs/inventory_system.md):
// - 1 Item oder 1 Stack = exakt 1 Slot. Keine Item-Größe, kein Multi-Slot.
// - Grundinventar: Basis-Slots (Content-Wert, Konfiguration).
// - Ausgerüstete Rucksäcke: eigene Slots, Spieler-nennbar, keine
//   serverseitige Kategoriebindung an den Namen.
// - Gesamtkapazität = Grundinventar + Summe der Slots aller Rucksäcke.
// - Stacks: vorhandene passende Stacks auffüllen, dann freie Basis-Slots,
//   danach freie Bag-Slots. Nicht alles passt → Aufnahme schlägt (teilweise)
//   fehl, Rest bleibt beim Aufrufer. Kein Postfach / Überlauf.
// - Individuelle Instanzen (Modifier / Haltbarkeit) stacken nicht.
// - Equipment: genau 21 Slots; ausgerüstetes Item nie gleichzeitig im
//   normalen Inventar. Equip-Check: Level + Klasse (ItemDefinition::can_equip),
//   keine Attribute. Kategorie→Slot-Zuordnung = späterer Hook (V1: nur
//   Weapon/Armor/Accessory rüstbar).
// - Defektes Equipped (0 Haltbarkeit) wird serverseitig entfernt: freier
//   normaler Slot zuerst, sonst Sicherheits-Puffer.
// - Puffer: temporär, keine Kapazität, keine automatische Rückverschiebung
//   (nur bewusste Spieleraktion mit erneuter Raumprüfung), Inhalt beim
//   Logout verloren.
#![allow(dead_code)]
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::class::ClassStatus;
use crate::item::{ItemDefinition, ItemInstance, ItemModifiers};

/// Konfiguration des Inventory-Kerns (Content-Werte, keine Architekturregel).
#[derive(Debug, Clone, Copy)]
pub struct InventoryCfg {
    /// Anzahl der Basis-Slots des Grundinventars (Content-Wert, Default 8).
    pub base_slots: u16,
    /// Maximale Anzahl gleichzeitig ausrüstbarer Rucksäcke. Die Doku nennt
    /// keinen verpflichtenden Wert (docs/inventory_system.md §2): None =
    /// keine harte Grenze; die Architektur unterstützt eine spätere
    /// daten-/konfigurationsgetriebene Zahl.
    pub max_equipped_bags: Option<u16>,
}

impl Default for InventoryCfg {
    fn default() -> Self {
        InventoryCfg {
            base_slots: 8,
            max_equipped_bags: None,
        }
    }
}

/// Die 21 Equipment-/Funktionsslots von Inventory V1
/// (docs/inventory_system.md §7, Tabelle 1–21).
/// Serialize/Deserialize: vollständiger Player-Snapshot der Stufe B; die
/// Serde-Variantennamen ("Chest", "MainHand", …) dienen als JSON-Schlüssel.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum EquipSlot {
    Head,
    Earring1,
    Earring2,
    Neck,
    Shoulders,
    Arms,
    Hands,
    Chest,
    Waist,
    Legs,
    Feet,
    Back,
    MainHand,
    OffHand,
    Ring1,
    Ring2,
    LightSource,
    RangedWeapon,
    QuiverAmmo,
    Food,
    Drink,
}

impl EquipSlot {
    /// Kanonischer DB-Schlüssel (character_equipment.slot).
    pub fn as_db(self) -> &'static str {
        match self {
            EquipSlot::Head => "head",
            EquipSlot::Earring1 => "earring1",
            EquipSlot::Earring2 => "earring2",
            EquipSlot::Neck => "neck",
            EquipSlot::Shoulders => "shoulders",
            EquipSlot::Arms => "arms",
            EquipSlot::Hands => "hands",
            EquipSlot::Chest => "chest",
            EquipSlot::Waist => "waist",
            EquipSlot::Legs => "legs",
            EquipSlot::Feet => "feet",
            EquipSlot::Back => "back",
            EquipSlot::MainHand => "main_hand",
            EquipSlot::OffHand => "off_hand",
            EquipSlot::Ring1 => "ring1",
            EquipSlot::Ring2 => "ring2",
            EquipSlot::LightSource => "light_source",
            EquipSlot::RangedWeapon => "ranged_weapon",
            EquipSlot::QuiverAmmo => "quiver_ammo",
            EquipSlot::Food => "food",
            EquipSlot::Drink => "drink",
        }
    }

    pub fn from_db(s: &str) -> Option<EquipSlot> {
        match &*s.trim().to_lowercase() {
            "head" => Some(EquipSlot::Head),
            "earring1" => Some(EquipSlot::Earring1),
            "earring2" => Some(EquipSlot::Earring2),
            "neck" => Some(EquipSlot::Neck),
            "shoulders" => Some(EquipSlot::Shoulders),
            "arms" => Some(EquipSlot::Arms),
            "hands" => Some(EquipSlot::Hands),
            "chest" => Some(EquipSlot::Chest),
            "waist" => Some(EquipSlot::Waist),
            "legs" => Some(EquipSlot::Legs),
            "feet" => Some(EquipSlot::Feet),
            "back" => Some(EquipSlot::Back),
            "main_hand" => Some(EquipSlot::MainHand),
            "off_hand" => Some(EquipSlot::OffHand),
            "ring1" => Some(EquipSlot::Ring1),
            "ring2" => Some(EquipSlot::Ring2),
            "light_source" => Some(EquipSlot::LightSource),
            "ranged_weapon" => Some(EquipSlot::RangedWeapon),
            "quiver_ammo" => Some(EquipSlot::QuiverAmmo),
            "food" => Some(EquipSlot::Food),
            "drink" => Some(EquipSlot::Drink),
            _ => None,
        }
    }
}

/// Alle 21 V1-Equipment-Slots (Reihenfolge = docs/inventory_system.md §7).
pub const EQUIP_SLOTS: [EquipSlot; 21] = [
    EquipSlot::Head,
    EquipSlot::Earring1,
    EquipSlot::Earring2,
    EquipSlot::Neck,
    EquipSlot::Shoulders,
    EquipSlot::Arms,
    EquipSlot::Hands,
    EquipSlot::Chest,
    EquipSlot::Waist,
    EquipSlot::Legs,
    EquipSlot::Feet,
    EquipSlot::Back,
    EquipSlot::MainHand,
    EquipSlot::OffHand,
    EquipSlot::Ring1,
    EquipSlot::Ring2,
    EquipSlot::LightSource,
    EquipSlot::RangedWeapon,
    EquipSlot::QuiverAmmo,
    EquipSlot::Food,
    EquipSlot::Drink,
];

/// Ein Rucksack / eine Tasche als eigener, benannter Inventarbereich.
/// Serialize/Deserialize: Teil des vollständigen Player-Snapshots (Stufe B).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Bag {
    /// Lokaler Bag-Index des Charakters (>= 1; PK-Teil char_id+bag_id).
    pub bag_id: u64,
    /// Spieler-Namen (Organisations-/Anzeigedatum, keine Kategoriebindung).
    pub name: String,
    /// Eigene Slots (slot_count als Content-/Progressions-Wert).
    pub slots: Vec<Option<ItemInstance>>,
}

/// Ort eines Items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemLoc {
    /// Grundinventar (Slot-Index).
    Base(usize),
    /// Rucksack (bag_id, Slot-Index).
    Bag(u64, usize),
    /// Equipment-Slot.
    Equipped(EquipSlot),
    /// Position im Sicherheits-Puffer.
    Buffer(usize),
}

/// Ergebnis einer `try_add`-Aufnahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AddOutcome {
    /// Effektiv aufgenommene Menge.
    pub accepted: i64,
    /// Nicht unterbringbare Restmenge (verbleibt beim Aufrufer).
    pub remainder: i64,
}

/// Zustand eines Spieler-Inventars (docs/inventory_system.md):
/// Grundinventar, Rucksäcke, Equipment und temporärer Sicherheits-Puffer.
/// Serialize/Deserialize: vollständiger Player-Snapshot der Stufe B
/// (docs/Player_Persistenz.md §23). Der Sicherheits-Puffer wird bewusst
/// NIE serialisiert (`#[serde(skip)]`) — er ist flüchtiger Runtime-State
/// (docs/inventory_system.md §11) und verfällt beim Logout.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InventoryState {
    /// Basis-Slots des Grundinventars (len = Anzahl, Config).
    pub base_slots: Vec<Option<ItemInstance>>,
    /// Ausgerüstete Rucksäcke (Reihenfolge = Anzeige-/Füllreihenfolge).
    pub bags: Vec<Bag>,
    /// Equipment-/Funktionsslots (21, docs §7).
    pub equipped: BTreeMap<EquipSlot, ItemInstance>,
    /// Temporärer Sicherheits-Puffer (serverseitiger Ausnahmefall, §11).
    #[serde(skip)]
    pub buffer: Vec<Option<ItemInstance>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    /// Kein freier Slot / kein passender Stack.
    NoSpace,
    /// Menge <= 0 (Qt. darf nie negativ oder null sein).
    InvalidQuantity,
    /// Nicht genügend vorhandene Menge einer item_id (keine Teilentfernung).
    NotEnoughItems,
    /// Item ist nicht im normalen Inventar (Base/Bag) — equip/unequip.
    NotInInventory,
    /// Equipment-Slot ist bereits belegt.
    SlotOccupied,
    /// Equipment-Slot ist leer (unequip).
    SlotEmpty,
    /// Equip-Voraussetzungen (Level/Klasse) nicht erfüllt.
    CannotEquip,
    /// Defektes Item darf nicht angelegt werden.
    BrokenItem,
    /// Nur V1-Equip-Kategorien (Weapon/Armor/Accessory) sind rüstbar.
    NotEquippable,
    /// Bag-Name leer oder bereits vorhanden.
    InvalidBagName,
    /// Max. Anzahl ausrüstbarer Rucksäcke erreicht.
    MaxEquippedBags,
    /// Bag existiert nicht.
    NoSuchBag,
    /// Bag ist nicht leer (Entfernen würde Items verlieren).
    BagNotEmpty,
}

fn sample(def: &ItemDefinition) -> ItemInstance {
    ItemInstance {
        item_uuid: String::new(),
        item_id: def.item_id.clone(),
        count: 1,
        durability_current: None,
        durability_max: None,
        binding: crate::item::BindingState::Tradeable,
        creator_id: None,
        modifiers: ItemModifiers::default(),
    }
}

/// Stacktauge: keine individuellen Modifikatoren und keine Haltbarkeit.
/// Individuelle Instanzen (gecraftet, modifiziert, verschlissen) stacken nie
/// mit normalen identischen Items (docs/inventory_system.md §4).
fn plain_copy(inst: &ItemInstance) -> bool {
    inst.modifiers == ItemModifiers::default()
        && inst.durability_current.is_none()
        && inst.durability_max.is_none()
}

/// Ist `existing` so ergänzbar, dass `incoming` (ganz) hineinstapeln kann?
pub fn mergeable_into(existing: &ItemInstance, incoming: &ItemInstance) -> bool {
    existing.item_id == incoming.item_id && plain_copy(existing) && plain_copy(incoming)
}

/// Entfernt aus einem einzelnen Stack-Slot bis zu `remaining` Stück einer
/// item_id (deterministische Entfernungsreihenfolge von try_remove).
/// Ein auf 0 reduzierter Stack wird aufgelöst (Slot = None). Liefert die
/// noch zu entfernende Restmenge.
fn remove_from_slot(slot: &mut Option<ItemInstance>, item_id: &str, mut remaining: i64) -> i64 {
    let Some(it) = slot.as_mut() else {
        return remaining;
    };
    if it.item_id != item_id {
        return remaining;
    }
    let take = it.count.min(remaining);
    it.count -= take;
    remaining -= take;
    if it.count == 0 {
        *slot = None;
    }
    remaining
}

static UUID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn new_uuid() -> String {
    let n = UUID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("si-{:x}-{:x}", stamp, n)
}

impl InventoryState {
    /// Neues, leeres Inventar mit `base_slots` Basis-Slots.
    pub fn new(base_slots: usize) -> Self {
        InventoryState {
            base_slots: vec![None; base_slots],
            bags: Vec::new(),
            equipped: BTreeMap::new(),
            buffer: Vec::new(),
        }
    }

    /// Gesamtkapazität: Basis-Slots + Summe (Slots aller Rucksäcke).
    pub fn total_slots(&self) -> usize {
        self.base_slots.len() + self.bags.iter().map(|b| b.slots.len()).sum::<usize>()
    }

    /// Freie Basis-Slots (Grundinventar).
    pub fn free_base_slots(&self) -> usize {
        self.base_slots.iter().filter(|s| s.is_none()).count()
    }

    /// Freie normale Slots (Grundinventar + alle Rucksäcke).
    pub fn free_slots(&self) -> usize {
        self.free_base_slots()
            + self
                .bags
                .iter()
                .map(|b| b.slots.iter().filter(|s| s.is_none()).count())
                .sum::<usize>()
    }

    /// Machbarkeitsprobe (Quest-/Loot-API, docs §6): passt `qty` vollständig?
    pub fn fits(&self, def: &ItemDefinition, qty: i64) -> bool {
        let mut sim = self.clone();
        sim.try_add(def, qty).remainder == 0
    }

    /// Aktuelle Gesamtmenge einer item_id im normalen Inventar.
    pub fn count_of(&self, item_id: &str) -> i64 {
        let base: i64 = self
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|it| it.item_id == item_id)
            .map(|it| it.count)
            .sum();
        let bags: i64 = self
            .bags
            .iter()
            .flat_map(|b| &b.slots)
            .filter_map(|s| s.as_ref())
            .filter(|it| it.item_id == item_id)
            .map(|it| it.count)
            .sum();
        base + bags
    }

    fn slot_of(&self, uuid: &str) -> Option<ItemLoc> {
        for (i, s) in self.base_slots.iter().enumerate() {
            if s.as_ref().map(|it| it.item_uuid.as_str()) == Some(uuid) {
                return Some(ItemLoc::Base(i));
            }
        }
        for b in &self.bags {
            for (i, s) in b.slots.iter().enumerate() {
                if s.as_ref().map(|it| it.item_uuid.as_str()) == Some(uuid) {
                    return Some(ItemLoc::Bag(b.bag_id, i));
                }
            }
        }
        for (slot, it) in &self.equipped {
            if it.item_uuid == uuid {
                return Some(ItemLoc::Equipped(*slot));
            }
        }
        for (i, s) in self.buffer.iter().enumerate() {
            if s.as_ref().map(|it| it.item_uuid.as_str()) == Some(uuid) {
                return Some(ItemLoc::Buffer(i));
            }
        }
        None
    }

    /// Passender, noch nicht voller Stack einer item_id (Basis zuerst).
    fn matching_stack(&self, def: &ItemDefinition) -> Option<(ItemLoc, i64)> {
        for (i, s) in self.base_slots.iter().enumerate() {
            if let Some(it) = s {
                if mergeable_into(it, &sample(def)) && it.count < def.max_stack {
                    return Some((ItemLoc::Base(i), def.max_stack - it.count));
                }
            }
        }
        for b in &self.bags {
            for (i, s) in b.slots.iter().enumerate() {
                if let Some(it) = s {
                    if mergeable_into(it, &sample(def)) && it.count < def.max_stack {
                        return Some((ItemLoc::Bag(b.bag_id, i), def.max_stack - it.count));
                    }
                }
            }
        }
        None
    }

    /// Erster freier Slot (Basis, dann Rucksäcke in Anzeige-Reihenfolge).
    fn next_free_slot(&self) -> Option<(ItemLoc, usize)> {
        if let Some(i) = self.base_slots.iter().position(|s| s.is_none()) {
            return Some((ItemLoc::Base(i), i));
        }
        for b in &self.bags {
            if let Some(i) = b.slots.iter().position(|s| s.is_none()) {
                return Some((ItemLoc::Bag(b.bag_id, i), i));
            }
        }
        None
    }

    fn instance_of(&self, uuid: &str) -> Option<ItemInstance> {
        match self.slot_of(uuid) {
            Some(ItemLoc::Base(i)) => self.base_slots[i].clone(),
            Some(ItemLoc::Bag(id, i)) => self
                .bags
                .iter()
                .find(|b| b.bag_id == id)
                .and_then(|b| b.slots[i].clone()),
            Some(ItemLoc::Equipped(slot)) => self.equipped.get(&slot).cloned(),
            Some(ItemLoc::Buffer(i)) => self.buffer[i].clone(),
            None => None,
        }
    }

    fn put_instance(&mut self, def: &ItemDefinition, inst: &ItemInstance) -> bool {
        // Erst passenden Stack auffüllen (ganzer Stack, kein Aufteilen).
        if def.max_stack > 1 && plain_copy(inst) {
            if let Some((loc, cap)) = self.matching_stack(def) {
                if cap >= inst.count {
                    self.add_to(loc, inst.count);
                    return true;
                }
            }
        }
        // Sonst ganzer Stack in den nächsten freien Slot (Basis, dann Bag).
        if let Some(i) = self.base_slots.iter().position(|s| s.is_none()) {
            self.base_slots[i] = Some(inst.clone());
            return true;
        }
        for b in &mut self.bags {
            if let Some(i) = b.slots.iter().position(|s| s.is_none()) {
                b.slots[i] = Some(inst.clone());
                return true;
            }
        }
        false
    }

    /// Erhöht die Menge eines bestehenden Stacks um `qty`.
    fn add_to(&mut self, loc: ItemLoc, qty: i64) {
        match loc {
            ItemLoc::Base(i) => {
                if let Some(it) = self.base_slots[i].as_mut() {
                    it.count += qty;
                }
            }
            ItemLoc::Bag(id, i) => {
                if let Some(b) = self.bags.iter_mut().find(|b| b.bag_id == id) {
                    if let Some(it) = b.slots[i].as_mut() {
                        it.count += qty;
                    }
                }
            }
            _ => {}
        }
    }

    /// Aufnahme normaler Items (Loot/Quest, docs §4/§6): erst vorhandene
    /// passende Stacks auffüllen, dann freie normale Slots. Nicht passende
    /// Menge bleibt beim Aufrufer (kein Verlust, kein Puffer).
    pub fn try_add(&mut self, def: &ItemDefinition, qty: i64) -> AddOutcome {
        if qty <= 0 {
            return AddOutcome::default();
        }
        let mut remaining = qty;
        // 1) Vorhandene passende Stacks auffüllen (bis max_stack).
        while let Some((loc, cap)) = self.matching_stack(def) {
            let take = cap.min(remaining);
            self.add_to(loc, take);
            remaining -= take;
            if remaining == 0 {
                break;
            }
        }
        // 2) Neue Stacks auf freie Slots (Basis, dann Rucksäcke).
        while remaining > 0 {
            let take = if def.max_stack > 1 {
                remaining.min(def.max_stack)
            } else {
                1
            };
            if !self.place_new_stack(def, take) {
                break;
            }
            remaining -= take;
        }
        AddOutcome {
            accepted: qty - remaining,
            remainder: remaining,
        }
    }

    /// Erzeugt einen frischen Stack auf dem nächsten freien Slot.
    fn place_new_stack(&mut self, def: &ItemDefinition, qty: i64) -> bool {
        let Some((loc, _)) = self.next_free_slot() else {
            return false;
        };
        let inst = ItemInstance {
            item_uuid: new_uuid(),
            item_id: def.item_id.clone(),
            count: qty,
            durability_current: None,
            durability_max: None,
            binding: crate::item::BindingState::Tradeable,
            creator_id: None,
            modifiers: ItemModifiers::default(),
        };
        match loc {
            ItemLoc::Base(i) => self.base_slots[i] = Some(inst),
            ItemLoc::Bag(id, i) => {
                if let Some(b) = self.bags.iter_mut().find(|b| b.bag_id == id) {
                    b.slots[i] = Some(inst);
                } else {
                    return false;
                }
            }
            ItemLoc::Equipped(_) | ItemLoc::Buffer(_) => return false,
        }
        true
    }

    /// Serverseitige, exakt validierte Entfernung einer Item-Menge aus dem
    /// normalen Inventar (docs/Quest-System.md §27.13, §27.26 „Stacks – nur
    /// die benötigte Menge"): Eine item_id wird über die Item-ID referenziert,
    /// nicht über einzelne Instanzen/UUIDs. Es wird nur die benötigte Menge
    /// entfernt; der Rest eines Stacks bleibt normal nutzbar. Bei unzureichen-
    /// der Gesamtmenge wird NICHTS entfernt (keine Teilentfernung). Entfernt
    /// wird deterministisch (Basis-Slots von vorn, dann Rucksäcke in Anzeige-
    /// Reihenfolge), ohne Auswahl einzelner Instanzen. Liefert die tatsächlich
    /// entfernte Menge (== qty).
    pub fn try_remove(&mut self, item_id: &str, qty: i64) -> Result<i64, InventoryError> {
        if qty <= 0 {
            return Err(InventoryError::InvalidQuantity);
        }
        // Keine Teilentfernung: erst vollständige Vorprüfung, dann Mutieren.
        if self.count_of(item_id) < qty {
            return Err(InventoryError::NotEnoughItems);
        }
        let mut remaining = qty;
        for slot in self.base_slots.iter_mut() {
            remaining = remove_from_slot(slot, item_id, remaining);
            if remaining == 0 {
                return Ok(qty);
            }
        }
        for b in &mut self.bags {
            for slot in b.slots.iter_mut() {
                remaining = remove_from_slot(slot, item_id, remaining);
                if remaining == 0 {
                    return Ok(qty);
                }
            }
        }
        unreachable!("validierte Menge konnte nicht vollständig entfernt werden")
    }

    /// Anzahl freier Plätze für eine item_id unter Berücksichtigung der
    /// Stack-Auffüllung (Kapazitäts-API, docs §6).
    pub fn capacity_for(&self, def: &ItemDefinition) -> i64 {
        let free_slots = self.free_slots() as i64;
        if def.max_stack <= 1 {
            return free_slots;
        }
        let partial: i64 = self
            .base_slots
            .iter()
            .chain(self.bags.iter().flat_map(|b| &b.slots))
            .filter_map(|s| s.as_ref())
            .filter(|it| mergeable_into(it, &sample(def)))
            .map(|it| (def.max_stack - it.count).max(0))
            .sum();
        partial + free_slots * def.max_stack
    }

    /// Equip: Item aus dem normalen Inventar in einen freien Equipment-Slot.
    /// Voraussetzungen: V1-Equip-Kategorie, Level/Klasse per Definition,
    /// nicht defekt. Ausgerüstetes Item liegt nie gleichzeitig im Inventar.
    pub fn try_equip(
        &mut self,
        def: &ItemDefinition,
        uuid: &str,
        slot: EquipSlot,
        level: i64,
        class: ClassStatus,
    ) -> Result<(), InventoryError> {
        if self.equipped.contains_key(&slot) {
            return Err(InventoryError::SlotOccupied);
        }
        let loc = self.slot_of(uuid).ok_or(InventoryError::NotInInventory)?;
        if !matches!(loc, ItemLoc::Base(_) | ItemLoc::Bag(_, _)) {
            return Err(InventoryError::NotInInventory);
        }
        if !matches!(
            def.category,
            crate::item::ItemCategory::Weapon
                | crate::item::ItemCategory::Armor
                | crate::item::ItemCategory::Accessory
        ) {
            return Err(InventoryError::NotEquippable);
        }
        if !def.can_equip(level, class) {
            return Err(InventoryError::CannotEquip);
        }
        let inst = self
            .instance_of(uuid)
            .ok_or(InventoryError::NotInInventory)?;
        if inst.item_id != def.item_id {
            return Err(InventoryError::NotInInventory);
        }
        if inst.is_broken() {
            return Err(InventoryError::BrokenItem);
        }
        match loc {
            ItemLoc::Base(i) => self.base_slots[i] = None,
            ItemLoc::Bag(id, i) => {
                if let Some(b) = self.bags.iter_mut().find(|b| b.bag_id == id) {
                    b.slots[i] = None;
                }
            }
            _ => return Err(InventoryError::NotInInventory),
        }
        self.equipped.insert(slot, inst);
        Ok(())
    }

    /// Unequip: Equipment-Item zurück ins normale Inventar. Ziel: vorhandener
    /// passender Stack (ganz), sonst freier Slot. Ohne Platz: Fehler.
    pub fn try_unequip(
        &mut self,
        def: &ItemDefinition,
        slot: EquipSlot,
    ) -> Result<(), InventoryError> {
        let inst = self
            .equipped
            .get(&slot)
            .cloned()
            .ok_or(InventoryError::SlotEmpty)?;
        if inst.item_id != def.item_id {
            return Err(InventoryError::NotInInventory);
        }
        let ok = self.put_instance(def, &inst);
        if !ok {
            return Err(InventoryError::NoSpace);
        }
        self.equipped.remove(&slot);
        Ok(())
    }

    /// Serverseitige Entfernung defekter Equipment-Items (docs §10/§12):
    /// 0 Haltbarkeit → aus dem Equipment-Slot. Ziel: freier normaler Slot,
    /// sonst Sicherheits-Puffer. Liefert Anzahl verschobener Items.
    pub fn remove_broken_equipment(
        &mut self,
        defs: &std::collections::HashMap<String, ItemDefinition>,
    ) -> usize {
        let broken: Vec<(EquipSlot, ItemDefinition, ItemInstance)> = self
            .equipped
            .iter()
            .filter(|(_, it)| it.is_broken())
            .map(|(slot, it)| {
                let def = defs
                    .get(&it.item_id)
                    .cloned()
                    .unwrap_or_else(ItemDefinition::default);
                (*slot, def, it.clone())
            })
            .collect();
        let mut moved = 0;
        for (slot, def, inst) in broken {
            if self.put_instance(&def, &inst) {
                self.equipped.remove(&slot);
            } else {
                // Serverseitiger Ausnahmefall → Sicherheits-Puffer.
                self.equipped.remove(&slot);
                self.buffer.push(Some(inst));
            }
            moved += 1;
        }
        moved
    }

    /// Anzahl Items im Sicherheits-Puffer.
    pub fn buffer_len(&self) -> usize {
        self.buffer.iter().filter(|s| s.is_some()).count()
    }

    /// Bewusste Spieleraktion Puffer → Inventar (docs §11). Nie automatisch.
    /// Server prüft erneut (Stack-Auffüllen zuerst, sonst freier Slot).
    pub fn try_buffer_to_inventory(
        &mut self,
        def: &ItemDefinition,
        uuid: &str,
    ) -> Result<(), InventoryError> {
        let loc = self.slot_of(uuid).ok_or(InventoryError::NotInInventory)?;
        let ItemLoc::Buffer(i) = loc else {
            return Err(InventoryError::NotInInventory);
        };
        let inst = self
            .buffer
            .get(i)
            .and_then(|s| s.as_ref())
            .cloned()
            .ok_or(InventoryError::NotInInventory)?;
        let ok = self.put_instance(def, &inst);
        if !ok {
            return Err(InventoryError::NoSpace);
        }
        self.buffer[i] = None;
        Ok(())
    }

    /// Puffer beim Logout leeren (docs §11): Items verfallen. Liefert die
    /// verfallenen Instanzen (für DB-Cleanup der item_instances-Zeilen).
    pub fn drop_buffer(&mut self) -> Vec<ItemInstance> {
        let items: Vec<ItemInstance> = self.buffer.iter_mut().filter_map(|s| s.take()).collect();
        self.buffer.clear();
        items
    }

    /// Rucksack hinzufügen (Content-/Progressions-Integrationspunkt).
    /// slot_count ist ein Content-Wert (Erwerb ist Progressions-Frage).
    pub fn create_bag(
        &mut self,
        cfg: &InventoryCfg,
        name: &str,
        slot_count: u16,
    ) -> Result<u64, InventoryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(InventoryError::InvalidBagName);
        }
        if self.bags.iter().any(|b| b.name == name) {
            return Err(InventoryError::InvalidBagName);
        }
        if slot_count == 0 {
            return Err(InventoryError::BagNotEmpty);
        }
        if let Some(max) = cfg.max_equipped_bags {
            if self.bags.len() as u16 >= max {
                return Err(InventoryError::MaxEquippedBags);
            }
        }
        let next_id = self.bags.iter().map(|b| b.bag_id).max().unwrap_or(0) + 1;
        self.bags.push(Bag {
            bag_id: next_id,
            name: name.to_string(),
            slots: vec![None; slot_count as usize],
        });
        Ok(next_id)
    }

    /// Spieler benennt einen Rucksack um (rein organisatorisch, keine
    /// Kategoriebindung — docs §2).
    pub fn rename_bag(&mut self, bag_id: u64, name: &str) -> Result<(), InventoryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(InventoryError::InvalidBagName);
        }
        if self
            .bags
            .iter()
            .any(|b| b.bag_id != bag_id && b.name == name)
        {
            return Err(InventoryError::InvalidBagName);
        }
        let b = self
            .bags
            .iter_mut()
            .find(|b| b.bag_id == bag_id)
            .ok_or(InventoryError::NoSuchBag)?;
        b.name = name.to_string();
        Ok(())
    }

    /// Rucksack entfernen. Nur leere Rucksäcke sind entfernbar (Inhalt würde
    /// sonst verloren; die automatische Umverteilung gilt ausschließlich für
    /// die serverseitige Equipment-Entfernung, docs §10).
    pub fn remove_bag(&mut self, bag_id: u64) -> Result<(), InventoryError> {
        let idx = self
            .bags
            .iter()
            .position(|b| b.bag_id == bag_id)
            .ok_or(InventoryError::NoSuchBag)?;
        if self.bags[idx].slots.iter().any(|s| s.is_some()) {
            return Err(InventoryError::BagNotEmpty);
        }
        self.bags.remove(idx);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class::ClassStatus;
    use crate::item::ItemCategory;

    fn cfg(n: u16) -> InventoryCfg {
        InventoryCfg {
            base_slots: n,
            max_equipped_bags: None,
        }
    }

    fn potion() -> ItemDefinition {
        let mut d = ItemDefinition::new("hp_potion", "Heiltrank", ItemCategory::Potion);
        d.max_stack = 20;
        d
    }

    fn sword() -> ItemDefinition {
        ItemDefinition::new("eisenschwert", "Eisenschwert", ItemCategory::Weapon)
    }

    #[test]
    fn default_cfg_has_8_base_slots() {
        let c = InventoryCfg::default();
        assert_eq!(c.base_slots, 8);
        assert_eq!(c.max_equipped_bags, None);
        let inv = InventoryState::new(c.base_slots as usize);
        assert_eq!(inv.total_slots(), 8);
        assert_eq!(inv.free_slots(), 8);
    }

    #[test]
    fn stacking_fills_existing_stack_first() {
        let mut inv = InventoryState::new(8);
        let def = potion();
        let r = inv.try_add(&def, 10);
        assert_eq!((r.accepted, r.remainder), (10, 0));
        assert_eq!(inv.count_of("hp_potion"), 10);
        assert_eq!(inv.total_slots(), 8);
        // Zweite Aufnahme füllt denselben Stack auf (20 = max_stack).
        let r = inv.try_add(&def, 30);
        assert_eq!((r.accepted, r.remainder), (30, 0));
        assert_eq!(inv.count_of("hp_potion"), 40);
        // 2 Stacks: 20 + 20.
        let stacks: Vec<&ItemInstance> = inv
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|it| it.item_id == "hp_potion")
            .collect();
        assert_eq!(stacks.len(), 2);
        assert_eq!(stacks[0].count, 20);
        assert_eq!(stacks[1].count, 20);
    }

    #[test]
    fn full_inventory_returns_remainder() {
        let mut inv = InventoryState::new(2);
        let def = potion();
        inv.try_add(&def, 40); // 2 volle Stacks à 20
        assert_eq!(inv.free_slots(), 0);
        let r = inv.try_add(&def, 25);
        assert_eq!((r.accepted, r.remainder), (0, 25));
    }

    #[test]
    fn non_stackable_items_need_one_slot_each() {
        let mut inv = InventoryState::new(3);
        let def = sword();
        assert_eq!(def.max_stack, 1);
        let r = inv.try_add(&def, 5);
        // Nur 3 freie Slots → 3 angenommen, 2 Rest.
        assert_eq!((r.accepted, r.remainder), (3, 2));
    }

    #[test]
    fn individual_instances_do_not_merge() {
        let mut inv = InventoryState::new(4);
        let def = potion();
        inv.try_add(&def, 10);
        // Instanz mit individuellem Modifier (gecraftet) — darf nichts füllen.
        let mut crafted = ItemInstance::new(
            "c1",
            "hp_potion",
            ItemModifiers {
                quality_modifier: 5.0,
                ..Default::default()
            },
        );
        crafted.count = 1;
        assert!(!plain_copy(&crafted));
        assert!(!mergeable_into(
            inv.base_slots[0].as_ref().unwrap(),
            &crafted
        ));
        let ok = inv.put_instance(&def, &crafted);
        assert!(ok);
        // Neue Zeile, keine Stack-Verschmelzung.
        assert_eq!(inv.count_of("hp_potion"), 11);
        let n = inv
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|it| it.item_id == "hp_potion")
            .count();
        assert_eq!(n, 2);
    }

    #[test]
    fn bags_add_capacity_and_are_used_after_base() {
        let c = cfg(1);
        let mut inv = InventoryState::new(1);
        let def = potion();
        inv.try_add(&def, 20); // Basis-Slot mit vollem Stack belegt
        assert_eq!(inv.free_slots(), 0);
        // Bag mit 3 Slots anlegen → freie Slots springen auf 3.
        let bag_id = inv.create_bag(&c, "Tränke", 3).unwrap();
        assert_eq!(bag_id, 1);
        assert_eq!(inv.free_slots(), 3);
        assert_eq!(inv.total_slots(), 4);
        // Neue Stacks landen in der Bag (nur noch Bag-Slots frei).
        let r = inv.try_add(&def, 15);
        assert_eq!((r.accepted, r.remainder), (15, 0));
        let b = inv.bags.iter().find(|b| b.bag_id == bag_id).unwrap();
        assert_eq!(b.slots.iter().filter(|s| s.is_some()).count(), 1);
        assert_eq!(inv.count_of("hp_potion"), 35);
    }

    #[test]
    fn bag_names_are_organized_not_categorized() {
        let c = cfg(4);
        let mut inv = InventoryState::new(4);
        // "Rohstoffe" darf beliebige Items enthalten (keine Kategoriebindung).
        let def = potion();
        inv.try_add(&def, 3);
        assert_eq!(inv.count_of("hp_potion"), 3);
        // Bag kann unabhängig vom Namen befüllt werden.
        inv.create_bag(&c, "Rohstoffe", 10).unwrap();
        inv.try_add(&def, 5);
        assert_eq!(inv.count_of("hp_potion"), 8);
    }

    #[test]
    fn equip_checks_slot_and_category_and_req() {
        let mut inv = InventoryState::new(8);
        let def = sword();
        inv.try_add(&def, 1);
        let uuid = inv.base_slots[0].as_ref().unwrap().item_uuid.clone();
        // Potion nicht rüstbar.
        let p = potion();
        assert_eq!(
            inv.try_equip(&p, &uuid, EquipSlot::MainHand, 10, ClassStatus::Adventurer),
            Err(InventoryError::NotEquippable)
        );
        // Slot bereits belegt.
        assert_eq!(
            inv.try_equip(
                &def,
                &uuid,
                EquipSlot::MainHand,
                10,
                ClassStatus::Adventurer
            ),
            Ok(())
        );
        assert_eq!(
            inv.try_equip(
                &def,
                &uuid,
                EquipSlot::MainHand,
                10,
                ClassStatus::Adventurer
            ),
            Err(InventoryError::SlotOccupied)
        );
        // Ausgerüstet → nicht mehr im normalen Inventar.
        assert!(inv.base_slots.iter().all(|s| s.is_none()));
        assert_eq!(inv.equipped.len(), 1);
    }

    #[test]
    fn equip_min_level_and_class_req() {
        let mut def = sword();
        def.min_level = Some(5);
        def.allowed_classes = vec![ClassStatus::Fighter];
        let mut inv = InventoryState::new(4);
        inv.try_add(&def, 1);
        let uuid = inv.base_slots[0].as_ref().unwrap().item_uuid.clone();
        assert_eq!(
            inv.try_equip(&def, &uuid, EquipSlot::MainHand, 4, ClassStatus::Fighter),
            Err(InventoryError::CannotEquip)
        );
        assert_eq!(
            inv.try_equip(&def, &uuid, EquipSlot::MainHand, 5, ClassStatus::Mage),
            Err(InventoryError::CannotEquip)
        );
        assert_eq!(
            inv.try_equip(&def, &uuid, EquipSlot::MainHand, 5, ClassStatus::Fighter),
            Ok(())
        );
    }

    #[test]
    fn equip_rejects_broken_item() {
        let mut inv = InventoryState::new(4);
        let def = sword();
        inv.try_add(&def, 1);
        // Instanz kaputt setzen (durability = 0).
        inv.base_slots[0].as_mut().unwrap().durability_current = Some(0);
        inv.base_slots[0].as_mut().unwrap().durability_max = Some(100);
        let uuid = inv.base_slots[0].as_ref().unwrap().item_uuid.clone();
        assert_eq!(
            inv.try_equip(
                &def,
                &uuid,
                EquipSlot::MainHand,
                10,
                ClassStatus::Adventurer
            ),
            Err(InventoryError::BrokenItem)
        );
    }

    #[test]
    fn unequip_requires_free_slot() {
        let mut inv = InventoryState::new(0);
        let def = sword();
        // Item direkt ins Equipment legen (zulasten des normalen Inventars).
        inv.equipped.insert(
            EquipSlot::MainHand,
            ItemInstance::new("sw1", "eisenschwert", ItemModifiers::default()),
        );
        assert_eq!(inv.free_slots(), 0);
        assert_eq!(
            inv.try_unequip(&def, EquipSlot::MainHand),
            Err(InventoryError::NoSpace)
        );
        assert!(inv.equipped.contains_key(&EquipSlot::MainHand));
    }

    #[test]
    fn unequip_merges_into_matching_stack() {
        let mut inv = InventoryState::new(4);
        let pot = potion();
        inv.try_add(&pot, 5);
        // Equipment-Stack mit 3 Tränken (normale Instanz).
        inv.equipped.insert(
            EquipSlot::Food,
            ItemInstance::new("p-unit", "hp_potion", ItemModifiers::default()).with_ct(3),
        );
        inv.try_unequip(&pot, EquipSlot::Food).unwrap();
        // 5 + 3 = 8 auf EINEN Stack verschmolzen.
        assert_eq!(inv.count_of("hp_potion"), 8);
        let stacks: Vec<_> = inv
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|it| it.item_id == "hp_potion")
            .collect();
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].count, 8);
    }

    #[test]
    fn broken_equipment_goes_to_inventory_then_buffer() {
        // Fall 1: freier Slot → Inventar.
        let mut inv = InventoryState::new(2);
        let def = sword();
        let mut broken = ItemInstance::new("b1", "eisenschwert", ItemModifiers::default());
        broken.durability_current = Some(0);
        broken.durability_max = Some(100);
        inv.equipped.insert(EquipSlot::MainHand, broken);
        let defs: std::collections::HashMap<String, ItemDefinition> =
            [("eisenschwert".to_string(), def.clone())]
                .into_iter()
                .collect();
        assert_eq!(inv.remove_broken_equipment(&defs), 1);
        assert!(inv.equipped.is_empty());
        assert_eq!(inv.buffer_len(), 0);
        assert_eq!(inv.base_slots.iter().filter(|s| s.is_some()).count(), 1);

        // Fall 2: kein Platz → Sicherheits-Puffer (serverseitiger Ausnahmefall).
        let mut inv = InventoryState::new(0);
        let mut broken = ItemInstance::new("b2", "eisenschwert", ItemModifiers::default());
        broken.durability_current = Some(0);
        broken.durability_max = Some(100);
        inv.equipped.insert(EquipSlot::MainHand, broken);
        assert_eq!(inv.remove_broken_equipment(&defs), 1);
        assert!(inv.equipped.is_empty());
        assert_eq!(inv.buffer_len(), 1);
    }

    #[test]
    fn buffer_requires_explicit_move_with_capacity_check() {
        let mut inv = InventoryState::new(0);
        let def = potion();
        inv.buffer.push(Some(
            ItemInstance::new("pb1", "hp_potion", ItemModifiers::default()).with_ct(3),
        ));
        // Kein Platz → Fehler, Item bleibt im Puffer.
        assert_eq!(
            inv.try_buffer_to_inventory(&def, "pb1"),
            Err(InventoryError::NoSpace)
        );
        assert_eq!(inv.buffer_len(), 1);
        // Platz schaffen → bewusste Übertragung klappt.
        inv = InventoryState::new(4);
        inv.buffer.push(Some(
            ItemInstance::new("pb1", "hp_potion", ItemModifiers::default()).with_ct(3),
        ));
        inv.try_add(&def, 2);
        inv.try_buffer_to_inventory(&def, "pb1").unwrap();
        assert_eq!(inv.buffer_len(), 0);
        assert_eq!(inv.count_of("hp_potion"), 5);
        // Kein automatisches Zurückspringen (kein weiteres Mock nötig —
        // Kernalgorithmus hat keinen Auto-Move).
    }

    #[test]
    fn drop_buffer_loses_items_on_logout() {
        let mut inv = InventoryState::new(4);
        inv.buffer.push(Some(
            ItemInstance::new("x1", "hp_potion", ItemModifiers::default()).with_ct(2),
        ));
        inv.buffer.push(Some(ItemInstance::new(
            "x2",
            "eisenschwert",
            ItemModifiers::default(),
        )));
        let lost = inv.drop_buffer();
        assert_eq!(lost.len(), 2);
        assert_eq!(inv.buffer_len(), 0);
    }

    #[test]
    fn bag_create_rename_remove_and_cap() {
        let c = cfg(4);
        let mut inv = InventoryState::new(4);
        let id = inv.create_bag(&c, "Erste", 4).unwrap();
        // Duplikatname abgelehnt.
        assert_eq!(
            inv.create_bag(&c, "Erste", 4),
            Err(InventoryError::InvalidBagName)
        );
        assert!(inv.rename_bag(id, "Zweiter Name").is_ok());
        // Bag mit Inhalt nicht entfernbar.
        inv.bags.iter_mut().find(|b| b.bag_id == id).unwrap().slots[0] =
            Some(ItemInstance::new("in", "hp_potion", ItemModifiers::default()).with_ct(1));
        assert_eq!(inv.remove_bag(id), Err(InventoryError::BagNotEmpty));
        // Leer → entfernbar.
        inv.bags.iter_mut().find(|b| b.bag_id == id).unwrap().slots[0] = None;
        assert!(inv.remove_bag(id).is_ok());

        // Max-Grenze (konfigurierbar, docs §2).
        let capped = InventoryCfg {
            base_slots: 4,
            max_equipped_bags: Some(2),
        };
        let mut inv = InventoryState::new(4);
        assert!(inv.create_bag(&capped, "A", 2).is_ok());
        assert!(inv.create_bag(&capped, "B", 2).is_ok());
        assert_eq!(
            inv.create_bag(&capped, "C", 2),
            Err(InventoryError::MaxEquippedBags)
        );
    }

    #[test]
    fn fits_and_capacity_for_quest_api() {
        let mut inv = InventoryState::new(2);
        let pot = potion();
        inv.try_add(&pot, 10); // 1 Stack à 10, 1 freier Slot
        let sim = inv.clone();
        // 15 Tränke passen: 10 in den vorhandenen Stack + 15 → 25, zu viel:
        // 10 freie (Stack 10→20) + 20 (freier Slot) = 30 → ja.
        assert!(sim.fits(&pot, 30));
        assert!(!sim.fits(&pot, 31));
        assert_eq!(sim.capacity_for(&pot), 30);
        // Nicht-stackbar: nur der freie Slot.
        let sw = sword();
        assert_eq!(sim.capacity_for(&sw), 1);
    }

    // ── try_remove (Quest V1.2a, docs/Quest-System.md §27.13/§27.26) ──────

    #[test]
    fn remove_stacks_only_required_quantity_rest_stays() {
        let mut inv = InventoryState::new(4);
        let def = potion();
        inv.try_add(&def, 50); // 1 Stack à 20 + 1 Stack à 20 + 1 Stack à 10
        let removed = inv.try_remove("hp_potion", 45).unwrap();
        assert_eq!(removed, 45);
        assert_eq!(inv.count_of("hp_potion"), 5);
        let stacks: Vec<i64> = inv
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|it| it.item_id == "hp_potion")
            .map(|it| it.count)
            .collect();
        // Rest bleibt als nutzbarer Stack (5).
        assert_eq!(stacks, vec![5]);
    }

    #[test]
    fn remove_rejects_nonpositive_quantities() {
        let mut inv = InventoryState::new(4);
        let def = potion();
        inv.try_add(&def, 10);
        assert_eq!(
            inv.try_remove("hp_potion", 0),
            Err(InventoryError::InvalidQuantity)
        );
        assert_eq!(
            inv.try_remove("hp_potion", -5),
            Err(InventoryError::InvalidQuantity)
        );
        // Keine Mutation durch abgelehnte Anfragen.
        assert_eq!(inv.count_of("hp_potion"), 10);
    }

    #[test]
    fn remove_insufficient_quantity_removes_nothing() {
        let mut inv = InventoryState::new(4);
        let def = potion();
        inv.try_add(&def, 10);
        assert_eq!(
            inv.try_remove("hp_potion", 11),
            Err(InventoryError::NotEnoughItems)
        );
        // Keine Teilentfernung: Zustand bleibt unverändert.
        assert_eq!(inv.count_of("hp_potion"), 10);
        assert_eq!(inv.base_slots.iter().filter(|s| s.is_some()).count(), 1);
        // Unbekannte Item-ID → nichts zu entfernen.
        assert_eq!(
            inv.try_remove("einhornhorn", 1),
            Err(InventoryError::NotEnoughItems)
        );
    }

    #[test]
    fn remove_handles_exact_quantity_and_multiple_stacks() {
        let mut inv = InventoryState::new(4);
        let def = potion();
        inv.try_add(&def, 50); // 3 Stacks: 20 + 20 + 10
        assert_eq!(inv.try_remove("hp_potion", 40).unwrap(), 40);
        assert_eq!(inv.count_of("hp_potion"), 10);
        // Stack 3 (10) bleibt unangetastet — nur die benötigte Menge.
        let stacks: Vec<i64> = inv
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|it| it.item_id == "hp_potion")
            .map(|it| it.count)
            .collect();
        assert_eq!(stacks, vec![10]);
    }

    #[test]
    fn remove_spans_base_and_bag_slots_deterministically() {
        let c = cfg(1);
        let mut inv = InventoryState::new(1);
        let def = potion();
        inv.try_add(&def, 20); // Basis: voller Stack
        let bag_id = inv.create_bag(&c, "Tränke", 2).unwrap();
        inv.try_add(&def, 10); // Bag: 1 Stack à 10
                               // Basis zuerst (20), dann Bag (10) → 30 insgesamt entfernbar.
        assert_eq!(inv.try_remove("hp_potion", 25).unwrap(), 25);
        // Basis leer, Bag-Rest 5.
        assert_eq!(inv.count_of("hp_potion"), 5);
        assert!(inv.base_slots[0].is_none());
        let b = inv.bags.iter().find(|b| b.bag_id == bag_id).unwrap();
        assert_eq!(
            b.slots
                .iter()
                .map(|s| s.as_ref().map(|it| it.count))
                .collect::<Vec<_>>(),
            vec![Some(5), None]
        );
    }

    #[test]
    fn remove_non_stackable_items_needs_exact_count() {
        let mut inv = InventoryState::new(3);
        let sw = sword();
        // 1 Item = 1 Slot (max_stack 1): 3 Schwerter.
        inv.try_add(&sw, 3);
        assert_eq!(inv.try_remove("eisenschwert", 2).unwrap(), 2);
        assert_eq!(inv.count_of("eisenschwert"), 1);
        assert_eq!(
            inv.try_remove("eisenschwert", 2),
            Err(InventoryError::NotEnoughItems)
        );
        // Kein Teilentfernen auch über mehrere nicht-stapelbare Slots.
        assert_eq!(inv.count_of("eisenschwert"), 1);
    }

    #[test]
    fn equip_slot_db_keys_roundtrip() {
        for s in EQUIP_SLOTS {
            assert_eq!(EquipSlot::from_db(s.as_db()), Some(s));
        }
        assert!(EquipSlot::from_db("unsinn").is_none());
    }

    /// Test-Helfer: Instanz mit gegebener Stackgröße.
    trait WithCtr {
        fn with_ct(self, count: i64) -> Self;
    }
    impl WithCtr for ItemInstance {
        fn with_ct(mut self, count: i64) -> Self {
            self.count = count;
            self
        }
    }
}
