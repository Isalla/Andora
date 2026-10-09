// trade — NPC-Händler, vollständig serverseitig (docs/Handelssystem.md).
//
// Nutzt ausschließlich veröffentlichte Bausteine: instanzgenaue
// Inventaroperationen (`try_take_instance`/`try_insert_instance`,
// `retired_uuid`), `SellHistory` und `ItemLifecycle`. Es gibt keine zweite
// History-/Lifecycle-Implementierung.
//
// Verbindliche Regeln:
// - Eigenes Verkaufssortiment je Händler (`merchant_offers`, Migration 022).
// - Explizite serverseitige Kauf-/Verkaufspreise aus Content-Daten
//   (`buy_price_idia` für den Spielerkauf, `sell_price_idia` für den
//   Händlerankauf); unbegrenzter Angebotsbestand, keine Quote.
// - Jeder Händler nimmt alle nach Bindungs-/Questregeln verkäuflichen Items
//   mit hinterlegtem Verkaufspreis an.
// - Buyback spielergebunden, händlerübergreifend (kein Angebotszwang am
//   Rückkauf-Händler), Runtime-only, max. 20, FIFO, zum erhaltenen
//   Gesamtbetrag, vollständiger Eintrag (kein Teilrückkauf, kein Puffer).
// - Wiederholte gültige Absichten sind weitere Operationen (kein
//   Idempotenz-Dedup); `seq` bleibt Korrelation und wird nur zurückgespiegelt.
// - Clientpreise und behauptete Endwerte sind nicht maßgeblich: Preise,
//   Eigentum, Mengen und Gold stammen aus Serverwerten.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::item::{BindingRule, ItemDefinition, ItemInstance};
use crate::npc::NpcStatus;
use crate::persist::PersistComponent;
use crate::quest::{ObjectiveType, QuestService, QuestState};
use crate::world::World;

/// Angebotspreise eines Händlers für eine Item-Definition (Idia, absolute
/// Beträge). `None` = nicht im Verkauf / keine Annahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MerchantOffer {
    pub buy_price: Option<i64>,
    pub sell_price: Option<i64>,
}

/// Händlerkatalog eines Realms (Content-Schicht, Migration 022):
/// Händlerrolle je NPC-Spawn plus Sortiment mit Preisen je (Spawn, Item).
/// Unbegrenzter Bestand (keine Mengen-/Quotenspalten).
#[derive(Debug, Clone, Default)]
pub struct MerchantCatalog {
    /// Spawn-IDs mit Händlerrolle.
    pub merchants: HashSet<i64>,
    /// Sortiment: (Spawn-ID, Item-ID) → Preise.
    pub offers: HashMap<(i64, String), MerchantOffer>,
}

impl MerchantCatalog {
    pub fn is_merchant(&self, spawn_id: i64) -> bool {
        self.merchants.contains(&spawn_id)
    }

    pub fn offer_for(&self, spawn_id: i64, item_id: &str) -> Option<&MerchantOffer> {
        self.offers.get(&(spawn_id, item_id.to_string()))
    }
}

/// Händleraktion (`NPC_TALK`-Feld `action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeAction {
    Open,
    Buy,
    Sell,
    Buyback,
}

impl TradeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            TradeAction::Open => "open",
            TradeAction::Buy => "buy",
            TradeAction::Sell => "sell",
            TradeAction::Buyback => "buyback",
        }
    }

    /// Parst die Client-Aktion (exakte Kleinschreibung nach Trim;
    /// alles andere ist `UnknownAction`, kein stiller Default).
    pub fn parse(raw: &str) -> Option<TradeAction> {
        match raw.trim() {
            "open" => Some(TradeAction::Open),
            "buy" => Some(TradeAction::Buy),
            "sell" => Some(TradeAction::Sell),
            "buyback" => Some(TradeAction::Buyback),
            _ => None,
        }
    }
}

/// Geparste Händlerabsicht (der Handler füllt aus dem NPC_TALK-Payload;
/// unbelegte Felder bleiben leer, Mengen ohne Angabe sind 0 und damit
/// ungültig — keine erfundenen Defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeRequest {
    pub npc_id: String,
    pub action: TradeAction,
    /// Kauf: Item-ID aus dem Angebot.
    pub item_id: String,
    /// Verkauf: konkrete Inventar-UUID.
    pub item_uuid: String,
    /// Buyback: eindeutige History-Kennung (= verkaufte Instanz-UUID,
    /// je Session eindeutig, siehe `SellHistoryEntry::item_uuid`).
    pub history_id: String,
    /// Kauf-/Verkaufsmenge (Buyback: Eintrag ist vollständig, Anzahl folgt
    /// dem Eintrag, nicht dem Request).
    pub count: i64,
}

/// Stabile Ablehnungsgründe (`NPC_TEXT`-Feld `reason`; serverseitig zusätzlich
/// über `log_reject` protokolliert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeReject {
    UnknownAction,
    UnknownPlayer,
    UnknownNpc,
    NotAMerchant,
    MerchantUnavailable,
    OutOfRange,
    OfferUnavailable,
    NoSellPrice,
    InvalidQuantity,
    NotInInventory,
    NotEnoughItems,
    BoundItem,
    QuestItemProtected,
    /// Mindestens eine ACTIVE Quest ist nicht auflösbar (Definition fehlt):
    /// Der Questschutz ist dann nicht prüfbar — fail-closed, keine Mutation.
    /// Questzuordnungen selbst werden beim Einstieg zuverlässig geladen
    /// (fail-closed, `handle_hello`); nur Definitionen fehlen ohne
    /// Content-Loader. Kein Violation-Zähler, Disconnect oder Bann.
    QuestDataUnavailable,
    /// Economic state is frozen while a paired spool publication is pending.
    CommitPending,
    PriceOverflow,
    InsufficientIdia,
    InventoryFull,
    HistoryExpired,
}

impl TradeReject {
    pub fn reason(self) -> &'static str {
        match self {
            TradeReject::UnknownAction => "unknown_action",
            TradeReject::UnknownPlayer => "unknown_player",
            TradeReject::UnknownNpc => "unknown_npc",
            TradeReject::NotAMerchant => "not_a_merchant",
            TradeReject::MerchantUnavailable => "merchant_unavailable",
            TradeReject::OutOfRange => "out_of_range",
            TradeReject::OfferUnavailable => "offer_unavailable",
            TradeReject::NoSellPrice => "no_sell_price",
            TradeReject::InvalidQuantity => "invalid_quantity",
            TradeReject::NotInInventory => "not_in_inventory",
            TradeReject::NotEnoughItems => "not_enough_items",
            TradeReject::BoundItem => "bound_item",
            TradeReject::QuestItemProtected => "quest_item_protected",
            TradeReject::QuestDataUnavailable => "quest_data_unavailable",
            TradeReject::CommitPending => "trade_commit_pending",
            TradeReject::PriceOverflow => "price_overflow",
            TradeReject::InsufficientIdia => "insufficient_idia",
            TradeReject::InventoryFull => "inventory_full",
            TradeReject::HistoryExpired => "history_expired",
        }
    }
}

/// Angebotszeile für `open` (Preise aus Content-Daten, `None` = nicht
/// angeboten / nicht angenommen).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferView {
    pub item_id: String,
    pub name: String,
    pub buy_price: Option<i64>,
    pub sell_price: Option<i64>,
}

/// History-Zeile für `open` und Erfolgsantworten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryView {
    pub history_id: String,
    pub item_id: String,
    pub count: i64,
    pub price: i64,
}

