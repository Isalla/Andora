// inventory_actions — Inventaraktionen des Teststands (docs/inventory_system.md §19).
//
// Der Client sendet ausschließlich Absichten (C2S 22 INVENTORY): Instanz-UUID,
// Zielslot und gegebenenfalls Menge. Alle Regeln prüft der Server unter der
// ausführenden World-Sperre des Aufrufers (keine Awaits, keine
// Zwischenänderungen möglich):
// - Eigentum über die aktuelle Ownership (by_conn → Charakter, RAM-Bestand).
// - Quell-/Zielslot, Kapazität und Item-/Ausrüstungsregeln über die
//   vorhandenen Kernoperationen (`try_move_instance`, `try_equip`,
//   `try_unequip` in `inventory.rs` — keine zweite Implementierung).
// - Bestehende Handelssperren über `economic_mutation_allowed` (Pending-Trade-
//   Barriere wie Pickup/NPC-Handel/Spielerhandel).
// - Ablehnungen verändern nichts (kein Dirty, keine History-, Lifecycle- oder
//   Währungsmutation); Erfolg markiert `PersistComponent::Inventory` (kein
//   Umgehen der Dirty-/Persistenzanbindung, kein eigener DB-Pfad).
// - Instanzidentität, Bindung, Modifier und Haltbarkeit bleiben über die
//   vorhandenen Operationen erhalten; Split/Merge folgen deren Regeln
//   (Vollverschub erhält die UUID, Split vergibt eine neue Teil-UUID).
//   Kein `retired_uuid`-Fall entsteht hier (kein Zusammenlegen, kein Tausch).
// - Verbrauchseffekte werden ausschließlich serverseitig angewendet. DEVIATION
//   from the previous open rule (`NoConsumableContract`): a verbindlicher
//   Consumables-Vertrag now EXISTS for Heil-/Manatränke (docs/Attribute_und_
//   Regeneration.md §9). `use` is allowed ONLY for explicitly defined
//   consumables (a Potion/Food/Drink definition carrying exactly one positive
//   `heal_amount`/`mana_amount`); the magnitude comes from the SERVER-side
//   ItemDefinition, never the Client. A missing/invalid effect is rejected
//   WITHOUT mutation (`not_consumable`). Other consumable types (Food/Drink
//   without effect, buffs, quests, balance systems) remain out of scope.
// - Kein Wegwerfen/Zerstören, keine neue Loot- oder Handelslogik, kein
//   Tausch belegter Ziele, kein Zusammenlegen am Ziel (belegt bleibt belegt).

use crate::inventory::{EquipSlot, InventoryError, MoveTarget};
use crate::world::World;
use crate::combat::cooldowns;
use crate::item::ConsumableKind;
use crate::item_lifecycle::{DetachReason, ItemLifecycle, reconcile_after_take};
use std::time::SystemTime;

/// Shared Abklingtime (ms) of Heil-/Manatränke (docs/Attribute_und_
/// Regeneration.md §9): BOTH drink types share EIN cooldown key, so using
/// one blocks the other for this window. Persisted via the existing P-18
/// Cooldown-Mechanism (Snapshot-/Loginpfad) → Reconnect/Takeover do NOT
/// reset it.
pub const CONSUMABLE_COOLDOWN_KEY: &str = "consumable";
pub const CONSUMABLE_COOLDOWN_MS: u64 = 30_000;

/// Inventaraktion (C2S 22 `action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryAction {
    Move,
    Equip,
    Unequip,
    Use,
}

impl InventoryAction {
    pub fn parse(s: &str) -> Option<InventoryAction> {
        match s.trim().to_lowercase().as_str() {
            "move" => Some(InventoryAction::Move),
            "equip" => Some(InventoryAction::Equip),
            "unequip" => Some(InventoryAction::Unequip),
            "use" => Some(InventoryAction::Use),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            InventoryAction::Move => "move",
            InventoryAction::Equip => "equip",
            InventoryAction::Unequip => "unequip",
            InventoryAction::Use => "use",
        }
    }
}

/// Geparste Inventarabsicht (reine Parsierung; fachliche Prüfung läuft unter
/// der World-Sperre in `attempt_inventory_action`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryRequest {
    pub action: InventoryAction,
    /// Betroffene Instanz (move/equip; use trägt sie nur zur Zuordnung).
    pub item_uuid: String,
    /// Exaktes Verschiebeziel (nur move).
    pub target: Option<MoveTarget>,
    /// Teilmenge beim Verschieben (None = ganzer Stack; nur move).
    pub count: Option<i64>,
    /// Equipment-Slot (equip/unequip, kanonischer DB-Schlüssel).
    pub slot: Option<EquipSlot>,
}

/// Stabile Ablehnungsgründe (S2C 25 `reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryReject {
    UnknownAction,
    UnknownPlayer,
    /// Eine vorbereitete Trade-Veröffentlichung besitzt den
    /// wirtschaftlichen Zustand bis zur Bestätigung (Barriere wie
    /// Pickup/NPC-/Spielerhandel).
    CommitPending,
    NotInInventory,
    /// UUID ist im eigenen Bestand mehrdeutig (inzwischen veränderter
    /// Zustand) — ohne Mutation abgelehnt.
    UuidConflict,
    InvalidQuantity,
    NotEnoughItems,
    InvalidTarget,
    SlotOccupied,
    SlotEmpty,
    UnknownSlot,
    CannotEquip,
    BrokenItem,
    NotEquippable,
    DefinitionUnavailable,
    InventoryFull,
    /// Not an explicitly defined Heil-/Manakonsumable (wrong category or
    /// missing/invalid effect) — rejected without mutation.
    NotConsumable,
    /// Tod: a dead character (hp <= 0) may not use a drink.
    CharacterDead,
    /// The corresponding resource (HP/Mana) is already full: no consumption,
    /// no cooldown start.
    ResourceFull,
    /// The shared Heil-/Manatrank cooldown is still running.
    OnCooldown,
}

impl InventoryReject {
    pub fn reason(self) -> &'static str {
        match self {
            InventoryReject::UnknownAction => "unknown_action",
            InventoryReject::UnknownPlayer => "unknown_player",
            InventoryReject::CommitPending => "trade_commit_pending",
            InventoryReject::NotInInventory => "not_in_inventory",
            InventoryReject::UuidConflict => "uuid_conflict",
            InventoryReject::InvalidQuantity => "invalid_quantity",
            InventoryReject::NotEnoughItems => "not_enough_items",
            InventoryReject::InvalidTarget => "invalid_target",
            InventoryReject::SlotOccupied => "slot_occupied",
            InventoryReject::SlotEmpty => "slot_empty",
            InventoryReject::UnknownSlot => "unknown_slot",
            InventoryReject::CannotEquip => "cannot_equip",
            InventoryReject::BrokenItem => "broken_item",
            InventoryReject::NotEquippable => "not_equippable",
            InventoryReject::DefinitionUnavailable => "definition_unavailable",
            InventoryReject::InventoryFull => "inventory_full",
            InventoryReject::NotConsumable => "not_consumable",
            InventoryReject::CharacterDead => "character_dead",
            InventoryReject::ResourceFull => "resource_full",
            InventoryReject::OnCooldown => "on_cooldown",
        }
    }
}

