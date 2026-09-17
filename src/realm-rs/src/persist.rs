// persist — Zentrale Spieler-Persistenz (docs/Player_Persistenz.md).
//
// Stufe A: komponentenspezifischer Dirty-State (Position, Progression, Gold,
// Inventory, Quest-State/-Progress) + Generation-/Race-Schutz (§15) + zentraler
// Player-Persistenzpfad, der von späteren Aufrufern (periodischer Flush,
// Disconnect-Save, Graceful Shutdown) wiederverwendet wird.
//
// Bewusst NICHT in Stufe A: periodischer Flush/Ticker, Umbau des bestehenden
// Disconnect-/Shutdown-Saves, HP/Mana/Attribute/Klasse/Skills/Abilities/
// Cooldowns/Effekte, Sell-/Buyback-History sowie der temporäre Inventory-
// Sicherheits-Puffer (docs/inventory_system.md §10/§11, flüchtiger
// Runtime-State, nie Teil des Dirty-/Persistenzpfads).
//
// Grundprinzip (§15): konsistenten Snapshot unter der World-Sperre erfassen
// (inkl. Generation), Sperre freigeben, DB-I/O ausschließlich außerhalb der
// Sperre, danach erneut sperren und Dirty-Flags NUR zurücksetzen, wenn die
// Generation unverändert ist (kein neuerer RAM-Zustand während des Writes
// entstanden).
use sqlx::{MySql, Pool};

use crate::inventory::InventoryState;
use crate::quest::CharacterQuestState;
use crate::world::{Player, Shared};

/// Die fünf persistenzpflichtigen Spielerkomponenten von Stufe A
/// (docs/Player_Persistenz.md §6, §20). Ausdrücklich NICHT enthalten: HP,
/// Mana, Attribute, Klasse, Skills, Abilities, Cooldowns, Effekte,
/// Sell-/Buyback-History und der temporäre Inventory-Sicherheits-Puffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistComponent {
    Position = 0,
    Progression = 1,
    Gold = 2,
    Inventory = 3,
    QuestState = 4,
}

impl PersistComponent {
    /// Alle Komponenten in fester Reihenfolge (Iterations-/Testreihenfolge).
    pub const ALL: [PersistComponent; 5] = [
        PersistComponent::Position,
        PersistComponent::Progression,
        PersistComponent::Gold,
        PersistComponent::Inventory,
        PersistComponent::QuestState,
    ];

    fn bit(self) -> u8 {
        1u8 << (self as u8)
    }
}

/// Komponentenspezifischer Dirty-State eines Spielers (docs §6): ein Bit je
/// persistenter Komponente. „inventory dirty" umfasst ausschließlich die
/// persistenten Inventarbestandteile (Grundinventar, Rucksäcke/Bag-Slots,
/// Equipment); der temporäre Sicherheits-Puffer ist flüchtiger Runtime-State
/// und nie Teil dieses Flags (docs/inventory_system.md §10/§11).
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

/// Progressions-Abbild für den zentralen Save (Level/EXP/Attributpunkte/
/// Rested-Pool). `logout_at` wird vom zentralen Pfad bewusst nicht gesetzt:
/// Die Spalte ist während einer laufenden Session bereits NULL (Reset beim
/// Login, handlers::handle_hello); der Logout-Zeitstempel gehört
/// ausschließlich zum finalen Disconnect-Save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressionSnapshot {
    pub level: u32,
    pub exp: i64,
    pub free_attr_points: u32,
    pub rested_pool: i64,
}