/// Ergebnis einer ausgeführten Händleraktion (Antwortdaten für `NPC_TEXT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeOutcome {
    pub action: TradeAction,
    pub npc_id: String,
    pub merchant_name: String,
    /// Resultierender Idia-Bestand (Erfolg liefert den Währungszustand).
    pub idia: i64,
    /// Sortiment (nur bei `open` gefüllt).
    pub offers: Vec<OfferView>,
    /// Aktuelle Buyback-History (Erfolg liefert den Historyzustand).
    pub history: Vec<HistoryView>,
    /// Gehandeltes Item (buy/sell/buyback; open: leer).
    pub item_id: String,
    /// Entnommene/wiederhergestellte Instanz-UUID (sell/buyback).
    pub item_uuid: String,
    /// Gehandelte Menge.
    pub count: i64,
    /// Gezahlter/erhaltener Gesamtbetrag.
    pub total_price: i64,
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).hypot(ay - by)
}

/// Questgeschützte Item-IDs: Sammel-/Bringziele (`collect`/`deliver`) aller
/// auflösbaren ACTIVEen Quests (docs/Quest-System.md §27.19). Nach Abbruch
/// oder Abschluss ist dieselbe ID frei verkäuflich (§27.20).
///
/// Fail-closed-Dreiwegeunterscheidung (kein Quest-Content-Loader in diesem
/// Auftrag):
/// - keine ACTIVE Quest → leere Menge (normale Verkaufsprüfung);
/// - alle ACTIVEen Quests auflösbar → bestehender Itemschutz;
/// - mindestens eine ACTIVE Quest ohne Definition → `QuestDataUnavailable`
///   (fehlende Definition ist NICHT „kein geschütztes Item").
///
/// Die Questzuordnungen selbst (`player.quests`) werden beim Einstieg
/// zuverlässig aus der RealmDB geladen und sind bei Ladefehler
/// einsteigungsverweigernd (`handle_hello`); nur die Definitionen fehlen
/// ohne Loader. Diese Funktion erfindet keine Definitionen.
pub fn active_quest_item_ids(
    player: &crate::world::Player,
    quests: &QuestService,
) -> Result<HashSet<String>, TradeReject> {
    let mut out = HashSet::new();
    for (qid, qs) in &player.quests {
        if qs.state != QuestState::Active {
            continue;
        }
        let Some(def) = quests.find_definition(qid) else {
            return Err(TradeReject::QuestDataUnavailable);
        };
        for o in &def.objectives {
            if matches!(o.kind, ObjectiveType::Collect | ObjectiveType::Deliver) {
                out.insert(o.target.clone());
            }
        }
    }
    Ok(out)
}

/// NPC-Verkäuflichkeit: Instanzstatus UND Definitionsregel — bewusst nicht
/// ungeprüft mit Spielerhandel-`can_trade` gleichgesetzt. Eine
/// charaktergebundene Definition ist auch in freiem Instanzzustand nicht
/// verkäuflich; Questschutz prüft der Aufrufer getrennt.
fn sellable(
    instance: &ItemInstance,
    def: &ItemDefinition,
    quest_items: &HashSet<String>,
) -> Result<(), TradeReject> {
    if def.binding_rule != BindingRule::Tradeable || instance.is_bound() {
        return Err(TradeReject::BoundItem);
    }
    if quest_items.contains(instance.item_id.as_str()) {
        return Err(TradeReject::QuestItemProtected);
    }
    Ok(())
}

