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
) -> Option<PersistSnapshot> {
    let dirty = player.dirty;
    if !force && !dirty.any() {
        return None;
    }
    let revision = player.persist_revision.saturating_add(1);
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
    Some(PersistSnapshot {
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
    })
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
    let snapshot = {
        let world = shared.lock().await;
        match world.players.get(player_id) {
            Some(player) => {
                let empty_lifecycle = crate::item_lifecycle::ItemLifecycle::new();
                let lifecycle = world
                    .item_lifecycle
                    .get(player_id)
                    .unwrap_or(&empty_lifecycle);
                match build_snapshot(player, force, lifecycle, &world.runtime_id) {
                    Some(snapshot) => snapshot,
                    None => return Ok(()), // nichts dirty (und nicht erzwungen) → kein Write
                }
            }
            None => return Ok(()), // Spieler offline → nichts zu flushen
        }
    };
    // Steuergrößen vor dem Verschieben des Snapshots in den Schreiber kopieren.
    let snapshot_revision = snapshot.persist_revision;
    let generation = snapshot.generation;
    let dirty = snapshot.dirty;
    let id = snapshot.player_id.clone();
    // Phase 2: Durable-Write außerhalb der World-Sperre.
    write(snapshot).await?;
    // Phase 3: erneut sperren. §39: Revision immer weiterschreiben; §15:
    // Dirty-Flags nur bei unveränderter Generation zurücksetzen.
    let mut world = shared.lock().await;
    if let Some(player) = world.players.get_mut(&id) {
        if player.persist_revision < snapshot_revision {
            player.persist_revision = snapshot_revision;
        }
        if player.persist_generation == generation {
            player.dirty.clear_components(dirty);
        }
    }
    Ok(())
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
    let (snapshots, controls) = {
        let world = shared.lock().await;
        let mut snapshots = Vec::new();
        let mut controls: Vec<(String, i64, u64, PersistDirty)> = Vec::new();
        let empty_lifecycle = crate::item_lifecycle::ItemLifecycle::new();
        for id in player_ids {
            let Some(player) = world.players.get(id) else {
                continue; // Spieler offline → nichts zu flushen
            };
            let lifecycle = world.item_lifecycle.get(id).unwrap_or(&empty_lifecycle);
            let Some(snapshot) = build_snapshot(player, false, lifecycle, &world.runtime_id)
            else {
                continue; // nicht dirty → kein Snapshot in diesem Lauf
            };
            controls.push((
                snapshot.player_id.clone(),
                snapshot.persist_revision,
                snapshot.generation,
                snapshot.dirty,
            ));
            snapshots.push(snapshot);
        }
        (snapshots, controls)
    };
    if snapshots.is_empty() {
        return Ok(0); // leere Dirty-Menge → keine Datei
    }
    let count = snapshots.len() as u32;
    // Phase 2: eine gemeinsame Batch-Datei, außerhalb der World-Sperre.
    // `snapshots` wandert in `write` und wird danach freigegeben.
    write(snapshots).await?;
    // Phase 3: Dirty-Rücknahme je Eintrag, an gesicherte Revision und
    // unveränderte Generation gebunden.
    let mut world = shared.lock().await;
    for (id, revision, generation, dirty) in controls {
        if let Some(player) = world.players.get_mut(&id) {
            if player.persist_revision < revision {
                player.persist_revision = revision;
            }
            if player.persist_generation == generation {
                player.dirty.clear_components(dirty);
            }
        }
    }
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
    let _guard = spool.player_gate(player_id).await.lock_owned().await;
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

    crate::db::write_position(&mut tx, char_id, snapshot.x, snapshot.y).await?;
    crate::db::write_progression_fields(
        &mut tx,
        char_id,
        snapshot.level,
        snapshot.exp,
        snapshot.free_attr_points,
        snapshot.rested_pool,
    )
    .await?;
    crate::db::write_idia(&mut tx, char_id, snapshot.idia).await?;
    crate::db::write_resources(&mut tx, char_id, snapshot.hp, snapshot.mana).await?;
    crate::db::write_attributes(&mut tx, char_id, &snapshot.attributes).await?;
    crate::db::write_inventory(&mut tx, char_id, &snapshot.inventory).await?;
    // Item-Lifecycle (docs/inventory_system.md §18, Migration 021): NUR bei
    // neuem Snapshot-Format. Altformat (`None`) lässt Metadaten und Instanzen
    // unberührt. Alles läuft in derselben Transaktion wie Inventar, Idia und
    // `persist_revision` — entweder wird alles committet oder nichts.
    if let Some(lifecycle) = snapshot.item_lifecycle.as_ref() {
        crate::db::apply_item_lifecycle(&mut tx, char_id, lifecycle, &snapshot.inventory).await?;
    }
    let class = crate::class::ClassStatus::from_db_name(&snapshot.char_class);
    crate::db::write_character_class(&mut tx, char_id, class, snapshot.faction_transition).await?;
    crate::db::write_weapon_skill(&mut tx, char_id, weapon_skill_id, snapshot.weapon_skill).await?;
    crate::db::write_character_abilities(&mut tx, char_id, &snapshot.learned_abilities).await?;
    // `P-18`: Cooldowns gehören zum normalen Snapshot und werden in derselben
    // Transaktion gespeichert. Nur ein **vorhandenes** Feld ersetzt den
    // Bestand; `None` = Altformat ohne Feld und lässt ihn unberührt.
    if let Some(cooldowns) = snapshot.cooldowns.as_ref() {
        crate::db::write_character_cooldowns(&mut tx, char_id, cooldowns).await?;
    }
    crate::db::write_persist_revision(&mut tx, char_id, snapshot.persist_revision).await?;

    tx.commit()
        .await
        .map_err(|e| format!("Drain {char_id}: commit: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use tokio::sync::mpsc;

    use crate::inventory::InventoryState;

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
