// world — Spieler-Registry + AOFB-Tick (Port von src/realm world.ts).
// Versand pro Spieler über einen MPSC-Kanal (Trennung Spielzustand /
// Socket-IO; ohne echte Sockets testbar). Disconnect = Kanal zu.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, Mutex};

use crate::combat::ability::ActiveCast;
use crate::combat::effects::Effect;
use crate::combat::CombatState;
use crate::protocol::{s2c, Frame};

/// Autoritativer Spieler-State auf dem Server. Felder hp/max_hp/lang
/// werden heute gesetzt und von künftigen Systemen (Combat, Level)
/// gelesen — kein Feld entfernen.
///
#[allow(dead_code)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub face: f64,
    pub ping_ms: u32,
    pub zone_id: u32,
    pub hp: i32,
    pub max_hp: i32,
    pub lang: String,
    /// Auth-API-Account (0 = unbekannt/Dev ohne Auth-API).
    pub account_id: u32,
    /// Login-Session ("" = keine); Basis für Elternkontroll-Polling.
    pub session_id: String,
    /// IDs, die dieser Spieler gerade sieht.
    pub entities: HashSet<String>,
    pub last_activity: Instant,
    pub tx: mpsc::UnboundedSender<String>,
    /// Klasse (char_class aus der DB).
    pub char_class: String,
    /// Typsichere Klassenbasis (docs/Klassensystem.md): permanente
    /// Grundklasse oder Adventurer; kanonischer Zustand für Tutorialphase,
    /// Hauptattribute-Metadaten, L9-Wahl und L10-Progressions-Hook.
    /// (Aus `char_class` abgeleitet; Tutorialphase L1–8 ist für Adventurer
    /// levelabgeleitet, benötigt kein zusätzliches DB-Feld.)
    pub class: crate::class::ClassStatus,
    /// Fraktions-Übergangs-Hook (docs/Klassensystem.md, L10-Regel): true,
    /// wenn der Charakter nach Fraktionswahl in ein Fraktionsgebiet
    /// gewechselt ist; sonst false. Fraktions-/Zonensystem folgt technisch.
    pub faction_transition: bool,
    pub level: u32,
    /// Gesammelte Erfahrungspunkte (Gruppensystem V1, §7).
    pub exp: i64,
    /// Freie Attributpunkte (docs/Erfahrung_und_Progressionssystem.md §5):
    /// bei Levelaufstieg gutgeschrieben, per Attributs-UI verbrauchbar.
    pub free_attr_points: u32,
    /// Rested-EXP-Pool (docs/Erfahrung_und_Progressionssystem.md §12):
    /// offline bis zu 50 % der aktuellen Level-Anforderung; wird nur durch
    /// Kill-EXP abgerufen (Kill EXP System V1).
    pub rested_pool: i64,
    /// Geldstand der kanonischen Spielerwährung (docs/Player_Persistenz.md
    /// §23): `idia`. Content-Loot vom Typ 'gold' (Loot System V1, Migration
    /// 017) erhöht diese Währung; der Content-Typ 'gold' bleibt unbenannt.
    pub idia: i64,
    /// Aktueller Rüstungswert (relevante physische Rüstung).
    pub armor: i32,
    /// Level des relevanten Waffen-/Kampfskills (startet bei 1).
    pub weapon_skill: u32,
    /// Aktueller Auto-Angriff (Combat V1): None = nicht im Kampf.
    pub combat: Option<CombatState>,
    /// Maßgeblicher Zeitpunkt des zuletzt **tatsächlich ausgeführten** Grund-
    /// angriffs (docs/Kampfsystem.md §3: die Duration bestimmt die Zeit
    /// zwischen zwei Grundangriffen). `None` = in dieser RAM-Existenz wurde
    /// noch kein Schlag ausgeführt.
    ///
    /// bewusst getrennt von `CombatState.last_attack`: dort steht der Bezugs-
    /// zeitpunkt, ab dem der nächste Schlag **fällig** ist (erster Start ohne
    /// Vorlauf), hier der Zeitpunkt eines **bereits erfolgten** Schlags. Nur
    /// `combat_tick` schreibt `last_strike`; `handle_attack` liest es als
    /// Takt-Referenz, damit wiederholte Absichten, ein Zielwechsel, ein
    /// Beenden/Neubeginn oder ein Zieltod die laufende Wartezeit nicht
    /// umgehen.
    ///
    /// Lebensdauer: RAM-only, nicht Teil des Snapshots und keiner Migration.
    /// Es überlebt Kampfende, Tod und RAM-Übernahme (derselbe Player); ein
    /// neuer Login baut einen neuen Player und damit den dokumentierten
    /// Sofortschlag bei erstmaliger Aktivierung.
    pub last_strike: Option<Instant>,
    /// Mana (Fähigkeits-Ressource, Ability-System.md §2).
    pub mana: i32,
    pub max_mana: i32,
    /// Aktive Effekte (Buffs, Debuffs, DoT/HoT, Control, Combat V3).
    pub effects: Vec<Effect>,
    /// Fähigkeits-Cooldowns: ability_id → ready_at (SystemTime).
    pub cooldowns: BTreeMap<String, std::time::SystemTime>,
    /// Aktiver Cast-Zustand (Combat V3): None = kein Cast aktiv.
    pub active_cast: Option<ActiveCast>,
    /// Gelernte Fähigkeiten (ability_id).
    pub learned_abilities: HashSet<String>,
    /// Grundattribute (docs/Attribute_und_Regeneration.md §§1–3).
    pub attributes: crate::attributes::Attributes,
    /// Basis-Max-HP vor Attributs-Bonus (für recompute_max_resources).
    pub max_hp_base: i32,
    /// Basis-Max-Mana vor Attributs-Bonus (für recompute_max_resources).
    pub max_mana_base: i32,
    /// Sitz-Zustand (docs/Attribute_und_Regeneration.md §5): 125 %
    /// Regeneration nur außerhalb des Kampfes.
    pub sitting: bool,
    /// Additive Regenerationsboni (absolute Werte, §7): Essen/Buffs/…
    /// wirken hier als +HP/s bzw. +Mana/s (Technik-Anschluss; das
    /// Consumable-System folgt später).
    pub hp_regen_bonus: f64,
    pub mana_regen_bonus: f64,
    /// Bruchteil-Carry der Regeneration (f64, §8): dezimale Raten
    /// ohne vorgezogenes Runden über Ticks.
    pub hp_regen_carry: f64,
    pub mana_regen_carry: f64,
    /// Inventory System V1 (docs/inventory_system.md): Grundinventar,
    /// Rucksäcke, Equipment, temporärer Sicherheits-Puffer. Wird bei HELLO
    /// aus realm_state geladen, bei Änderung/Disconnect persistiert.
    pub inventory: crate::inventory::InventoryState,
    /// Quest V1-Spielerzustand (docs/Quest-System.md §27): persistierte
    /// ACTIVE/COMPLETED/FAILED-Zustände je Quest, geladen bei HELLO aus
    /// der Tabelle `quests` (realm_state). HIDDEN/AVAILABLE sind abgeleitet
    /// (§27.5) und liegen nie hier.
    pub quests: BTreeMap<String, crate::quest::CharacterQuestState>,
    /// Persistenz-Dirty-State (docs/Player_Persistenz.md §5/§6): genau ein
    /// Flag je persistenter Komponente (Position, Progression, Gold,
    /// Inventory, Quest-State/-Progress). Mutationen setzen das Flag über
    /// `mark_dirty`; der zentrale Persistenzpfad (crate::persist) schreibt
    /// nur dirty Komponenten und setzt die Flags nach erfolgreichem Save
    /// zurück.
    pub dirty: crate::persist::PersistDirty,
    /// Persistenz-Generation (docs/Player_Persistenz.md §15 Race-Regel):
    /// monoton steigender Zähler, der bei JEDER Mutation einer persistenzen
    /// Komponente erhöht wird. Der zentrale Save snapshotted diese Generation
    /// unter der World-Sperre und setzt Dirty-Flags NUR zurück, wenn sie beim
    /// Abschluss des DB-Writes unverändert ist (kein neuerer RAM-Zustand
    /// während des Writes entstanden).
    pub persist_generation: u64,
    /// Persistenz-Revision (docs/Player_Persistenz.md §29): RAM-Abbild der
    /// im letzten angewendeten Snapshot persistierten Revision (DB-Spalte
    /// `characters.persist_revision`). Jeder neu aufgenommene Snapshot
    /// bekommt `persist_revision + 1`; nach durablem Spool-Erfolg wird der
    /// Zähler fortgeschrieben (auch wenn `persist_generation` während des
    /// Writes stieg). Grundlage der idempotenten Drain-/Superseded-Logik.
    pub persist_revision: i64,
}