fn history_views(world: &World, actor: &str) -> Vec<HistoryView> {
    world
        .sell_history
        .get(actor)
        .map(|h| {
            h.iter()
                .map(|e| HistoryView {
                    history_id: e.item_uuid().to_string(),
                    item_id: e.item_id().to_string(),
                    count: e.count(),
                    price: e.sell_gold_value,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Führt eine Händlerabsicht atomar aus: validieren, dann gemeinsam
/// übernehmen — oder vollständig unverändert lassen. Läuft unter der
/// ausführenden World-Sperre des Aufrufers (keine awaits, keine
/// Zwischenänderungen möglich).
pub fn attempt_trade(
    world: &mut World,
    quests: &QuestService,
    pickup_radius: f64,
    actor: &str,
    req: &TradeRequest,
) -> Result<TradeOutcome, TradeReject> {
    if !world.economic_mutation_allowed(actor) {
        return Err(TradeReject::CommitPending);
    }
    let player = world.players.get(actor).ok_or(TradeReject::UnknownPlayer)?;
    let npc = world.npcs.get(&req.npc_id).ok_or(TradeReject::UnknownNpc)?;
    if !world.merchant_catalog.is_merchant(npc.spawn_id) {
        return Err(TradeReject::NotAMerchant);
    }
    if npc.status != NpcStatus::Alive {
        return Err(TradeReject::MerchantUnavailable);
    }
    let (px, py) = (player.x, player.y);
    if dist(px, py, npc.x, npc.y) > pickup_radius {
        return Err(TradeReject::OutOfRange);
    }
    let merchant_name = npc.name.clone();
    let spawn_id = npc.spawn_id;
    match req.action {
        TradeAction::Open => Ok(open_outcome(world, actor, req, &merchant_name, spawn_id)),
        TradeAction::Buy => buy(world, actor, req, spawn_id, &merchant_name),
        TradeAction::Sell => sell(world, quests, actor, req, spawn_id, &merchant_name),
        TradeAction::Buyback => buyback(world, actor, req, &merchant_name),
    }
}

fn open_outcome(
    world: &World,
    actor: &str,
    req: &TradeRequest,
    merchant_name: &str,
    spawn_id: i64,
) -> TradeOutcome {
    let mut offers: Vec<OfferView> = world
        .merchant_catalog
        .offers
        .iter()
        .filter(|((s, _), _)| *s == spawn_id)
        .filter_map(|((_, item_id), o)| {
            if o.buy_price.is_none() && o.sell_price.is_none() {
                return None;
            }
            let def = world.item_definitions.get(item_id)?;
            Some(OfferView {
                item_id: item_id.clone(),
                name: def.name.clone(),
                buy_price: o.buy_price,
                sell_price: o.sell_price,
            })
        })
        .collect();
    offers.sort_by(|a, b| a.item_id.cmp(&b.item_id));
    let player = world.players.get(actor);
    TradeOutcome {
        action: TradeAction::Open,
        npc_id: req.npc_id.clone(),
        merchant_name: merchant_name.to_string(),
        idia: player.map(|p| p.idia).unwrap_or(0),
        offers,
        history: history_views(world, actor),
        item_id: String::new(),
        item_uuid: String::new(),
        count: 0,
        total_price: 0,
    }
}

/// Kauf: neues Exemplar aus unbegrenztem Angebot (frische UUIDs, keine
/// Lifecycle-Pflicht — nichts wurde abgekoppelt).
fn buy(
    world: &mut World,
    actor: &str,
    req: &TradeRequest,
    spawn_id: i64,
    merchant_name: &str,
) -> Result<TradeOutcome, TradeReject> {
    if req.count <= 0 {
        return Err(TradeReject::InvalidQuantity);
    }
    let def = world
        .item_definitions
        .get(&req.item_id)
        .ok_or(TradeReject::OfferUnavailable)?
        .clone();
    let price = world
        .merchant_catalog
        .offer_for(spawn_id, &req.item_id)
        .and_then(|o| o.buy_price)
        .filter(|p| *p >= 0)
        .ok_or(TradeReject::OfferUnavailable)?;
    let total = price
        .checked_mul(req.count)
        .ok_or(TradeReject::PriceOverflow)?;
    {
        let p = world.players.get(actor).ok_or(TradeReject::UnknownPlayer)?;
        if p.idia < total {
            return Err(TradeReject::InsufficientIdia);
        }
        if !p.inventory.fits(&def, req.count) {
            return Err(TradeReject::InventoryFull);
        }
    }
    // Mutation: Idia (geprüft) abziehen, dann aufnehmen. Ein unerwarteter
    // Rest wird exakt revertiert — ohne Dirty-Markierung, ohne Teilbuchung.
    let remainder = {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.idia -= total;
        p.inventory.try_add(&def, req.count).remainder
    };
    if remainder > 0 {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.idia += total;
        return Err(TradeReject::InventoryFull);
    }
    let (idia, history) = {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.mark_dirty(PersistComponent::Idia);
        p.mark_dirty(PersistComponent::Inventory);
        (p.idia, history_views(world, actor))
    };
    Ok(TradeOutcome {
        action: TradeAction::Buy,
        npc_id: req.npc_id.clone(),
        merchant_name: merchant_name.to_string(),
        idia,
        offers: Vec::new(),
        history,
        item_id: def.item_id.clone(),
        item_uuid: String::new(),
        count: req.count,
        total_price: total,
    })
}

/// Verkauf: exakte Entnahme per UUID, Idia-Gutschrift, History-Eintrag mit
/// vollständigem Exemplar, Lifecycle-Abkopplung — gemeinsam oder gar nicht.
fn sell(
    world: &mut World,
    quests: &QuestService,
    actor: &str,
    req: &TradeRequest,
    spawn_id: i64,
    merchant_name: &str,
) -> Result<TradeOutcome, TradeReject> {
    if req.count <= 0 {
        return Err(TradeReject::InvalidQuantity);
    }
    // Lesende Vorprüfung (Eigentum, Menge, Verkäuflichkeit, Preis, Goldraum).
    let (item_id, total) = {
        let p = world.players.get(actor).ok_or(TradeReject::UnknownPlayer)?;
        let inst =
            find_takeable(&p.inventory, &req.item_uuid).ok_or(TradeReject::NotInInventory)?;
        if req.count > inst.count {
            return Err(TradeReject::NotEnoughItems);
        }
        let def = world
            .item_definitions
            .get(&inst.item_id)
            .ok_or(TradeReject::OfferUnavailable)?;
        sellable(&inst, def, &active_quest_item_ids(p, quests)?)?;
        let sell_price = world
            .merchant_catalog
            .offer_for(spawn_id, &inst.item_id)
            .and_then(|o| o.sell_price)
            .filter(|pr| *pr >= 0)
            .ok_or(TradeReject::NoSellPrice)?;
        let total = sell_price
            .checked_mul(req.count)
            .ok_or(TradeReject::PriceOverflow)?;
        p.idia
            .checked_add(total)
            .ok_or(TradeReject::PriceOverflow)?;
        (inst.item_id.clone(), total)
    };
    // Mutation: Entnahme (atomar; Fehler → nichts verändert).
    let taken = {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.inventory
            .try_take_instance(&req.item_uuid, req.count)
            .map_err(map_take_error)?
    };
    {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.idia += total;
    }
    // History (vollständiges Exemplar, FIFO) + Lifecycle-Abkopplung.
    {
        let rt = world.runtime_id.clone();
        let uuids = world
            .players
            .get(actor)
            .map(|p| p.inventory.persistent_uuids())
            .unwrap_or_default();
        let lc = world.item_lifecycle.entry(actor.to_string()).or_default();
        crate::item_lifecycle::reconcile_after_take(
            lc,
            &uuids,
            &taken.item_uuid,
            crate::item_lifecycle::DetachReason::Sold,
            &rt,
            crate::item_lifecycle::now_ms(),
        );
    }
    let history = {
        world
            .sell_history
            .entry(actor.to_string())
            .or_default()
            .record(crate::item_lifecycle::SellHistoryEntry {
                instance: taken.clone(),
                sell_gold_value: total,
            });
        history_views(world, actor)
    };
    let idia = {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.mark_dirty(PersistComponent::Inventory);
        p.mark_dirty(PersistComponent::Idia);
        p.idia
    };
    Ok(TradeOutcome {
        action: TradeAction::Sell,
        npc_id: req.npc_id.clone(),
        merchant_name: merchant_name.to_string(),
        idia,
        offers: Vec::new(),
        history,
        item_id,
        item_uuid: taken.item_uuid,
        count: req.count,
        total_price: total,
    })
}

/// Rückkauf: vollständiger History-Eintrag zum erhaltenen Gesamtbetrag.
/// Händlerübergreifend (kein Angebotszwang am Rückkauf-Händler).
fn buyback(
    world: &mut World,
    actor: &str,
    req: &TradeRequest,
    merchant_name: &str,
) -> Result<TradeOutcome, TradeReject> {
    let entry = world
        .sell_history
        .get(actor)
        .and_then(|h| h.iter().find(|e| e.item_uuid() == req.history_id).cloned())
        .ok_or(TradeReject::HistoryExpired)?;
    let def = world
        .item_definitions
        .get(entry.item_id())
        .ok_or(TradeReject::OfferUnavailable)?
        .clone();
    let total = entry.sell_gold_value;
    {
        let p = world.players.get(actor).ok_or(TradeReject::UnknownPlayer)?;
        if p.idia < total {
            return Err(TradeReject::InsufficientIdia);
        }
        if !p.inventory.fits(&def, entry.count()) {
            return Err(TradeReject::InventoryFull);
        }
    }
    // Mutation: zuerst einsetzen (atomar; Fehler → Eintrag bleibt, kein Gold,
    // kein Dirty). Erst danach Eintrag verbrauchen, Gold abziehen,
    // Lifecycle verrechnen.
    let outcome = {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.inventory
            .try_insert_instance(&def, &entry.instance)
            // Defensiv nach fits: exakte Regeln (Bindung/Hersteller) können
            // strenger sein als die fits-Probe.
            .map_err(|_| TradeReject::InventoryFull)?
    };
    {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.idia -= total;
    }
    {
        if let Some(h) = world.sell_history.get_mut(actor) {
            let _ = h.take_for_buyback(entry.item_uuid());
        }
    }
    {
        let rt = world.runtime_id.clone();
        let uuids = world
            .players
            .get(actor)
            .map(|p| p.inventory.persistent_uuids())
            .unwrap_or_default();
        let lc = world.item_lifecycle.entry(actor.to_string()).or_default();
        crate::item_lifecycle::reconcile_after_insert(
            lc,
            &uuids,
            entry.item_uuid(),
            outcome.retired_uuid.as_deref(),
            &rt,
            crate::item_lifecycle::now_ms(),
        );
    }
    let (idia, history) = {
        let p = world
            .players
            .get_mut(actor)
            .ok_or(TradeReject::UnknownPlayer)?;
        p.mark_dirty(PersistComponent::Idia);
        p.mark_dirty(PersistComponent::Inventory);
        (p.idia, history_views(world, actor))
    };
    Ok(TradeOutcome {
        action: TradeAction::Buyback,
        npc_id: req.npc_id.clone(),
        merchant_name: merchant_name.to_string(),
        idia,
        offers: Vec::new(),
        history,
        item_id: entry.item_id().to_string(),
        item_uuid: entry.item_uuid().to_string(),
        count: entry.count(),
        total_price: total,
    })
}

/// Entnahmekandidat im normalen Inventar (Basis/Taschen): genau eine
/// eindeutige UUID, sonst keine Entnahme (fail-closed, auch bei Kollision).
fn find_takeable(inv: &crate::inventory::InventoryState, uuid: &str) -> Option<ItemInstance> {
    if uuid.trim().is_empty() {
        return None;
    }
    let mut found: Option<ItemInstance> = None;
    for it in inv
        .base_slots
        .iter()
        .chain(inv.bags.iter().flat_map(|b| &b.slots))
        .filter_map(|s| s.as_ref())
    {
        if it.item_uuid == uuid {
            if found.is_some() {
                return None; // mehrdeutig → keine Entnahme
            }
            found = Some(it.clone());
        }
    }
    found
}

/// Defensive Abbildung von Entnahmefehlern (Vorprüfung lief unter derselben
/// Sperre; ein Fehler hier verändert nichts).
fn map_take_error(e: crate::inventory::InventoryError) -> TradeReject {
    use crate::inventory::InventoryError as IE;
    match e {
        IE::InvalidQuantity => TradeReject::InvalidQuantity,
        IE::NotEnoughItems => TradeReject::NotEnoughItems,
        _ => TradeReject::NotInInventory,
    }
}

/// Antwortdaten für `NPC_TEXT` (Erfolg; Ablehnung baut der Handler).
pub fn success_payload(out: &TradeOutcome) -> serde_json::Value {
    let history: Vec<serde_json::Value> = out
        .history
        .iter()
        .map(|h| {
            serde_json::json!({
                "history_id": h.history_id,
                "item_id": h.item_id,
                "count": h.count,
                "price": h.price,
            })
        })
        .collect();
    let mut v = serde_json::json!({
        "ok": true,
        "action": out.action.as_str(),
        "npc_id": out.npc_id,
        "merchant_name": out.merchant_name,
        "idia": out.idia,
        "history": history,
    });
    if out.action == TradeAction::Open {
        v["offers"] = out
            .offers
            .iter()
            .map(|o| {
                serde_json::json!({
                    "item_id": o.item_id,
                    "name": o.name,
                    "buy_price": o.buy_price,
                    "sell_price": o.sell_price,
                })
            })
            .collect();
    } else {
        v["item_id"] = serde_json::Value::String(out.item_id.clone());
        v["count"] = serde_json::json!(out.count);
        v["total_price"] = serde_json::json!(out.total_price);
        if out.action != TradeAction::Buy {
            v["item_uuid"] = serde_json::Value::String(out.item_uuid.clone());
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};
    use std::time::Instant;
    use tokio::sync::mpsc;

    use crate::item::{BindingState, ItemCategory, ItemModifiers};
    use crate::quest::{
        CharacterQuestState, ObjectiveType, QuestDefinition, QuestObjective, QuestService,
        QuestState,
    };

    fn trader(
        id: &str,
        x: f64,
        y: f64,
        idia: i64,
    ) -> (crate::world::Player, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            crate::world::Player {
                id: id.into(),
                name: id.into(),
                x,
                y,
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
                idia,
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
                inventory: crate::inventory::InventoryState::new(8),
                quests: BTreeMap::new(),
                dirty: Default::default(),
                persist_generation: 0,
                persist_revision: 0,
            },
            rx,
        )
    }

    fn merchant_npc(spawn_id: i64, x: f64, y: f64) -> crate::npc::Npc {
        npc_with_status(spawn_id, x, y, crate::npc::NpcStatus::Alive)
    }

    fn npc_with_status(
        spawn_id: i64,
        x: f64,
        y: f64,
        status: crate::npc::NpcStatus,
    ) -> crate::npc::Npc {
        crate::npc::Npc {
            id: crate::npc::npc_id(spawn_id),
            spawn_id,
            name: format!("Händler_{spawn_id}"),
            kind: "named".into(),
            attackable: false,
            aggressive: false,
            aggro_range: 0.0,
            attack_range: 0.0,
            attack_duration_ms: 0,
            weapon_damage: 0,
            weapon_skill: 0,
            armor: 0,
            max_hp: 100,
            move_speed: 0.0,
            respawn_ms: 0,
            faction: None,
            exp_reward: 0,
            level: 1,
            loot_table_id: None,
            pack_id: None,
            home_x: x,
            home_y: y,
            home_radius: 1.0,
            leash_radius: 1.0,
            status,
            hp: 100,
            x,
            y,
            target_id: None,
            last_attack: Instant::now(),
            no_link_since: None,
            return_started_at: None,
            respawn_after: None,
            claimed_by: None,
            override_ctx: None,
            effects: Vec::new(),
            cooldowns: BTreeMap::new(),
            active_cast: None,
        }
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

    fn bound_rule_def() -> crate::item::ItemDefinition {
        let mut d =
            crate::item::ItemDefinition::new("bop_schwert", "BoP-Schwert", ItemCategory::Weapon);
        d.binding_rule = crate::item::BindingRule::BindOnPickup;
        d
    }

    fn quest_relic_def() -> crate::item::ItemDefinition {
        let mut d =
            crate::item::ItemDefinition::new("quest_relic", "Questrelikt", ItemCategory::QuestItem);
        d.max_stack = 10;
        d
    }

    fn deko_def() -> crate::item::ItemDefinition {
        crate::item::ItemDefinition::new("deko", "Deko", ItemCategory::Accessory)
    }

    /// Fixture-Katalog (explizit Testdaten, keine Produktionspreise):
    /// Händler-Spawn 5 mit Sortiment, Spawn 7 (tot) und 9 (fern) als Händler.
    fn catalog() -> MerchantCatalog {
        let mut c = MerchantCatalog::default();
        c.merchants.insert(5);
        c.merchants.insert(7);
        c.merchants.insert(9);
        c.offers.insert(
            (5, "hp_potion".into()),
            MerchantOffer {
                buy_price: Some(10),
                sell_price: Some(4),
            },
        );
        c.offers.insert(
            (5, "eisenschwert".into()),
            MerchantOffer {
                buy_price: Some(100),
                sell_price: Some(30),
            },
        );
        c.offers.insert(
            (5, "quest_relic".into()),
            MerchantOffer {
                buy_price: None,
                sell_price: Some(7),
            },
        );
        c.offers.insert(
            (5, "deko".into()),
            MerchantOffer {
                buy_price: Some(5),
                sell_price: None,
            },
        );
        c
    }

    /// Welt mit Spieler "held" (0,0), Händler npc_5 (1,0), Sortiment und
    /// Definitionen. Questservice leer (Aufrufer registriert bei Bedarf).
    fn world() -> (World, QuestService) {
        let mut w = World::new();
        let (p, _rx) = trader("held", 0.0, 0.0, 1000);
        w.players.insert("held".into(), p);
        w.npcs.insert("npc_5".into(), merchant_npc(5, 1.0, 0.0));
        w.item_definitions.insert("hp_potion".into(), potion_def());
        w.item_definitions
            .insert("eisenschwert".into(), sword_def());
        w.item_definitions
            .insert("quest_relic".into(), quest_relic_def());
        w.item_definitions.insert("deko".into(), deko_def());
        w.item_definitions
            .insert("bop_schwert".into(), bound_rule_def());
        w.merchant_catalog = catalog();
        (w, QuestService::new())
    }

    fn req(npc: &str, action: TradeAction) -> TradeRequest {
        TradeRequest {
            npc_id: npc.into(),
            action,
            item_id: String::new(),
            item_uuid: String::new(),
            history_id: String::new(),
            count: 0,
        }
    }

    fn give(
        inv: &mut crate::inventory::InventoryState,
        def: &crate::item::ItemDefinition,
        qty: i64,
    ) {
        let r = inv.try_add(def, qty);
        assert_eq!(r.remainder, 0, "Fixture-Aufnahme muss passen");
    }

    fn quest_def() -> QuestDefinition {
        QuestDefinition {
            id: "q_relic".into(),
            title_key: "t".into(),
            description_key: "d".into(),
            objectives: vec![
                QuestObjective {
                    id: "o_collect".into(),
                    kind: ObjectiveType::Collect,
                    target: "quest_relic".into(),
                    required: 3,
                },
                QuestObjective {
                    id: "o_kill".into(),
                    kind: ObjectiveType::Kill,
                    target: "wolf".into(),
                    required: 5,
                },
            ],
            min_level: 1,
            requires: vec![],
            repeatable: false,
        }
    }

    fn set_quest_state(w: &mut World, qid: &str, state: QuestState) {
        w.players.get_mut("held").unwrap().quests.insert(
            qid.into(),
            CharacterQuestState {
                quest_id: qid.into(),
                state,
                progress: vec![],
                started_at_ms: None,
                completed_at_ms: None,
            },
        );
    }

    const RADIUS: f64 = 5.0;

    #[test]
    fn action_parse_is_explicit() {
        assert_eq!(TradeAction::parse("open"), Some(TradeAction::Open));
        assert_eq!(TradeAction::parse("buy"), Some(TradeAction::Buy));
        assert_eq!(TradeAction::parse("sell"), Some(TradeAction::Sell));
        assert_eq!(TradeAction::parse("buyback"), Some(TradeAction::Buyback));
        assert_eq!(TradeAction::parse("  sell  "), Some(TradeAction::Sell));
        assert_eq!(
            TradeAction::parse("SELL"),
            None,
            "kein stiller Case-Fallback"
        );
        assert_eq!(TradeAction::parse(""), None);
        assert_eq!(TradeAction::parse("handel"), None);
        assert_eq!(TradeAction::Open.as_str(), "open");
    }

    #[test]
    fn open_returns_sorted_offers_and_history() {
        let (mut w, quests) = world();
        let out = attempt_trade(
            &mut w,
            &quests,
            RADIUS,
            "held",
            &req("npc_5", TradeAction::Open),
        )
        .expect("open gelingt");
        assert_eq!(out.action, TradeAction::Open);
        assert_eq!(out.npc_id, "npc_5");
        assert_eq!(out.merchant_name, "Händler_5");
        assert_eq!(out.idia, 1000);
        let ids: Vec<&str> = out.offers.iter().map(|o| o.item_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["deko", "eisenschwert", "hp_potion", "quest_relic"]
        );
        let potion = &out.offers[2];
        assert_eq!((potion.buy_price, potion.sell_price), (Some(10), Some(4)));
        assert!(out.history.is_empty());
        // Keine Mutation, keine Dirty-Markierung.
        let p = &w.players["held"];
        assert!(!p.dirty.any());
        assert_eq!(p.persist_generation, 0);
    }

    #[test]
    fn open_without_offers_returns_empty_list() {
        let (mut w, quests) = world();
        w.npcs.insert("npc_9".into(), merchant_npc(9, 1.0, 0.0));
        let out = attempt_trade(
            &mut w,
            &quests,
            RADIUS,
            "held",
            &req("npc_9", TradeAction::Open),
        )
        .expect("Händler ohne Sortiment öffnet");
        assert!(out.offers.is_empty());
    }

    #[test]
    fn merchant_access_is_checked_per_action() {
        let (mut w, quests) = world();
        w.npcs.insert("npc_6".into(), merchant_npc(6, 1.0, 0.0));
        w.npcs.insert(
            "npc_7".into(),
            npc_with_status(7, 1.0, 0.0, crate::npc::NpcStatus::Dead),
        );
        w.npcs.insert("npc_9".into(), merchant_npc(9, 100.0, 0.0));
        let open = req("npc_5", TradeAction::Open);
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "fremd", &open),
            Err(TradeReject::UnknownPlayer)
        );
        assert_eq!(
            attempt_trade(
                &mut w,
                &quests,
                RADIUS,
                "held",
                &req("npc_x", TradeAction::Open)
            ),
            Err(TradeReject::UnknownNpc)
        );
        assert_eq!(
            attempt_trade(
                &mut w,
                &quests,
                RADIUS,
                "held",
                &req("npc_6", TradeAction::Open)
            ),
            Err(TradeReject::NotAMerchant)
        );
        assert_eq!(
            attempt_trade(
                &mut w,
                &quests,
                RADIUS,
                "held",
                &req("npc_7", TradeAction::Open)
            ),
            Err(TradeReject::MerchantUnavailable)
        );
        assert_eq!(
            attempt_trade(
                &mut w,
                &quests,
                RADIUS,
                "held",
                &req("npc_9", TradeAction::Open)
            ),
            Err(TradeReject::OutOfRange)
        );
        // Grenzfall: exakt auf der Reichweite ist noch zulässig.
        w.npcs.insert("npc_9".into(), merchant_npc(9, 5.0, 0.0));
        assert!(attempt_trade(
            &mut w,
            &quests,
            RADIUS,
            "held",
            &req("npc_9", TradeAction::Open)
        )
        .is_ok());
    }

    #[test]
    fn buy_applies_gold_and_items_without_history() {
        let (mut w, quests) = world();
        let mut r = req("npc_5", TradeAction::Buy);
        r.item_id = "hp_potion".into();
        r.count = 5;
        let out = attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Kauf gelingt");
        assert_eq!(out.total_price, 50);
        assert_eq!(out.idia, 950);
        assert_eq!(out.count, 5);
        assert!(out.history.is_empty(), "Kauf schreibt keine History");
        let p = &w.players["held"];
        assert_eq!(p.idia, 950);
        assert_eq!(p.inventory.count_of("hp_potion"), 5);
        assert!(p.dirty.is_dirty(PersistComponent::Idia));
        assert!(p.dirty.is_dirty(PersistComponent::Inventory));
        assert!(w
            .item_lifecycle
            .get("held")
            .map(|l| l.is_empty())
            .unwrap_or(true));
    }

    #[test]
    fn buy_rejects_without_any_mutation() {
        let (mut w, quests) = world();
        let mut r = req("npc_5", TradeAction::Buy);
        r.item_id = "unbekannt".into();
        r.count = 1;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::OfferUnavailable)
        );
        // quest_relic hat buy_price None → nicht käuflich.
        r.item_id = "quest_relic".into();
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::OfferUnavailable)
        );
        // Negativpreis im Katalog → wie fehlendes Angebot.
        w.merchant_catalog.offers.insert(
            (5, "hp_potion".into()),
            MerchantOffer {
                buy_price: Some(-3),
                sell_price: Some(4),
            },
        );
        r.item_id = "hp_potion".into();
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::OfferUnavailable)
        );
        w.merchant_catalog = catalog();
        // Menge 0/negativ, Überlauf, zu wenig Gold, voller Platz.
        for count in [0, -2] {
            r.count = count;
            assert_eq!(
                attempt_trade(&mut w, &quests, RADIUS, "held", &r),
                Err(TradeReject::InvalidQuantity)
            );
        }
        w.merchant_catalog.offers.insert(
            (5, "hp_potion".into()),
            MerchantOffer {
                buy_price: Some(i64::MAX / 2),
                sell_price: Some(4),
            },
        );
        r.count = 3;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::PriceOverflow)
        );
        w.merchant_catalog = catalog();
        r.count = 200; // 2000 > 1000 Gold.
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::InsufficientIdia)
        );
        // Volles Inventar: alle Slots mit Schwertern belegen.
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &sword_def(), 8);
        }
        r.item_id = "hp_potion".into();
        r.count = 1;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::InventoryFull)
        );
        // Nichts verändert: Gold, Inventar, Dirty, Generation, History.
        let p = &w.players["held"];
        assert_eq!(p.idia, 1000);
        assert_eq!(p.inventory.count_of("hp_potion"), 0);
        assert!(!p.dirty.any());
        assert_eq!(p.persist_generation, 0);
        assert!(w
            .sell_history
            .get("held")
            .map(|h| h.is_empty())
            .unwrap_or(true));
    }

    #[test]
    fn sell_full_stack_records_history_and_lifecycle() {
        let (mut w, quests) = world();
        let uuid = {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &potion_def(), 10);
            p.inventory.base_slots[0]
                .as_ref()
                .unwrap()
                .item_uuid
                .clone()
        };
        let mut r = req("npc_5", TradeAction::Sell);
        r.item_uuid = uuid.clone();
        r.count = 10;
        let out = attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Verkauf gelingt");
        assert_eq!(out.total_price, 40);
        assert_eq!(out.idia, 1040);
        assert_eq!(out.item_uuid, uuid);
        assert_eq!(out.history.len(), 1);
        assert_eq!(out.history[0].history_id, uuid);
        assert_eq!(out.history[0].count, 10);
        assert_eq!(out.history[0].price, 40);
        let p = &w.players["held"];
        assert_eq!(p.inventory.count_of("hp_potion"), 0);
        assert!(p.dirty.is_dirty(PersistComponent::Inventory));
        assert!(p.dirty.is_dirty(PersistComponent::Idia));
        let lc = &w.item_lifecycle["held"];
        assert!(lc.contains(&uuid), "Vollentnahme ist abgekoppelt");
    }

    #[test]
    fn sell_partial_stack_keeps_rest_with_properties() {
        let (mut w, quests) = world();
        let (rest_uuid, before) = {
            let p = w.players.get_mut("held").unwrap();
            let mut inst = crate::item::ItemInstance::new(
                "rest-1",
                "hp_potion",
                ItemModifiers {
                    quality_modifier: 5.0,
                    ..Default::default()
                },
            );
            inst.count = 10;
            inst.creator_id = Some(7);
            p.inventory.base_slots[0] = Some(inst.clone());
            (inst.item_uuid.clone(), inst)
        };
        let mut r = req("npc_5", TradeAction::Sell);
        r.item_uuid = rest_uuid.clone();
        r.count = 4;
        // Individuelle Instanz (Modifier) stackt nicht plain — Verkauf nur
        // als exakte Entnahme; hier ist sie einzeln platziert.
        let out = attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Teilkauf gelingt");
        assert_eq!(out.total_price, 16);
        let p = &w.players["held"];
        // Rest: gleiche UUID, gleiche Eigenschaften, Menge 6.
        let rest = p.inventory.base_slots[0].as_ref().expect("Rest bleibt");
        assert_eq!(rest.item_uuid, rest_uuid);
        assert_eq!(rest.count, 6);
        assert_eq!(rest.modifiers.quality_modifier, 5.0);
        assert_eq!(rest.creator_id, Some(7));
        // Entnommener Teil: neue UUID, in History und Lifecycle.
        assert_ne!(out.item_uuid, rest_uuid);
        assert_eq!(out.history.len(), 1);
        assert_eq!(out.history[0].history_id, out.item_uuid);
        assert!(w.item_lifecycle["held"].contains(&out.item_uuid));
        assert!(!w.item_lifecycle["held"].contains(&rest_uuid));
        assert_eq!(before.count, 10);
    }

    #[test]
    fn sell_rejects_bound_quest_unpriced_and_unknown() {
        let (mut w, mut quests) = world();
        quests.register(quest_def()).unwrap();
        set_quest_state(&mut w, "q_relic", QuestState::Active);
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &quest_relic_def(), 2);
            let mut bound =
                crate::item::ItemInstance::new("bound-1", "eisenschwert", ItemModifiers::default());
            bound.binding = BindingState::Bound;
            p.inventory.base_slots[1] = Some(bound);
            let tradeable_but_rule =
                crate::item::ItemInstance::new("rule-1", "bop_schwert", ItemModifiers::default());
            p.inventory.base_slots[2] = Some(tradeable_but_rule);
            give(&mut p.inventory, &deko_def(), 1);
        }
        let mut r = req("npc_5", TradeAction::Sell);
        // Gebunden (Instanzstatus).
        r.item_uuid = "bound-1".into();
        r.count = 1;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::BoundItem)
        );
        // Definitionsregel nicht-tradeable trotz freiem Status.
        r.item_uuid = "rule-1".into();
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::BoundItem)
        );
        // Aktives Questitem trotz hinterlegtem Preis.
        let relic_uuid = w.players["held"].inventory.base_slots[0]
            .as_ref()
            .unwrap()
            .item_uuid
            .clone();
        r.item_uuid = relic_uuid.clone();
        r.count = 1;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::QuestItemProtected)
        );
        // Nach Abschluss ist dasselbe Item verkäuflich.
        set_quest_state(&mut w, "q_relic", QuestState::Completed);
        assert!(attempt_trade(&mut w, &quests, RADIUS, "held", &r).is_ok());
        // deko hat sell_price None → keine Annahme.
        let deko_uuid = w.players["held"]
            .inventory
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .find(|it| it.item_id == "deko")
            .unwrap()
            .item_uuid
            .clone();
        r.item_uuid = deko_uuid;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::NoSellPrice)
        );
        // Unbekannte/leere UUID, Equip-UUID, Übermenge, Menge 0.
        r.item_uuid = "fremd".into();
        r.count = 1;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::NotInInventory)
        );
        r.item_uuid = "".into();
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::NotInInventory)
        );
        r.item_uuid = relic_uuid;
        r.count = 0;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::InvalidQuantity)
        );
        r.count = 99;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::NotEnoughItems)
        );
    }

    #[test]
    fn sell_rejection_leaves_everything_unchanged() {
        let (mut w, quests) = world();
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &potion_def(), 5);
        }
        let before_idia = w.players["held"].idia;
        let before_gen = w.players["held"].persist_generation;
        let mut r = req("npc_5", TradeAction::Sell);
        r.item_uuid = "fremd".into();
        r.count = 1;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::NotInInventory)
        );
        let p = &w.players["held"];
        assert_eq!(p.idia, before_idia);
        assert_eq!(p.inventory.count_of("hp_potion"), 5);
        assert!(!p.dirty.any(), "kein Dirty bei Ablehnung");
        assert_eq!(p.persist_generation, before_gen, "kein Generationssprung");
        assert!(w
            .sell_history
            .get("held")
            .map(|h| h.is_empty())
            .unwrap_or(true));
        assert!(w
            .item_lifecycle
            .get("held")
            .map(|l| l.is_empty())
            .unwrap_or(true));
    }

    #[test]
    fn idia_credit_overflow_rejects_before_mutation() {
        let (mut w, quests) = world();
        {
            let p = w.players.get_mut("held").unwrap();
            p.idia = i64::MAX - 10;
            give(&mut p.inventory, &potion_def(), 5);
        }
        let uuid = w.players["held"].inventory.base_slots[0]
            .as_ref()
            .unwrap()
            .item_uuid
            .clone();
        let mut r = req("npc_5", TradeAction::Sell);
        r.item_uuid = uuid;
        r.count = 5; // 5*4=20 > MAX-10 → Überlauf.
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::PriceOverflow)
        );
        assert_eq!(w.players["held"].inventory.count_of("hp_potion"), 5);
    }

    #[test]
    fn buyback_restores_exact_instance_and_consumes_entry() {
        let (mut w, quests) = world();
        let original = {
            let p = w.players.get_mut("held").unwrap();
            let mut inst = crate::item::ItemInstance::new(
                "keep-1",
                "hp_potion",
                ItemModifiers {
                    damage_modifier: 3.0,
                    ..Default::default()
                },
            );
            inst.count = 4;
            inst.durability_current = Some(80);
            inst.durability_max = Some(100);
            inst.creator_id = Some(7);
            p.inventory.base_slots[0] = Some(inst.clone());
            inst
        };
        // Verkaufen (4*4=16), dann zurückkaufen.
        let mut r = req("npc_5", TradeAction::Sell);
        r.item_uuid = "keep-1".into();
        r.count = 4;
        attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Verkauf");
        assert_eq!(w.players["held"].idia, 1016);
        let mut b = req("npc_5", TradeAction::Buyback);
        b.history_id = "keep-1".into();
        let out = attempt_trade(&mut w, &quests, RADIUS, "held", &b).expect("Buyback");
        assert_eq!(out.total_price, 16, "Rückkauf zum erhaltenen Betrag");
        assert_eq!(out.item_uuid, "keep-1");
        assert_eq!(w.players["held"].idia, 1000);
        // Exakt: alle Eigenschaften und dieselbe UUID.
        let back = w.players["held"]
            .inventory
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .find(|it| it.item_uuid == "keep-1")
            .expect("Instanz zurück")
            .clone();
        assert_eq!(back, original);
        // Eintrag verbraucht, Abkopplung aufgehoben.
        assert!(w.sell_history["held"].is_empty());
        assert!(!w.item_lifecycle["held"].contains("keep-1"));
        assert!(out.history.is_empty());
        let p = &w.players["held"];
        assert!(p.dirty.is_dirty(PersistComponent::Idia));
        assert!(p.dirty.is_dirty(PersistComponent::Inventory));
    }

    #[test]
    fn buyback_merged_entry_records_retired_uuid() {
        let (mut w, quests) = world();
        // Verkaufter Plain-Stack (5 Stück), danach neuer Plain-Stack
        // derselben Definition im Inventar.
        let mut r = req("npc_5", TradeAction::Sell);
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &potion_def(), 5);
        }
        let uuid = w.players["held"].inventory.base_slots[0]
            .as_ref()
            .unwrap()
            .item_uuid
            .clone();
        r.item_uuid = uuid.clone();
        r.count = 5;
        attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Verkauf");
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &potion_def(), 3);
        }
        let mut b = req("npc_5", TradeAction::Buyback);
        b.history_id = uuid.clone();
        attempt_trade(&mut w, &quests, RADIUS, "held", &b).expect("Buyback");
        // Vollverschmelzung: eingehende UUID aufgegeben und erfasst.
        assert!(w.sell_history["held"].is_empty());
        assert!(
            w.item_lifecycle["held"].contains(&uuid),
            "retired_uuid ist abgekoppelt"
        );
        assert_eq!(w.players["held"].inventory.count_of("hp_potion"), 8);
    }

    #[test]
    fn buyback_rejects_consumed_unknown_poor_and_full() {
        let (mut w, quests) = world();
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &potion_def(), 5);
        }
        let uuid = w.players["held"].inventory.base_slots[0]
            .as_ref()
            .unwrap()
            .item_uuid
            .clone();
        let mut r = req("npc_5", TradeAction::Sell);
        r.item_uuid = uuid.clone();
        r.count = 5;
        attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Verkauf");
        let mut b = req("npc_5", TradeAction::Buyback);
        b.history_id = "unbekannt".into();
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &b),
            Err(TradeReject::HistoryExpired)
        );
        // Zu wenig Gold.
        w.players.get_mut("held").unwrap().idia = 0;
        b.history_id = uuid.clone();
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &b),
            Err(TradeReject::InsufficientIdia)
        );
        w.players.get_mut("held").unwrap().idia = 1000;
        // Volles Inventar (Eintrag bleibt).
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &sword_def(), 8);
        }
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &b),
            Err(TradeReject::InventoryFull)
        );
        assert_eq!(w.sell_history["held"].len(), 1, "Eintrag bleibt erhalten");
        // Erfolgreich, danach ist der Eintrag verbraucht.
        // Platz schaffen: Schwerter entfernen (Testhilfe, kein Handel).
        {
            let p = w.players.get_mut("held").unwrap();
            p.inventory.base_slots.iter_mut().for_each(|s| *s = None);
        }
        let out = attempt_trade(&mut w, &quests, RADIUS, "held", &b).expect("Buyback");
        assert_eq!(out.item_uuid, uuid);
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &b),
            Err(TradeReject::HistoryExpired),
            "verbrauchter Eintrag wirkt nicht erneut"
        );
    }

    #[test]
    fn history_fifo_displaces_oldest_and_keeps_duties() {
        let (mut w, quests) = world();
        let mut def = potion_def();
        def.max_stack = 100;
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &def, 100);
        }
        // 21 Teile zu je 1 Stück verkaufen (neue UUID je Teil).
        for _ in 0..21 {
            let uuid = w.players["held"].inventory.base_slots[0]
                .as_ref()
                .unwrap()
                .item_uuid
                .clone();
            // Rest-UUID ist stabil; der entnommene Teil bekommt eine neue.
            let mut r = req("npc_5", TradeAction::Sell);
            r.item_uuid = uuid;
            r.count = 1;
            attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Verkauf");
        }
        let h = &w.sell_history["held"];
        assert_eq!(h.len(), 20, "FIFO cappt bei 20");
        let uuids: Vec<String> = h.iter().map(|e| e.item_uuid().to_string()).collect();
        let uniq: HashSet<String> = uuids.iter().cloned().collect();
        assert_eq!(uniq.len(), 20, "History-UUIDs eindeutig");
        let lc = &w.item_lifecycle["held"];
        assert_eq!(lc.len(), 21, "alle Abkopplungen bleiben Pflichten");
        for u in &uuids {
            assert!(lc.contains(u));
        }
    }

    #[test]
    fn history_uuids_stay_unique_across_buyback_cycles() {
        let (mut w, quests) = world();
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &potion_def(), 5);
        }
        let uuid = w.players["held"].inventory.base_slots[0]
            .as_ref()
            .unwrap()
            .item_uuid
            .clone();
        for _ in 0..3 {
            let mut r = req("npc_5", TradeAction::Sell);
            r.item_uuid = uuid.clone();
            r.count = 5;
            // Nach dem ersten Verkauf ist die UUID weg; nur der erste
            // Durchlauf verkauft, danach Buyback und erneut.
            if w.players["held"].inventory.count_of("hp_potion") == 0 {
                let mut b = req("npc_5", TradeAction::Buyback);
                b.history_id = uuid.clone();
                attempt_trade(&mut w, &quests, RADIUS, "held", &b).expect("Buyback");
            } else {
                attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Verkauf");
            }
            let h = &w.sell_history["held"];
            let uuids: Vec<&str> = h.iter().map(|e| e.item_uuid()).collect();
            let uniq: HashSet<&str> = uuids.iter().copied().collect();
            assert_eq!(uuids.len(), uniq.len(), "keine doppelten History-UUIDs");
        }
    }

    #[test]
    fn active_quest_items_cover_collect_and_deliver_only() {
        let (_, mut quests) = world();
        quests.register(quest_def()).unwrap();
        let (mut w, _) = world();
        // Ohne Questzustand: nichts geschützt.
        assert!(active_quest_item_ids(&w.players["held"], &quests)
            .expect("ohne ACTIVE auflösbar")
            .is_empty());
        set_quest_state(&mut w, "q_relic", QuestState::Active);
        let ids = active_quest_item_ids(&w.players["held"], &quests)
            .expect("registrierte ACTIVE-Quest auflösbar");
        assert!(ids.contains("quest_relic"), "Collect-Ziel geschützt");
        assert!(!ids.contains("wolf"), "Kill-Ziel ohne Itembezug");
        // COMPLETED/FAILED heben den Schutz auf.
        set_quest_state(&mut w, "q_relic", QuestState::Completed);
        assert!(active_quest_item_ids(&w.players["held"], &quests)
            .expect("abgeschlossene Quest auflösbar")
            .is_empty());
    }

    #[test]
    fn unresolvable_active_quest_rejects_sale_without_mutation() {
        // ACTIVE Quest ohne registrierte Definition: Questschutz nicht
        // prüfbar → Verkauf fail-closed ablehnen (kein fail-open).
        let (mut w, quests) = world();
        set_quest_state(&mut w, "q_unbekannt", QuestState::Active);
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &potion_def(), 5);
        }
        let uuid = w.players["held"].inventory.base_slots[0]
            .as_ref()
            .unwrap()
            .item_uuid
            .clone();
        let before_idia = w.players["held"].idia;
        let before_gen = w.players["held"].persist_generation;
        let mut r = req("npc_5", TradeAction::Sell);
        r.item_uuid = uuid;
        r.count = 5;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &r),
            Err(TradeReject::QuestDataUnavailable)
        );
        // Vollständig unverändert: Inventar, Idia, History, Lifecycle, Dirty.
        let p = &w.players["held"];
        assert_eq!(p.inventory.count_of("hp_potion"), 5);
        assert_eq!(p.idia, before_idia);
        assert!(!p.dirty.any());
        assert_eq!(p.persist_generation, before_gen);
        assert!(w.sell_history.get("held").map(|h| h.is_empty()).unwrap_or(true));
        assert!(w.item_lifecycle.get("held").map(|l| l.is_empty()).unwrap_or(true));
    }

    #[test]
    fn resolvable_quest_without_item_protection_allows_sale() {
        // ACTIVE, vollständig auflösbar, aber das verkaufte Item ist kein
        // Questziel → Verkauf gemäß übrigen Regeln erlaubt.
        let (mut w, mut quests) = world();
        quests.register(quest_def()).unwrap();
        set_quest_state(&mut w, "q_relic", QuestState::Active);
        {
            let p = w.players.get_mut("held").unwrap();
            give(&mut p.inventory, &potion_def(), 5);
        }
        let uuid = w.players["held"].inventory.base_slots[0]
            .as_ref()
            .unwrap()
            .item_uuid
            .clone();
        let mut r = req("npc_5", TradeAction::Sell);
        r.item_uuid = uuid;
        r.count = 5;
        let out = attempt_trade(&mut w, &quests, RADIUS, "held", &r)
            .expect("auflösbar ohne Schutz → verkäuflich");
        assert_eq!(out.total_price, 20);
        assert_eq!(w.players["held"].idia, 1020);
    }

    #[test]
    fn repeated_valid_intents_are_further_operations() {
        let (mut w, quests) = world();
        // Gleicher Kauf zweimal: zwei Operationen (kein Dedup).
        let mut r = req("npc_5", TradeAction::Buy);
        r.item_id = "hp_potion".into();
        r.count = 2;
        attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Kauf 1");
        attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("Kauf 2");
        assert_eq!(w.players["held"].idia, 1000 - 40);
        assert_eq!(w.players["held"].inventory.count_of("hp_potion"), 4);
        // Verbrauchter Buyback wirkt nicht erneut (HistoryExpired statt Effekt).
        {
            let p = w.players.get_mut("held").unwrap();
            p.inventory.base_slots.iter_mut().for_each(|s| *s = None);
            give(&mut p.inventory, &potion_def(), 2);
        }
        let uuid = w.players["held"]
            .inventory
            .base_slots
            .iter()
            .filter_map(|s| s.as_ref())
            .find(|it| it.item_id == "hp_potion")
            .unwrap()
            .item_uuid
            .clone();
        let mut s = req("npc_5", TradeAction::Sell);
        s.item_uuid = uuid.clone();
        s.count = 2;
        attempt_trade(&mut w, &quests, RADIUS, "held", &s).expect("Verkauf");
        let mut b = req("npc_5", TradeAction::Buyback);
        b.history_id = uuid.clone();
        attempt_trade(&mut w, &quests, RADIUS, "held", &b).expect("Buyback 1");
        let idia_after = w.players["held"].idia;
        assert_eq!(
            attempt_trade(&mut w, &quests, RADIUS, "held", &b),
            Err(TradeReject::HistoryExpired)
        );
        assert_eq!(w.players["held"].idia, idia_after, "kein zweiter Effekt");
    }

    #[test]
    fn reject_reasons_are_stable() {
        assert_eq!(TradeReject::OutOfRange.reason(), "out_of_range");
        assert_eq!(TradeReject::BoundItem.reason(), "bound_item");
        assert_eq!(
            TradeReject::QuestItemProtected.reason(),
            "quest_item_protected"
        );
        assert_eq!(TradeReject::HistoryExpired.reason(), "history_expired");
        assert_eq!(TradeReject::InsufficientIdia.reason(), "insufficient_idia");
        assert_eq!(TradeReject::InventoryFull.reason(), "inventory_full");
    }

    #[test]
    fn success_payload_carries_result_state() {
        let (mut w, quests) = world();
        let out = attempt_trade(
            &mut w,
            &quests,
            RADIUS,
            "held",
            &req("npc_5", TradeAction::Open),
        )
        .expect("open");
        let v = success_payload(&out);
        assert_eq!(v["ok"], true);
        assert_eq!(v["action"], "open");
        assert_eq!(v["offers"].as_array().unwrap().len(), 4);
        assert_eq!(v["history"].as_array().unwrap().len(), 0);
        assert_eq!(v["idia"], 1000);
        let mut r = req("npc_5", TradeAction::Buy);
        r.item_id = "hp_potion".into();
        r.count = 1;
        let out = attempt_trade(&mut w, &quests, RADIUS, "held", &r).expect("buy");
        let v = success_payload(&out);
        assert_eq!(v["ok"], true);
        assert_eq!(v["item_id"], "hp_potion");
        assert_eq!(v["total_price"], 10);
        assert!(v.get("item_uuid").is_none(), "Kauf ohne UUID-Feld");
    }
}