/// Konsistenter, unter der World-Sperre erfasster Snapshot der persistenten
/// Spielerkomponente. Enthält NUR die zum Snapshot-Zeitpunkt dirty
/// Komponenten sowie die Generation, gegen die der spätere Dirty-Reset
/// geprüft wird (§15).
#[derive(Debug, Clone)]
pub struct PersistSnapshot {
    pub player_id: String,
    /// Generation zum Snapshot-Zeitpunkt (Race-Prüfung §15).
    pub generation: u64,
    /// Nach erfolgreichem DB-Write UND unveränderter Generation
    /// zurückzusetzende Komponenten.
    pub dirty: PersistDirty,
    pub position: Option<(f64, f64)>,
    pub progression: Option<ProgressionSnapshot>,
    pub gold: Option<i64>,
    pub inventory: Option<InventoryState>,
    pub quest_states: Vec<CharacterQuestState>,
}

/// Erzeugt den Persistenz-Snapshot eines Spielers. Liefert `None`, wenn der
/// Spieler keinen Dirty-State besitzt (kein Write nötig).
fn build_snapshot(player: &Player) -> Option<PersistSnapshot> {
    let dirty = player.dirty;
    if !dirty.any() {
        return None;
    }
    let mut snapshot = PersistSnapshot {
        player_id: player.id.clone(),
        generation: player.persist_generation,
        dirty,
        position: None,
        progression: None,
        gold: None,
        inventory: None,
        quest_states: Vec::new(),
    };
    for component in dirty.iter() {
        match component {
            PersistComponent::Position => snapshot.position = Some((player.x, player.y)),
            PersistComponent::Progression => {
                snapshot.progression = Some(ProgressionSnapshot {
                    level: player.level,
                    exp: player.exp,
                    free_attr_points: player.free_attr_points,
                    rested_pool: player.rested_pool,
                });
            }
            PersistComponent::Gold => snapshot.gold = Some(player.gold),
            PersistComponent::Inventory => snapshot.inventory = Some(player.inventory.clone()),
            PersistComponent::QuestState => {
                // Nur persistierbare Zustände (ACTIVE/COMPLETED/FAILED) werden
                // übernommen. HIDDEN/AVAILABLE sind abgeleitet (§27.5) und
                // liegen nie im Spielerzustand — defensiv trotzdem gefiltert.
                snapshot.quest_states = player
                    .quests
                    .values()
                    .filter(|q| crate::quest::is_persistable(q))
                    .cloned()
                    .collect();
            }
        }
    }
    Some(snapshot)
}

/// Zentraler Player-Persistenzpfad (Stufe A): snapshotted die dirty
/// Komponenten eines Spielers unter der World-Sperre, schreibt sie über
/// `write` AUSSERHALB der Sperre (kein DB-I/O unter dem World-Lock) und
/// setzt die Dirty-Flags nach erfolgreichem Write nur zurück, wenn die
/// Generation unverändert geblieben ist (§15 Race-Regel).
///
/// `write` ist als injizierter Schreiber gehalten, damit die Lock-/Generation-
/// Logik ohne echte MariaDB-Verbindung testbar ist. In Produktion
/// (`persist_player`) wird der zentrale DB-Schreiber `write_persist_snapshot`
/// verwendet.
///
/// Fehlerverhalten (docs §16): Realm wird nicht beendet, der Spieler bleibt
/// im autoritativen RAM, Dirty-State bleibt bestehen (späterer Flush
/// versucht erneut); der Fehler wird an den Aufrufer zurückgegeben.
pub async fn persist_dirty_into<W, F>(
    shared: &Shared,
    player_id: &str,
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
            Some(player) => match build_snapshot(player) {
                Some(snapshot) => snapshot,
                None => return Ok(()), // nichts dirty → kein Write
            },
            None => return Ok(()), // Spieler offline → nichts zu flushen
        }
    };
    // Steuergrößen vor dem Verschieben des Snapshots in den Schreiber kopieren.
    let generation = snapshot.generation;
    let dirty = snapshot.dirty;
    let id = snapshot.player_id.clone();
    // Phase 2: DB-I/O außerhalb der World-Sperre.
    write(snapshot).await?;
    // Phase 3: erneut sperren; Dirty-State nur bei unveränderter Generation
    // zurücksetzen (kein neuerer RAM-Zustand während des Writes entstanden).
    let mut world = shared.lock().await;
    if let Some(player) = world.players.get_mut(&id) {
        if player.persist_generation == generation {
            player.dirty.clear_components(dirty);
        }
    }
    Ok(())
}