impl Player {
    pub fn send(&self, frame: &Frame) {
        let _ = self.tx.send(frame.encode());
    }

    /// Reiner Progressions-Zustand (docs/Erfahrung_und_Progressionssystem.md)
    /// für die zentrale Berechnungslogik (src/progression.rs).
    pub fn progression_state(&self) -> crate::progression::Progression {
        crate::progression::Progression {
            level: self.level,
            exp: self.exp,
            free_attr_points: self.free_attr_points,
            rested_pool: self.rested_pool,
            class: self.class,
            faction_transition: self.faction_transition,
        }
    }

    /// Schreibt das Ergebnis einer Progressions-Berechnung zurück.
    pub fn apply_progression(&mut self, prog: crate::progression::Progression) {
        self.level = prog.level;
        self.exp = prog.exp;
        self.free_attr_points = prog.free_attr_points;
        self.rested_pool = prog.rested_pool;
        self.mark_dirty(crate::persist::PersistComponent::Progression);
    }

    /// Markiert eine persistente Spielerkomponente als dirty
    /// (docs/Player_Persistenz.md §5/§6) und erhöht den Persistenz-Generation-
    /// Zähler (§15 Race-Regel). Jede Mutation einer persistenzen Komponente
    /// MUSS diese Methode aufrufen; nur so verhindert der zentrale
    /// Persistenzpfad, dass ein während des DB-Writes entstandener neuerer
    /// RAM-Zustand fälschlich als clean markiert wird.
    pub fn mark_dirty(&mut self, component: crate::persist::PersistComponent) {
        self.persist_generation = self.persist_generation.wrapping_add(1);
        self.dirty.mark(component);
    }
}

#[derive(Debug, Default, Clone)]
pub struct TickStat {
    pub last_ms: f64,
    pub avg_ms: f64,
    pub count: u64,
}

pub struct World {
    pub players: HashMap<String, Player>,
    /// Eigene Runtime-Kennung dieses Realm-Prozesslaufs
    /// (`item_lifecycle::new_runtime_id`): stempelt Lifecycle-Metadaten zur
    /// Zuordnung und wird im Snapshot mitgeführt. Takeover und RAM-Übernahme
    /// innerhalb des laufenden Prozesses erhalten History und Metadaten, weil
    /// Spieler und World-Einträge dabei bestehen bleiben.
    pub runtime_id: String,
    /// Sell-/Buyback-History je Charakter (docs/Handelssystem.md §1–§3):
    /// ausschließlich Runtime-State der laufenden Session, nicht persistent.
    /// Wird am Session-Ende mit dem Spieler verworfen (`disconnect_conn`).
    pub sell_history: HashMap<String, crate::item_lifecycle::SellHistory>,
    /// Dauerhafte Lifecycle-Metadaten je Charakter (RAM-Abbild; persistent
    /// über den Snapshot-/Drain-Pfad, Migration 021). Unbestätigte
    /// Entfernungen bleiben hier erhalten, bis der DB-Commit bestätigt ist;
    /// bei Savefehler bleiben sie mit dem erhaltenen RAM bestehen (§16).
    /// Erst der maßgebliche Snapshot trägt sie — `disconnect_conn` entfernt
    /// sie deshalb erst nach dem finalen Flush.
    pub item_lifecycle: HashMap<String, crate::item_lifecycle::ItemLifecycle>,
    /// NPC-/Monster-Registry (Combat V2): key = npc_id().
    pub npcs: HashMap<String, crate::npc::Npc>,
    /// Statische Item-Definitionen (Item System V1, Content-Schicht).
    /// Wird beim Start aus item_definitions geladen; Konsum durch
    /// Inventory/Crafting/Loot folgt in späteren Systemen.
    #[allow(dead_code)]
    pub item_definitions: HashMap<String, crate::item::ItemDefinition>,
    /// Loot-Tabellen (Loot System V1, Content-Schicht, Migration 017).
    pub loot_tables: HashMap<i64, crate::loot::LootTable>,
    /// Aktiver Boden-Loot (Loot System V1): id = "loot_<n>".
    pub loot_drops: HashMap<String, crate::loot::WorldLoot>,
    /// Monoton steigender Zähler für Loot-IDs.
    pub loot_next_id: i64,
    /// Verbindung (interne Conn-ID) → Spieler-ID. Genau EIN Eintrag je
    /// aktivem Charakter (docs/Login_Realm_Architektur.md „Verbindungs-
    /// Einzigkeit und Takeover“): der Eintrag ist die Berechtigung zur
    /// Spiellogik und damit zugleich die Eigentümer-Generation.
    pub by_conn: HashMap<u64, String>,
    /// Schließ-Signale je Verbindung (Socket-Closes laufen über net.rs).
    pub closers: HashMap<u64, tokio::sync::oneshot::Sender<()>>,
    /// Direkte TCP-Peer-Adresse je Verbindung — AUSSCHLIESSLICH im RAM
    /// (docs/netzwerk_ip_schutz.md). Bewusst NICHT protokolliert: eine
    /// dauerhafte IP-Protokollierung mit Löschfrist ist ein eigener
    /// Auftrag (AUTH-03B) und braucht einen freigegebenen Log-Sink.
    pub peer_addrs: HashMap<u64, String>,
    /// `P-18`: Ability-IDs mit `cooldown_persistent = 1`, aus der **tatsächlich
    /// geladenen** Ability-Registry. Bestimmt ausschließlich, welche
    /// Cooldowns den **Tod** überdauern; alle anderen werden beim Tod
    /// zurückgesetzt (docs/Player_Persistenz.md §23).
    ///
    /// Gefüllt wird das Set beim Start des WebSocket-Spielers, unmittelbar nach
    /// dem Laden der Fähigkeitsdefinitionen und damit **vor** dem Binden der
    /// Listener — zu diesem Zeitpunkt kann noch kein Kampf stattgefunden haben.
    /// Solange das Set leer ist, überdauert **kein** Cooldown den Tod; das ist
    /// die sichere Richtung, weil die Seed-Fähigkeiten ohnehin ohne Markierung
    /// ausgeliefert werden.
    pub persistent_cooldown_ids: HashSet<String>,
    pub tick: TickStat,
    pub started: Instant,
}