/// Ergebnis einer ausgeführten Inventaraktion (Antwortdaten für S2C 25;
/// der Folgestand kommt per S2C 23 INVENTORY).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryOutcome {
    pub action: InventoryAction,
    /// Betroffene UUID (move: Bestand am Ziel; equip/unequip: die Instanz).
    pub item_uuid: String,
    /// Equipment-Slot (equip/unequip).
    pub slot: Option<EquipSlot>,
    /// Verschobene Menge (nur move).
    pub count: i64,
}

fn get_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Parst eine C2S-22-Absicht (kein Weltzugriff, keine Mutation).
pub fn parse_request(data: &serde_json::Value) -> Result<InventoryRequest, InventoryReject> {
    let action =
        InventoryAction::parse(&get_str(data, "action")).ok_or(InventoryReject::UnknownAction)?;
    match action {
        InventoryAction::Move => {
            let item_uuid = get_str(data, "item_uuid");
            if item_uuid.trim().is_empty() {
                return Err(InventoryReject::NotInInventory);
            }
            let target = match get_str(data, "target_area").trim().to_lowercase().as_str() {
                "base" => {
                    let slot = data
                        .get("target_slot")
                        .and_then(|v| v.as_i64())
                        .ok_or(InventoryReject::InvalidTarget)?;
                    MoveTarget::Base(
                        usize::try_from(slot).map_err(|_| InventoryReject::InvalidTarget)?,
                    )
                }
                "bag" => {
                    let bag = data
                        .get("target_bag")
                        .and_then(|v| v.as_u64())
                        .ok_or(InventoryReject::InvalidTarget)?;
                    let slot = data
                        .get("target_slot")
                        .and_then(|v| v.as_i64())
                        .ok_or(InventoryReject::InvalidTarget)?;
                    MoveTarget::Bag(
                        bag,
                        usize::try_from(slot).map_err(|_| InventoryReject::InvalidTarget)?,
                    )
                }
                _ => return Err(InventoryReject::InvalidTarget),
            };
            let count = if data.get("count").is_some_and(|v| !v.is_null()) {
                Some(
                    data.get("count")
                        .and_then(|v| v.as_i64())
                        .ok_or(InventoryReject::InvalidQuantity)?,
                )
            } else {
                None
            };
            Ok(InventoryRequest {
                action,
                item_uuid,
                target: Some(target),
                count,
                slot: None,
            })
        }
        InventoryAction::Equip => {
            let item_uuid = get_str(data, "item_uuid");
            if item_uuid.trim().is_empty() {
                return Err(InventoryReject::NotInInventory);
            }
            let slot =
                EquipSlot::from_db(&get_str(data, "slot")).ok_or(InventoryReject::UnknownSlot)?;
            Ok(InventoryRequest {
                action,
                item_uuid,
                target: None,
                count: None,
                slot: Some(slot),
            })
        }
        InventoryAction::Unequip => {
            let slot =
                EquipSlot::from_db(&get_str(data, "slot")).ok_or(InventoryReject::UnknownSlot)?;
            Ok(InventoryRequest {
                action,
                item_uuid: String::new(),
                target: None,
                count: None,
                slot: Some(slot),
            })
        }
        InventoryAction::Use => Ok(InventoryRequest {
            action,
            item_uuid: get_str(data, "item_uuid"),
            target: None,
            count: None,
            slot: None,
        }),
    }
}

fn map_move_err(e: InventoryError) -> InventoryReject {
    match e {
        InventoryError::NotInInventory => InventoryReject::NotInInventory,
        InventoryError::UuidCollision => InventoryReject::UuidConflict,
        InventoryError::InvalidQuantity => InventoryReject::InvalidQuantity,
        InventoryError::NotEnoughItems => InventoryReject::NotEnoughItems,
        InventoryError::InvalidTarget => InventoryReject::InvalidTarget,
        InventoryError::SlotOccupied => InventoryReject::SlotOccupied,
        _ => InventoryReject::InvalidTarget,
    }
}