/// Produktions-Einstiegspunkt des zentralen Player-Persistenzpfads:
/// persistiert die dirty Komponenten von `player_id` in EINER
/// MariaDB-Transaktion über die transaktionskomponierbaren db-Bausteine
/// (`write_position`, `write_progression`, `write_gold`, `write_inventory`,
/// `write_quest_state`). Loggt Fehler (docs §16) und gibt sie an den
/// Aufrufer zurück; der Dirty-State bleibt in diesem Fall bestehen.
pub async fn persist_player(
    db: &Pool<MySql>,
    shared: &Shared,
    player_id: &str,
) -> Result<(), String> {
    let db = db.clone();
    persist_dirty_into(shared, player_id, move |snapshot| {
        let db = db.clone();
        async move {
            match write_persist_snapshot(&db, &snapshot).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    log::error!("Player-Persistenz {}: {e}", snapshot.player_id);
                    Err(e)
                }
            }
        }
    })
    .await
}

/// Zentraler DB-Schreiber (Stufe A): alle dirty Komponenten des Snapshots in
/// EINER Transaktion über die vorhandenen write_*-Bausteine schreiben.
/// Der Sicherheits-Puffer wird durch `write_inventory` bewusst nie
/// geschrieben; Questzustände werden über den gemeinsamen
/// `quest::encode_state_for_db`-Encoder persistiert (abgeleitete
/// HIDDEN/AVAILABLE werden abgelehnt).
pub(crate) async fn write_persist_snapshot(
    pool: &Pool<MySql>,
    snapshot: &PersistSnapshot,
) -> Result<(), String> {
    let player_id = &snapshot.player_id;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Player-Persistenz {player_id}: Transaktion beginnen: {e}"))?;
    if let Some((x, y)) = snapshot.position {
        crate::db::write_position(&mut tx, player_id, x, y).await?;
    }
    if let Some(progression) = snapshot.progression {
        // logout_at bewusst None (Spalte ist während der Session NULL).
        crate::db::write_progression(
            &mut tx,
            player_id,
            progression.level,
            progression.exp,
            progression.free_attr_points,
            progression.rested_pool,
            None,
        )
        .await?;
    }
    if let Some(gold) = snapshot.gold {
        crate::db::write_gold(&mut tx, player_id, gold).await?;
    }
    if let Some(inventory) = &snapshot.inventory {
        crate::db::write_inventory(&mut tx, player_id, inventory).await?;
    }
    for state in &snapshot.quest_states {
        let data = crate::quest::encode_state_for_db(state)?;
        crate::db::write_quest_state(
            &mut tx,
            player_id,
            &state.quest_id,
            state.state.db_value(),
            &data,
        )
        .await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("Player-Persistenz {player_id}: commit: {e}"))?;
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
    use crate::quest::{CharacterObjectiveProgress, CharacterQuestState, QuestState};

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
                gold: 77,
                armor: 0,
                weapon_skill: 1,
                combat: None,
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
            },
            rx,
        )
    }

    async fn put_player(shared: &crate::world::Shared, player: Player) {
        let mut world = shared.lock().await;
        world.players.insert(player.id.clone(), player);
    }

    fn quest_state(quest_id: &str, state: QuestState, current: u32) -> CharacterQuestState {
        CharacterQuestState {
            quest_id: quest_id.into(),
            state,
            progress: vec![CharacterObjectiveProgress {
                objective_id: "o1".into(),
                current,
            }],
            started_at_ms: Some(1),
            completed_at_ms: if state == QuestState::Completed {
                Some(2)
            } else {
                None
            },
        }
    }

    /// Führt einen Save mit Erfolg/Fehler aus und liefert das vom Schreiber
    /// erfasste Ergebnis (Snapshot inklusive).
    async fn run_save(
        shared: &crate::world::Shared,
        ok: bool,
    ) -> (Result<(), String>, Arc<Mutex<Option<PersistSnapshot>>>) {
        let captured = Arc::new(Mutex::new(None));
        let cap = captured.clone();
        let res = persist_dirty_into(shared, "p", move |snapshot| {
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
        p.mark_dirty(PersistComponent::Gold);
        assert!(p.dirty.is_dirty(PersistComponent::Position));
        assert!(p.dirty.is_dirty(PersistComponent::Gold));
        assert!(!p.dirty.is_dirty(PersistComponent::Progression));
        assert!(!p.dirty.is_dirty(PersistComponent::Inventory));
        assert!(!p.dirty.is_dirty(PersistComponent::QuestState));
        assert_eq!(p.persist_generation, 2);
        p.dirty.clear(PersistComponent::Position);
        assert!(!p.dirty.is_dirty(PersistComponent::Position));
        assert!(p.dirty.is_dirty(PersistComponent::Gold));
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
        // Inventory dirty markieren noch einen Persistenz-Write auslösen.
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
        let res = persist_dirty_into(&shared, "p", move |_snapshot| {
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
            "Buffer-only-Änderung erzeugt keinen Inventar-Write"
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
        let res = persist_dirty_into(&shared, "p", move |_snapshot| {
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
    async fn persist_skips_writer_when_player_is_offline() {
        let shared = crate::world::new_shared();
        let called = Arc::new(Mutex::new(false));
        let called2 = called.clone();
        let res = persist_dirty_into(&shared, "offline", move |_snapshot| {
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
    async fn snapshot_carries_only_dirty_components() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.mark_dirty(PersistComponent::Position);
        p.mark_dirty(PersistComponent::Gold);
        put_player(&shared, p).await;
        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        assert_eq!(snapshot.player_id, "p");
        assert_eq!(snapshot.position, Some((10.0, 20.0)));
        assert_eq!(snapshot.gold, Some(77));
        assert!(snapshot.progression.is_none());
        assert!(snapshot.inventory.is_none());
        assert!(snapshot.quest_states.is_empty());
        assert!(snapshot.dirty.is_dirty(PersistComponent::Position));
        assert!(snapshot.dirty.is_dirty(PersistComponent::Gold));
    }

    #[tokio::test]
    async fn successful_save_clears_dirty_when_generation_unchanged() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
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
    }

    #[tokio::test]
    async fn failed_save_keeps_dirty() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.mark_dirty(PersistComponent::Position);
        p.mark_dirty(PersistComponent::Inventory);
        put_player(&shared, p).await;
        let (res, _captured) = run_save(&shared, false).await;
        assert!(res.is_err());
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(player.dirty.is_dirty(PersistComponent::Position));
        assert!(player.dirty.is_dirty(PersistComponent::Inventory));
    }

    #[tokio::test]
    async fn change_during_save_keeps_dirty_state() {
        // §15 Race-Regel: Ändert sich der RAM-Zustand NACH dem Snapshot aber
        // VOR erfolgreichem DB-Write, darf der neuere Zustand nicht als clean
        // markiert werden. Der Schreiber markiert hier während des Writes
        // (außerhalb der Sperre) eine weitere Komponente.
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.mark_dirty(PersistComponent::Position);
        put_player(&shared, p).await;
        let sh = shared.clone();
        let res = persist_dirty_into(&shared, "p", move |_snapshot| {
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
    }

    #[tokio::test]
    async fn inventory_buffer_is_not_a_persistent_component_and_survives_save() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        let mut buffered = crate::item::ItemInstance::new(
            "buf1",
            "hp_potion",
            crate::item::ItemModifiers::default(),
        );
        buffered.count = 2;
        p.inventory.buffer.push(Some(buffered));
        p.mark_dirty(PersistComponent::Inventory);
        put_player(&shared, p).await;
        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        // Der Puffer ist kein eigener Persistentbestandteil: es gibt kein
        // PersistComponent dafür, sondern ausschließlich Inventory.
        let components: Vec<PersistComponent> = snapshot.dirty.iter().collect();
        assert_eq!(components, vec![PersistComponent::Inventory]);
        // Der Snapshot trägt den Inventar-Inhalt inklusive Puffer weiter;
        // ob der Puffer geschrieben wird, entscheidet ausschließlich
        // write_inventory (bewusst nicht, docs/inventory_system.md §11).
        assert!(snapshot.inventory.as_ref().unwrap().buffer_len() == 1);
        // Der Save verändert den RAM-Puffer nicht (Persistenz ≠ Cleanup).
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert_eq!(player.inventory.buffer_len(), 1);
        assert!(!player.dirty.any());
    }

    #[tokio::test]
    async fn unselected_components_are_not_saved_or_cleaned() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.mark_dirty(PersistComponent::Gold);
        put_player(&shared, p).await;
        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        // Nur Gold wurde ausgewählt: alle übrigen Komponenten fehlen.
        let components: Vec<PersistComponent> = snapshot.dirty.iter().collect();
        assert_eq!(components, vec![PersistComponent::Gold]);
        assert!(snapshot.position.is_none());
        assert!(snapshot.progression.is_none());
        assert!(snapshot.inventory.is_none());
        assert!(snapshot.quest_states.is_empty());
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(!player.dirty.is_dirty(PersistComponent::Gold));
        assert!(!player.dirty.is_dirty(PersistComponent::Position));
        assert!(!player.dirty.is_dirty(PersistComponent::Progression));
        assert!(!player.dirty.is_dirty(PersistComponent::Inventory));
        assert!(!player.dirty.is_dirty(PersistComponent::QuestState));
    }

    #[tokio::test]
    async fn multiple_dirty_components_are_all_cleared_after_success() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        for c in PersistComponent::ALL {
            p.mark_dirty(c);
        }
        put_player(&shared, p).await;
        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        assert!(snapshot.position.is_some());
        assert!(snapshot.progression.is_some());
        assert_eq!(snapshot.gold, Some(77));
        assert!(snapshot.inventory.is_some());
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(
            !player.dirty.any(),
            "alle dirty Komponenten wurden zurückgesetzt"
        );
    }

    #[tokio::test]
    async fn quest_states_are_forwarded_as_is_without_replacing_completion() {
        let shared = crate::world::new_shared();
        let (mut p, _rx) = test_player("p");
        p.quests.insert(
            "q_active".into(),
            quest_state("q_active", QuestState::Active, 3),
        );
        p.quests.insert(
            "q_completed".into(),
            quest_state("q_completed", QuestState::Completed, 5),
        );
        // Abgeleitete Zustände gehören nie in den Spielerzustand; defensiv
        // gefiltert (Quest-System.md §27.5).
        p.quests.insert(
            "q_derived".into(),
            quest_state("q_derived", QuestState::Available, 0),
        );
        p.mark_dirty(PersistComponent::QuestState);
        put_player(&shared, p).await;
        let (res, captured) = run_save(&shared, true).await;
        assert!(res.is_ok());
        let snapshot = captured.lock().unwrap().take().unwrap();
        let states: BTreeMap<String, u8> = snapshot
            .quest_states
            .iter()
            .map(|q| (q.quest_id.clone(), q.state.db_value() as u8))
            .collect();
        // Der zentrale Pfad leitet den Zustand unverändert weiter: ACTIVE=1,
        // COMPLETED=2 — er ersetzt/dupliziert den Questabschluss nicht.
        assert_eq!(states.get("q_active"), Some(&1));
        assert_eq!(states.get("q_completed"), Some(&2));
        assert!(!states.contains_key("q_derived"));
        let world = shared.lock().await;
        let player = world.players.get("p").unwrap();
        assert!(!player.dirty.is_dirty(PersistComponent::QuestState));
    }
}