impl World {
    pub fn new() -> Self {
        Self {
            players: HashMap::new(),
            runtime_id: crate::item_lifecycle::new_runtime_id(),
            npcs: HashMap::new(),
            item_definitions: HashMap::new(),
            loot_tables: HashMap::new(),
            loot_drops: HashMap::new(),
            loot_next_id: 1,
            sell_history: HashMap::new(),
            item_lifecycle: HashMap::new(),
            by_conn: HashMap::new(),
            closers: HashMap::new(),
            peer_addrs: HashMap::new(),
            persistent_cooldown_ids: HashSet::new(),
            tick: TickStat::default(),
            started: Instant::now(),
        }
    }
}

/// Verbindungs-ID eines Spielers (für gezielte Socket-Closes).
pub fn conn_of(world: &World, player_id: &str) -> Option<u64> {
    world
        .by_conn
        .iter()
        .find_map(|(c, p)| (p == player_id).then_some(*c))
}

/// Ist `conn_id` aktueller Eigentümer von `player_id`? Jedes Cleanup, das
/// Player- oder Persistenzzustand entfernt, MUSS diese Prüfung unter der
/// World-Sperre treffen (docs/Login_Realm_Architektur.md: Cleanup einer
/// verdrängten Verbindung entfernt weder Player noch neuen Eigentümer).
pub fn is_owner(world: &World, conn_id: u64, player_id: &str) -> bool {
    world.by_conn.get(&conn_id).is_some_and(|p| p == player_id)
}

/// Verbindungsbezogene Felder, die ein Eigentümerwechsel aktualisiert.
/// Der übrige RAM-Zustand des Players (Position, HP/Mana, Inventar, Quests,
/// Effekte, Cooldowns, Kampf, Dirty-State, Persistenzgeneration/-revision)
/// bleibt unangetastet — der RAM-Player ist maßgeblich und darf nicht von
/// einem älteren DB-Stand überschrieben werden.
pub struct ConnectionFields {
    pub tx: mpsc::UnboundedSender<String>,
    pub session_id: String,
    pub lang: String,
}

fn apply_connection_fields(p: &mut Player, conn: &ConnectionFields) {
    p.tx = conn.tx.clone();
    p.session_id = conn.session_id.clone();
    p.lang = conn.lang.clone();
    p.last_activity = Instant::now();
}

/// Ergebnis des atomaren Login-Commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOutcome {
    /// Erstlogin: der Kandidat wurde als Player registriert.
    Registered,
    /// Erneutes HELLO derselben Verbindung: Eigentümer bleibt, nur die
    /// verbindungsbezogenen Felder werden aufgefrischt.
    Refreshed,
    /// Übernahme eines Players, der nach fehlgeschlagenem Disconnect-Save im
    /// autoritativen RAM behalten wurde (§16): RAM-Zustand bleibt maßgeblich,
    /// es gab keinen Vorgänger zu entmachten.
    Adopted,
    /// Takeover: `old_conn_id` wurde VOR der Freigabe entmachtet.
    Takeover { old_conn_id: u64 },
}

/// Fail-closed-Vorprüfung vor jedem DB-Zugriff im HELLO: Nur der
/// authentifizierte Account, der den aktiven RAM-Player besitzt, darf den
/// Charakter übernehmen. Verhindert zusätzlich, dass ein fremder Account den
/// Charakter über die DB lädt, und schließt die Registry-Inkonsistenz aus, die
/// `commit_login` sonst als kontrollierten Fehler nach der Offline-Abrechnung
/// ablehnen würde (docs/Security.md `P-32`).
///
/// **Stabilität bis `commit_login`:** Der Aufrufer hält das per-player-Gate vom
/// Login bis über den Commit. Damit kann `disconnect_conn` (das `by_conn` und
/// `players` gemeinsam entfernt) für denselben Charakter nicht laufen, und kein
/// weiterer Produktionsschreiber schreibt `by_conn` oder `players` für diesen
/// Charakter. Die `account_id` eines vorhandenen RAM-Players kann in diesem
/// Fenster nicht wechseln. Damit bleiben **alle heutigen kontrollierten
/// `commit_login`-Fehlerbedingungen bis zum Commit ausgeschlossen**.
///
/// **Pflicht für neue Registry-Schreiber:** Jeder künftige Schreibvorgang auf
/// `by_conn` oder `players`, der den Charakter eines laufenden Logins betrifft,
/// muss das per-player-Gate nehmen. Andernfalls kann der hier geprüfte Zustand
/// zwischen Vorprüfung und `commit_login` wechseln, und die Vorprüfung würde
/// ihre Garantie verlieren.
pub fn ensure_takeover_allowed(
    world: &World,
    player_id: &str,
    account_id: u32,
) -> Result<(), String> {
    // Zwei fail-closed Vorprüfungen. Beide müssen VOR dem ersten DB-Zugriff
    // liegen, weil die Offline-Abrechnung unmittelbar vor `commit_login`
    // läuft (docs/Login_Realm_Architektur.md, „Offline-Abrechnung unmittelbar
    // vor dem Commit"; docs/Security.md `P-32`): ein kontrollierter Fehler des
    // Commits würde sonst eine bereits verbrauchte Offline-Zeit hinterlassen.
    match world.players.get(player_id) {
        // (1) Fremder Account: der Charakter darf nicht einmal geladen werden.
        Some(p) if p.account_id != account_id => {
            Err("character is owned by another account".to_string())
        }
        // (2) Registry-Invariante: existiert der Player nicht, darf auch keine
        // Verbindungszuordnung auf ihn zeigen. `commit_login` lehnt genau
        // diesen Zustand ab ("inconsistent connection registry"); die
        // Ablehnung muss hier erfolgen, damit die Abrechnung nicht ohne
        // erfolgreichen Login verbraucht wird. **Kein automatisches Reparieren
        // und kein Entfernen der inkonsistenten Zuordnung** — die Registry wird
        // ausschließlich über `commit_login` verändert.
        None if conn_of(world, player_id).is_some() => Err(format!(
            "inconsistent connection registry for character {player_id}"
        )),
        _ => Ok(()),
    }
}