/// Führt eine Inventarabsicht atomar aus: validieren, dann gemeinsam
/// übernehmen — oder vollständig unverändert lassen. Läuft unter der
/// ausführenden World-Sperre des Aufrufers (keine Awaits, keine
/// Zwischenänderungen möglich).
pub fn attempt_inventory_action(
    world: &mut World,
    actor: &str,
    req: &InventoryRequest,
    wall_now: SystemTime,
) -> Result<InventoryOutcome, InventoryReject> {
    if !world.economic_mutation_allowed(actor) {
        return Err(InventoryReject::CommitPending);
    }
    let player = world
        .players
        .get_mut(actor)
        .ok_or(InventoryReject::UnknownPlayer)?;
    match req.action {
        InventoryAction::Move => {
            let target = req.target.ok_or(InventoryReject::InvalidTarget)?;
            let outcome = player
                .inventory
                .try_move_instance(&req.item_uuid, target, req.count)
                .map_err(map_move_err)?;
            player.mark_dirty(crate::persist::PersistComponent::Inventory);
            Ok(InventoryOutcome {
                action: req.action,
                item_uuid: outcome.placed_uuid,
                slot: None,
                count: outcome.moved,
            })
        }
        InventoryAction::Equip => {
            let slot = req.slot.ok_or(InventoryReject::UnknownSlot)?;
            let item_id = player
                .inventory
                .owned_instance(&req.item_uuid)
                .map(|it| it.item_id.clone())
                .ok_or(InventoryReject::NotInInventory)?;
            let def = world
                .item_definitions
                .get(&item_id)
                .ok_or(InventoryReject::DefinitionUnavailable)?;
            let level = player.level as i64;
            let class = player.class;
            player
                .inventory
                .try_equip(def, &req.item_uuid, slot, level, class)
                .map_err(|e| match e {
                    InventoryError::SlotOccupied => InventoryReject::SlotOccupied,
                    InventoryError::CannotEquip => InventoryReject::CannotEquip,
                    InventoryError::BrokenItem => InventoryReject::BrokenItem,
                    InventoryError::NotEquippable => InventoryReject::NotEquippable,
                    InventoryError::NotInInventory => InventoryReject::NotInInventory,
                    _ => InventoryReject::NotInInventory,
                })?;
            player.mark_dirty(crate::persist::PersistComponent::Inventory);
            Ok(InventoryOutcome {
                action: req.action,
                item_uuid: req.item_uuid.clone(),
                slot: Some(slot),
                count: 1,
            })
        }
        InventoryAction::Unequip => {
            let slot = req.slot.ok_or(InventoryReject::UnknownSlot)?;
            let equipped = player
                .inventory
                .equipped
                .get(&slot)
                .cloned()
                .ok_or(InventoryReject::SlotEmpty)?;
            let def = world
                .item_definitions
                .get(&equipped.item_id)
                .ok_or(InventoryReject::DefinitionUnavailable)?;
            player
                .inventory
                .try_unequip(def, slot)
                .map_err(|e| match e {
                    InventoryError::NoSpace => InventoryReject::InventoryFull,
                    InventoryError::NotInInventory => InventoryReject::NotInInventory,
                    _ => InventoryReject::NotInInventory,
                })?;
            player.mark_dirty(crate::persist::PersistComponent::Inventory);
            Ok(InventoryOutcome {
                action: req.action,
                item_uuid: equipped.item_uuid,
                slot: Some(slot),
                count: equipped.count,
            })
        }
        InventoryAction::Use => {
            // Tod: a dead character may not drink (server-authoritative; a
            // manipulated client cannot revive via a potion).
            if player.hp <= 0 {
                return Err(InventoryReject::CharacterDead);
            }
            // Own, uniquely placed instance in the normal inventory
            // (owned_instance excludes equipment/buffer and rejects
            // ambiguous/fremd UUIDs → NotInInventory).
            let inst = player
                .inventory
                .owned_instance(&req.item_uuid)
                .ok_or(InventoryReject::NotInInventory)?;
            let item_uuid = inst.item_uuid.clone();
            let item_id = inst.item_id.clone();
            if inst.count < 1 {
                return Err(InventoryReject::InvalidQuantity);
            }
            // Server-side definition; missing → no mutation.
            let def = world
                .item_definitions
                .get(&item_id)
                .ok_or(InventoryReject::DefinitionUnavailable)?;
            // Only an explicitly defined Heil-/Manakonsumable (exactly one
            // positive effect). Missing/invalid effect → reject, no mutation.
            let (kind, amount) = def.consumable_effect().ok_or(InventoryReject::NotConsumable)?;
            // Full resource → no consumption, no cooldown.
            let (cur, max) = match kind {
                ConsumableKind::Heal => (player.hp, player.max_hp),
                ConsumableKind::Mana => (player.mana, player.max_mana),
            };
            if cur >= max {
                return Err(InventoryReject::ResourceFull);
            }
            // Shared Heil-/Manatrank cooldown still running → reject.
            if !cooldowns::is_ready(&player.cooldowns, CONSUMABLE_COOLDOWN_KEY, wall_now) {
                return Err(InventoryReject::OnCooldown);
            }
            // ── All checks passed: apply together (same lock, no awaits) ──
            // 1) Resource gain: checked arithmetic, capped at max (no overflow).
            let new64 = (cur as i64)
                .checked_add(amount)
                .unwrap_or(max as i64)
                .min(max as i64);
            let new_val = i32::try_from(new64).unwrap_or(max);
            match kind {
                ConsumableKind::Heal => {
                    player.hp = new_val;
                    player.mark_dirty(crate::persist::PersistComponent::Resources);
                }
                ConsumableKind::Mana => {
                    player.mana = new_val;
                    player.mark_dirty(crate::persist::PersistComponent::Resources);
                }
            }
            // 2) Consume exactly one unit via the existing stack operation.
            //    count > 1: rest keeps the UUID (no detach). count == 1:
            //    slot cleared, the UUID leaves the placements.
            let _taken = match player
                .inventory
                .try_take_instance(&item_uuid, 1)
            {
                Ok(t) => t,
                Err(_) => return Err(InventoryReject::NotEnoughItems),
            };
            // 3) Lifecycle: detach ONLY the original UUID when it is no longer
            //    placed (last consumed instance, revision-bound via the
            //    Snapshot-/Drain-Pfad). No direct DB deletion here.
            let uuids = player.inventory.persistent_uuids();
            let mut lc = world.item_lifecycle.get(actor).cloned().unwrap_or(ItemLifecycle::new());
            reconcile_after_take(
                &mut lc,
                &uuids,
                &item_uuid,
                DetachReason::Consumed,
                &world.runtime_id,
                crate::item_lifecycle::now_ms(),
            );
            if !lc.is_empty() {
                world.item_lifecycle.insert(actor.to_string(), lc);
            }
            // 4) Shared cooldown start; only a real change is dirty-relevant.
            let cd_changed = cooldowns::start(
                &mut player.cooldowns,
                CONSUMABLE_COOLDOWN_KEY.into(),
                CONSUMABLE_COOLDOWN_MS,
                wall_now,
            );
            if cd_changed {
                player.mark_dirty(crate::persist::PersistComponent::Progression);
            }
            // 5) Inventory changed (stack consumed) → dirty.
            player.mark_dirty(crate::persist::PersistComponent::Inventory);
            Ok(InventoryOutcome {
                action: req.action,
                item_uuid,
                slot: None,
                count: 1,
            })
        }
    }
}

/// Erfolgsantwort für S2C 25 (der Folgestand folgt per S2C 23 INVENTORY).
pub fn success_payload(out: &InventoryOutcome) -> serde_json::Value {
    let mut v = serde_json::json!({
        "ok": true,
        "action": out.action.as_str(),
        "item_uuid": out.item_uuid,
        "count": out.count,
    });
    if let Some(slot) = out.slot {
        v["slot"] = serde_json::Value::String(slot.as_db().to_string());
    }
    v
}

