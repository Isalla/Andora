// persist — Zentrale Spieler-Persistenz Stufe B (docs/Player_Persistenz.md).
//
// Stufe B (§20–§42): vollständiger Player-Snapshot, kanonische Währung
// `idia`, persistierte `persist_revision` (§29) und JSON-Spool-Durability
// (§21/§38) — der zentrale Player-Persistenzpfad schreibt NUR noch über den
// Spool (Datei-Batch mit Durable-Write), der DB-Drain wendet die Snapshots
// in EINER Transaktion an (docs §30). RAM-Dirty-/Generation-Race-Schutz
// (§15) und Retry-/DEGRADED-Verhalten (§16) bleiben erhalten.
//
// Komponentenmodell: Position, Progression, Idia, Inventory, Resources.
// Sobald IRGENDEINE Komponente dirty ist, wird EIN VOLLständiger Snapshot
// erfasst (docs §42 lässt die Gewichtung offen): Position, Progression,
// Idia, Inventar/Equipment (ohne Sicherheits-Puffer), aktuelle HP/Mana,
// Attribute, Klasse/Fraktions-Hook, Weapon-Skill und gelernte Fähigkeiten.
// Questzustände, Cooldowns, Effekte, Sell-/Buyback-History und der
// temporäre Sicherheits-Puffer sind bewusst NICHT Teil des Snapshots
// (Quest bleibt atomar über den DB-Guard, docs/Quest-System.md §27.26).
//
// Grundprinzip (§15): konsistenten Snapshot unter der World-Sperre erfassen
// (inkl. Generation + Revision), Sperre freigeben, Durable-Write
// ausschließlich außerhalb der Sperre, danach erneut sperren: Die
// `persist_revision` wird bei JEDEM Erfolg weitergeschrieben (§39),
// die Dirty-Flags NUR zurückgesetzt, wenn die Generation unverändert ist
// (kein neuerer RAM-Zustand während des Writes entstanden).
use sqlx::{MySql, Pool};

use serde::{Deserialize, Serialize};

use crate::inventory::InventoryState;
use crate::world::{Player, Shared};

pub(crate) type NormalPublication = std::sync::Arc<Vec<PersistSnapshot>>;

/// Reserve before invoking any writer. The World owns the immutable snapshots,
/// not the saving future: cancellation cannot release a revision for new data.
fn reserve_normal(
    world: &mut crate::world::World,
    snapshots: Vec<PersistSnapshot>,
) -> Result<NormalPublication, String> {
    let mut ids = std::collections::BTreeSet::new();
    for s in &snapshots {
        if !ids.insert(&s.player_id) || world.normal_publications.contains_key(&s.player_id) {
            return Err("normal publication participant conflict".into());
        }
    }
    let publication = std::sync::Arc::new(snapshots);
    for s in publication.iter() {
        world
            .normal_publications
            .insert(s.player_id.clone(), publication.clone());
    }
    Ok(publication)
}

/// Caller holds ALL participant gates. Repeated writes use the same complete
/// batch, including capture time. The reservation is removed only after RAM
/// revision/dirty handoff, under the same World lock as that handoff.
async fn finish_normal_publication<W, F>(
    shared: &Shared,
    publication: NormalPublication,
    write: W,
) -> Result<(), String>
where
    W: FnOnce(Vec<PersistSnapshot>) -> F,
    F: std::future::Future<Output = Result<(), String>>,
{
    write(publication.as_ref().clone()).await?;
    let mut world = shared.lock().await;
    for s in publication.iter() {
        if !world.players.contains_key(&s.player_id)
            || !world
                .normal_publications
                .get(&s.player_id)
                .is_some_and(|p| std::sync::Arc::ptr_eq(p, &publication))
        {
            return Err("normal publication handoff conflict".into());
        }
    }
    for s in publication.iter() {
        let p = world
            .players
            .get_mut(&s.player_id)
            .ok_or("normal publication player lost")?;
        p.persist_revision = p.persist_revision.max(s.persist_revision);
        if p.persist_generation == s.generation {
            p.dirty.clear_components(s.dirty);
        }
        world.normal_publications.remove(&s.player_id);
    }
    Ok(())
}