/// EINZIGER Punkt, der `by_conn` (die Berechtigung zur Spiellogik) verändert.
/// Wird unter der World-Sperre aufgerufen und ist damit atomar: Auth,
/// Charakterprüfung und alle weiteren falliblen Schritte (inkl. Elternkontrolle)
/// müssen VORHER erfolgreich gewesen sein, sonst wird der Aufrufer abgelehnt.
///
/// - Erstlogin: Kandidat wird Player, `conn_id` wird Eigentümer.
/// - Zweiter Login: der bestehende RAM-Player bleibt maßgeblich; die alte
///   `conn_id` wird zuerst aus `by_conn` entfernt (entmachtet) und erst danach
///   die neue eingetragen. Beide Verbindungen sind nie gleichzeitig berechtigt.
/// - Fehler: fail-closed, die Registry bleibt unverändert.
pub fn commit_login(
    world: &mut World,
    conn_id: u64,
    candidate: Player,
    conn: ConnectionFields,
) -> Result<CommitOutcome, String> {
    let player_id = candidate.id.clone();
    let account_id = candidate.account_id;
    let previous_owner = conn_of(world, &player_id);
    match (world.players.contains_key(&player_id), previous_owner) {
        (false, None) => {
            world.players.insert(player_id.clone(), candidate);
            world.by_conn.insert(conn_id, player_id);
            Ok(CommitOutcome::Registered)
        }
        // Registry-Inkonsistenz (Zuordnung ohne Player): fail-closed, damit
        // nie zwei Eigentümer entstehen.
        (false, Some(_)) => Err(format!(
            "inconsistent connection registry for character {player_id}"
        )),
        // Player im RAM, aber keine Eigentümer-Verbindung: der Fall
        // „Disconnect-Save fehlgeschlagen" (§16, siehe `release_conn`) oder ein
        // Zwischenstand aus dem Startup. Der RAM-Player ist der zuletzt
        // autoritative Zustand und wird ÜBERNOMMEN — ein Registrieren aus der
        // DB-Zeile würde genau diesen Zustand überschreiben.
        (true, None) => {
            {
                let p = world
                    .players
                    .get(&player_id)
                    .expect("player presence checked above");
                if p.account_id != account_id {
                    return Err("character is owned by another account".to_string());
                }
            }
            let p = world
                .players
                .get_mut(&player_id)
                .expect("player presence checked above");
            apply_connection_fields(p, &conn);
            world.by_conn.insert(conn_id, player_id);
            Ok(CommitOutcome::Adopted)
        }
        (true, Some(old)) if old == conn_id => {
            let p = world
                .players
                .get_mut(&player_id)
                .expect("player presence checked above");
            apply_connection_fields(p, &conn);
            Ok(CommitOutcome::Refreshed)
        }
        (true, Some(old)) => {
            {
                let p = world
                    .players
                    .get(&player_id)
                    .expect("player presence checked above");
                if p.account_id != account_id {
                    return Err("character is owned by another account".to_string());
                }
            }
            let p = world
                .players
                .get_mut(&player_id)
                .expect("player presence checked above");
            apply_connection_fields(p, &conn);
            // Atomarer Eigentümerwechsel: alte Zuordnung ZUERST entfernen,
            // dann die neue setzen. Der RAM-Player wird nicht ersetzt.
            world.by_conn.remove(&old);
            world.by_conn.insert(conn_id, player_id);
            Ok(CommitOutcome::Takeover { old_conn_id: old })
        }
    }
}

/// Ist die geladene DB-Zeile durch einen offenen Spool-Snapshot überholt?
///
/// Rein und damit im Test prüfbar. `Ok(None)` (kein offener Batch): die
/// DB-Zeile ist aktuell. `Ok(Some(rev))`: nur bei `rev > db_revision` ist sie
/// veraltet. `Err` (Spool nicht lesbar) gilt als veraltet — ungeprüfter
/// persistenter Zustand ist kein Grund für einen Login
/// (docs/Player_Persistenz.md §34/§37, docs/Security.md AUTH-03).
pub fn db_row_is_stale(db_revision: i64, pending: Result<Option<i64>, String>) -> bool {
    match pending {
        Err(_) => true,
        Ok(Some(rev)) => rev > db_revision,
        Ok(None) => false,
    }
}

/// Gibt die Eigentümerschaft einer Verbindung frei, BEHÄLT aber den
/// serverautoritativen Player im RAM.
///
/// docs/Player_Persistenz.md §16: Nach einem fehlgeschlagenen Disconnect-Save
/// bleibt der Spieler im autoritativen RAM (Dirty-State/Revision unverändert),
/// damit ein späterer Flush erneut versuchen kann. Würde er entfernt, ginge
/// der letzte autoritative Zustand verloren. Der nächste Login übernimmt diesen
/// Player über `commit_login` (`CommitOutcome::Adopted`) und setzt die
/// Eigentümerschaft neu — es wird zu keinem Zeitpunkt ein veralteter
/// DB-Stand zur aktiven Instanz.
pub fn release_conn(world: &mut World, conn_id: u64) -> Option<String> {
    world.by_conn.remove(&conn_id)
}

/// Socket einer Verbindung schließen (HELLO-Ablehnung, Force-Logout).
/// Danach räumt der Disconnect-Pfad in net.rs auf.
pub fn close_conn(world: &mut World, conn_id: u64) {
    if let Some(tx) = world.closers.remove(&conn_id) {
        let _ = tx.send(());
    }
}

pub type Shared = Arc<Mutex<World>>;

pub fn new_shared() -> Shared {
    Arc::new(Mutex::new(World::new()))
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).hypot(ay - by)
}

/// Sendet SPAWN + STATE an o (falls p noch nicht sichtbar).
pub fn ensure_visible(o: &mut Player, p: &Player) {
    if !o.entities.contains(&p.id) {
        o.send(&Frame::new(
            0,
            s2c::SPAWN,
            serde_json::json!({"id": p.id, "kind": "player", "x": p.x, "y": p.y, "face": p.face}),
        ));
        o.entities.insert(p.id.clone());
    }
    o.send(&Frame::new(
        0,
        s2c::STATE,
        serde_json::json!({
            "id": p.id, "x": p.x, "y": p.y, "face": p.face,
            "hp": p.hp, "max_hp": p.max_hp
        }),
    ));
}