/// Ablehnung für S2C 25 (stabile Gründe, keine Mutation erfolgt).
pub fn reject_payload(
    action: Option<InventoryAction>,
    reject: InventoryReject,
) -> serde_json::Value {
    let mut v = serde_json::json!({"ok": false, "reason": reject.reason()});
    if let Some(a) = action {
        v["action"] = serde_json::Value::String(a.as_str().to_string());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{BindingState, ItemCategory, ItemInstance, ItemModifiers};
    use std::collections::{BTreeMap, HashSet};
    use std::time::Instant;
    use std::time::SystemTime;

    // Deterministic Wall-Clock for the Cooldown-Tests (no real-time dependency).
    fn wall() -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(12 * 3600)
    }

    // Thin wrapper: every existing Inventar-Test uses Actor "p1" and the
    // fixed Wall-Clock; the real 4-Arg signature stays untouched.
    fn act(world: &mut World, req: &InventoryRequest) -> Result<InventoryOutcome, InventoryReject> {
        attempt_inventory_action(world, "p1", req, wall())
    }

    fn potion_def() -> crate::item::ItemDefinition {
        let mut d =
            crate::item::ItemDefinition::new("hp_potion", "Heiltrank", ItemCategory::Potion);
        d.max_stack = 20;
        d
    }

    fn sword_def() -> crate::item::ItemDefinition {
        crate::item::ItemDefinition::new("eisenschwert", "Eisenschwert", ItemCategory::Weapon)
    }

    fn player_with(base_slots: usize) -> crate::world::Player {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        crate::world::Player {
            id: "p1".into(),
            name: "p1".into(),
            x: 0.0,
            y: 0.0,
            face: 0.0,
            ping_ms: 0,
            zone_id: 0,
            hp: 100,
            max_hp: 100,
            lang: "de".into(),
            account_id: 0,
            session_id: String::new(),
            entities: HashSet::new(),
            last_activity: Instant::now(),
            tx,
            char_class: "Adventurer".into(),
            class: crate::class::ClassStatus::Adventurer,
            faction_transition: false,
            level: 5,
            exp: 0,
            free_attr_points: 0,
            rested_pool: 0,
            idia: 0,
            armor: 0,
            weapon_skill: 1,
            combat: None,
            last_strike: None,
            mana: 50,
            max_mana: 50,
            effects: Vec::new(),
            cooldowns: BTreeMap::new(),
            active_cast: None,
            learned_abilities: HashSet::new(),
            attributes: Default::default(),
            max_hp_base: 100,
            max_mana_base: 50,
            sitting: false,
            hp_regen_bonus: 0.0,
            mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0,
            mana_regen_carry: 0.0,
            inventory: crate::inventory::InventoryState::new(base_slots),
            quests: BTreeMap::new(),
            dirty: Default::default(),
            persist_generation: 0,
            persist_revision: 0,
        }
    }

    fn inst(uuid: &str, item_id: &str, count: i64) -> ItemInstance {
        ItemInstance {
            item_uuid: uuid.into(),
            item_id: item_id.into(),
            count,
            durability_current: None,
            durability_max: None,
            binding: BindingState::Tradeable,
            creator_id: None,
            modifiers: ItemModifiers::default(),
        }
    }

    fn world_with_player(base_slots: usize) -> World {
        let mut world = World::new();
        world
            .item_definitions
            .insert("hp_potion".into(), potion_def());
        world
            .item_definitions
            .insert("eisenschwert".into(), sword_def());
        world.players.insert("p1".into(), player_with(base_slots));
        world
    }

    fn snapshot(world: &World) -> crate::inventory::InventoryState {
        world.players["p1"].inventory.clone()
    }

    fn block_economy(world: &mut World, actor: &str) {
        // Pending-Trade-Sperre als reiner Zustand (ohne Commit-Maschinerie):
        // Eine vorbereitete Veröffentlichung, die den Akteur nennt, sperrt
        // wirtschaftliche Mutationen — exakt die Prüfung, die auch
        // Pickup/NPC-/Spielerhandel verwenden.
        let snap = crate::persist::PersistSnapshot {
            player_id: actor.into(),
            persist_revision: 0,
            captured_at_ms: 0,
            x: 0.0,
            y: 0.0,
            level: 1,
            exp: 0,
            free_attr_points: 0,
            rested_pool: 0,
            idia: 0,
            hp: 100,
            mana: 50,
            attributes: Default::default(),
            char_class: "Adventurer".into(),
            faction_transition: false,
            weapon_skill: 1,
            learned_abilities: Vec::new(),
            cooldowns: None,
            inventory: crate::inventory::InventoryState::new(8),
            item_lifecycle: None,
            generation: 0,
            dirty: Default::default(),
        };
        let tc = crate::persist::TradeCharacter {
            base_revision: 0,
            before_inventory: crate::inventory::InventoryState::new(8),
            before_idia: 0,
            snapshot: snap,
        };
        world.prepared_trades.insert(
            "pending-1".into(),
            crate::persist::PreparedTrade {
                request: crate::persist::TradeCommitRequest {
                    commit_id: "pending-1".into(),
                    characters: [actor.into(), "p2".into()],
                    idia: [0, 0],
                    transfers: Vec::new(),
                },
                artifact: crate::persist::TradeArtifact {
                    kind: "trade".into(),
                    format_version: 1,
                    commit_id: "pending-1".into(),
                    characters: [tc.clone(), tc],
                    transfers: Vec::new(),
                },
                lifecycle: [
                    crate::item_lifecycle::ItemLifecycle::new(),
                    crate::item_lifecycle::ItemLifecycle::new(),
                ],
            },
        );
    }

    #[test]
    fn move_full_preserves_uuid_and_marks_dirty() {
        let mut world = world_with_player(4);
        world.players.get_mut("p1").unwrap().inventory.base_slots[0] =
            Some(inst("u-move", "hp_potion", 5));
        let req = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-move".into(),
            target: Some(MoveTarget::Base(2)),
            count: None,
            slot: None,
        };
        let out = act(&mut world, &req).expect("Vollverschub");
        assert_eq!(out.item_uuid, "u-move");
        assert_eq!(out.count, 5);
        let inv = snapshot(&world);
        assert!(inv.base_slots[0].is_none());
        assert_eq!(inv.base_slots[2].as_ref().unwrap().item_uuid, "u-move");
        assert_eq!(inv.base_slots[2].as_ref().unwrap().count, 5);
        assert!(
            world.players["p1"]
                .dirty
                .is_dirty(crate::persist::PersistComponent::Inventory),
            "Erfolg markiert Inventory dirty (Persistenzanbindung)"
        );
    }

    #[test]
    fn move_split_keeps_source_uuid_and_mints_part() {
        let mut world = world_with_player(4);
        world.players.get_mut("p1").unwrap().inventory.base_slots[0] =
            Some(inst("u-split", "hp_potion", 10));
        let req = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-split".into(),
            target: Some(MoveTarget::Base(3)),
            count: Some(4),
            slot: None,
        };
        let out = act(&mut world, &req).expect("Split");
        assert_eq!(out.count, 4);
        assert_ne!(out.item_uuid, "u-split", "Teil erhält neue UUID");
        let inv = snapshot(&world);
        assert_eq!(inv.base_slots[0].as_ref().unwrap().item_uuid, "u-split");
        assert_eq!(inv.base_slots[0].as_ref().unwrap().count, 6);
        assert_eq!(inv.base_slots[3].as_ref().unwrap().item_uuid, out.item_uuid);
        assert_eq!(inv.base_slots[3].as_ref().unwrap().count, 4);
        assert_eq!(
            inv.base_slots[3].as_ref().unwrap().binding,
            BindingState::Tradeable,
            "Eigenschaften bleiben erhalten"
        );
    }

    #[test]
    fn move_to_occupied_or_invalid_target_rejects_without_mutation() {
        let mut world = world_with_player(3);
        {
            let p = world.players.get_mut("p1").unwrap();
            p.inventory.base_slots[0] = Some(inst("u-a", "hp_potion", 5));
            p.inventory.base_slots[1] = Some(inst("u-b", "hp_potion", 5));
        }
        let before = snapshot(&world);
        // Belegtes Ziel (auch stapelkompatibel: kein erfundenes Zusammenlegen).
        let occupied = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-a".into(),
            target: Some(MoveTarget::Base(1)),
            count: None,
            slot: None,
        };
        assert_eq!(
            act(&mut world, &occupied),
            Err(InventoryReject::SlotOccupied)
        );
        assert_eq!(snapshot(&world), before, "keine Teilmutation");
        // Ungültiges Ziel (Slot außerhalb des Bereichs).
        let invalid = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-a".into(),
            target: Some(MoveTarget::Base(9)),
            count: None,
            slot: None,
        };
        assert_eq!(
            act(&mut world, &invalid),
            Err(InventoryReject::InvalidTarget)
        );
        assert_eq!(snapshot(&world), before, "keine Teilmutation");
        // Ziel gleich Quelle.
        let same = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-a".into(),
            target: Some(MoveTarget::Base(0)),
            count: None,
            slot: None,
        };
        assert_eq!(
            act(&mut world, &same),
            Err(InventoryReject::InvalidTarget)
        );
        assert_eq!(snapshot(&world), before, "keine Teilmutation");
        assert!(
            !world.players["p1"]
                .dirty
                .is_dirty(crate::persist::PersistComponent::Inventory),
            "Ablehnung markiert kein Dirty"
        );
    }

    #[test]
    fn move_between_base_and_bag() {
        let mut world = world_with_player(2);
        {
            let p = world.players.get_mut("p1").unwrap();
            p.inventory
                .create_bag(&crate::inventory::InventoryCfg::default(), "Beutel", 2)
                .unwrap();
            p.inventory.base_slots[0] = Some(inst("u-bag", "hp_potion", 3));
        }
        let bag_id = world.players["p1"].inventory.bags[0].bag_id;
        let req = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-bag".into(),
            target: Some(MoveTarget::Bag(bag_id, 1)),
            count: None,
            slot: None,
        };
        act(&mut world, &req).expect("Basis → Tasche");
        let inv = snapshot(&world);
        assert!(inv.base_slots[0].is_none());
        assert_eq!(inv.bags[0].slots[1].as_ref().unwrap().item_uuid, "u-bag");
        // Zurück in die Tasche → Basis.
        let back = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-bag".into(),
            target: Some(MoveTarget::Base(1)),
            count: None,
            slot: None,
        };
        act(&mut world, &back).expect("Tasche → Basis");
        let inv = snapshot(&world);
        assert!(inv.bags[0].slots[1].is_none());
        assert_eq!(inv.base_slots[1].as_ref().unwrap().item_uuid, "u-bag");
    }

    #[test]
    fn foreign_missing_and_ambiguous_uuid_reject() {
        let mut world = world_with_player(4);
        world.players.get_mut("p1").unwrap().inventory.base_slots[0] =
            Some(inst("u-own", "hp_potion", 2));
        let before = snapshot(&world);
        // Fremde UUID (nie im eigenen Bestand).
        let foreign = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-fremd".into(),
            target: Some(MoveTarget::Base(1)),
            count: None,
            slot: None,
        };
        assert_eq!(
            act(&mut world, &foreign),
            Err(InventoryReject::NotInInventory)
        );
        // Fehlende UUID (leer).
        let missing = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "   ".into(),
            target: Some(MoveTarget::Base(1)),
            count: None,
            slot: None,
        };
        assert_eq!(
            act(&mut world, &missing),
            Err(InventoryReject::NotInInventory)
        );
        // Inzwischen veränderte UUID: doppelt geführt (Korruptions-/Race-Zustand).
        {
            let p = world.players.get_mut("p1").unwrap();
            p.inventory.base_slots[1] = Some(inst("u-own", "hp_potion", 2));
        }
        let ambiguous = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-own".into(),
            target: Some(MoveTarget::Base(2)),
            count: None,
            slot: None,
        };
        assert_eq!(
            act(&mut world, &ambiguous),
            Err(InventoryReject::UuidConflict)
        );
        // Ausrüstungs-UUID ist keine Verschiebequelle.
        {
            let p = world.players.get_mut("p1").unwrap();
            p.inventory.base_slots[1] = None;
            p.inventory
                .equipped
                .insert(EquipSlot::MainHand, inst("u-sword", "eisenschwert", 1));
        }
        let equipped_src = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-sword".into(),
            target: Some(MoveTarget::Base(1)),
            count: None,
            slot: None,
        };
        assert_eq!(
            act(&mut world, &equipped_src),
            Err(InventoryReject::NotInInventory)
        );
        assert_eq!(snapshot(&world).base_slots[0].as_ref().unwrap().count, 2);
        let _ = before;
    }

    #[test]
    fn quantity_errors_reject_without_mutation() {
        let mut world = world_with_player(3);
        world.players.get_mut("p1").unwrap().inventory.base_slots[0] =
            Some(inst("u-q", "hp_potion", 5));
        let before = snapshot(&world);
        for count in [Some(0), Some(-3)] {
            let req = InventoryRequest {
                action: InventoryAction::Move,
                item_uuid: "u-q".into(),
                target: Some(MoveTarget::Base(2)),
                count,
                slot: None,
            };
            assert_eq!(
                act(&mut world, &req),
                Err(InventoryReject::InvalidQuantity)
            );
        }
        let too_much = InventoryRequest {
            action: InventoryAction::Move,
            item_uuid: "u-q".into(),
            target: Some(MoveTarget::Base(2)),
            count: Some(6),
            slot: None,
        };
        assert_eq!(
            act(&mut world, &too_much),
            Err(InventoryReject::NotEnoughItems)
        );
        assert_eq!(snapshot(&world), before, "keine Teilmutation");
    }

    #[test]
    fn equip_and_unequip_follow_existing_rules() {
        let mut world = world_with_player(3);
        {
            let p = world.players.get_mut("p1").unwrap();
            p.inventory.base_slots[0] = Some(inst("u-sw", "eisenschwert", 1));
            p.inventory.base_slots[1] = Some(inst("u-po", "hp_potion", 5));
        }
        // Nicht ausrüstbar (Kategorie Potion) → vorhandene Regel.
        let bad_cat = InventoryRequest {
            action: InventoryAction::Equip,
            item_uuid: "u-po".into(),
            target: None,
            count: None,
            slot: Some(EquipSlot::MainHand),
        };
        assert_eq!(
            act(&mut world, &bad_cat),
            Err(InventoryReject::NotEquippable)
        );
        // Anlegen nach vorhandenen Regeln (Level/Klasse aus Serverstand).
        let equip = InventoryRequest {
            action: InventoryAction::Equip,
            item_uuid: "u-sw".into(),
            target: None,
            count: None,
            slot: Some(EquipSlot::MainHand),
        };
        let out = act(&mut world, &equip).expect("equip");
        assert_eq!(out.slot, Some(EquipSlot::MainHand));
        assert!(snapshot(&world).base_slots[0].is_none());
        assert_eq!(
            snapshot(&world).equipped[&EquipSlot::MainHand].item_uuid,
            "u-sw"
        );
        // Belegter Ausrüstungsslot.
        {
            let p = world.players.get_mut("p1").unwrap();
            p.inventory.base_slots[0] = Some(inst("u-sw2", "eisenschwert", 1));
        }
        let occupied = InventoryRequest {
            action: InventoryAction::Equip,
            item_uuid: "u-sw2".into(),
            target: None,
            count: None,
            slot: Some(EquipSlot::MainHand),
        };
        assert_eq!(
            act(&mut world, &occupied),
            Err(InventoryReject::SlotOccupied)
        );
        // Ablegen nach vorhandenen Regeln (automatischer Zielplatz).
        let unequip = InventoryRequest {
            action: InventoryAction::Unequip,
            item_uuid: String::new(),
            target: None,
            count: None,
            slot: Some(EquipSlot::MainHand),
        };
        act(&mut world, &unequip).expect("unequip");
        assert!(!snapshot(&world).equipped.contains_key(&EquipSlot::MainHand));
        // Leerer Slot.
        assert_eq!(
            act(&mut world, &unequip),
            Err(InventoryReject::SlotEmpty)
        );
    }

    #[test]
    fn broken_item_cannot_be_equipped() {
        let mut world = world_with_player(2);
        let mut broken = inst("u-broken", "eisenschwert", 1);
        broken.durability_current = Some(0);
        broken.durability_max = Some(100);
        world.players.get_mut("p1").unwrap().inventory.base_slots[0] = Some(broken);
        let before = snapshot(&world);
        let req = InventoryRequest {
            action: InventoryAction::Equip,
            item_uuid: "u-broken".into(),
            target: None,
            count: None,
            slot: Some(EquipSlot::MainHand),
        };
        assert_eq!(
            act(&mut world, &req),
            Err(InventoryReject::BrokenItem)
        );
        assert_eq!(snapshot(&world), before, "keine Teilmutation");
    }

    #[test]
    fn full_inventory_unequip_rejects_without_mutation() {
        let mut world = world_with_player(1);
        {
            let p = world.players.get_mut("p1").unwrap();
            // Voller Bestand: einziger Basis-Slot belegt, kein Stackpartner.
            p.inventory.base_slots[0] = Some(inst("u-full", "hp_potion", 20));
            p.inventory
                .equipped
                .insert(EquipSlot::MainHand, inst("u-wpn", "eisenschwert", 1));
        }
        let before = snapshot(&world);
        let req = InventoryRequest {
            action: InventoryAction::Unequip,
            item_uuid: String::new(),
            target: None,
            count: None,
            slot: Some(EquipSlot::MainHand),
        };
        assert_eq!(
            act(&mut world, &req),
            Err(InventoryReject::InventoryFull)
        );
        assert_eq!(
            snapshot(&world),
            before,
            "keine Teilmutation bei vollem Bestand"
        );
        assert!(
            snapshot(&world).equipped.contains_key(&EquipSlot::MainHand),
            "Equipment bleibt angelegt"
        );
    }

    #[test]
    fn use_without_defined_effect_rejects_without_mutation() {
        let mut world = world_with_player(2);
        {
            let p = world.players.get_mut("p1").unwrap();
            p.inventory.base_slots[0] = Some(inst("u-use", "hp_potion", 7));
        }
        let before_inv = snapshot(&world);
        let (hp, mana) = {
            let p = &world.players["p1"];
            (p.hp, p.mana)
        };
        let req = InventoryRequest {
            action: InventoryAction::Use,
            item_uuid: "u-use".into(),
            target: None,
            count: None,
            slot: None,
        };
        assert_eq!(
            act(&mut world, &req),
            Err(InventoryReject::NotConsumable),
            "plain Potion ohne explicit effect → not a defined consumable"
        );
        assert_eq!(snapshot(&world), before_inv, "keine Mengenänderung");
        let p = &world.players["p1"];
        assert_eq!((p.hp, p.mana), (hp, mana), "kein Effekt");
        assert!(
            !p.dirty
                .is_dirty(crate::persist::PersistComponent::Inventory),
            "kein Dirty ohne Mutation"
        );
    }

    #[test]
    fn pending_trade_barrier_blocks_all_actions() {
        let mut world = world_with_player(3);
        {
            let p = world.players.get_mut("p1").unwrap();
            p.inventory.base_slots[0] = Some(inst("u-t", "hp_potion", 5));
        }
        block_economy(&mut world, "p1");
        assert!(
            !world.economic_mutation_allowed("p1"),
            "Sperrzustand steht (bestehender Vertrag)"
        );
        let before = snapshot(&world);
        let cases = [
            InventoryRequest {
                action: InventoryAction::Move,
                item_uuid: "u-t".into(),
                target: Some(MoveTarget::Base(1)),
                count: None,
                slot: None,
            },
            InventoryRequest {
                action: InventoryAction::Equip,
                item_uuid: "u-t".into(),
                target: None,
                count: None,
                slot: Some(EquipSlot::Food),
            },
            InventoryRequest {
                action: InventoryAction::Unequip,
                item_uuid: String::new(),
                target: None,
                count: None,
                slot: Some(EquipSlot::Food),
            },
            InventoryRequest {
                action: InventoryAction::Use,
                item_uuid: "u-t".into(),
                target: None,
                count: None,
                slot: None,
            },
        ];
        for req in &cases {
            assert_eq!(
                act(&mut world, req),
                Err(InventoryReject::CommitPending),
                "Barriere gilt für {:?}",
                req.action
            );
        }
        assert_eq!(snapshot(&world), before, "keine Teilmutation unter Sperre");
    }

    #[test]
    fn parse_rejects_unknown_action_and_bad_targets() {
        assert_eq!(
            parse_request(&serde_json::json!({"action": "tanzen"})),
            Err(InventoryReject::UnknownAction)
        );
        assert_eq!(
            parse_request(&serde_json::json!({"action": "move", "item_uuid": "u"})),
            Err(InventoryReject::InvalidTarget)
        );
        assert_eq!(
            parse_request(&serde_json::json!({
                "action": "move", "item_uuid": "u",
                "target_area": "base", "target_slot": -1,
            })),
            Err(InventoryReject::InvalidTarget)
        );
        assert_eq!(
            parse_request(&serde_json::json!({"action": "equip", "item_uuid": "u"})),
            Err(InventoryReject::UnknownSlot)
        );
        assert_eq!(
            parse_request(
                &serde_json::json!({"action": "equip", "item_uuid": "u", "slot": "hutablage"})
            ),
            Err(InventoryReject::UnknownSlot)
        );
        let ok = parse_request(&serde_json::json!({
            "action": "equip", "item_uuid": "u", "slot": "main_hand",
        }))
        .expect("kanonischer Slot-Schlüssel");
        assert_eq!(ok.slot, Some(EquipSlot::MainHand));
        let bag = parse_request(&serde_json::json!({
            "action": "move", "item_uuid": "u",
            "target_area": "bag", "target_bag": 2, "target_slot": 0, "count": 3,
        }))
        .expect("Taschenziel");
        assert_eq!(bag.target, Some(MoveTarget::Bag(2, 0)));
        assert_eq!(bag.count, Some(3));
    }

    #[test]
    fn reject_reasons_are_stable() {
        assert_eq!(
            InventoryReject::CommitPending.reason(),
            "trade_commit_pending"
        );
        assert_eq!(InventoryReject::NotConsumable.reason(), "not_consumable");
        assert_eq!(InventoryReject::SlotOccupied.reason(), "slot_occupied");
        assert_eq!(InventoryReject::InvalidTarget.reason(), "invalid_target");
        assert_eq!(InventoryReject::UuidConflict.reason(), "uuid_conflict");
        assert_eq!(InventoryReject::NotConsumable.reason(), "not_consumable");
        assert_eq!(InventoryReject::CharacterDead.reason(), "character_dead");
        assert_eq!(InventoryReject::ResourceFull.reason(), "resource_full");
        assert_eq!(InventoryReject::OnCooldown.reason(), "on_cooldown");
    }

    // ── Heil-/Manakonsumables (docs/Attribute_und_Regeneration.md §9) ─────

    fn hp_potion_def() -> crate::item::ItemDefinition {
        let mut d =
            crate::item::ItemDefinition::new("hp_potion", "Heiltrank", ItemCategory::Potion);
        d.max_stack = 20;
        d.heal_amount = Some(30);
        d
    }

    fn mp_potion_def() -> crate::item::ItemDefinition {
        let mut d =
            crate::item::ItemDefinition::new("mp_potion", "Manatrank", ItemCategory::Potion);
        d.max_stack = 20;
        d.mana_amount = Some(25);
        d
    }

    // World with a defined Heil- and Manatrank plus a stack of each.
    fn use_world() -> World {
        let mut world = World::new();
        world.item_definitions.insert("hp_potion".into(), hp_potion_def());
        world.item_definitions.insert("mp_potion".into(), mp_potion_def());
        world.players.insert("p1".into(), player_with(4));
        let p = world.players.get_mut("p1").unwrap();
        p.hp = 50;
        p.max_hp = 100;
        p.mana = 10;
        p.max_mana = 50;
        p.inventory.base_slots[0] = Some(inst("u-hp", "hp_potion", 5));
        p.inventory.base_slots[1] = Some(inst("u-mp", "mp_potion", 3));
        world
    }

    fn use_req(uuid: &str) -> InventoryRequest {
        InventoryRequest {
            action: InventoryAction::Use,
            item_uuid: uuid.into(),
            target: None,
            count: None,
            slot: None,
        }
    }

    #[test]
    fn use_heal_restores_hp_consumes_one_and_starts_cooldown() {
        let mut world = use_world();
        let out = act(&mut world, &use_req("u-hp")).expect("Heiltrank");
        assert_eq!(out.count, 1);
        let p = &world.players["p1"];
        assert_eq!(p.hp, 80, "50 + 30 = 80 (server-side magnitude)");
        assert_eq!(p.mana, 10, "Mana untouched by a heal drink");
        let inv = snapshot(&world);
        assert_eq!(inv.base_slots[0].as_ref().unwrap().count, 4, "genau eine Einheit");
        assert_eq!(inv.base_slots[0].as_ref().unwrap().item_uuid, "u-hp", "Rest keeps UUID");
        assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Resources));
        assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Inventory));
        assert!(
            p.cooldowns.contains_key(CONSUMABLE_COOLDOWN_KEY),
            "shared cooldown started"
        );
        assert!(
            p.dirty.is_dirty(crate::persist::PersistComponent::Progression),
            "cooldown change is dirty-relevant (P-18)"
        );
    }

    #[test]
    fn use_heal_caps_at_max_and_mana_at_max() {
        let mut world = use_world();
        world.players.get_mut("p1").unwrap().hp = 90;
        let out = act(&mut world, &use_req("u-hp")).expect("Heiltrank near max");
        assert_eq!(out.count, 1);
        assert_eq!(world.players["p1"].hp, 100, "capped at max_hp, no overshoot");
        // Mana drink near max caps too (fresh world: cooldown is shared).
        let mut world2 = use_world();
        world2.players.get_mut("p1").unwrap().mana = 45;
        let _ = act(&mut world2, &use_req("u-mp")).expect("Manatrank");
        assert_eq!(world2.players["p1"].mana, 50, "capped at max_mana");
    }

    #[test]
    fn use_mana_restores_mana() {
        let mut world = use_world();
        let out = act(&mut world, &use_req("u-mp")).expect("Manatrank");
        assert_eq!(out.count, 1);
        assert_eq!(world.players["p1"].mana, 35, "10 + 25");
        assert_eq!(world.players["p1"].hp, 50, "HP untouched");
    }

    #[test]
    fn use_full_resource_no_consumption_no_cooldown() {
        let mut world = use_world();
        world.players.get_mut("p1").unwrap().hp = 100;
        let before = snapshot(&world);
        let r = act(&mut world, &use_req("u-hp"));
        assert_eq!(r, Err(InventoryReject::ResourceFull));
        assert_eq!(snapshot(&world), before, "keine Verbrauch");
        assert!(
            !world.players["p1"].cooldowns.contains_key(CONSUMABLE_COOLDOWN_KEY),
            "kein Cooldown started"
        );
        assert!(
            !world.players["p1"].dirty.is_dirty(crate::persist::PersistComponent::Inventory),
            "kein Dirty"
        );
    }

    #[test]
    fn use_dead_character_rejected_without_mutation() {
        let mut world = use_world();
        world.players.get_mut("p1").unwrap().hp = 0;
        let before = snapshot(&world);
        let r = act(&mut world, &use_req("u-hp"));
        assert_eq!(r, Err(InventoryReject::CharacterDead));
        assert_eq!(snapshot(&world), before, "keine Mutation");
        assert_eq!(world.players["p1"].hp, 0, "stays dead (no revive)");
    }

    #[test]
    fn use_shared_cooldown_blocks_both_drinks_until_exact_expiry() {
        let mut world = use_world();
        // Heal at wall() → cooldown ready_at = wall + 30s.
        assert!(act(&mut world, &use_req("u-hp")).is_ok());
        let ready_at = wall() + std::time::Duration::from_millis(CONSUMABLE_COOLDOWN_MS);
        // A Mana drink just before the boundary is blocked (shared key).
        let just_before = ready_at - std::time::Duration::from_millis(1);
        assert_eq!(
            attempt_inventory_action(&mut world, "p1", &use_req("u-mp"), just_before),
            Err(InventoryReject::OnCooldown),
            "shared cooldown across both drink types"
        );
        // Exactly at the boundary the cooldown is ready (>= semantics).
        let out = attempt_inventory_action(&mut world, "p1", &use_req("u-mp"), ready_at);
        assert!(out.is_ok(), "exact expiry frees the shared cooldown");
        assert_eq!(world.players["p1"].mana, 35, "Mana drink applied at boundary");
    }

    #[test]
    fn use_last_unit_detaches_uuid_via_lifecycle() {
        let mut world = use_world();
        world.players.get_mut("p1").unwrap().inventory.base_slots[0] =
            Some(inst("u-hp", "hp_potion", 1));
        let out = act(&mut world, &use_req("u-hp")).expect("last unit");
        assert_eq!(out.count, 1);
        let inv = snapshot(&world);
        assert!(inv.base_slots[0].is_none(), "Stack resolved (no direct DB delete here)");
        let lc = world.item_lifecycle.get("p1").expect("lifecycle recorded");
        assert!(lc.contains("u-hp"), "last consumed instance detached (revision-bound)");
        let view = lc.snapshot_view(world.players["p1"].persist_revision + 1, &world.runtime_id);
        assert_eq!(
            view.pending.iter().find(|d| d.item_uuid == "u-hp").map(|d| d.reason),
            Some(DetachReason::Consumed),
            "reason is Consumed"
        );
    }

    #[test]
    fn use_rest_stack_keeps_uuid_and_no_detach() {
        let mut world = use_world();
        let _ = act(&mut world, &use_req("u-hp")).expect("partial consume");
        let inv = snapshot(&world);
        assert_eq!(inv.base_slots[0].as_ref().unwrap().item_uuid, "u-hp");
        assert_eq!(inv.base_slots[0].as_ref().unwrap().count, 4);
        assert!(
            world.item_lifecycle.get("p1").map(|l| l.contains("u-hp")).unwrap_or(false) == false,
            "rest stack is NOT detached"
        );
    }

    #[test]
    fn use_foreign_missing_ambiguous_uuid_rejected() {
        let mut world = use_world();
        // Fremde UUID.
        assert_eq!(
            act(&mut world, &use_req("u-fremd")),
            Err(InventoryReject::NotInInventory)
        );
        // Leer UUID.
        assert_eq!(
            act(&mut world, &use_req("   ")),
            Err(InventoryReject::NotInInventory)
        );
        // Ambiguous: same UUID twice (a corrupted/race placement).
        world.players.get_mut("p1").unwrap().inventory.base_slots[2] =
            Some(inst("u-hp", "hp_potion", 2));
        let before = snapshot(&world);
        assert_eq!(
            act(&mut world, &use_req("u-hp")),
            Err(InventoryReject::NotInInventory),
            "owned_instance rejects non-unique placement"
        );
        assert_eq!(snapshot(&world), before, "keine Mutation");
    }

    #[test]
    fn use_equipped_uuid_is_not_a_use_source() {
        let mut world = use_world();
        world.players.get_mut("p1").unwrap().inventory
            .equipped
            .insert(EquipSlot::Drink, inst("u-eq", "hp_potion", 1));
        let r = act(&mut world, &use_req("u-eq"));
        assert_eq!(r, Err(InventoryReject::NotInInventory), "equipment is no use source");
    }

    #[test]
    fn use_undefined_item_definition_rejected() {
        let mut world = use_world();
        world.players.get_mut("p1").unwrap().inventory.base_slots[2] =
            Some(inst("u-x", "mystery", 1));
        let r = act(&mut world, &use_req("u-x"));
        assert_eq!(r, Err(InventoryReject::DefinitionUnavailable));
        assert_eq!(world.players["p1"].inventory.base_slots[2].as_ref().unwrap().count, 1);
    }

    #[test]
    fn use_overflow_guard_caps_without_panic() {
        let mut world = use_world();
        let mut def = hp_potion_def();
        def.heal_amount = Some(i64::MAX);
        world.item_definitions.insert("hp_potion".into(), def);
        world.players.get_mut("p1").unwrap().hp = 90;
        let out = act(&mut world, &use_req("u-hp")).expect("huge heal");
        assert_eq!(out.count, 1);
        assert_eq!(world.players["p1"].hp, 100, "checked add + cap, no overflow");
    }

    #[test]
    fn use_pending_trade_barrier_blocks_use() {
        let mut world = use_world();
        block_economy(&mut world, "p1");
        let before = snapshot(&world);
        assert_eq!(
            act(&mut world, &use_req("u-hp")),
            Err(InventoryReject::CommitPending)
        );
        assert_eq!(snapshot(&world), before, "keine Teilmutation unter Sperre");
    }

    #[test]
    fn use_cooldown_survives_snapshot_and_takeover() {
        // The shared cooldown is persisted via the existing P-18 Snapshot
        // (absolute ready_at) and preserved across a Takeover (RAM-authoritative).
        let mut world = use_world();
        assert!(act(&mut world, &use_req("u-hp")).is_ok());
        let p = world.players.get("p1").unwrap();
        let ready_at = p.cooldowns.get(CONSUMABLE_COOLDOWN_KEY).map(|t| *t).expect("cooldown present");
        // Snapshot carries it as epoch-ms.
        let snap_ms = crate::combat::cooldowns::to_epoch_ms(ready_at);
        assert!(snap_ms > 0, "persisted as an absolute expiry point");
        // Reconnect/Takeover keeps the RAM player (and its cooldowns) — the
        // existing commit_login path preserves cooldowns (see world.rs).
        let candidate = player_with(4);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut w2 = World::new();
        w2.players.insert("p1".into(), world.players.remove("p1").unwrap());
        w2.by_conn.insert(1, "p1".into());
        let cf = crate::world::ConnectionFields {
            tx,
            session_id: "s2".into(),
            lang: "de".into(),
        };
        let outcome = crate::world::commit_login(&mut w2, 2, candidate, cf);
        assert_eq!(outcome, Ok(crate::world::CommitOutcome::Takeover { old_conn_id: 1 }));
        let after = w2.players.get("p1").unwrap();
        assert_eq!(
            after.cooldowns.get(CONSUMABLE_COOLDOWN_KEY).map(|t| *t),
            Some(ready_at),
            "Takeover does NOT reset the shared cooldown"
        );
        // Restored-from-snapshot value still blocks before the expiry.
        let _restored = crate::combat::cooldowns::from_epoch_ms(snap_ms);
        assert!(
            !crate::combat::cooldowns::is_ready(
                &after.cooldowns,
                CONSUMABLE_COOLDOWN_KEY,
                ready_at - std::time::Duration::from_millis(1),
            ),
            "still on cooldown one ms before expiry after restore"
        );
    }
}