/// Resume matching groups independently. No single gate is held while acquiring
/// a group's gates; stale handles are rechecked after stable-order acquisition.
pub(crate) async fn retry_normal_publications(
    spool: &crate::spool::Spool,
    shared: &Shared,
    requested: Option<&[String]>,
) -> Result<(), String> {
    let publications = {
        let world = shared.lock().await;
        let mut groups: Vec<NormalPublication> = Vec::new();
        for (id, p) in &world.normal_publications {
            if requested.is_none_or(|ids| ids.contains(id))
                && !groups.iter().any(|g| std::sync::Arc::ptr_eq(g, p))
            {
                groups.push(p.clone());
            }
        }
        groups
    };
    let mut errors = Vec::new();
    for publication in publications {
        let mut ids: Vec<_> = publication.iter().map(|s| s.player_id.clone()).collect();
        ids.sort();
        let mut guards = Vec::new();
        for id in &ids {
            guards.push(spool.player_gate(id).await.lock_owned().await);
        }
        let still_reserved = {
            let world = shared.lock().await;
            publication.iter().all(|s| {
                world
                    .normal_publications
                    .get(&s.player_id)
                    .is_some_and(|p| std::sync::Arc::ptr_eq(p, &publication))
            })
        };
        if !still_reserved {
            continue;
        }
        let mut allowed = true;
        for snapshot in publication.iter() {
            match spool.normal_publication_allowed(snapshot) {
                Ok(true) => {}
                Ok(false) => {
                    allowed = false;
                    errors.push("normal publication trade dependency pending".to_string());
                }
                Err(e) => {
                    allowed = false;
                    errors.push(e);
                }
            }
        }
        if allowed {
            if let Err(e) = finish_normal_publication(shared, publication, |snapshots| async move {
                spool.write_batch_run(snapshots).map(|_| ())
            })
            .await
            {
                errors.push(e);
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Internal persistence intent, not a trade dialog/offer or an authorization
/// API. The future game layer must validate binding/quest/ownership policy.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeCommitRequest {
    pub commit_id: String,
    pub characters: [String; 2],
    /// Absolute resulting balances; their sum must be conserved.
    pub idia: [i64; 2],
    pub transfers: Vec<TradeTransferRequest>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradeTransferRequest {
    pub source: String,
    pub item_uuid: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradeCharacter {
    pub base_revision: i64,
    /// RAM source witnesses also cover items acquired since the last save.
    pub before_inventory: InventoryState,
    pub before_idia: i64,
    pub snapshot: PersistSnapshot,
}

/// Source witness plus the actual split/transfer and merge outcome. No Sold
/// detachment is created by ownership transfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradeTransfer {
    pub source: String,
    pub destination: String,
    pub original: crate::item::ItemInstance,
    pub moved: crate::item::ItemInstance,
    pub retired_uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradeArtifact {
    pub kind: String,
    pub format_version: u16,
    pub commit_id: String,
    pub characters: [TradeCharacter; 2],
    pub transfers: Vec<TradeTransfer>,
}

#[derive(Clone)]
pub struct PreparedTrade {
    pub request: TradeCommitRequest,
    pub artifact: TradeArtifact,
    pub lifecycle: [crate::item_lifecycle::ItemLifecycle; 2],
}

impl TradeArtifact {
    pub(crate) fn commit_payload(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|e| format!("trade proof serialize: {e}"))
    }
    pub(crate) fn matches_request(&self, request: &TradeCommitRequest) -> bool {
        self.commit_id == request.commit_id
            && request.characters.iter().enumerate().all(|(i, id)| {
                self.characters
                    .iter()
                    .any(|c| &c.snapshot.player_id == id && c.snapshot.idia == request.idia[i])
            })
            && self.transfers.len() == request.transfers.len()
            && self.transfers.iter().zip(&request.transfers).all(|(t, r)| {
                t.source == r.source
                    && t.original.item_uuid == r.item_uuid
                    && t.moved.count == r.count
            })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.kind != "two_character_trade"
            || self.format_version != 1
            || self.commit_id.is_empty()
            || self.commit_id.len() > 64
            || !self
                .commit_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("invalid trade envelope".into());
        }
        let ids = self
            .characters
            .each_ref()
            .map(|c| c.snapshot.player_id.as_str());
        if ids[0] >= ids[1] {
            return Err("trade needs two canonically ordered distinct characters".into());
        }
        let mut uuids = std::collections::BTreeSet::new();
        let mut before_owners = std::collections::BTreeMap::new();
        for c in &self.characters {
            crate::db::parse_character_id(&c.snapshot.player_id)?;
            if c.base_revision < 0
                || c.base_revision.checked_add(1) != Some(c.snapshot.persist_revision)
                || c.snapshot.idia < 0
                || c.snapshot.item_lifecycle.is_none()
            {
                return Err("invalid trade revision/state".into());
            }
            for it in persistent_instances(&c.snapshot.inventory) {
                if it.item_uuid.trim().is_empty()
                    || it.count <= 0
                    || !uuids.insert(it.item_uuid.clone())
                {
                    return Err("invalid or duplicate trade placement".into());
                }
            }
            for it in persistent_instances(&c.before_inventory) {
                if it.item_uuid.trim().is_empty()
                    || it.count <= 0
                    || before_owners
                        .insert(it.item_uuid.clone(), c.snapshot.player_id.clone())
                        .is_some()
                {
                    return Err("duplicate trade source placement".into());
                }
            }
        }
        if self
            .characters
            .iter()
            .map(|c| i128::from(c.before_idia))
            .sum::<i128>()
            != self
                .characters
                .iter()
                .map(|c| i128::from(c.snapshot.idia))
                .sum::<i128>()
            || self.characters.iter().any(|c| c.before_idia < 0)
        {
            return Err("trade currency not conserved".into());
        }
        let quantities =
            |before: bool| -> Result<std::collections::BTreeMap<String, i128>, String> {
                let mut out = std::collections::BTreeMap::new();
                for c in &self.characters {
                    let inv = if before {
                        &c.before_inventory
                    } else {
                        &c.snapshot.inventory
                    };
                    for it in persistent_instances(inv) {
                        let mut properties = it.clone();
                        properties.item_uuid.clear();
                        properties.count = 0;
                        let key = serde_json::to_string(&properties).map_err(|e| e.to_string())?;
                        *out.entry(key).or_insert(0) += i128::from(it.count);
                    }
                }
                Ok(out)
            };
        if quantities(true)? != quantities(false)? {
            return Err("trade items not conserved".into());
        }
        let mut sources = std::collections::BTreeSet::new();
        let mut moved = std::collections::BTreeSet::new();
        for t in &self.transfers {
            if !ids.contains(&t.source.as_str())
                || !ids.contains(&t.destination.as_str())
                || t.source == t.destination
                || t.moved.count <= 0
                || t.moved.count > t.original.count
                || t.original.item_uuid.trim().is_empty()
                || !sources.insert(t.original.item_uuid.clone())
                || !moved.insert(t.moved.item_uuid.clone())
                || (t.original.count == t.moved.count)
                    != (t.original.item_uuid == t.moved.item_uuid)
                || t.retired_uuid
                    .as_ref()
                    .is_some_and(|u| u != &t.moved.item_uuid)
            {
                return Err("invalid trade transfer witness".into());
            }
            let mut expected = t.original.clone();
            expected.item_uuid = t.moved.item_uuid.clone();
            expected.count = t.moved.count;
            if expected != t.moved {
                return Err("trade changed instance properties".into());
            }
            let source = self
                .characters
                .iter()
                .find(|c| c.snapshot.player_id == t.source)
                .ok_or("missing source")?;
            if !persistent_instances(&source.before_inventory)
                .iter()
                .any(|it| **it == t.original)
            {
                return Err("transfer source witness missing".into());
            }
            if t.moved.count < t.original.count {
                let mut rest = t.original.clone();
                rest.count -= t.moved.count;
                if !persistent_instances(&source.snapshot.inventory)
                    .iter()
                    .any(|it| **it == rest)
                {
                    return Err("split remainder changed".into());
                }
            }
            let destination = &self
                .characters
                .iter()
                .find(|c| c.snapshot.player_id == t.destination)
                .ok_or("missing destination")?
                .snapshot
                .inventory;
            if t.retired_uuid.is_some() {
                if uuids.contains(&t.moved.item_uuid) {
                    return Err("retired trade UUID still placed".into());
                }
            } else if !persistent_instances(destination).iter().any(|it| {
                let mut remainder = t.moved.clone();
                remainder.count = it.count;
                **it == remainder && it.count <= t.moved.count
            }) {
                return Err("transferred UUID missing from destination".into());
            }
        }
        for c in &self.characters {
            for it in persistent_instances(&c.snapshot.inventory) {
                if before_owners.get(&it.item_uuid) == Some(&c.snapshot.player_id) {
                    continue;
                }
                if !self.transfers.iter().any(|t| {
                    t.moved.item_uuid == it.item_uuid
                        && t.destination == c.snapshot.player_id
                        && t.retired_uuid.is_none()
                }) {
                    return Err("unwitnessed UUID ownership change".into());
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn persistent_instances(inv: &InventoryState) -> Vec<&crate::item::ItemInstance> {
    inv.base_slots
        .iter()
        .chain(inv.bags.iter().flat_map(|b| &b.slots))
        .filter_map(|s| s.as_ref())
        .chain(inv.equipped.values())
        .collect()
}

fn prepare_trade(
    world: &crate::world::World,
    req: &TradeCommitRequest,
) -> Result<PreparedTrade, String> {
    if req.characters[0] == req.characters[1] {
        return Err("trade needs distinct characters".into());
    }
    let players = req.characters.each_ref().map(|id| world.players.get(id));
    let [Some(a), Some(b)] = players else {
        return Err("trade character unavailable".into());
    };
    if req
        .characters
        .iter()
        .any(|id| !world.economic_mutation_allowed(id))
    {
        return Err("trade commit pending".into());
    }
    if req.idia.iter().any(|n| *n < 0)
        || a.idia < 0
        || b.idia < 0
        || i128::from(a.idia) + i128::from(b.idia)
            != i128::from(req.idia[0]) + i128::from(req.idia[1])
    {
        return Err("trade currency not conserved".into());
    }
    let mut inventories = [a.inventory.clone(), b.inventory.clone()];
    for inventory in &inventories {
        for it in persistent_instances(inventory) {
            let def = world
                .item_definitions
                .get(&it.item_id)
                .ok_or("trade item definition unavailable")?;
            def.validate()
                .map_err(|e| format!("trade definition: {e}"))?;
            it.validate(def)
                .map_err(|e| format!("trade instance: {e}"))?;
        }
    }
    let mut lifecycle = req
        .characters
        .each_ref()
        .map(|id| world.item_lifecycle.get(id).cloned().unwrap_or_default());
    let mut taken = Vec::new();
    let mut sources = std::collections::BTreeSet::new();
    for r in &req.transfers {
        let i = req
            .characters
            .iter()
            .position(|id| id == &r.source)
            .ok_or("invalid transfer source")?;
        if !sources.insert(r.item_uuid.clone()) {
            return Err("duplicate transfer source".into());
        }
        let original = persistent_instances(&inventories[i])
            .into_iter()
            .find(|it| it.item_uuid == r.item_uuid)
            .ok_or("transfer source unavailable")?
            .clone();
        let occurrences = world
            .players
            .values()
            .map(|p| {
                persistent_instances(&p.inventory)
                    .into_iter()
                    .filter(|it| it.item_uuid == r.item_uuid)
                    .count()
                    + p.inventory
                        .buffer
                        .iter()
                        .flatten()
                        .filter(|it| it.item_uuid == r.item_uuid)
                        .count()
            })
            .sum::<usize>();
        if occurrences != 1 {
            return Err("global UUID placement conflict".into());
        }
        let moved = inventories[i]
            .try_take_instance(&r.item_uuid, r.count)
            .map_err(|e| format!("trade take: {e:?}"))?;
        taken.push((i, original, moved));
    }
    let mut transfers = Vec::new();
    for (i, original, moved) in taken {
        let j = 1 - i;
        let def = world
            .item_definitions
            .get(&moved.item_id)
            .ok_or("trade item definition unavailable")?;
        let outcome = inventories[j]
            .try_insert_instance(def, &moved)
            .map_err(|e| format!("trade insert: {e:?}"))?;
        // Transfer cancels old-owner obligations; only a real merge retires UUID.
        crate::item_lifecycle::reconcile_transfer(&mut lifecycle[i], &moved.item_uuid);
        crate::item_lifecycle::reconcile_transfer(&mut lifecycle[j], &moved.item_uuid);
        crate::item_lifecycle::reconcile_after_insert(
            &mut lifecycle[j],
            &inventories[j].persistent_uuids(),
            &moved.item_uuid,
            outcome.retired_uuid.as_deref(),
            &world.runtime_id,
            now_ms(),
        );
        transfers.push(TradeTransfer {
            source: req.characters[i].clone(),
            destination: req.characters[j].clone(),
            original,
            moved,
            retired_uuid: outcome.retired_uuid,
        });
    }
    let mut characters = Vec::new();
    for (i, p) in [a, b].into_iter().enumerate() {
        let revision = p
            .persist_revision
            .checked_add(1)
            .ok_or("trade revision exhausted")?;
        let mut snap = build_snapshot(p, true, &lifecycle[i], &world.runtime_id)?
            .ok_or("missing trade snapshot")?;
        snap.persist_revision = revision;
        snap.inventory = inventories[i].clone();
        snap.idia = req.idia[i];
        snap.item_lifecycle = Some(lifecycle[i].snapshot_view(revision, &world.runtime_id));
        characters.push((
            TradeCharacter {
                base_revision: p.persist_revision,
                before_inventory: p.inventory.clone(),
                before_idia: p.idia,
                snapshot: snap,
            },
            lifecycle[i].clone(),
        ));
    }
    characters.sort_by(|a, b| a.0.snapshot.player_id.cmp(&b.0.snapshot.player_id));
    let [(a, la), (b, lb)] = characters.try_into().map_err(|_| "trade pair size")?;
    let artifact = TradeArtifact {
        kind: "two_character_trade".into(),
        format_version: 1,
        commit_id: req.commit_id.clone(),
        characters: [a, b],
        transfers,
    };
    artifact.validate()?;
    Ok(PreparedTrade {
        request: req.clone(),
        artifact,
        lifecycle: [la, lb],
    })
}

/// The only success boundary is confirmed spool durability, not a RAM swap.
/// A failed/uncertain write retains the exact prepared data for the same ID.
pub async fn commit_trade(
    spool: &crate::spool::Spool,
    shared: &Shared,
    request: TradeCommitRequest,
) -> Result<(), String> {
    commit_trade_with(spool, shared, request, |artifact| async move {
        spool.write_trade(&artifact)
    })
    .await
}

/// Produktionseintrag mit Dialog-Nachprüfung (Spielerhandel): `validate`
/// läuft unter den erworbenen Gates und derselben Sperre wie die
/// Vorbereitung (siehe `commit_trade_with_validation`); die Veröffentlichung
/// ist der bestehende Spool-Write. Keine zweite Pipeline.
pub async fn commit_validated_trade<V>(
    spool: &crate::spool::Spool,
    shared: &Shared,
    request: TradeCommitRequest,
    validate: V,
) -> Result<(), String>
where
    V: FnOnce(&crate::world::World) -> Result<(), String>,
{
    commit_trade_with_validation(spool, shared, request, validate, |artifact| async move {
        spool.write_trade(&artifact)
    })
    .await
}

pub(crate) async fn commit_trade_with<W, F>(
    spool: &crate::spool::Spool,
    shared: &Shared,
    request: TradeCommitRequest,
    publish: W,
) -> Result<(), String>
where
    W: FnOnce(TradeArtifact) -> F,
    F: std::future::Future<Output = Result<(), String>>,
{
    commit_trade_with_validation(spool, shared, request, |_| Ok(()), publish).await
}

/// Commit mit zusätzlicher verbindlicher Nachprüfung unmittelbar vor der
/// Vorbereitung: `validate` läuft unter den erworbenen Charakter-Gates und
/// derselben World-Sperre, unter der `prepare_trade` die verbindlichen
/// Nachzustände baut — ohne Await und ohne Freigeben der Sperre dazwischen.
/// Aufrufer mit Dialogzustand (Spielerhandel) prüfen hier Version,
/// Bestätigungen, Eigentümer, Alive-/Reichweitenstand, Bindung und
/// Questschutz erneut; Inventarmechanik (Eigentumsplatzierung, Mengen,
/// Idia-Erhalt, Kapazität, Split/Merge) prüft `prepare_trade` atomar in
/// derselben Sperre. Bereits retainede Vorbereitungen (Pending-Wiederholung)
/// werden nicht erneut validiert: nach verbindlicher Vorbereitung bleibt die
/// bestehende Pending-/Recovery-Pflicht unverändert maßgeblich.
pub(crate) async fn commit_trade_with_validation<W, F, V>(
    spool: &crate::spool::Spool,
    shared: &Shared,
    request: TradeCommitRequest,
    validate: V,
    publish: W,
) -> Result<(), String>
where
    V: FnOnce(&crate::world::World) -> Result<(), String>,
    W: FnOnce(TradeArtifact) -> F,
    F: std::future::Future<Output = Result<(), String>>,
{
    retry_normal_publications(spool, shared, Some(&request.characters)).await?;
    let mut ids = request.characters.clone();
    ids.sort();
    if ids[0] == ids[1] {
        return Err("trade needs distinct characters".into());
    }
    let _a = spool.player_gate(&ids[0]).await.lock_owned().await;
    let _b = spool.player_gate(&ids[1]).await.lock_owned().await;
    if let Some(receipt) = spool.trade_receipt(&request.commit_id)? {
        return if receipt.matches_request(&request) {
            Ok(())
        } else {
            Err("trade commit ID conflict".into())
        };
    }
    let retained = {
        let world = shared.lock().await;
        if let Some(receipt) = world.trade_receipts.get(&request.commit_id) {
            return if receipt == &request {
                Ok(())
            } else {
                Err("trade commit ID conflict".into())
            };
        }
        if let Some(prepared) = world.prepared_trades.get(&request.commit_id) {
            if prepared.request != request {
                return Err("trade commit ID conflict".into());
            }
            Some(prepared.clone())
        } else {
            None
        }
    };
    let prepared = match retained {
        Some(prepared) => prepared,
        None => {
            // Filesystem availability checks stay outside the World lock.
            for id in &ids {
                spool.ensure_trade_save_allowed(id)?;
            }
            let mut world = shared.lock().await;
            if ids
                .iter()
                .any(|id| world.normal_publications.contains_key(id))
            {
                return Err("normal publication pending".into());
            }
            if world.prepared_trades.contains_key(&request.commit_id)
                || world.trade_receipts.contains_key(&request.commit_id)
            {
                return Err("trade commit ID conflict".into());
            }
            // Verbindliche Nachprüfung (siehe `commit_trade_with_validation`):
            // kein Await, keine Sperrfreigabe bis einschließlich Vorbereitung.
            validate(&world)?;
            let prepared = prepare_trade(&world, &request)?;
            world
                .prepared_trades
                .insert(request.commit_id.clone(), prepared.clone());
            spool.hold_trade_publication(&request.commit_id);
            prepared
        }
    };
    publish(prepared.artifact.clone()).await?;
    let mut world = shared.lock().await;
    if prepared
        .artifact
        .characters
        .iter()
        .any(|c| !world.players.contains_key(&c.snapshot.player_id))
    {
        return Err("prepared trade player lost".into());
    }
    for lifecycle in world.item_lifecycle.values_mut() {
        for transfer in &prepared.artifact.transfers {
            crate::item_lifecycle::reconcile_transfer(lifecycle, &transfer.moved.item_uuid);
        }
    }
    // Registry writers are serialized by both gates. Copy only economic fields.
    for (i, c) in prepared.artifact.characters.iter().enumerate() {
        let p = world
            .players
            .get_mut(&c.snapshot.player_id)
            .ok_or("prepared trade player lost")?;
        let unchanged = p.persist_generation == c.snapshot.generation;
        p.inventory = c.snapshot.inventory.clone();
        p.idia = c.snapshot.idia;
        p.mark_dirty(PersistComponent::Inventory);
        p.mark_dirty(PersistComponent::Idia);
        p.persist_revision = c.snapshot.persist_revision;
        if unchanged {
            let mut saved = c.snapshot.dirty;
            saved.mark(PersistComponent::Inventory);
            saved.mark(PersistComponent::Idia);
            p.dirty.clear_components(saved);
        }
        world
            .item_lifecycle
            .insert(c.snapshot.player_id.clone(), prepared.lifecycle[i].clone());
    }
    world.prepared_trades.remove(&request.commit_id);
    world
        .trade_receipts
        .insert(request.commit_id.clone(), request.clone());
    spool.release_trade_publication(&request.commit_id);
    Ok(())
}

/// Die fünf Komponenten des Player-Dirty-State (Stufe B, docs
/// Player_Persistenz.md §6/§42). Der Snapshot selbst ist vollständig;
/// die Komponenten steuern nur, WANN persistiert wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistComponent {
    Position = 0,
    Progression = 1,
    Idia = 2,
    Inventory = 3,
    Resources = 4,
}

impl PersistComponent {
    /// Alle Komponenten in fester Reihenfolge (Iterations-/Testreihenfolge).
    pub const ALL: [PersistComponent; 5] = [
        PersistComponent::Position,
        PersistComponent::Progression,
        PersistComponent::Idia,
        PersistComponent::Inventory,
        PersistComponent::Resources,
    ];

    fn bit(self) -> u8 {
        1u8 << (self as u8)
    }
}

/// Komponentenspezifischer Dirty-State eines Spielers (docs §6): ein Bit je
/// persistenter Komponente. Der temporäre Sicherheits-Puffer des Inventars
/// ist flüchtiger Runtime-State und nie Teil dieses Flags
/// (docs/inventory_system.md §10/§11.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PersistDirty {
    bits: u8,
}

impl PersistDirty {
    pub fn mark(&mut self, component: PersistComponent) {
        self.bits |= component.bit();
    }

    pub fn clear(&mut self, component: PersistComponent) {
        self.bits &= !component.bit();
    }

    pub fn is_dirty(&self, component: PersistComponent) -> bool {
        self.bits & component.bit() != 0
    }

    pub fn any(&self) -> bool {
        self.bits != 0
    }

    /// Iteriert über genau die aktuell dirty Komponenten (feste Reihenfolge).
    pub fn iter(&self) -> impl Iterator<Item = PersistComponent> + use<'_> {
        PersistComponent::ALL
            .iter()
            .copied()
            .filter(|c| self.is_dirty(*c))
    }

    /// Entfernt genau die Komponenten von `other` (Post-Save-Cleanup).
    fn clear_components(&mut self, other: PersistDirty) {
        self.bits &= !other.bits;
    }
}

/// Vollständiger Player-Snapshot (Stufe B, docs §23/§42). Dieses Struct ist
/// zugleich das Drahtformat der Spool-Dateien (Serde). `generation` und
/// `dirty` sind reine RAM-Steuergrößen und werden nie serialisiert.
///
/// Sicherheitsregeln des Formats:
/// - `logout_at` fehlt bewusst (gehört ausschließlich zum finalen
///   Disconnect-Save, docs §23).
/// - Der Inventar-Sicherheits-Puffer fehlt (docs/inventory_system.md §11;
///   `InventoryState` serialisiert ihn nicht).
/// - Questzustände fehlen (atomarer Questabschluss über den DB-Guard).
/// - Abgeleitete Werte (max HP/Mana, Armor) fehlen — sie werden beim Laden
///   neu berechnet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistSnapshot {
    /// Rational id = DB-Charakter-ID; im Drahtformat als `character_id`.
    #[serde(rename = "character_id")]
    pub player_id: String,
    /// Persistenz-Revision dieses Snapshots = RAM-`persist_revision` + 1 (§29).
    pub persist_revision: i64,
    /// Erfassungszeitpunkt (Epoch-Millis).
    pub captured_at_ms: i64,
    pub x: f64,
    pub y: f64,
    pub level: u32,
    pub exp: i64,
    pub free_attr_points: u32,
    pub rested_pool: i64,
    pub idia: i64,
    pub hp: i32,
    pub mana: i32,
    pub attributes: crate::attributes::Attributes,
    pub char_class: String,
    pub faction_transition: bool,
    pub weapon_skill: u32,
    pub learned_abilities: Vec<String>,
    /// `P-18`: Laufende Ability-Cooldowns als **absolute Ablaufzeitpunkte** in
    /// **Millisekunden seit dem Unix-Epoch** (`ability_id` → `ready_at_ms`).
    ///
    /// - `Some(map)` = neuer Snapshot: die gespeicherte Map wird beim Anwenden
    ///   **vollständig ersetzt**; `Some({})` entfernt zuvor gespeicherte
    ///   Cooldowns bewusst.
    /// - `None` = **Altformat**: das Feld fehlt in der Datei. Solche Dateien
    ///   bleiben lesbar, und der gespeicherte Cooldown-Bestand bleibt dabei
    ///   **unberührt** (es wird nichts ersetzt).
    ///
    /// Die Zeitbasis ist die bestehende Server-Uhr (Wall-Clock); ein
    /// Uhrsprung kann die verbleibende Dauer verändern (docs/Player_Persistenz.md
    /// §23 „Cooldowns im Player-Snapshot").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldowns: Option<std::collections::BTreeMap<String, i64>>,
    pub inventory: InventoryState,
    /// Item-Lifecycle-Metadaten (docs/inventory_system.md §18, Migration
    /// 021): ausstehende UUID-Abkopplungen dieses Snapshots zur Zuordnung
    /// und kontrollierten Finalisierung im Drain.
    ///
    /// - `Some(view)` = neues Format: der Drain schreibt die Metadaten
    ///   vollständig neu und finalisiert zulässige Instanzen — in derselben
    ///   Transaktion wie Inventar, Idia und `persist_revision`. Eine bewusst
    ///   leere Sicht bereinigt zuvor gespeicherte Metadaten.
    /// - `None` = **Altformat**: das Feld fehlt in der Datei. Solche Dateien
    ///   bleiben lesbar; der Drain lässt Metadaten und Instanzen dabei
    ///   **unberührt** (kein Ersetzen, keine Löschung).
    ///
    /// Die Sell-/Buyback-History ist bewusst KEIN Teil des Snapshots
    /// (ausschließlich Runtime-State, docs/Handelssystem.md §2/§10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_lifecycle: Option<crate::item_lifecycle::ItemLifecycleSnapshot>,
    /// RAS-Steuergröße (§15), niemals serialisiert.
    #[serde(skip)]
    pub generation: u64,
    /// RAM-Dirty-Flags zum Snapshot-Zeitpunkt (Steuergröße, nie serialisiert).
    #[serde(skip)]
    pub dirty: PersistDirty,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Erzeugt den vollständigen Persistenz-Snapshot eines Spielers.
/// Liefert `None`, wenn der Spieler keinen Dirty-State besitzt UND keine
/// Erzwingung (`force`) verlangt ist (kein Write nötig). Die
/// Lifecycle-Sicht stempelt die Snapshot-Revision auf alle ausstehenden
/// Abkopplungen; unbestätigte Entfernungen bleiben dadurch in neueren
/// Snapshots erhalten, bis der DB-Commit bestätigt ist.
fn build_snapshot(
    player: &Player,
    force: bool,
    lifecycle: &crate::item_lifecycle::ItemLifecycle,
    runtime_id: &str,
) -> Result<Option<PersistSnapshot>, String> {
    let dirty = player.dirty;
    if !force && !dirty.any() {
        return Ok(None);
    }
    let revision = player
        .persist_revision
        .checked_add(1)
        .ok_or("persist revision exhausted")?;
    let mut learned: Vec<String> = player.learned_abilities.iter().cloned().collect();
    learned.sort();
    // `P-18`: Ablaufzeitpunkte in die persistierte Zeiteinheit umrechnen.
    // `BTreeMap` ist bereits nach `ability_id` sortiert → deterministisches
    // Drahtformat.
    let cooldowns: std::collections::BTreeMap<String, i64> = player
        .cooldowns
        .iter()
        .map(|(id, ready_at)| (id.clone(), crate::combat::cooldowns::to_epoch_ms(*ready_at)))
        .collect();
    Ok(Some(PersistSnapshot {
        player_id: player.id.clone(),
        persist_revision: revision,
        captured_at_ms: now_ms(),
        x: player.x,
        y: player.y,
        level: player.level,
        exp: player.exp,
        free_attr_points: player.free_attr_points,
        rested_pool: player.rested_pool,
        idia: player.idia,
        hp: player.hp,
        mana: player.mana,
        attributes: player.attributes,
        char_class: player.char_class.clone(),
        faction_transition: player.faction_transition,
        weapon_skill: player.weapon_skill,
        learned_abilities: learned,
        cooldowns: Some(cooldowns),
        inventory: player.inventory.clone(),
        item_lifecycle: Some(lifecycle.snapshot_view(revision, runtime_id)),
        generation: player.persist_generation,
        dirty,
    }))
}

/// Zentraler Player-Persistenzpfad (Stufe A-Skelett, Stufe B-Duck):
/// erfasst den vollständigen Snapshot eines Spielers unter der World-Sperre,
/// schreibt ihn über `write` (Produktion: Durable-Spool-Batch) AUSSERHALB der
/// Sperre und führt danach unter erneuter Sperre die §15-/§39-Regeln aus:
///
/// - Die RAM-`persist_revision` wird bei JEDEM Erfolg auf die Snapshot-
///   Revision gesetzt (§39: Revision folgt dem durable gespeicherten Stand) —
///   auch wenn eine neuere Generation während des Writes entstanden ist
///   (dann läuft der nächste Intervall erneut und erfasst den neueren Stand).
/// - Die Dirty-Flags werden NUR zurückgesetzt, wenn die Generation unverändert
///   geblieben ist (§15 Race-Regel).
///
/// Fehlerverhalten (docs §16): Realm wird nicht beendet, der Spieler bleibt
/// im autoritativen RAM, Dirty-State/Revision bleiben unverändert (späterer
/// Flush versucht erneut); der Fehler wird an den Aufrufer zurückgegeben.
pub async fn persist_dirty_into<W, F>(
    shared: &Shared,
    player_id: &str,
    force: bool,
    write: W,
) -> Result<(), String>
where
    W: FnOnce(PersistSnapshot) -> F,
    F: std::future::Future<Output = Result<(), String>>,
{
    // Phase 1: konsistenten Snapshot unter der Sperre erfassen, danach
    // Sperre sofort freigeben.
    let publication = {
        let mut world = shared.lock().await;
        if !world.economic_mutation_allowed(player_id) {
            return Err("trade commit publication pending".into());
        }
        if let Some(p) = world.normal_publications.get(player_id) {
            // A force-save must cover the CURRENT state, not acknowledge only
            // an older reserved snapshot. Its caller resolves groups first.
            if force || p.len() != 1 {
                return Err("normal publication pending".into());
            }
            p.clone()
        } else {
        let snapshot = match world.players.get(player_id) {
            Some(player) => {
                let empty_lifecycle = crate::item_lifecycle::ItemLifecycle::new();
                let lifecycle = world
                    .item_lifecycle
                    .get(player_id)
                    .unwrap_or(&empty_lifecycle);
                match build_snapshot(player, force, lifecycle, &world.runtime_id)? {
                    Some(snapshot) => snapshot,
                    None => return Ok(()), // nichts dirty (und nicht erzwungen) → kein Write
                }
            }
            None => return Ok(()), // Spieler offline → nichts zu flushen
        };
        reserve_normal(&mut world, vec![snapshot])?
        }
    };
    finish_normal_publication(shared, publication, |snapshots| write(snapshots[0].clone())).await
}

/// `P-12`/§35: **ein** Persistenzlauf über mehrere Spieler.
///
/// Phase 1 sammelt unter **einer** World-Sperre die Snapshots aller im Lauf
/// erfassten dirty Spieler. Ist die Menge leer, entsteht **keine** Datei und es
/// wird nichts reserviert. Phase 2 schreibt **eine** gemeinsame Batch-Datei und
/// gibt den reservierten Snapshot-Speicher beim Rückkehr aus `write` frei,
/// **ohne** auf die DB-Verarbeitung zu warten. Phase 3 nimmt Dirty-Bits
/// ausschließlich bei unveränderter Generation zurück; neuere Änderungen
/// bleiben dirty (§15/§39).
pub async fn persist_dirty_run<W, F>(
    shared: &Shared,
    player_ids: &[String],
    write: W,
) -> Result<u32, String>
where
    W: FnOnce(Vec<PersistSnapshot>) -> F,
    F: std::future::Future<Output = Result<(), String>>,
{
    // Phase 1: konsistente Snapshots aller dirty Spieler unter EINER Sperre.
    let publication = {
        let mut world = shared.lock().await;
        let existing = player_ids
            .iter()
            .find_map(|id| world.normal_publications.get(id))
            .cloned();
        if let Some(p) = existing {
            let ids: std::collections::BTreeSet<_> = player_ids.iter().collect();
            if ids.len() != p.len() || p.iter().any(|s| !ids.contains(&s.player_id)) {
                return Err("normal publication pending".into());
            }
            p
        } else {
        let mut snapshots = Vec::new();
        let empty_lifecycle = crate::item_lifecycle::ItemLifecycle::new();
        for id in player_ids {
            if !world.economic_mutation_allowed(id) {
                return Err("trade commit publication pending".into());
            }
            let Some(player) = world.players.get(id) else {
                continue; // Spieler offline → nichts zu flushen
            };
            let lifecycle = world.item_lifecycle.get(id).unwrap_or(&empty_lifecycle);
            let Some(snapshot) = build_snapshot(player, false, lifecycle, &world.runtime_id)?
            else {
                continue; // nicht dirty → kein Snapshot in diesem Lauf
            };
            snapshots.push(snapshot);
        }
        if snapshots.is_empty() { return Ok(0); }
        reserve_normal(&mut world, snapshots)?
        }
    };
    let count = publication.len() as u32;
    finish_normal_publication(shared, publication, write).await?;
    Ok(count)
}

/// Produktions-Einstiegspunkt des zentralen Player-Persistenzpfads (Stufe B):
/// persistiert `player_id` als vollständigen Durable-Spool-Batch
/// (docs/Player_Persistenz.md §21; ein Batch = eine Spieler-Snapshot-Datei).
///
/// `force=true` erzwingt den Snapshot auch ohne Dirty-State (finaler
/// Disconnect-/Shutdown-Flush, docs §42). Per-Spieler-Serialisierung über
/// `in_flight`-Gates verhindert konkurrierende Snapshots desselben Spielers
/// mit derselben Baseline-Revision (kein Last-Writer-Loses-Still).
pub async fn persist_player(
    spool: &crate::spool::Spool,
    shared: &Shared,
    player_id: &str,
    force: bool,
) -> Result<(), String> {
    retry_normal_publications(spool, shared, Some(&[player_id.to_string()])).await?;
    let _guard = spool.player_gate(player_id).await.lock_owned().await;
    spool.ensure_trade_save_allowed(player_id)?;
    let spool = spool.clone();
    persist_dirty_into(shared, player_id, force, move |snapshot| {
        let spool = spool.clone();
        async move { spool.write_batch(&snapshot) }
    })
    .await
}

/// Wendet einen vollständigen Snapshot in EINER MariaDB-Transaktion auf die
/// Charaktertabelle an (docs/Player_Persistenz.md §30, DB-Drain). Namens-
/// gemäß **Vollschreib** des Snapshots; `logout_at` wird bewusst NICHT
/// angefasst (gehört zum finalen Disconnect-Save). `persist_revision` wird
/// atomar MIT dem Snapshot geschrieben (keine Lücke zwischen Inhalt und
/// Revisionsstand). Der Status des Aufrufers (SEHR wichtig für Retention):
/// Der Aufrufer (spool::drain) vergleicht vorher DB-Revision vs.
/// Snapshot-Revision.
pub(crate) async fn apply_snapshot_to_db(
    pool: &Pool<MySql>,
    snapshot: &PersistSnapshot,
    weapon_skill_id: &str,
) -> Result<(), String> {
    let char_id = &snapshot.player_id;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Drain {char_id}: Transaktion beginnen: {e}"))?;

    write_snapshot_fields(&mut tx, snapshot, weapon_skill_id).await?;
    if let Some(lifecycle) = snapshot.item_lifecycle.as_ref() {
        crate::db::apply_item_lifecycle(&mut tx, char_id, lifecycle, &snapshot.inventory).await?;
    }
    crate::db::write_persist_revision(&mut tx, char_id, snapshot.persist_revision).await?;
    tx.commit()
        .await
        .map_err(|e| format!("Drain {char_id}: commit: {e}"))?;
    Ok(())
}

/// Common full-state writer; lifecycle and revision are deliberately separate
/// so the pair can establish BOTH inventories before any finalization.
async fn write_snapshot_fields(
    tx: &mut sqlx::Transaction<'_, MySql>,
    snapshot: &PersistSnapshot,
    weapon_skill_id: &str,
) -> Result<(), String> {
    let char_id = &snapshot.player_id;
    crate::db::write_position(tx, char_id, snapshot.x, snapshot.y).await?;
    crate::db::write_progression_fields(
        tx,
        char_id,
        snapshot.level,
        snapshot.exp,
        snapshot.free_attr_points,
        snapshot.rested_pool,
    )
    .await?;
    crate::db::write_idia(tx, char_id, snapshot.idia).await?;
    crate::db::write_resources(tx, char_id, snapshot.hp, snapshot.mana).await?;
    crate::db::write_attributes(tx, char_id, &snapshot.attributes).await?;
    crate::db::write_inventory(tx, char_id, &snapshot.inventory).await?;
    // Item-Lifecycle (docs/inventory_system.md §18, Migration 021): NUR bei
    // neuem Snapshot-Format. Altformat (`None`) lässt Metadaten und Instanzen
    // unberührt. Alles läuft in derselben Transaktion wie Inventar, Idia und
    // `persist_revision` — entweder wird alles committet oder nichts.
    let class = crate::class::ClassStatus::from_db_name(&snapshot.char_class);
    crate::db::write_character_class(tx, char_id, class, snapshot.faction_transition).await?;
    crate::db::write_weapon_skill(tx, char_id, weapon_skill_id, snapshot.weapon_skill).await?;
    crate::db::write_character_abilities(tx, char_id, &snapshot.learned_abilities).await?;
    // `P-18`: Cooldowns gehören zum normalen Snapshot und werden in derselben
    // Transaktion gespeichert. Nur ein **vorhandenes** Feld ersetzt den
    // Bestand; `None` = Altformat ohne Feld und lässt ihn unberührt.
    if let Some(cooldowns) = snapshot.cooldowns.as_ref() {
        crate::db::write_character_cooldowns(tx, char_id, cooldowns).await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TradeApply {
    Applied,
    AlreadyApplied,
    Waiting,
    Conflict,
}

/// Used by the real transaction, not a duplicate test decision model.
pub(crate) fn trade_revision_action(
    trade: &TradeArtifact,
    current: [Option<i64>; 2],
) -> TradeApply {
    trade_revision_action_with_proof(trade, current, None)
}

pub(crate) fn trade_revision_action_with_proof(
    trade: &TradeArtifact,
    current: [Option<i64>; 2],
    committed_payload: Option<&str>,
) -> TradeApply {
    let [Some(a), Some(b)] = current else {
        return TradeApply::Conflict;
    };
    let values = [a, b];
    if let Some(payload) = committed_payload {
        if trade.commit_payload().as_deref() != Ok(payload) {
            return TradeApply::Conflict;
        }
        return if trade
            .characters
            .iter()
            .zip(values)
            .all(|(c, r)| r >= c.snapshot.persist_revision)
        {
            TradeApply::AlreadyApplied
        } else {
            TradeApply::Conflict
        };
    }
    if trade
        .characters
        .iter()
        .zip(values)
        .any(|(c, r)| r >= c.snapshot.persist_revision)
    {
        return TradeApply::Conflict;
    }
    if trade
        .characters
        .iter()
        .zip(values)
        .all(|(c, r)| r == c.base_revision)
    {
        TradeApply::Applied
    } else {
        TradeApply::Waiting
    }
}

pub(crate) async fn apply_trade_to_db(
    pool: &Pool<MySql>,
    trade: &TradeArtifact,
    weapon_skill_id: &str,
) -> Result<TradeApply, String> {
    trade.validate()?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("trade begin: {e}"))?;
    let mut current = [None; 2];
    for (i, c) in trade.characters.iter().enumerate() {
        current[i] = crate::db::lock_persist_revision(&mut tx, &c.snapshot.player_id).await?;
    }
    let proof = crate::db::load_trade_commit_proof(&mut tx, &trade.commit_id).await?;
    let action = if proof.is_some() {
        trade_revision_action_with_proof(trade, current, proof.as_deref())
    } else {
        trade_revision_action(trade, current)
    };
    if action != TradeApply::Applied {
        return Ok(action);
    }
    if !crate::db::validate_trade_placements(&mut tx, trade).await? {
        return Ok(TradeApply::Conflict);
    }
    for c in &trade.characters {
        write_snapshot_fields(&mut tx, &c.snapshot, weapon_skill_id).await?;
    }
    crate::db::clear_transferred_lifecycle(&mut tx, trade).await?;
    for c in &trade.characters {
        crate::db::apply_item_lifecycle(
            &mut tx,
            &c.snapshot.player_id,
            c.snapshot
                .item_lifecycle
                .as_ref()
                .ok_or("trade lifecycle missing")?,
            &c.snapshot.inventory,
        )
        .await?;
    }
    for c in &trade.characters {
        crate::db::write_persist_revision(
            &mut tx,
            &c.snapshot.player_id,
            c.snapshot.persist_revision,
        )
        .await?;
    }
    crate::db::write_trade_commit_proof(&mut tx, trade).await?;
    tx.commit()
        .await
        .map_err(|e| format!("trade commit: {e}"))?;
    Ok(TradeApply::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use tokio::sync::mpsc;

    use crate::inventory::InventoryState;

    fn trade_runtime(tag: &str) -> (crate::spool::PersistRuntime, std::path::PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "pair-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        (
            crate::spool::PersistRuntime::new(&path, "sword").unwrap(),
            path,
        )
    }

    async fn trade_world() -> (Shared, TradeCommitRequest) {
        let shared = crate::world::new_shared();
        let (mut a, _) = test_player("1");
        let (mut b, _) = test_player("2");
        a.idia = 100;
        b.idia = 50;
        let mut def =
            crate::item::ItemDefinition::new("ore", "Ore", crate::item::ItemCategory::RawMaterial);
        def.max_stack = 100;
        let mut item = crate::item::ItemInstance::new("source", "ore", Default::default());
        item.count = 10;
        a.inventory.base_slots[0] = Some(item);
        let mut world = shared.lock().await;
        world.item_definitions.insert("ore".into(), def);
        world.players.insert("1".into(), a);
        world.players.insert("2".into(), b);
        world.by_conn.insert(1, "1".into());
        world.by_conn.insert(2, "2".into());
        drop(world);
        (
            shared,
            TradeCommitRequest {
                commit_id: "test_commit".into(),
                characters: ["1".into(), "2".into()],
                idia: [90, 60],
                transfers: vec![TradeTransferRequest {
                    source: "1".into(),
                    item_uuid: "source".into(),
                    count: 10,
                }],
            },
        )
    }

    #[tokio::test]
    async fn trade_commit_durable_pair_preserves_other_fields_and_retries_once() {
        let (runtime, path) = trade_runtime("durable");
        let (shared, req) = trade_world().await;
        let spool = runtime.spool();
        let shared_during = &shared;
        commit_trade_with(spool, &shared, req.clone(), |artifact| async move {
            let mut w = shared_during.lock().await;
            assert!(!w.economic_mutation_allowed("1"));
            assert!(!w.economic_mutation_allowed("2"));
            let p = w.players.get_mut("1").unwrap();
            p.x = 321.0;
            p.hp = 7;
            p.mark_dirty(PersistComponent::Position);
            p.mark_dirty(PersistComponent::Resources);
            drop(w);
            spool.write_trade(&artifact)
        })
        .await
        .unwrap();
        {
            let w = shared.lock().await;
            assert_eq!((w.players["1"].x, w.players["1"].hp), (321.0, 7));
            assert_eq!((w.players["1"].idia, w.players["2"].idia), (90, 60));
            assert_eq!(
                w.players["2"].inventory.base_slots[0]
                    .as_ref()
                    .unwrap()
                    .item_uuid,
                "source"
            );
            assert!(w.players["1"].dirty.any());
            assert!(!w.players["2"].dirty.any());
        }
        commit_trade(spool, &shared, req.clone()).await.unwrap();
        assert_eq!(shared.lock().await.players["1"].persist_revision, 1);
        let mut changed = req;
        changed.idia = [80, 70];
        assert!(commit_trade(spool, &shared, changed).await.is_err());
        assert!(runtime.persist_player(&shared, "1", true).await.is_err());
        assert!(runtime
            .persist_dirty_run(&shared, &["1".into(), "2".into()])
            .await
            .is_err());
        assert_eq!(spool.count_batches().unwrap(), 1);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn trade_commit_failed_publication_keeps_exact_split_and_freezes_mutators() {
        let (runtime, path) = trade_runtime("failure");
        let (shared, mut req) = trade_world().await;
        req.transfers[0].count = 4;
        assert!(
            commit_trade_with(runtime.spool(), &shared, req.clone(), |_| async {
                Err("publication failed".into())
            })
            .await
            .is_err()
        );
        let split = {
            let mut w = shared.lock().await;
            assert_eq!(w.players["1"].idia, 100);
            assert_eq!(w.players["1"].persist_revision, 0);
            assert!(!w.economic_mutation_allowed("1"));
            assert!(crate::world::ensure_takeover_allowed(&w, "1", 0).is_err());
            assert!(crate::world::disconnect_conn(&mut w, 1).is_none());
            assert_eq!(
                crate::trade::attempt_trade(
                    &mut w,
                    &crate::quest::QuestService::new(),
                    5.0,
                    "1",
                    &crate::trade::TradeRequest {
                        npc_id: "npc_1".into(),
                        action: crate::trade::TradeAction::Open,
                        item_id: String::new(),
                        item_uuid: String::new(),
                        history_id: String::new(),
                        count: 0
                    }
                ),
                Err(crate::trade::TradeReject::CommitPending)
            );
            w.prepared_trades["test_commit"].artifact.transfers[0]
                .moved
                .item_uuid
                .clone()
        };
        assert!(persist_dirty_into(&shared, "1", true, |_| async { Ok(()) })
            .await
            .is_err());
        runtime.retry_trade_publications(&shared).await.unwrap();
        let w = shared.lock().await;
        assert_eq!(
            w.players["1"].inventory.base_slots[0]
                .as_ref()
                .unwrap()
                .count,
            6
        );
        assert_eq!(
            w.players["2"].inventory.base_slots[0]
                .as_ref()
                .unwrap()
                .item_uuid,
            split
        );
        assert!(w.prepared_trades.is_empty());
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn trade_commit_visible_sync_failure_keeps_pair_until_confirmation() {
        let (runtime, path) = trade_runtime("uncertain");
        let (shared, req) = trade_world().await;
        let spool = runtime.spool();
        assert!(
            commit_trade_with(spool, &shared, req.clone(), |artifact| async move {
                spool.write_trade_with_sync(&artifact, |_| Err("injected sync uncertainty".into()))
            })
            .await
            .is_err()
        );
        assert_eq!(spool.count_batches().unwrap(), 1);
        {
            let w = shared.lock().await;
            assert_eq!(w.players["1"].idia, 100);
            assert!(!w.economic_mutation_allowed("2"));
        }
        commit_trade(spool, &shared, req).await.unwrap();
        assert_eq!(spool.count_batches().unwrap(), 1);
        assert_eq!(shared.lock().await.players["1"].idia, 90);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn trade_commit_revision_matrix_and_global_placement_decision() {
        let (shared, req) = trade_world().await;
        let world = shared.lock().await;
        let trade = prepare_trade(&world, &req).unwrap().artifact;
        drop(world);
        for (current, expected) in [
            ([Some(0), Some(0)], TradeApply::Applied),
            ([Some(1), Some(1)], TradeApply::Conflict),
            ([Some(1), Some(0)], TradeApply::Conflict),
            ([Some(0), Some(1)], TradeApply::Conflict),
            ([Some(2), Some(2)], TradeApply::Conflict),
            ([None, Some(0)], TradeApply::Conflict),
        ] {
            assert_eq!(trade_revision_action(&trade, current), expected);
        }
        let mut predecessors = trade.clone();
        for c in &mut predecessors.characters {
            c.base_revision = 4;
            c.snapshot.persist_revision = 5;
        }
        for current in [[Some(4), Some(3)], [Some(3), Some(4)]] {
            assert_eq!(
                trade_revision_action(&predecessors, current),
                TradeApply::Waiting
            );
        }
        assert_eq!(
            trade_revision_action_with_proof(
                &trade,
                [Some(2), Some(2)],
                Some(&trade.commit_payload().unwrap())
            ),
            TradeApply::AlreadyApplied
        );
        assert_eq!(
            trade_revision_action_with_proof(
                &trade,
                [Some(2), Some(0)],
                Some(&trade.commit_payload().unwrap())
            ),
            TradeApply::Conflict
        );
        assert!(crate::db::trade_placement_refs_valid(
            Some("1"),
            &[("1".into(), false)]
        ));
        assert!(!crate::db::trade_placement_refs_valid(
            Some("1"),
            &[("2".into(), false)]
        ));
        assert!(!crate::db::trade_placement_refs_valid(
            Some("1"),
            &[("1".into(), true)]
        ));
        assert!(!crate::db::trade_placement_refs_valid(
            Some("1"),
            &[("1".into(), false), ("1".into(), false)]
        ));
        assert!(crate::db::trade_placement_refs_valid(None, &[])); // new unsaved split UUID
        assert!(!crate::db::trade_placement_refs_valid(
            None,
            &[("3".into(), false)]
        ));
    }

    #[tokio::test]
    async fn trade_commit_full_merge_cancels_old_owner_only_retiring_incoming_uuid() {
        let (shared, req) = trade_world().await;
        let mut w = shared.lock().await;
        let mut other = w.players["1"].inventory.base_slots[0].clone().unwrap();
        other.item_uuid = "destination".into();
        w.players.get_mut("2").unwrap().inventory.base_slots[0] = Some(other);
        let rt = w.runtime_id.clone();
        crate::item_lifecycle::reconcile_after_take(
            w.item_lifecycle.entry("1".into()).or_default(),
            &Default::default(),
            "source",
            crate::item_lifecycle::DetachReason::Sold,
            &rt,
            1,
        );
        let prepared = prepare_trade(&w, &req).unwrap();
        assert_eq!(
            prepared.artifact.transfers[0].retired_uuid.as_deref(),
            Some("source")
        );
        assert!(!prepared.lifecycle[0].contains("source"));
        assert!(prepared.lifecycle[1].contains("source"));
        assert_eq!(
            prepared.artifact.characters[1]
                .snapshot
                .item_lifecycle
                .as_ref()
                .unwrap()
                .pending[0]
                .reason,
            crate::item_lifecycle::DetachReason::Merged
        );
        prepared.artifact.validate().unwrap();
    }

    #[tokio::test]
    async fn trade_commit_rejects_partial_failure_without_prepared_state() {
        let (runtime, path) = trade_runtime("validation");
        let (shared, req) = trade_world().await;
        shared.lock().await.players.get_mut("2").unwrap().inventory = InventoryState::new(0);
        assert!(commit_trade(runtime.spool(), &shared, req).await.is_err());
        let w = shared.lock().await;
        assert!(w.prepared_trades.is_empty());
        assert_eq!(w.players["1"].inventory.count_of("ore"), 10);
        assert_eq!(w.players["1"].idia, 100);
        assert_eq!(runtime.spool().count_batches().unwrap(), 0);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn trade_commit_gates_periodic_and_force_saves_without_sleep() {
        use futures_util::FutureExt;
        let (runtime, path) = trade_runtime("gates");
        let (shared, req) = trade_world().await;
        let gate = runtime.player_gate("1").await.lock_owned().await;
        let periodic = runtime.persist_dirty_run(&shared, &req.characters);
        let force = runtime.persist_player(&shared, "1", true);
        let commit = runtime.commit_trade(&shared, req.clone());
        tokio::pin!(periodic, force, commit);
        assert!(periodic.as_mut().now_or_never().is_none());
        assert!(force.as_mut().now_or_never().is_none());
        assert!(commit.as_mut().now_or_never().is_none());
        drop(gate);
        periodic.await.unwrap();
        force.await.unwrap();
        commit.await.unwrap();
        let w = shared.lock().await;
        // Force reserved revision 1 before the trade; no duplicate revision.
        assert_eq!(w.players["1"].persist_revision, 2);
        assert_eq!(w.players["2"].persist_revision, 1);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn cancellation_fix_failed_pair_retries_other_pair_and_saves_unrelated_c() {
        let (runtime, path) = trade_runtime("isolated-retries");
        let (shared, req) = trade_world().await;
        assert!(
            commit_trade_with(runtime.spool(), &shared, req.clone(), |_| async {
                Err("initial publication unavailable".into())
            })
            .await
            .is_err()
        );
        let artifact = shared.lock().await.prepared_trades[&req.commit_id]
            .artifact
            .clone();
        // A file-specific failure, not a broken common spool directory. It
        // remains bound to A/B and must not stop publication for C or D/E.
        let name = format!(
            "t-{}-{}-r{}-{}-r{}.json",
            artifact.commit_id,
            artifact.characters[0].snapshot.player_id,
            artifact.characters[0].snapshot.persist_revision,
            artifact.characters[1].snapshot.player_id,
            artifact.characters[1].snapshot.persist_revision
        );
        std::fs::write(path.join("spool").join(name), b"conflicting content").unwrap();
        for id in ["3", "4", "5"] {
            let (mut p, _) = test_player(id);
            if id == "3" {
                p.mark_dirty(PersistComponent::Position);
            }
            shared.lock().await.players.insert(id.into(), p);
        }
        let healthy = TradeCommitRequest {
            commit_id: "healthy_pair".into(),
            characters: ["4".into(), "5".into()],
            idia: [76, 78],
            transfers: Vec::new(),
        };
        assert!(
            commit_trade_with(runtime.spool(), &shared, healthy, |_| async {
                Err("injected first attempt".into())
            })
            .await
            .is_err()
        );
        assert!(runtime
            .persist_dirty_run(&shared, &["1".into(), "2".into(), "3".into()])
            .await
            .is_err());
        runtime.persist_player(&shared, "3", true).await.unwrap();
        let w = shared.lock().await;
        assert_eq!(w.players["3"].persist_revision, 2);
        assert!(!w.players["3"].dirty.any());
        assert!(w.prepared_trades.contains_key(&req.commit_id));
        assert!(!w.prepared_trades.contains_key("healthy_pair"));
        assert_eq!((w.players["4"].idia, w.players["5"].idia), (76, 78));
        assert!(!w.economic_mutation_allowed("1"));
        assert!(!w.economic_mutation_allowed("2"));
        assert_eq!(runtime.status(), crate::spool::PersistStatus::Degraded);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }

    /// Test-Spieler mit definierten persistenten Werten.
    fn test_player(id: &str) -> (Player, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Player {
                id: id.into(),
                name: id.into(),
                x: 10.0,
                y: 20.0,
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
                exp: 1234,
                free_attr_points: 2,
                rested_pool: 50,
                idia: 77,
                armor: 0,
                weapon_skill: 3,
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
                inventory: InventoryState::new(8),
                quests: BTreeMap::new(),
                dirty: PersistDirty::default(),
                persist_generation: 0,
                persist_revision: 0,
            },
            rx,
        )
    }

    async fn put_player(shared: &crate::world::Shared, player: Player) {
        let mut world = shared.lock().await;
        world.players.insert(player.id.clone(), player);
    }

    /// Führt einen Save mit Erfolg/Fehler aus und liefert das vom Schreiber
    /// erfasste Ergebnis (Snapshot inklusive).
    async fn run_save(
        shared: &crate::world::Shared,
        ok: bool,
    ) -> (Result<(), String>, Arc<Mutex<Option<PersistSnapshot>>>) {
        let captured = Arc::new(Mutex::new(None));
        let cap = captured.clone();
        let res = persist_dirty_into(shared, "p", false, move |snapshot| {
            let cap = cap.clone();
            async move {
                *cap.lock().unwrap() = Some(snapshot);
                if ok {
                    Ok(())
                } else {
                    Err("db down".to_string())
                }
            }
        })
        .await;
        (res, captured)
    }

    #[test]
    fn dirty_flags_mark_only_selected_components_and_raise_generation() {
        let (mut p, _rx) = test_player("p");
        assert!(!p.dirty.any());
        assert_eq!(p.persist_generation, 0);
        p.mark_dirty(PersistComponent::Position);
        p.mark_dirty(PersistComponent::Idia);
        assert!(p.dirty.is_dirty(PersistComponent::Position));
        assert!(p.dirty.is_dirty(PersistComponent::Idia));
        assert!(!p.dirty.is_dirty(PersistComponent::Progression));
        assert!(!p.dirty.is_dirty(PersistComponent::Inventory));
        assert!(!p.dirty.is_dirty(PersistComponent::Resources));
        assert_eq!(p.persist_generation, 2);
        p.dirty.clear(PersistComponent::Position);
        assert!(!p.dirty.is_dirty(PersistComponent::Position));
        assert!(p.dirty.is_dirty(PersistComponent::Idia));
    }

    #[test]
    fn apply_progression_marks_progression_dirty() {
        // `apply_progression` ist der einzige Rückweg für
        // Progressions-Berechnungsergebnisse (Level/EXP/Attributpunkte/
        // Rested-Pool, src/progression.rs); jede Anwendung muss die
        // Komponente Progression als dirty markieren.
        let (mut p, _rx) = test_player("p");
        assert!(!p.dirty.any());
        p.apply_progression(crate::progression::Progression::new(
            6,
            2000,
            3,
            60,
            crate::class::ClassStatus::Adventurer,
            false,
        ));
        assert_eq!(p.level, 6);
        assert_eq!(p.exp, 2000);
        assert_eq!(p.free_attr_points, 3);
        assert_eq!(p.rested_pool, 60);
        assert!(p.dirty.is_dirty(PersistComponent::Progression));
        assert!(!p.dirty.is_dirty(PersistComponent::Position));
        assert_eq!(p.persist_generation, 1);
    }

    #[tokio::test]
    async fn buffer_only_changes_never_trigger_inventory_persistence() {
        // docs/inventory_system.md §10/§11: Der Sicherheits-Puffer ist
        // flüchtiger Runtime-State. Eine Änderung NUR am Puffer darf weder
        // eine Komponente dirty markieren noch einen Persistenz-Write auslösen.
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        let mut item = crate::item::ItemInstance::new(
            "b1",
            "hp_potion",
            crate::item::ItemModifiers::default(),
        );
        item.count = 2;
        p.inventory.buffer.push(Some(item));
        put_player(&shared, p).await;
        let called = Arc::new(Mutex::new(false));
        let called2 = called.clone();
        let res = persist_dirty_into(&shared, "p", false, move |_snapshot| {
            let called = called2.clone();
            async move {
                *called.lock().unwrap() = true;
                Ok(())
            }
        })
        .await;
        assert!(res.is_ok());
        assert!(
            !*called.lock().unwrap(),
            "Buffer-only-Änderung erzeugt keinen Persistenz-Write"
        );
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(!player.dirty.any());
        assert_eq!(player.inventory.buffer_len(), 1);
    }

    #[tokio::test]
    async fn persist_skips_writer_when_nothing_is_dirty() {
        let shared = crate::world::new_shared();
        let (p, _rx) = test_player("p");
        put_player(&shared, p).await;
        let called = Arc::new(Mutex::new(false));
        let called2 = called.clone();
        let res = persist_dirty_into(&shared, "p", false, move |_snapshot| {
            let called = called2.clone();
            async move {
                *called.lock().unwrap() = true;
                Ok(())
            }
        })
        .await;
        assert!(res.is_ok());
        assert!(
            !*called.lock().unwrap(),
            "kein Write bei leerem Dirty-State"
        );
    }

    #[tokio::test]
    async fn force_persists_even_without_dirty_state() {
        let shared = crate::world::new_shared();
        let (p, _rx) = test_player("p");
        put_player(&shared, p).await;
        let called = Arc::new(Mutex::new(false));
        let called2 = called.clone();
        let res = persist_dirty_into(&shared, "p", true, move |_snapshot| {
            let called = called2.clone();
            async move {
                *called.lock().unwrap() = true;
                Ok(())
            }
        })
        .await;
        assert!(res.is_ok());
        assert!(*called.lock().unwrap(), "Force erzwingt den Write");
    }

    #[tokio::test]
    async fn persist_skips_writer_when_player_is_offline() {
        let shared = crate::world::new_shared();
        let called = Arc::new(Mutex::new(false));
        let called2 = called.clone();
        let res = persist_dirty_into(&shared, "offline", false, move |_snapshot| {
            let called = called2.clone();
            async move {
                *called.lock().unwrap() = true;
                Ok(())
            }
        })
        .await;
        assert!(res.is_ok());
        assert!(!*called.lock().unwrap());
    }

    #[tokio::test]
    async fn snapshot_is_the_full_player_state() {
        // Stufe B (docs §42): sobald irgendeine Komponente dirty ist, trägt
        // der Snapshot den VOLLständigen persistenzpflichtigen Spielerzustand.
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        let mut item =
            crate::item::ItemInstance::new("i1", "sword", crate::item::ItemModifiers::default());
        p.inventory.base_slots[0] = Some(item);
        p.mark_dirty(PersistComponent::Position);
        put_player(&shared, p).await;
        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        assert_eq!(snapshot.player_id, "p");
        assert_eq!(snapshot.position_value(), (10.0, 20.0));
        assert_eq!(snapshot.idia, 77);
        assert_eq!(snapshot.level, 5);
        assert_eq!(snapshot.exp, 1234);
        assert_eq!(snapshot.free_attr_points, 2);
        assert_eq!(snapshot.rested_pool, 50);
        assert_eq!(snapshot.hp, 100);
        assert_eq!(snapshot.mana, 50);
        assert_eq!(snapshot.weapon_skill, 3);
        assert_eq!(snapshot.char_class, "Adventurer");
        assert!(!snapshot.faction_transition);
        assert_eq!(
            snapshot.inventory.base_slots[0].as_ref().unwrap().item_id,
            "sword"
        );
        // Revision = RAM-Revision + 1 (§29).
        assert_eq!(snapshot.persist_revision, 1);
        // Steuergrößen sind nicht Teil des Drahtformats.
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("generation"));
        assert!(!json.contains("dirty"));
        assert!(!json.contains("buffer"));
        assert!(json.contains("\"character_id\":\"p\""));
    }

    #[tokio::test]
    async fn successful_save_clears_dirty_and_advances_revision() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.persist_revision = 41;
        p.mark_dirty(PersistComponent::Position);
        put_player(&shared, p).await;
        let (res, _captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(
            !player.dirty.any(),
            "erfolgreicher Save bei unveränderter Generation setzt dirty zurück"
        );
        // §39: Revision folgt dem durable gespeicherten Stand auch im RAM.
        assert_eq!(player.persist_revision, 42);
    }

    #[tokio::test]
    async fn failed_save_keeps_dirty_and_revision() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.persist_revision = 7;
        p.mark_dirty(PersistComponent::Position);
        p.mark_dirty(PersistComponent::Inventory);
        put_player(&shared, p).await;
        let (res, _captured) = run_save(&shared, false).await;
        assert!(res.is_err());
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(player.dirty.is_dirty(PersistComponent::Position));
        assert!(player.dirty.is_dirty(PersistComponent::Inventory));
        assert_eq!(player.persist_revision, 7, "Bei Fehler bleibt die Revision");
    }

    #[tokio::test]
    async fn change_during_save_keeps_dirty_state_but_advances_revision() {
        // §15 Race-Regel: Ändert sich der RAM-Zustand NACH dem Snapshot aber
        // VOR erfolgreichem DB-Write, darf der neuere Zustand nicht als clean
        // markiert werden. §39: Die Revision wird trotzdem weitergeschrieben
        // (der durable Stand ist gesichert); der nächste Lauf snapshotted den
        // neueren Zustand.
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.persist_revision = 3;
        p.mark_dirty(PersistComponent::Position);
        put_player(&shared, p).await;
        let sh = shared.clone();
        let res = persist_dirty_into(&shared, "p", false, move |_snapshot| {
            let sh = sh.clone();
            async move {
                let mut world = sh.lock().await;
                world
                    .players
                    .get_mut("p")
                    .unwrap()
                    .mark_dirty(PersistComponent::Progression);
                Ok(())
            }
        })
        .await;
        assert!(res.is_ok());
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(
            player.dirty.is_dirty(PersistComponent::Position),
            "Position darf trotz erfolgreichem Write nicht clean werden"
        );
        assert!(player.dirty.is_dirty(PersistComponent::Progression));
        assert_eq!(player.persist_generation, 2);
        assert_eq!(player.persist_revision, 4, "§39: Revision folgt dem Stand");
    }

    #[tokio::test]
    async fn multiple_dirty_components_are_all_cleared_after_success() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        for c in PersistComponent::ALL {
            p.mark_dirty(c);
        }
        put_player(&shared, p).await;
        let (res, _captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(
            !player.dirty.any(),
            "alle dirty Komponenten wurden zurückgesetzt (Voll-Snapshot)"
        );
    }

    // ── P-18: Cooldowns im Snapshot ────────────────────────────────────────

    /// `P-18`: Laufende Cooldowns erscheinen im Snapshot als **absolute
    /// Ablaufzeitpunkte in Epoch-Millisekunden** (persistierte Zeiteinheit).
    #[tokio::test]
    async fn p18_snapshot_carries_cooldowns_as_epoch_millis() {
        use std::time::{Duration, SystemTime};
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        let ready_at = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        p.cooldowns.insert("fire_bolt".into(), ready_at);
        p.cooldowns.insert(
            "choke".into(),
            SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_060_000),
        );
        p.mark_dirty(PersistComponent::Progression);
        put_player(&shared, p).await;

        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        let map = snapshot
            .cooldowns
            .as_ref()
            .expect("neuer Snapshot enthält immer ein Cooldown-Feld");
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("fire_bolt").copied(), Some(1_700_000_000_123));
        assert_eq!(map.get("choke").copied(), Some(1_700_000_060_000));
        // Der RAM-Zustand bleibt dabei unangetastet (nur Lesezugriff).
        let world = shared.lock().await;
        assert_eq!(world.players["p"].cooldowns["fire_bolt"], ready_at);
    }

    /// `P-18`: Ein Spieler **ohne** laufende Cooldowns erzeugt `Some({})` —
    /// das Feld ist bewusst vorhanden und leer. Das ist der Beleg, dass beim
    /// Anwenden des Snapshots zuvor gespeicherte Cooldowns entfernt werden
    /// (vollständiges Ersetzen) und nicht stillschweigend erhalten bleiben.
    #[tokio::test]
    async fn p18_snapshot_without_running_cooldowns_is_present_but_empty() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        assert!(p.cooldowns.is_empty());
        p.mark_dirty(PersistComponent::Progression);
        put_player(&shared, p).await;

        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        assert_eq!(
            snapshot.cooldowns,
            Some(std::collections::BTreeMap::new()),
            "Feld vorhanden, bewusst leer"
        );
    }

    /// `P-18`: **Altformat** — ein Snapshot ohne Cooldown-Feld bleibt lesbar und
    /// ergibt `None`. `None` bedeutet „Feld fehlt", nicht „leere Map": beim
    /// Anwenden wird der gespeicherte Bestand dann **nicht** ersetzt.
    #[test]
    fn p18_snapshot_without_cooldown_field_deserializes_as_none() {
        // Echter Snapshot, anschließend wird **nur** das Cooldown-Feld entfernt.
        // Das bildet eine alte Datei ab, ohne das Inventar-JSON von Hand nachzubauen.
        let mut map = std::collections::BTreeMap::new();
        map.insert("fire_bolt".to_string(), 1_700_000_000_000i64);
        let current = PersistSnapshot {
            player_id: "7".into(),
            persist_revision: 3,
            captured_at_ms: 1_700_000_000_000,
            x: 1.0,
            y: 2.0,
            level: 5,
            exp: 100,
            free_attr_points: 1,
            rested_pool: 0,
            idia: 42,
            hp: 80,
            mana: 30,
            attributes: Default::default(),
            char_class: "Adventurer".into(),
            faction_transition: false,
            weapon_skill: 2,
            learned_abilities: vec!["fire_bolt".into()],
            cooldowns: Some(map),
            // Item-Lifecycle: neues Format mit explizitem Feld.
            item_lifecycle: None,
            inventory: InventoryState::default(),
            generation: 0,
            dirty: PersistDirty::default(),
        };
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&current).unwrap()).unwrap();
        value
            .as_object_mut()
            .expect("Snapshot ist ein Objekt")
            .remove("cooldowns");

        let altformat = serde_json::to_string(&value).unwrap();
        assert!(
            !altformat.contains("cooldowns"),
            "Altformat-Datei enthält kein Cooldown-Feld"
        );
        let snapshot: PersistSnapshot = serde_json::from_str(&altformat)
            .expect("Altformat ohne Cooldown-Feld muss lesbar bleiben");
        assert_eq!(snapshot.player_id, "7");
        assert_eq!(snapshot.persist_revision, 3);
        assert_eq!(snapshot.learned_abilities, vec!["fire_bolt".to_string()]);
        assert_eq!(
            snapshot.cooldowns, None,
            "Fehlendes Feld wird als None unterschieden, nicht als leere Map"
        );
    }

    /// `P-18`: Gegenprobe zum vorigen Test — eine **vorhandene** leere Map ist
    /// `Some({})` und damit von `None` unterscheidbar. Genau diese
    /// Unterscheidung macht das vollständige Ersetzen beim Anwenden möglich.
    #[test]
    fn p18_present_empty_cooldown_field_is_distinguishable_from_missing() {
        let mut map = std::collections::BTreeMap::new();
        map.insert("fire_bolt".to_string(), 1_700_000_000_000i64);
        let with_cooldowns = PersistSnapshot {
            player_id: "7".into(),
            persist_revision: 4,
            captured_at_ms: 1,
            x: 0.0,
            y: 0.0,
            level: 1,
            exp: 0,
            free_attr_points: 0,
            rested_pool: 0,
            idia: 0,
            hp: 1,
            mana: 1,
            attributes: Default::default(),
            char_class: "Adventurer".into(),
            faction_transition: false,
            weapon_skill: 1,
            learned_abilities: vec![],
            cooldowns: Some(map),
            // Item-Lifecycle: neues Format mit explizitem Feld.
            item_lifecycle: None,
            inventory: InventoryState::default(),
            generation: 0,
            dirty: PersistDirty::default(),
        };
        let json = serde_json::to_string(&with_cooldowns).unwrap();
        assert!(
            json.contains("\"cooldowns\""),
            "Vorhandene Map wird serialisiert: {json}"
        );
        let back: PersistSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.cooldowns.unwrap().get("fire_bolt").copied(),
            Some(1_700_000_000_000)
        );

        let mut empty = with_cooldowns.clone();
        empty.cooldowns = Some(std::collections::BTreeMap::new());
        let back: PersistSnapshot =
            serde_json::from_str(&serde_json::to_string(&empty).unwrap()).unwrap();
        assert_eq!(back.cooldowns, Some(std::collections::BTreeMap::new()));
    }

    impl PersistSnapshot {
        fn position_value(&self) -> (f64, f64) {
            (self.x, self.y)
        }
    }

    // ── Item-Lifecycle im Snapshot (docs/inventory_system.md §18) ──────────

    /// Erzwungener Snapshot trägt die Lifecycle-Sicht (ausstehende
    /// Abkopplung mit Snapshot-Revision gestempelt); die History erscheint
    /// nicht im Drahtformat.
    #[tokio::test]
    async fn force_snapshot_carries_lifecycle_but_never_history() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.mark_dirty(PersistComponent::Inventory);
        put_player(&shared, p).await;
        {
            let mut world = shared.lock().await;
            let rt = world.runtime_id.clone();
            crate::item_lifecycle::reconcile_after_take(
                world.item_lifecycle.entry("p".into()).or_default(),
                &std::collections::BTreeSet::new(),
                "verkauft-1",
                crate::item_lifecycle::DetachReason::Sold,
                &rt,
                1_700_000_000_000,
            );
            world
                .sell_history
                .entry("p".into())
                .or_default()
                .record(crate::item_lifecycle::SellHistoryEntry {
                    instance: {
                        let mut inst = crate::item::ItemInstance::new(
                            "verkauft-1",
                            "hp_potion",
                            crate::item::ItemModifiers::default(),
                        );
                        inst.count = 1;
                        inst
                    },
                    sell_gold_value: 5,
                });
        }
        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        let lc = snapshot
            .item_lifecycle
            .as_ref()
            .expect("neuer Snapshot trägt immer eine Lifecycle-Sicht");
        assert_eq!(lc.pending.len(), 1);
        assert_eq!(lc.pending[0].item_uuid, "verkauft-1");
        assert_eq!(
            lc.pending[0].detached_at_revision, snapshot.persist_revision,
            "Abkopplung ist der Snapshot-Revision zugeordnet"
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("item_lifecycle"));
        assert!(
            !json.contains("sell_history"),
            "History bleibt nicht persistent"
        );
        // Roundtrip durchs Drahtformat.
        let back: PersistSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.item_lifecycle, snapshot.item_lifecycle);
    }

    /// Fehlgeschlagener Save: Lifecycle-Pflichten bleiben im erhaltenen RAM
    /// (keine vorzeitige Freigabe).
    #[tokio::test]
    async fn failed_save_keeps_lifecycle_obligations_in_ram() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.mark_dirty(PersistComponent::Inventory);
        put_player(&shared, p).await;
        {
            let mut world = shared.lock().await;
            let rt = world.runtime_id.clone();
            crate::item_lifecycle::reconcile_after_take(
                world.item_lifecycle.entry("p".into()).or_default(),
                &std::collections::BTreeSet::new(),
                "verkauft-1",
                crate::item_lifecycle::DetachReason::Sold,
                &rt,
                1_700_000_000_000,
            );
        }
        let (res, _captured) = run_save(&shared, false).await;
        assert!(res.is_err());
        let world = shared.lock().await;
        assert!(
            world.item_lifecycle["p"].contains("verkauft-1"),
            "unbestätigte Entfernung bleibt im RAM erhalten"
        );
        assert!(world.players["p"]
            .dirty
            .is_dirty(PersistComponent::Inventory));
    }

    /// Altformat: Ein Snapshot ohne Lifecycle-Feld bleibt lesbar und ergibt
    /// `None` — der Drain lässt Metadaten und Instanzen dann unberührt.
    #[test]
    fn snapshot_without_lifecycle_field_deserializes_as_none() {
        let current = PersistSnapshot {
            player_id: "7".into(),
            persist_revision: 3,
            captured_at_ms: 1_700_000_000_000,
            x: 1.0,
            y: 2.0,
            level: 5,
            exp: 100,
            free_attr_points: 1,
            rested_pool: 0,
            idia: 42,
            hp: 80,
            mana: 30,
            attributes: Default::default(),
            char_class: "Adventurer".into(),
            faction_transition: false,
            weapon_skill: 2,
            learned_abilities: vec![],
            cooldowns: None,
            inventory: InventoryState::default(),
            item_lifecycle: None,
            generation: 0,
            dirty: PersistDirty::default(),
        };
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&current).unwrap()).unwrap();
        value
            .as_object_mut()
            .expect("Snapshot ist ein Objekt")
            .remove("item_lifecycle");
        let altformat = serde_json::to_string(&value).unwrap();
        assert!(
            !altformat.contains("item_lifecycle"),
            "Altformat-Datei enthält kein Lifecycle-Feld"
        );
        let snapshot: PersistSnapshot = serde_json::from_str(&altformat)
            .expect("Altformat ohne Lifecycle-Feld muss lesbar bleiben");
        assert_eq!(
            snapshot.item_lifecycle, None,
            "fehlendes Feld bedeutet Altformat, kein leeres neues Format"
        );
    }
}