/// Welt-Tick: AOFB-Broadcast (SPAWN/STATE/DESPAWN) für alle Spieler UND
/// NPCs (Combat V2). Wie im Übergangsstand O(list²), ausreichend für die
/// Kanalgröße (40–70 Spieler). Arbeitet auf einem Positions-Snapshot, damit
/// keine Borrow-Konflikte zwischen Leser (p) und Schreiber (q) entstehen.
pub fn world_tick(world: &mut World, aofb_radius: f64) {
    let t0 = Instant::now();
    // Entity-Snapshot: Spieler + lebende/kehrende NPCs (tote sind unsichtbar).
    let mut snap: Vec<(String, String, f64, f64, f64, i32, i32)> = Vec::new();
    for p in world.players.values() {
        snap.push((
            p.id.clone(),
            "player".into(),
            p.x,
            p.y,
            p.face,
            p.hp,
            p.max_hp,
        ));
    }
    for n in world.npcs.values() {
        if n.status == crate::npc::NpcStatus::Dead {
            continue; // tot → unsichtbar (Respawn kommt später)
        }
        snap.push((n.id.clone(), "npc".into(), n.x, n.y, 0.0, n.hp, n.max_hp));
    }
    for l in world.loot_drops.values() {
        snap.push((l.id.clone(), "loot".into(), l.x, l.y, 0.0, 0, 0));
    }
    for (qid, _, qx, qy, _, _, _) in &snap {
        if !world.players.contains_key(qid) {
            continue; // nur Spieler empfangen Frames
        }
        let mut now_visible = HashSet::new();
        for (pid, _, px, py, _, _, _) in &snap {
            if pid == qid {
                continue;
            }
            if dist(*px, *py, *qx, *qy) <= aofb_radius {
                now_visible.insert(pid.clone());
            }
        }
        let Some(q) = world.players.get_mut(qid.as_str()) else {
            continue;
        };
        for (pid, kind, px, py, face, hp, max_hp) in &snap {
            if pid == qid || !now_visible.contains(pid) {
                continue;
            }
            if kind == "player" {
                if !q.entities.contains(pid) {
                    q.send(&Frame::new(
                        0,
                        s2c::SPAWN,
                        serde_json::json!({"id": pid, "kind": "player", "x": px, "y": py, "face": face}),
                    ));
                    q.entities.insert(pid.clone());
                }
                q.send(&Frame::new(
                    0,
                    s2c::STATE,
                    serde_json::json!({
                        "id": pid, "x": px, "y": py, "face": face,
                        "hp": hp, "max_hp": max_hp
                    }),
                ));
            } else if kind == "loot" {
                // Loot: LOOT-Frame bei jedem sichtbaren Spieler
                // (beim ersten Sichten einmalig; danach je Tick Mirror
                // von STATE; DESPAWN läuft über stale/entities).
                let Some(l) = world.loot_drops.get(pid.as_str()) else {
                    continue;
                };
                let payload = crate::loot::loot_json(l);
                if !q.entities.contains(pid) {
                    q.send(&Frame::new(0, s2c::LOOT, payload.clone()));
                    q.entities.insert(pid.clone());
                }
                q.send(&Frame::new(0, s2c::LOOT, payload));
            } else {
                let (status, aggro, claimed, name) = {
                    let n = world.npcs.get(pid.as_str());
                    (
                        n.map(|n| n.status.key()).unwrap_or("alive"),
                        n.map(|n| n.target_id.is_some()).unwrap_or(false),
                        n.map(|n| n.claimed_by.is_some()).unwrap_or(false),
                        n.map(|n| n.name.clone()).unwrap_or_default(),
                    )
                };
                if !q.entities.contains(pid) {
                    q.send(&Frame::new(
                        0,
                        s2c::SPAWN,
                        serde_json::json!({
                            "id": pid, "kind": "npc", "x": px, "y": py, "face": face,
                            "extra": {"status": status, "aggro": aggro, "claimed": claimed, "name": name}
                        }),
                    ));
                    q.entities.insert(pid.clone());
                }
                q.send(&Frame::new(
                    0,
                    s2c::STATE,
                    serde_json::json!({
                        "id": pid, "x": px, "y": py, "face": face,
                        "hp": hp, "max_hp": max_hp,
                        "kind": "npc", "status": status, "aggro": aggro, "claimed": claimed
                    }),
                ));
            }
        }
        let stale: Vec<String> = q
            .entities
            .iter()
            .filter(|e| !now_visible.contains(*e))
            .cloned()
            .collect();
        for eid in stale {
            q.send(&Frame::new(0, s2c::DESPAWN, serde_json::json!({"id": eid})));
            q.entities.remove(&eid);
        }
        for id in &now_visible {
            q.entities.insert(id.clone());
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    world.tick.last_ms = ms;
    world.tick.count += 1;
    world.tick.avg_ms += (ms - world.tick.avg_ms) / world.tick.count as f64;
}

/// HP-/Mana-Regeneration pro Tick (docs/Attribute_und_Regeneration.md
/// §§4–8): absolute Raten pro Sekunde × Zustandsmultiplikator
/// (Kampf 15 %, stehend 100 %, sitzend 125 % — Sitzbonus nie im Kampf),
/// intern f64 mit Carry je Ressource; gedeckelt auf 0 bzw. max. Tote
/// (hp == 0) regenerieren nicht (keine Wiederbelebung). Teilt sich die
/// gemeinsame Kernlogik in crate::regen (HP UND Mana, NPC-fähig).
pub fn world_regen_tick(world: &mut World, tick_ms: u64) {
    for p in world.players.values_mut() {
        crate::regen::apply_regen(p, tick_ms);
    }
}

/// Verbindungs-spezifischer Disconnect (AUTH-03): entfernt ausschließlich die
/// Zuordnung der übergebenen `conn_id` — KEIN charakterweites `retain`, das
/// einen inzwischen neuen Eigentümer mit entfernen würde. Ist `conn_id` nicht
/// (mehr) Eigentümer, wird weder sie selbst noch der Player entfernt; der
/// Aufrufer bekommt `None` und überspringt den Spieler-Cleanup.
/// Gibt die Spieler-ID zurück, wenn diese Verbindung (und nur sie) Eigentümer
/// war und der Player samt DESPAWN-Broadcast entfernt wurde.
/// Der Socket wird geschlossen, sobald der Kanal-Sender wegfällt
/// (Forward-Task in net.rs beendet sich dann selbst).
pub fn disconnect_conn(world: &mut World, conn_id: u64) -> Option<String> {
    let player_id = world.by_conn.get(&conn_id)?.clone();
    world.by_conn.remove(&conn_id);
    let me = world.players.remove(&player_id)?;
    // Session-Ende: Die Sell-/Buyback-History wird verworfen
    // (docs/Handelssystem.md §2). Die Lifecycle-Metadaten werden hier erst
    // entfernt, weil der maßgebliche finale Snapshot VOR diesem Aufruf
    // geflusht wurde (`finish_owner` in net.rs; bei Savefehler läuft
    // `release_conn` statt dieser Funktion und alles bleibt im RAM).
    world.sell_history.remove(&player_id);
    world.item_lifecycle.remove(&player_id);
    let frame = Frame::new(0, s2c::DESPAWN, serde_json::json!({"id": me.id}));
    for q in world.players.values() {
        if q.entities.contains(&me.id) {
            q.send(&frame);
        }
    }
    // Eigene entities-Sicht der anderen bereinigen.
    for q in world.players.values_mut() {
        q.entities.remove(&me.id);
    }
    Some(player_id)
}

/// MOVE-Anwendung mit Speed-Cap (max 210 m/s, skaliert aufs Tick-
/// Intervall — keine Teleports). Reine Funktion, testbar.
pub fn apply_move(x: f64, y: f64, dx: f64, dy: f64, tick_ms: u64) -> (f64, f64) {
    let len = dx.hypot(dy);
    if len == 0.0 {
        return (x, y);
    }
    let max_step = 210.0 * (tick_ms as f64 / 1000.0);
    let step = len.min(max_step);
    (x + dx / len * step, y + dy / len * step)
}

/// Chat-Text kürzen (240 Zeichen, wie Übergangsstand).
pub fn truncate_chat(text: &str) -> String {
    text.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_player(id: &str, x: f64, y: f64) -> (Player, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Player {
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
                level: 1,
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
                cooldowns: std::collections::BTreeMap::new(),
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
                inventory: Default::default(),
                quests: Default::default(),
                dirty: Default::default(),
                persist_generation: 0,
                persist_revision: 0,
            },
            rx,
        )
    }

    #[test]
    fn tick_spawns_and_despawns_by_radius() {
        let mut w = World::new();
        let (a, mut ra) = test_player("a", 0.0, 0.0);
        let (b, _rb) = test_player("b", 5.0, 0.0);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);
        world_tick(&mut w, 20.0);
        // a sieht b: SPAWN + STATE (Reihenfolge: erst SPAWN, dann STATE je Tick).
        let m1 = ra.try_recv().unwrap();
        let m2 = ra.try_recv().unwrap();
        assert!(m1.contains("\"type\":2"), "erst SPAWN, got {m1}");
        assert!(m2.contains("\"type\":4"), "dann STATE, got {m2}");
        assert!(w.players["a"].entities.contains("b"));
        // b weit weg -> DESPAWN.
        w.players.get_mut("b").unwrap().x = 1000.0;
        world_tick(&mut w, 20.0);
        let mut saw_despawn = false;
        while let Ok(m) = ra.try_recv() {
            if m.contains("\"type\":3") {
                saw_despawn = true;
            }
        }
        assert!(saw_despawn);
        assert!(!w.players["a"].entities.contains("b"));
    }

    #[test]
    fn move_cap_blocks_teleport() {
        // Tick 100ms -> max 21 m pro MOVE.
        let (x, y) = apply_move(0.0, 0.0, 1000.0, 0.0, 100);
        assert!((x - 21.0).abs() < 1e-9 && y == 0.0);
        let (x, y) = apply_move(0.0, 0.0, 3.0, 4.0, 100);
        assert!((x - 3.0).abs() < 1e-9 && (y - 4.0).abs() < 1e-9);
        let (x, y) = apply_move(1.0, 1.0, 0.0, 0.0, 100);
        assert_eq!((x, y), (1.0, 1.0));
    }

    #[test]
    fn chat_truncated_to_240_chars() {
        assert_eq!(truncate_chat(&"x".repeat(300)).chars().count(), 240);
        assert_eq!(truncate_chat("hi"), "hi");
    }

    // ── AUTH-03: Verbindungs-Einzigkeit, Takeover, owner-sicheres Cleanup ──

    /// Frisch aus der DB geladener Login-Kandidat (Werte bewusst
    /// markant, damit ein Überschreiben des RAM-Zustands sofort auffällt).
    fn candidate(id: &str, account_id: u32) -> (Player, mpsc::UnboundedReceiver<String>) {
        let (mut p, rx) = test_player(id, 0.0, 0.0);
        p.account_id = account_id;
        p.x = 999.0;
        p.y = 999.0;
        p.hp = 1;
        p.persist_revision = 7;
        (p, rx)
    }

    fn conn_fields(session_id: &str) -> (ConnectionFields, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            ConnectionFields {
                tx,
                session_id: session_id.to_string(),
                lang: "de".to_string(),
            },
            rx,
        )
    }

    /// Erstlogin: genau EIN Eigentümer, keine Zweitzuordnung.
    #[test]
    fn first_login_registers_exactly_one_owner() {
        let mut w = World::new();
        let (p, _) = candidate("hero", 7);
        let (cf, _rx) = conn_fields("sess-1");
        assert_eq!(
            commit_login(&mut w, 1, p, cf),
            Ok(CommitOutcome::Registered)
        );
        assert_eq!(conn_of(&w, "hero"), Some(1));
        assert!(is_owner(&w, 1, "hero"));
        assert_eq!(w.by_conn.len(), 1);
        assert_eq!(w.players.len(), 1);
    }

    /// Takeover: alter Eigentümer wird VOR der Freigabe des neuen entmachtet,
    /// der RAM-Zustand inkl. Dirty-State und Persistenzrevision bleibt
    /// erhalten (kein Überschreiben durch den DB-Kandidaten).
    #[test]
    fn takeover_keeps_ram_state_and_demotes_old_owner() {
        let mut w = World::new();
        let (p1, _rx1) = candidate("hero", 7);
        let (cf1, _r1) = conn_fields("sess-1");
        commit_login(&mut w, 1, p1, cf1).unwrap();
        // Laufzeit-Zustand, der durch keinen DB-Snapshot ersetzt werden darf.
        {
            let p = w.players.get_mut("hero").unwrap();
            p.x = 12.5;
            p.y = -3.0;
            p.hp = 88;
            p.mana = 21;
            p.effects.push(crate::combat::effects::Effect {
                id: "e1".into(),
                effect_id: "heal".into(),
                group: "heal".into(),
                source_entity: "hero".into(),
                source_kind: crate::combat::effects::SourceKind::Ability,
                target_entity: "hero".into(),
                kind: crate::combat::effects::EffectKind::Hot,
                started_at: Instant::now(),
                duration_ms: 60_000,
                tick_ms: 1_000,
                next_tick_at: None,
                value: 5.0,
                interrupts_on_damage: false,
            });
            p.cooldowns.insert(
                "fire".to_string(),
                std::time::SystemTime::now() + std::time::Duration::from_secs(30),
            );
            p.mark_dirty(crate::persist::PersistComponent::Position);
            p.mark_dirty(crate::persist::PersistComponent::Inventory);
            p.persist_revision = 41;
        }
        let (p2, _rx2) = candidate("hero", 7);
        let (cf2, _r2) = conn_fields("sess-2");
        assert_eq!(
            commit_login(&mut w, 2, p2, cf2),
            Ok(CommitOutcome::Takeover { old_conn_id: 1 })
        );
        // Genau eine berechtigte Verbindung: alt entmachtet, neu Eigentümer.
        assert!(!w.by_conn.contains_key(&1));
        assert!(!is_owner(&w, 1, "hero"));
        assert!(is_owner(&w, 2, "hero"));
        assert_eq!(conn_of(&w, "hero"), Some(2));
        assert_eq!(w.by_conn.len(), 1);
        // RAM-Zustand maßgeblich — keine DB-Werte (999.0/999.0/hp 1/rev 7).
        let p = w.players.get("hero").unwrap();
        assert_eq!((p.x, p.y), (12.5, -3.0));
        assert_eq!((p.hp, p.mana), (88, 21));
        assert_eq!(p.effects.len(), 1);
        assert!(p.cooldowns.contains_key("fire"));
        assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Position));
        assert!(p
            .dirty
            .is_dirty(crate::persist::PersistComponent::Inventory));
        assert_eq!(p.persist_generation, 2);
        assert_eq!(p.persist_revision, 41);
        // Nur verbindungsbezogene Felder wurden übernommen.
        assert_eq!(p.session_id, "sess-2");
    }

    /// Der neue Eigentümer sendet über seinen eigenen Kanal; Frames der
    /// verdrängten Verbindung können den Player nicht mehr erreichen.
    #[test]
    fn displaced_connection_is_not_authenticated_after_takeover() {
        let mut w = World::new();
        let (p1, _cand_rx1) = candidate("hero", 7);
        let (cf1, mut old_rx) = conn_fields("sess-1");
        // Kanal der ersten Verbindung behalten, um die Umstellung zu prüfen.
        let old_tx = cf1.tx.clone();
        commit_login(&mut w, 1, p1, cf1).unwrap();
        let (p2, _cand_rx2) = candidate("hero", 7);
        let (cf2, mut new_rx) = conn_fields("sess-2");
        commit_login(&mut w, 2, p2, cf2).unwrap();
        // Der Player sendet jetzt über den Kanal des neuen Eigentümers.
        w.players
            .get("hero")
            .unwrap()
            .send(&Frame::new(0, s2c::STATE, serde_json::json!({})));
        assert!(new_rx.try_recv().is_ok(), "neuer Kanal ohne Frames");
        assert!(old_rx.try_recv().is_err(), "alter Kanal erhielt Frames");
        // Sendet die verdrängte Verbindung, erreicht es den Player nicht mehr.
        old_tx.send("verdrängt".to_string()).unwrap();
        assert!(old_rx.try_recv().is_ok());
        assert!(new_rx.try_recv().is_err());
    }

    /// Cleanup der verdrängten Verbindung entfernt weder den neuen Owner
    /// noch den Player.
    #[test]
    fn cleanup_of_displaced_connection_keeps_player_and_new_owner() {
        let mut w = World::new();
        let (p1, _r1) = candidate("hero", 7);
        let (cf1, _rx1) = conn_fields("sess-1");
        commit_login(&mut w, 1, p1, cf1).unwrap();
        let (p2, _r2) = candidate("hero", 7);
        let (cf2, _rx2) = conn_fields("sess-2");
        commit_login(&mut w, 2, p2, cf2).unwrap();
        // Alte Verbindung meldet ihr Ende: kein Eigentümer -> kein Cleanup.
        assert_eq!(disconnect_conn(&mut w, 1), None);
        assert!(w.players.contains_key("hero"));
        assert!(is_owner(&w, 2, "hero"));
        assert_eq!(w.by_conn.len(), 1);
    }

    /// Cleanup des aktuellen Owners entfernt weiterhin vollständig.
    #[test]
    fn cleanup_of_current_owner_removes_player() {
        let mut w = World::new();
        let (p1, _r1) = candidate("hero", 7);
        let (cf1, _rx1) = conn_fields("sess-1");
        commit_login(&mut w, 1, p1, cf1).unwrap();
        let (p2, _r2) = candidate("hero", 7);
        let (cf2, _rx2) = conn_fields("sess-2");
        commit_login(&mut w, 2, p2, cf2).unwrap();
        assert_eq!(disconnect_conn(&mut w, 2).as_deref(), Some("hero"));
        assert!(!w.players.contains_key("hero"));
        assert!(!w.by_conn.contains_key(&2));
        assert!(w.by_conn.is_empty());
    }

    /// Fehlgeschlagener Takeover: fremder Account wird abgelehnt, es entstehen
    /// NIE zwei Eigentümer, der alte Owner bleibt unangetastet.
    #[test]
    fn failed_takeover_never_produces_two_owners() {
        let mut w = World::new();
        let (p1, _r1) = candidate("hero", 7);
        let (cf1, _rx1) = conn_fields("sess-1");
        commit_login(&mut w, 1, p1, cf1).unwrap();
        let before = w.players.get("hero").unwrap().session_id.clone();
        let (p2, _r2) = candidate("hero", 8);
        let (cf2, _rx2) = conn_fields("sess-2");
        assert!(commit_login(&mut w, 2, p2, cf2).is_err());
        assert!(ensure_takeover_allowed(&w, "hero", 8).is_err());
        assert!(ensure_takeover_allowed(&w, "hero", 7).is_ok());
        assert_eq!(conn_of(&w, "hero"), Some(1));
        assert!(is_owner(&w, 1, "hero"));
        assert_eq!(w.by_conn.len(), 1);
        assert_eq!(w.players["hero"].session_id, before);
    }

    /// Die Vorprüfung schließt **beide** kontrollierten `commit_login`-Fehler
    /// aus: den fremden Account und die Registry-Inkonsistenz. Ohne den zweiten
    /// Fall könnte `commit_login` nach der Offline-Abrechnung kontrolliert
    /// scheitern und die Offline-Zeit wäre ohne erfolgreichen Login verbraucht
    /// (docs/Security.md `P-32`).
    #[test]
    fn precheck_rejects_registry_inconsistency_without_touching_state() {
        let mut w = World::new();

        // (1) Player fehlt, keine Zuordnung -> erlaubt.
        assert!(ensure_takeover_allowed(&w, "hero", 7).is_ok());

        // (2) Player fehlt, `by_conn` zeigt auf die ID -> fail-closed.
        w.by_conn.insert(9, "hero".into());
        let err = ensure_takeover_allowed(&w, "hero", 7).unwrap_err();
        assert!(
            err.contains("inconsistent connection registry"),
            "Registry-Inkonsistenz muss benannt werden, war: {err}"
        );
        // (5) Die Vorprüfung verändert weder `players` noch `by_conn` und
        // repariert die Inkonsistenz NICHT.
        assert!(w.players.is_empty(), "kein Player angelegt");
        assert_eq!(w.by_conn.len(), 1, "Zuordnung bewusst unverändert");
        assert_eq!(
            conn_of(&w, "hero"),
            Some(9),
            "kein automatisches Reparieren"
        );

        // (2b) Auch mit passendem Konto bleibt die Inkonsistenz abgelehnt.
        assert!(ensure_takeover_allowed(&w, "hero", 42).is_err());

        // (3) Player vorhanden, passender Account -> erlaubt.
        w.by_conn.clear();
        let (p, _r) = candidate("hero", 7);
        w.players.insert("hero".into(), p);
        assert!(ensure_takeover_allowed(&w, "hero", 7).is_ok());

        // (4) Player vorhanden, fremder Account -> fail-closed.
        let err = ensure_takeover_allowed(&w, "hero", 8).unwrap_err();
        assert_eq!(err, "character is owned by another account");
        // (5) unverändert geblieben.
        assert_eq!(w.players.len(), 1);
        assert!(w.by_conn.is_empty());
        assert_eq!(w.players["hero"].account_id, 7);
    }

    /// Die erweiterte Vorprüfung macht den kontrollierten `commit_login`-Fehler
    /// der Registry-Inkonsistenz für den Login-Pfad unerreichbar: der
    /// HELLO-Aufrufer prüft vor der Offline-Abrechnung, `commit_login` selbst
    /// bleibt unverändert fail-closed (rein defensiv).
    #[test]
    fn precheck_excludes_the_registry_error_branch_of_commit_login() {
        let mut w = World::new();
        w.by_conn.insert(9, "hero".into());
        // Vorprüfung lehnt ab …
        assert!(ensure_takeover_allowed(&w, "hero", 7).is_err());
        // … und `commit_login` wäre in diesem Zustand ebenfalls fehlgeschlagen.
        let (p, _r) = candidate("hero", 7);
        let (cf, _rx) = conn_fields("sess-1");
        assert!(commit_login(&mut w, 1, p, cf).is_err());
        assert!(w.players.is_empty());
        assert_eq!(conn_of(&w, "hero"), Some(9));
    }

    /// Player im RAM ohne Eigentümer (Fall: Disconnect-Save fehlgeschlagen,
    /// §16) wird ÜBERNOMMEN: der RAM-Zustand bleibt maßgeblich, es entsteht
    /// genau ein Eigentümer und kein Player wird aus dem DB-Kandidaten
    /// aufgebaut. Ein fremder Account wird weiterhin abgelehnt.
    #[test]
    fn player_without_owner_is_adopted_from_ram() {
        let mut w = World::new();
        let (mut p, _r) = candidate("hero", 7);
        p.x = 42.0;
        p.persist_revision = 9;
        p.mark_dirty(crate::persist::PersistComponent::Position);
        w.players.insert("hero".into(), p);
        let gen_before = w.players["hero"].persist_generation;
        // Der DB-Kandidat ist bewusst ein völlig anderer Stand (x = 999).
        let (cf, _rx) = conn_fields("sess-2");
        assert_eq!(
            commit_login(&mut w, 5, candidate("hero", 7).0, cf),
            Ok(CommitOutcome::Adopted)
        );
        assert!(is_owner(&w, 5, "hero"));
        assert_eq!(w.by_conn.len(), 1);
        // RAM-Zustand unangetastet, nur die verbindungsbezogenen Felder neu.
        let p = &w.players["hero"];
        assert_eq!(p.x, 42.0);
        assert_eq!(p.persist_revision, 9);
        assert_eq!(p.persist_generation, gen_before);
        assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Position));
        assert_eq!(p.session_id, "sess-2");
        // Fremder Account darf den behaltenen Player nicht übernehmen.
        let (cf, _rx) = conn_fields("sess-3");
        assert!(commit_login(&mut w, 6, candidate("hero", 8).0, cf).is_err());
        assert!(is_owner(&w, 5, "hero"));
    }

    /// Registry-Inkonsistenz in der anderen Richtung (Zuordnung ohne Player)
    /// endet fail-closed, statt eine zweite berechtigte Verbindung zu erzeugen.
    #[test]
    fn commit_with_owner_entry_but_no_player_fails_closed() {
        let mut w = World::new();
        w.by_conn.insert(9, "ghost".into());
        let (cf, _rx) = conn_fields("sess-1");
        assert!(commit_login(&mut w, 5, candidate("ghost", 7).0, cf).is_err());
        assert_eq!(w.by_conn.len(), 1);
        assert!(w.players.is_empty());
    }

    /// Nur die Eigentümerschaft freigeben, Player bleibt im RAM (§16).
    #[test]
    fn release_conn_keeps_player_in_ram() {
        let mut w = World::new();
        let (p, _r) = candidate("hero", 7);
        w.players.insert("hero".into(), p);
        w.by_conn.insert(7, "hero".into());
        assert_eq!(release_conn(&mut w, 7).as_deref(), Some("hero"));
        assert!(w.by_conn.is_empty());
        assert!(
            w.players.contains_key("hero"),
            "Player darf nicht entfernt werden"
        );
        // Der nächste Login adoptiert ihn (siehe Test darüber).
        let (cf, _rx) = conn_fields("sess-2");
        assert_eq!(
            commit_login(&mut w, 8, candidate("hero", 7).0, cf),
            Ok(CommitOutcome::Adopted)
        );
    }

    /// Fail-closed-Entscheidung: veraltete DB-Zeile (offener, neuerer
    /// Snapshot) und unlesbarer Spool blockieren; aktuelle DB-Zeile nicht.
    #[test]
    fn db_row_is_stale_is_fail_closed() {
        assert!(db_row_is_stale(7, Ok(Some(8))), "neuerer Snapshot offen");
        assert!(
            !db_row_is_stale(8, Ok(Some(8))),
            "gleiche Revision ist aktuell"
        );
        assert!(
            !db_row_is_stale(9, Ok(Some(8))),
            "älterer Snapshot ist harmlos"
        );
        assert!(!db_row_is_stale(7, Ok(None)), "kein offener Batch");
        assert!(db_row_is_stale(7, Err("dir".into())), "Spool unlesbar");
    }

    /// Erneutes HELLO derselben Verbindung erzeugt keinen zweiten Owner und
    /// ersetzt den Player nicht.
    #[test]
    fn repeated_hello_on_same_connection_keeps_single_owner() {
        let mut w = World::new();
        let (p1, _r1) = candidate("hero", 7);
        let (cf1, _rx1) = conn_fields("sess-1");
        commit_login(&mut w, 1, p1, cf1).unwrap();
        w.players.get_mut("hero").unwrap().x = 5.0;
        let (cf2, _rx2) = conn_fields("sess-2");
        assert_eq!(
            commit_login(&mut w, 1, candidate("hero", 7).0, cf2),
            Ok(CommitOutcome::Refreshed)
        );
        assert_eq!(w.by_conn.len(), 1);
        assert_eq!(conn_of(&w, "hero"), Some(1));
        assert_eq!(w.players["hero"].x, 5.0);
        assert_eq!(w.players["hero"].session_id, "sess-2");
    }

    // ── Item-Lifecycle: History/Metadaten bei Takeover, Übernahme und Ende ──

    fn seed_history_and_lifecycle(w: &mut World, id: &str) {
        w.sell_history
            .entry(id.to_string())
            .or_default()
            .record(crate::item_lifecycle::SellHistoryEntry {
                item_id: "hp_potion".into(),
                item_uuid: "verkauft-1".into(),
                count: 3,
                sell_gold_value: 30,
            });
        crate::item_lifecycle::reconcile_after_take(
            w.item_lifecycle.entry(id.to_string()).or_default(),
            &std::collections::BTreeSet::new(),
            "verkauft-1",
            crate::item_lifecycle::DetachReason::Sold,
            &w.runtime_id.clone(),
            1_700_000_000_000,
        );
    }

    /// Takeover innerhalb des laufenden Prozesses erhält History und
    /// Lifecycle-Metadaten (der RAM-Spieler bleibt maßgeblich).
    #[test]
    fn takeover_preserves_history_and_lifecycle_metadata() {
        let mut w = World::new();
        let (p1, _r1) = candidate("hero", 7);
        let (cf1, _rx1) = conn_fields("sess-1");
        commit_login(&mut w, 1, p1, cf1).unwrap();
        seed_history_and_lifecycle(&mut w, "hero");
        let (p2, _r2) = candidate("hero", 7);
        let (cf2, _rx2) = conn_fields("sess-2");
        assert_eq!(
            commit_login(&mut w, 2, p2, cf2),
            Ok(CommitOutcome::Takeover { old_conn_id: 1 })
        );
        assert_eq!(w.sell_history["hero"].len(), 1);
        assert!(w.item_lifecycle["hero"].contains("verkauft-1"));
    }

    /// RAM-Übernahme (Adopted, z. B. nach fehlgeschlagenem Disconnect-Save)
    /// erhält History und Lifecycle-Metadaten ebenfalls.
    #[test]
    fn adopted_ram_takeover_preserves_history_and_lifecycle_metadata() {
        let mut w = World::new();
        let (p, _r) = candidate("hero", 7);
        w.players.insert("hero".into(), p);
        seed_history_and_lifecycle(&mut w, "hero");
        let (cf, _rx) = conn_fields("sess-2");
        assert_eq!(
            commit_login(&mut w, 5, candidate("hero", 7).0, cf),
            Ok(CommitOutcome::Adopted)
        );
        assert_eq!(w.sell_history["hero"].len(), 1);
        assert!(w.item_lifecycle["hero"].contains("verkauft-1"));
    }

    /// Session-Ende (`disconnect_conn`) verwirft History und Metadaten des
    /// Spielers — der maßgebliche Snapshot wurde davor geflusht.
    #[test]
    fn disconnect_drops_history_and_lifecycle_entries() {
        let mut w = World::new();
        let (p1, _r1) = candidate("hero", 7);
        let (cf1, _rx1) = conn_fields("sess-1");
        commit_login(&mut w, 1, p1, cf1).unwrap();
        seed_history_and_lifecycle(&mut w, "hero");
        assert_eq!(disconnect_conn(&mut w, 1).as_deref(), Some("hero"));
        assert!(!w.sell_history.contains_key("hero"));
        assert!(!w.item_lifecycle.contains_key("hero"));
    }

    /// `release_conn` (Savefehler, §16) behält History und Metadaten im RAM.
    #[test]
    fn release_conn_keeps_history_and_lifecycle_in_ram() {
        let mut w = World::new();
        let (p, _r) = candidate("hero", 7);
        w.players.insert("hero".into(), p);
        w.by_conn.insert(7, "hero".into());
        seed_history_and_lifecycle(&mut w, "hero");
        assert_eq!(release_conn(&mut w, 7).as_deref(), Some("hero"));
        assert_eq!(w.sell_history["hero"].len(), 1);
        assert!(w.item_lifecycle["hero"].contains("verkauft-1"));
    }

    /// Jede World trägt eine eigene Runtime-Kennung.
    #[test]
    fn worlds_carry_distinct_runtime_ids() {
        let a = World::new();
        let b = World::new();
        assert!(a.runtime_id.starts_with("rt-"));
        assert_ne!(a.runtime_id, b.runtime_id);
    }
}
