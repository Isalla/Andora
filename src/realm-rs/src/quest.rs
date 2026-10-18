// quest — Quest V1.1 (Server-/Datenkern).
//
// Verbindliche Grundlage: docs/Quest-System.md §27 („Quest V1 – verbindlicher
// Umfang") sowie docs/quests_stories.md, docs/Datenbank_Architektur.md.
//
// V1.1 umfasst ausschließlich das serverseitige Fundament:
//   * Quest-Zustandsmodell (HIDDEN/AVAILABLE abgeleitet; ACTIVE/COMPLETED/
//     FAILED als Datenzustände)
//   * Trennung Questdefinition vs. Spieler-Questzustand
//   * QuestService (zentrale serverseitige Questlogik)
//   * Persistenz über die bestehende Tabelle `quests` (Migration
//     004_quests.sql)
//   * Laden des Questzustands eines Charakters beim Einstieg (§13)
//   * sichere serverseitige Zustandsübergänge (Annahme, Fortschritt,
//     Abschluss, Doppelabschluss-Schutz)
//
// Bewusst NICHT Teil von V1.1 (keine Gameplay-Anbindung):
//   * Kill-/Talk-/Collect-/Deliver-Event-Anbindung
//   * Gruppen-Kill-Credit (offen, §27.12 — keine Regel wird erfunden)
//   * NPC_TALK-Protokoll, Quest-Protokoll-IDs
//   * Godot-Quest-UI, Questmarker
//   * Lua-Quest-Dateiformat / Lua-Quest-Gameplay-Verdrahtung (§27.9)
//   * FAILED-Trigger, Zeitquests, Timer, Area/Discover, Escort, Craft,
//     Events, Protect, Destroy, Interact
//   * Lua-Stufe 4/5
//
// Die Questdefinition ist eine MINIMALE INTERNE V1.1-Repräsentation. Sie ist
// bewusst KEINE endgültige Content-/Quest-API (das endgültige Format folgt
// laut Lua-Dokumentation später). Questdefinitionen werden nicht pro Spieler
// in MariaDB dupliziert — MariaDB (Tabelle `quests`) persistiert nur den
// individuellen Spieler-Questzustand.
//
// Bestandskorrection (Quest V1.2b — Kill-Credit): the Kill-Event-Anbindung is
// NOW embedded on the production path: `combat_tick` (src/realm-rs/src/combat/
// mod.rs) feeds the Einmaligkeits Alive→Dead Monster-Tod into `add_progress`
// for the documented Claim-/positionsempfängers (docs/Quest-System.md §27.8
// KILL, §27.16 positionsrule — identical recipient rule as the EXP award, no
// new attribution rule), matched by the stable Content-/Definitionskennung
// (`Npc.content_id`, never the Spawn-Key or the Anzeigename). Still open
// connections (§27.15): Talk-/Collect-/Deliver-Gameplay-Anbindung, C2S
// accept/hand-in messages + S2C display, Rewards (no `rewards` field yet),
// and the Quest-Content-/Definition-Loader (Lua-Format §27.9 — the runtime
// Registry stays leer until then, Kill-Credit fail-closed).
//
// Aktueller Erweiterungsstand: NPC-Angebot und ausdrückliche Annahme sind
// über npc_quest.rs angebunden (docs/Quest-System.md §27.27), einschließlich
// optionalem giver, Charakter-Gate und bestätigter DB/RAM-Übernahme. Die
// historischen V1.1/V1.2b-Grenzen oben beschreiben den jeweiligen Vorlauf.
//
// Autorität: Rust (Realm) bleibt autoritativ. Fortschritt darf niemals aus
// einem ungeprüften Clientwert entstehen (§27.8). Die Fortschritts-Schnitt-
// stelle (`add_progress`) akzeptiert ausschließlich serverseitig bestätigte
// Deltas und wird in V1.2 von Combat/NPC-Interaktion/Inventory aufgerufen.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use sqlx::{MySql, Pool};

/// Die fünf dokumentierten Questzustände (§27.5).
///
/// `Hidden` und `Available` sind ABGELEITETE Zustände: Sie werden für einen
/// Charakter nicht als aktiver Questdatensatz in MariaDB persistiert,
/// sondern aus der Questdefinition und dem aktuellen Charakterzustand
/// abgeleitet. `Active`, `Completed` und `Failed` sind Datenzustände der
/// Tabelle `quests` (Spalte `state`, TINYINT).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestState {
    /// Abgeleitet: Voraussetzungen nicht erfüllt, Quest nicht sichtbar.
    Hidden,
    /// Abgeleitet: Voraussetzungen erfüllt, Quest kann angenommen werden.
    Available,
    /// Persistiert: Quest wurde angenommen.
    Active,
    /// Persistiert: Quest wurde erfolgreich abgeschlossen.
    Completed,
    /// Datenzustand des Modells. V1.1 implementiert KEINEN automatischen
    /// Übergang nach FAILED (kein Questtimeout, kein NPC-/Spielertod, kein
    /// Logout, kein Gebietswechsel). Es werden keine FAILED-Ursachen
    /// erfunden.
    Failed,
}

/// Numerische Abbildung der persistierten Zustände auf die Spalte
/// `quests.state` (TINYINT, Migration 004_quests.sql). Dies ist eine
/// interne technische V1.1-Repräsentation, keine öffentliche API. Der
/// Default-Wert 0 der Spalte wird bewusst nicht für die Persistierung
/// eines Zustands verwendet.
impl QuestState {
    pub fn db_value(self) -> i8 {
        match self {
            QuestState::Active => 1,
            QuestState::Completed => 2,
            QuestState::Failed => 3,
            // HIDDEN/AVAILABLE werden nie persistiert (abgeleitet).
            QuestState::Hidden | QuestState::Available => 0,
        }
    }

    pub fn from_db_value(v: i8) -> Option<QuestState> {
        match v {
            1 => Some(QuestState::Active),
            2 => Some(QuestState::Completed),
            3 => Some(QuestState::Failed),
            _ => None,
        }
    }
}

/// Die vier verbindlichen V1-Zieltypen (§27.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveType {
    Kill,
    Talk,
    Collect,
    Deliver,
}

impl ObjectiveType {
    pub fn key(self) -> &'static str {
        match self {
            ObjectiveType::Kill => "kill",
            ObjectiveType::Talk => "talk",
            ObjectiveType::Collect => "collect",
            ObjectiveType::Deliver => "deliver",
        }
    }

    pub fn from_key(key: &str) -> Option<ObjectiveType> {
        match key {
            "kill" => Some(ObjectiveType::Kill),
            "talk" => Some(ObjectiveType::Talk),
            "collect" => Some(ObjectiveType::Collect),
            "deliver" => Some(ObjectiveType::Deliver),
            _ => None,
        }
    }
}

/// Statisches V1-Questziel (Teil der Questdefinition). `kill`, `talk`,
/// `collect` und `deliver` werden sauber unterschieden; eine Gameplay-
/// Anbindung erfolgt erst in V1.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestObjective {
    /// Quest-eindeutige Objective-ID (intern, kein Content-Format).
    pub id: String,
    pub kind: ObjectiveType,
    /// Zielparameter je Zieltyp (z. B. Monster-/NPC-/Item-ID). In V1.1
    /// nur als Unterscheidungsdaten des internen Modells verwendet.
    pub target: String,
    /// Benötigte Menge (bei `talk` üblich 1); muss >= 1 sein.
    pub required: u32,
}

/// Deklarierte Belohnung für die Vorschau: EXP und Idia gemäß §27.26.
/// Keine Vergabe vor Freigabe des gemeinsamen Persistenzvertrags.
/// Weitere Belohnungsfelder werden beim Laden ausdrücklich abgewiesen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RewardBundle {
    pub exp: u64,
    pub idia: u64,
}

impl RewardBundle {
    /// Reine Vorberechnung, keine Mutation und keine Persistenz.
    /// Quest-EXP verwendet den vorhandenen Progressionskern ohne Rested-Bonus.
    pub fn prepare(
        &self,
        progression: &crate::progression::Progression,
        idia: i64,
        cfg: &crate::progression::ProgressionCfg,
    ) -> Result<(crate::progression::Progression, i64), &'static str> {
        let exp = i64::try_from(self.exp).map_err(|_| "quest_reward_overflow")?;
        let reward_idia = i64::try_from(self.idia).map_err(|_| "quest_reward_overflow")?;
        let new_idia = idia.checked_add(reward_idia).ok_or("quest_reward_overflow")?;
        progression.exp.checked_add(exp).ok_or("quest_reward_overflow")?;
        let mut draft = progression.clone();
        // Punkte separat berechnen, damit der bestehende Kern keine vorhandenen
        // freien Punkte überlaufen lässt. Erst danach geprüft zusammenführen.
        draft.free_attr_points = 0;
        draft.grant_exp(cfg, exp);
        draft.free_attr_points = progression.free_attr_points
            .checked_add(draft.free_attr_points)
            .filter(|points| *points <= i32::MAX as u32)
            .ok_or("quest_reward_overflow")?;
        Ok((draft, new_idia))
    }
}

/// Statische Questdefinition (Quest V1 – minimale interne V1.1-
/// Repräsentation, KEINE endgültige Content-API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestDefinition {
    pub id: String,
    /// Stabile NPC-Content-ID des Questgebers; ohne Zuordnung kein NPC-Angebot.
    pub giver: Option<String>,
    /// Stabile NPC-Content-ID des Abgabe-NPC. Ohne explizite Zuordnung keine
    /// NPC-Abgabe: §27.22 definiert bislang keinen Fallback auf `giver`.
    pub turn_in: Option<String>,
    /// i18n-Schlüssel (§24).
    pub title_key: String,
    pub description_key: String,
    pub objectives: Vec<QuestObjective>,
    /// Dokumentierte Voraussetzung „Level" (quests_stories.md §14).
    pub min_level: u32,
    /// Dokumentierte Voraussetzung „vorherige Quest" (§14): Quest-IDs,
    /// die vor der Annahme abgeschlossen sein müssen.
    pub requires: Vec<String>,
    /// V1-Quest ist grundsätzlich nicht wiederholbar; `true` erlaubt eine
    /// erneute Annahme nach COMPLETED (sofern eine Definition dies
    /// ausdrücklich vorsieht, §17-Testliste).
    pub repeatable: bool,
    /// Dokumentierte Belohnungen (§23/§27.26): nur exp+idia (V1). Item-
    /// belohnungen remain open and are rejected at load.
    pub rewards: RewardBundle,
}

impl QuestDefinition {
    pub fn validate(&self) -> Result<(), QuestError> {
        if self.giver.as_ref().is_some_and(|id| id.trim().is_empty()) {
            return Err(QuestError::InvalidDefinition("leerer Questgeber".into()));
        }
        if self.turn_in.as_ref().is_some_and(|id| id.trim().is_empty()) {
            return Err(QuestError::InvalidDefinition("leerer Abgabe-NPC".into()));
        }
        if self.rewards.exp > i64::MAX as u64 || self.rewards.idia > i64::MAX as u64 {
            return Err(QuestError::InvalidDefinition("Belohnung überschreitet i64".into()));
        }
        if self.id.is_empty() {
            return Err(QuestError::InvalidDefinition("leere Quest-ID".into()));
        }
        if self.objectives.is_empty() {
            return Err(QuestError::InvalidDefinition(format!(
                "Quest {}: keine Ziele",
                self.id
            )));
        }
        let mut seen = HashSet::new();
        for o in &self.objectives {
            if o.id.is_empty() || o.target.is_empty() {
                return Err(QuestError::InvalidDefinition(format!(
                    "Quest {}: leeres Ziel-Feld",
                    self.id
                )));
            }
            if !seen.insert(o.id.as_str()) {
                return Err(QuestError::InvalidDefinition(format!(
                    "Quest {}: doppelte Objective-ID {}",
                    self.id, o.id
                )));
            }
            if o.required == 0 {
                return Err(QuestError::InvalidDefinition(format!(
                    "Quest {}: Ziel {} ohne Mindestmenge",
                    self.id, o.id
                )));
            }
        }
        Ok(())
    }
}

/// Fehlerkatalog des QuestService. Beschreibt nur serverseitig geprüfte
/// Ablehnungen; keine offenen Gameplay-Entscheidungen werden darin
/// festgeschrieben.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestError {
    /// Questdefinition existiert nicht (unbekannte Quest-ID).
    UnknownQuest,
    /// Objective gehört nicht zur Questdefinition (unbekannte Objective-ID).
    UnknownObjective,
    /// Quest ist (noch) nicht verfügbar (HIDDEN) bzw. nicht annehmbar —
    /// auch für einen persistierten FAILED-Zustand (Wiederannahme nach
    /// FAILED ist in V1.1 nicht festgelegt; fail-closed).
    NotAvailable,
    /// Quest ist bereits ACTIVE.
    AlreadyActive,
    /// Quest ist (nicht wiederholbar) bereits COMPLETED.
    AlreadyCompleted,
    /// Fortschritt/Abschluss nur für ACTIVE möglich.
    NotActive,
    /// Abschluss verweigert, weil nicht alle Ziele erfüllt sind.
    ObjectivesNotMet,
    /// Abschluss verweigert, weil benötigte Deliver-/Questitems fehlen
    /// (richtige Item-ID / ausreichende Menge, §27.26-Vorprüfung,
    /// docs/quests_stories.md §10).
    DeliverItemsMissing,
    /// Ungültiger serverseitig bestätigter Fortschritt (Delta 0).
    InvalidProgress,
    /// Ungültige Questdefinition.
    InvalidDefinition(String),
}

impl std::fmt::Display for QuestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuestError::UnknownQuest => write!(f, "unbekannte Quest"),
            QuestError::UnknownObjective => write!(f, "unbekanntes Ziel"),
            QuestError::NotAvailable => write!(f, "Quest nicht verfügbar"),
            QuestError::AlreadyActive => write!(f, "Quest bereits aktiv"),
            QuestError::AlreadyCompleted => write!(f, "Quest bereits abgeschlossen"),
            QuestError::NotActive => write!(f, "Quest nicht aktiv"),
            QuestError::ObjectivesNotMet => write!(f, "Ziele nicht erfüllt"),
            QuestError::InvalidProgress => write!(f, "ungültiger Fortschritt"),
            QuestError::DeliverItemsMissing => {
                write!(f, "benötigte Deliver-Items nicht (vollständig) vorhanden")
            }
            QuestError::InvalidDefinition(msg) => write!(f, "ungültige Questdefinition: {msg}"),
        }
    }
}

/// Ein bei der autoritativen Übergabe zu entfernendes Deliver-Item
/// (docs/Quest-System.md §27.13, §27.26 „benötigte Deliver-/Questitems").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRemoval {
    pub item_id: String,
    pub qty: i64,
}

/// Serverseitig validierter Abschlussplan (V1.2a-Fundament, §27.26
/// „Vollständige Vorprüfung"). Wird erzeugt, bevor irgendeine Mutation
/// stattfindet; jede Prüfung schlägt fehl → Quest bleibt ACTIVE, und es
/// wird nichts entfernt, vergeben oder persistiert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionPlan {
    pub quest_id: String,
    /// Beim Abschluss zu entfernende Deliver-Items (nur benötigte Menge).
    pub removals: Vec<ItemRemoval>,
}

/// Ergebnis einer erfolgreichen (abgeschlossen + persistent gesetzten)
/// Abschlussoperation: der COMPLETED-Zustand und das neue Inventar nach
/// Entfernung der Deliver-Items. Der Aufrufer übernimmt beide in die Welt —
/// erst nach erfolgreichem COMMIT der atomaren Abschluss-Transaktion
/// (`complete_for_character`, §27.26 Schritt 5; V1.2a.2).
#[derive(Debug, Clone, PartialEq)]
pub struct CompletedQuest {
    pub state: CharacterQuestState,
    pub inventory: crate::inventory::InventoryState,
}

/// Fehlerkatalog der serverseitigen Abschlussoperation (V1.2a-Fundament):
/// reine Quest-Ablehnungen (`Quest`), Inventarfehler (`Inventory`, z. B.
/// fehlende Menge bei der finalen Entfernung) oder Persistenzfehler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestCompletionError {
    Quest(QuestError),
    Inventory(crate::inventory::InventoryError),
    Persistence(String),
}

impl From<QuestError> for QuestCompletionError {
    fn from(e: QuestError) -> Self {
        QuestCompletionError::Quest(e)
    }
}

impl From<crate::inventory::InventoryError> for QuestCompletionError {
    fn from(e: crate::inventory::InventoryError) -> Self {
        QuestCompletionError::Inventory(e)
    }
}

/// Fehlerkatalog der serverseitigen Questannahme (docs/Player_Persistenz.md
/// §10): reine Quest-Ablehnungen (`Quest` — unveränderte Annahmevalidierung,
/// keine neuen Quest-Semantiken) oder Persistenzfehler der unmittelbaren
/// ACTIVE-Persistierung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestAcceptError {
    Quest(QuestError),
    Persistence(String),
    /// COMMIT wurde versucht; sein Ergebnis ist nicht bestätigt.
    Uncertain(String),
}

impl From<QuestError> for QuestAcceptError {
    fn from(e: QuestError) -> Self {
        QuestAcceptError::Quest(e)
    }
}

/// Individueller Fortschrittswert einer Objective (SPIELER-ZUSTAND).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterObjectiveProgress {
    pub objective_id: String,
    pub current: u32,
}

/// Persistenter Spieler-Questzustand (SPIELER-ZUSTAND). Enthält
/// ausschließlich charakterbezogene Laufzeitdaten und Fortschritt —
/// niemals die Questdefinition (§27.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterQuestState {
    pub quest_id: String,
    pub state: QuestState,
    pub progress: Vec<CharacterObjectiveProgress>,
    /// Epoch-Millisekunden der Annahme (None = unbekannt).
    pub started_at_ms: Option<i64>,
    /// Epoch-Millisekunden des Abschlusses (None = offen/nie).
    pub completed_at_ms: Option<i64>,
}

/// Ergebnis eines serverseitig bestätigten Fortschritts-Schrittes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveDelta {
    pub objective_id: String,
    pub from: u32,
    pub to: u32,
    /// true, wenn dieser Schritt die Objective neu erfüllt hat.
    pub met: bool,
}

/// Validierte Questdefinition auf einer Sinnhaftigkeits-Ebene: eindeutige
/// IDs, nicht-leere Felder, Mindestmenge > 0.
fn objectives_progress(def: &QuestDefinition) -> Vec<CharacterObjectiveProgress> {
    def.objectives
        .iter()
        .map(|o| CharacterObjectiveProgress {
            objective_id: o.id.clone(),
            current: 0,
        })
        .collect()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// QuestService — zentrale serverseitige Quest-Komponente (Quest V1.1).
///
/// Der Service besitzt die Questdefinitionen (statischer Teil, minimales
/// internes V1.1-Format) und implementiert die sicheren serverseitigen
/// Zustandsübergänge. Er hält bewusst KEINEN persistenten Spielerzustand
/// als zweite Wahrheit neben MariaDB; der individuelle Zustand liegt in der
/// Datenbank (`quests`) bzw. im geladenen Charakter-Spielzustand.
#[derive(Default)]
pub struct QuestService {
    definitions: BTreeMap<String, QuestDefinition>,
}

impl QuestService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registriert eine Questdefinition (interne V1.1-Repräsentation,
    /// keine endgültige Content-API). Leere/doppelte Objective-IDs und
    /// Ziellosigkeit werden abgelehnt.
    pub fn register(&mut self, def: QuestDefinition) -> Result<(), QuestError> {
        def.validate()?;
        self.definitions.insert(def.id.clone(), def);
        Ok(())
    }

    /// Quest V1 Content-Loader (docs/Quest-System.md §2 „Lua – statischer
    /// Teil", §27.9): loads the Questdefinitions from the documented
    /// Content-Pfad (the `quests/` Script-Domäne, existing
    /// `crate::lua::script::scan_script_files` + `ScriptManager` sandbox)
    /// and provides them to this shared Service.
    ///
    /// ALL-OR-NOTHING: any defective Definition aborts the WHOLE load —
    /// no partially loaded Definitionbestand is ever registered
    /// („Fehler dürfen keinen unbemerkt teilweise geladenen Definitions-
    /// bestand hinterlassen"). Reasons are reported structured
    /// ([`QuestLoadError`]).
    ///
    /// Full validation BEFORE registration:
    ///   * unique, non-empty Quest-IDs (within the batch and against the
    ///     already registered Definitions),
    ///   * only supported V1-Objective-types (`kill`/`talk`/`collect`/
    ///     `deliver`, §27.1 — others rejected, not silently dropped),
    ///   * valid quantities (`required` integer ≥ 1; existing
    ///     `QuestDefinition.validate` reused),
    ///   * actually resolvable Content-References: `kill`/`talk` targets
    ///     against the loaded Monster-/NPC-Content (`content_id`),
    ///     `collect`/`deliver` targets against the loaded Item-Definitions,
    ///     `requires` against Quest-IDs of the batch/Registry.
    ///
    /// Abgabevorbereitung (§27.28): `turn_in` referenziert NPC-Content;
    /// `rewards{exp,idia}` wird für die Vorschau validiert. Andere Rewardfelder
    /// werden abgewiesen. Vergabe erst nach Persistenzvertragsfreigabe.
    pub fn load_quest_definitions(
        &mut self,
        root: &Path,
        content: &ContentIndex,
    ) -> Result<usize, QuestLoadError> {
        let files = crate::lua::script::scan_script_files(
            root,
            crate::lua::domain::ScriptDomain::Quest,
        )
            .map_err(|e| QuestLoadError::Script(format!("{e}")))?;
        if files.is_empty() {
            return Ok(0);
        }

        let mut manager = crate::lua::runtime::ScriptManager::new()
            .map_err(|e| QuestLoadError::Script(format!("{e}")))?;
        let mut loaded: Vec<QuestDefinition> = Vec::with_capacity(files.len());
        let mut seen: HashSet<String> = HashSet::new();
        for file in files {
            let source = crate::lua::script::ScriptSource::File {
                name: file.name.clone(),
                path: file.path.clone(),
            };
            let (_, json) = manager
                .eval_data(crate::lua::domain::ScriptDomain::Quest, source)
                .map_err(|e| QuestLoadError::Script(format!("{e}")))?;
            let def = build_quest_definition(&json, content)?;
            if !seen.insert(def.id.clone()) || self.definitions.contains_key(&def.id) {
                return Err(QuestLoadError::Duplicate(
                    format!("Quest-ID {} registered twice", def.id)
                ));
            }
            loaded.push(def);
        }
        // `requires` must resolve against the batch or the existing Registry.
        for def in &loaded {
            for req in &def.requires {
                let resolvable = loaded.iter().any(|d| d.id.as_str() == req.as_str())
                    || self.definitions.contains_key(req);
                if !resolvable {
                    return Err(QuestLoadError::UnresolvedRef(
                        format!("Quest {}: Voraussetzung {} is no known Quest-ID", def.id, req)
                    ));
                }
            }
        }

        // Only here: the WHOLE batch is complete and valid → register.
        let count = loaded.len();
        for def in loaded {
            self.register(def)
                .map_err(|e| QuestLoadError::Invalid(format!("{e}")))?;
        }
        Ok(count)
    }

    /// Findet eine Questdefinition nach ID.
    pub fn find_definition(&self, quest_id: &str) -> Option<&QuestDefinition> {
        self.definitions.get(quest_id)
    }

    /// Iteriert über alle registrierten Definitionen.
    pub fn definitions(&self) -> impl Iterator<Item = &QuestDefinition> {
        self.definitions.values()
    }

    /// Abgeleitete Verfügbarkeit (§27.5): HIDDEN oder AVAILABLE aus
    /// Definition + Charakterzustand. Verwendet ausschließlich die für V1
    /// dokumentierten Voraussetzungen (Level, vorherige Quest).
    pub fn derive_availability(
        &self,
        def: &QuestDefinition,
        character_level: u32,
        completed_quests: &HashSet<String>,
    ) -> QuestState {
        if prerequisites_met(def, character_level, completed_quests) {
            QuestState::Available
        } else {
            QuestState::Hidden
        }
    }

    /// Effektiver Questzustand für einen Charakter: persistent vorliegende
    /// Datenzustände (ACTIVE/COMPLETED/FAILED) werden respektiert;
    /// HIDDEN/AVAILABLE werden abgeleitet.
    pub fn state_for(
        &self,
        def: &QuestDefinition,
        current: Option<&CharacterQuestState>,
        character_level: u32,
        completed_quests: &HashSet<String>,
    ) -> QuestState {
        match current {
            Some(c) => match c.state {
                QuestState::Active | QuestState::Completed | QuestState::Failed => c.state,
                // Persistierte HIDDEN/AVAILABLE-Zeilen sind kein Wahrheits-
                // wert (§27.5) → weiterhin sauber ableiten.
                QuestState::Hidden | QuestState::Available => {
                    self.derive_availability(def, character_level, completed_quests)
                }
            },
            None => self.derive_availability(def, character_level, completed_quests),
        }
    }

    /// Serverseitige Annahme-Grundlogik (§8, §27.3). Erzeugt ACTIVE nur,
    /// wenn:
    ///   * die Questdefinition existiert,
    ///   * die Quest für den Charakter AVAILABLE ist (Voraussetzungen
    ///     erfüllt),
    ///   * die Quest nicht unzulässig bereits ACTIVE bzw. (nicht wieder-
    ///     holbar) COMPLETED ist.
    ///
    /// FAILED → ACTIVE ist in V1.1 nicht festgelegt (fail-closed). Diese
    /// Methode wird später vom Questdialog bedient; keine automatische
    /// Annahme durch NPC-Nähe o. ä.
    pub fn accept(
        &self,
        quest_id: &str,
        current: Option<&CharacterQuestState>,
        character_level: u32,
        completed_quests: &HashSet<String>,
    ) -> Result<CharacterQuestState, QuestError> {
        let def = self
            .find_definition(quest_id)
            .ok_or(QuestError::UnknownQuest)?;
        if let Some(c) = current {
            match c.state {
                QuestState::Active => return Err(QuestError::AlreadyActive),
                QuestState::Completed if !def.repeatable => {
                    return Err(QuestError::AlreadyCompleted)
                }
                QuestState::Failed => {
                    // Wiederannahme nach FAILED ist in V1.1 nicht bestimmt —
                    // keine erfundene FAILED→ACTIVE-Regel.
                    return Err(QuestError::NotAvailable);
                }
                _ => {}
            }
        }
        if !prerequisites_met(def, character_level, completed_quests) {
            return Err(QuestError::NotAvailable);
        }
        let now = now_ms();
        Ok(CharacterQuestState {
            quest_id: def.id.clone(),
            state: QuestState::Active,
            progress: objectives_progress(def),
            started_at_ms: Some(now),
            completed_at_ms: None,
        })
    }

    /// Questannahme mit unmittelbarer Persistenz (docs/Player_Persistenz.md
    /// §10): der bedeutende AVAILABLE→ACTIVE-Übergang wird sofort in die
    /// Tabelle `quests` geschrieben — Einfügen ohne Überschreiben über
    /// `write_quest_acceptance` in einer eigenen Transaktion. Ablauf:
    ///
    ///   1. unveränderte Annahmevalidierung über `accept` (keine neuen
    ///      Quest-Semantiken, §10: keine Repeatable-/FAILED-/Abort-Ableitungen),
    ///   2. sofortiges Schreiben des ACTIVE-Zustands in EINER Transaktion,
    ///   3. state-Lieferung erst NACH erfolgreichem COMMIT.
    ///
    /// Der RAM-Übergang (Übernahme in `player.quests`) ist Sache des
    /// Aufrufers und darf ausschließlich nach `Ok` erfolgen. `Persistence`
    /// vor COMMIT erlaubt keinen ACTIVE-Erfolg; `Uncertain` bedeutet dagegen,
    /// dass COMMIT erfolgt sein kann und unter dem Charakter-Gate mit derselben
    /// Annahme geklärt werden muss. Kein stillschweigender RAM-only-ACTIVE.
    /// Der normale Objective-Fortschritt
    /// innerhalb ACTIVE läuft wie dokumentiert über Dirty-State/periodischen
    /// Flush (§8) — ersetzt wird hier nur der AVAILABLE→ACTIVE-Übergang.
    ///
    /// Der periodische Flush (§8) ist KEIN Ersatz für diese Sofortregel.
    /// Persistierbar sind ausschließlich Datenzustände; `encode_state_for_db`
    /// lehnt Abgeleitete ab. Ein Gameplay-Aufrufer (Questdialog) folgt in
    /// V1.2; in V1.1 existiert noch keine Annahme-Handler-Nachricht.
    #[allow(dead_code)]
    pub async fn accept_for_character(
        &self,
        db: &Pool<MySql>,
        char_id: &str,
        quest_id: &str,
        current: Option<&CharacterQuestState>,
        character_level: u32,
        completed_quests: &HashSet<String>,
    ) -> Result<CharacterQuestState, QuestAcceptError> {
        // Schritt 1: unveränderte Annahmevalidierung.
        let accepted = self.accept(quest_id, current, character_level, completed_quests)?;
        Self::persist_acceptance(db, char_id, &accepted).await
    }

    /// Gemeinsamer Sofortpersistenzkern der Annahme. Vorhandene Zeilen
    /// werden niemals überschrieben; insbesondere bleibt ACTIVE-Fortschritt
    /// bei identischer Wiederholung nach unklarem COMMIT erhalten.
    /// Der NPC-Aufrufer hält das Charakter-Gate bis zur RAM-Übernahme und
    /// behandelt Uncertain ausdrücklich nicht als bestätigten Rollback.
    pub async fn persist_acceptance(
        db: &Pool<MySql>,
        char_id: &str,
        accepted: &CharacterQuestState,
    ) -> Result<CharacterQuestState, QuestAcceptError> {
        if accepted.state != QuestState::Active {
            return Err(QuestError::NotAvailable.into());
        }
        let data = encode_state_for_db(accepted).map_err(QuestAcceptError::Persistence)?;
        let mut tx = db.begin().await.map_err(|e| {
            QuestAcceptError::Persistence(format!("Annahme-Transaktion beginnen: {e}"))
        })?;
        let row = crate::db::write_quest_acceptance(&mut tx, char_id, &accepted.quest_id, &data)
            .await
            .map_err(QuestAcceptError::Persistence)?;
        let state = QuestState::from_db_value(row.state)
            .ok_or_else(|| QuestAcceptError::Persistence("ungültiger Questzustand".into()))?;
        let confirmed = decode_quest_data(&row.quest_id, state, row.data.as_deref())
            .map_err(QuestAcceptError::Persistence)?;
        tx.commit().await.map_err(|e| {
            QuestAcceptError::Uncertain(format!("Annahme-Transaktion commit: {e}"))
        })?;
        Ok(confirmed)
    }

    /// Verwaltet serverseitig bestätigten Fortschritt eines ACTIVE-Zustands
    /// (§11). Akzeptiert ausschließlich serverseitig bestätigte Deltas
    /// (z. B. realm-bestätigter Gegner-Tod); ein ungeprüfter Clientwert ist
    /// keine gültige Eingabe. The `kill`-Anbindung is EMBEDDED (V1.2b):
    /// `combat_tick` calls this ONLY at the Einmaligkeits Alive→Dead
    /// Monster-Tod for the documented Claim-/positionsempfängers (§27.8/§27.16).
    /// Talk-/Collect-/Deliver-Gameplay-Anbindung follow in later steps.
    pub fn add_progress(
        &self,
        quest_id: &str,
        state: &mut CharacterQuestState,
        objective_id: &str,
        server_confirmed_delta: u32,
    ) -> Result<ObjectiveDelta, QuestError> {
        let def = self
            .find_definition(quest_id)
            .ok_or(QuestError::UnknownQuest)?;
        if state.state != QuestState::Active {
            return Err(QuestError::NotActive);
        }
        if server_confirmed_delta == 0 {
            return Err(QuestError::InvalidProgress);
        }
        let objective = def
            .objectives
            .iter()
            .find(|o| o.id == objective_id)
            .ok_or(QuestError::UnknownObjective)?;
        let progress = state
            .progress
            .iter_mut()
            .find(|p| p.objective_id == objective_id)
            .ok_or(QuestError::UnknownObjective)?;
        let from = progress.current;
        let already_met = from >= objective.required;
        let to = (from + server_confirmed_delta).min(objective.required);
        progress.current = to;
        Ok(ObjectiveDelta {
            objective_id: objective_id.to_string(),
            from,
            to,
            met: !already_met && to >= objective.required,
        })
    }

    /// Stellt fest, ob alle notwendigen Ziele der Quest erfüllt sind.
    pub fn objectives_complete(&self, quest_id: &str, state: &CharacterQuestState) -> bool {
        let Some(def) = self.find_definition(quest_id) else {
            return false;
        };
        def.objectives.iter().all(|o| {
            state
                .progress
                .iter()
                .find(|p| p.objective_id == o.id)
                .map(|p| p.current >= o.required)
                .unwrap_or(false)
        })
    }

    /// Serverseitige Abschluss-Grundlogik (§9, §27.4). Vor COMPLETED wird
    /// geprüft:
    ///   * Quest existiert (unbekannte ID → UnknownQuest),
    ///   * Spielerzustand ist ACTIVE,
    ///   * erforderliche Ziele sind erfüllt,
    ///   * Abschluss darf nicht doppelt erfolgen (COMPLETED →
    ///     AlreadyCompleted).
    ///
    /// Diese Methode erzeugt den COMPLETED-Zustand; das Persistieren
    /// übernimmt der Aufrufer (`persist_state`). Doppelabschluss derselben
    /// nicht wiederholbaren Quest wird dadurch unmöglich — ein zweiter
    /// Abschluss kann damit keine zweite Belohnung erzeugen.
    pub fn complete(
        &self,
        quest_id: &str,
        state: &CharacterQuestState,
    ) -> Result<CharacterQuestState, QuestError> {
        self.find_definition(quest_id)
            .ok_or(QuestError::UnknownQuest)?;
        match state.state {
            QuestState::Active => {}
            QuestState::Completed => return Err(QuestError::AlreadyCompleted),
            _ => return Err(QuestError::NotActive),
        }
        if !self.objectives_complete(quest_id, state) {
            return Err(QuestError::ObjectivesNotMet);
        }
        let now = now_ms();
        Ok(CharacterQuestState {
            quest_id: state.quest_id.clone(),
            state: QuestState::Completed,
            progress: state.progress.clone(),
            started_at_ms: state.started_at_ms,
            completed_at_ms: Some(now),
        })
    }

    /// V1.2a-Abschlussplan (§27.26 „Vollständige Vorprüfung") — PURE, ohne
    /// jegliche Mutation. Geprüft werden serverseitig und autoritativ:
    ///
    ///   * Quest existiert,
    ///   * Questzustand ist ACTIVE (nicht bereits COMPLETED → keine
    ///     Doppelbelohnung),
    ///   * alle erforderlichen Objectives sind erfüllt,
    ///   * benötigte Deliver-/Questitems sind vorhanden (richtige Item-IDs,
    ///     ausreichende Mengen) — abgeleitet aus den `deliver`-Zielen
    ///     (docs/quests_stories.md §10: „Server entfernt Gegenstände").
    ///
    /// Belohnungs-Prüfungen (Vergabefähigkeit, freie Inventarkapazität für
    /// die vollständige Itembelohnung, EXP/Geld) greifen an derselben
    /// Stelle; die minimale interne V1.1-Definition trägt aktuell keine
    /// Belohnungen, daher sind diese Punkte leer erfüllt (es wird keine
    /// Content-/Reward-Entscheidung erfunden, §27.26 „Reward-Arten").
    ///
    /// `collect`-Ziele sind Besitznachweise (Items bleiben beim Spieler);
    /// `kill`/`talk` haben keine Item-Komponente. Fehlt eine Deliver-Menge,
    /// bleibt die Quest ACTIVE; es wird nichts entfernt oder vergeben.
    pub fn plan_completion(
        &self,
        quest_id: &str,
        state: &CharacterQuestState,
        inventory: &crate::inventory::InventoryState,
    ) -> Result<CompletionPlan, QuestError> {
        let def = self
            .find_definition(quest_id)
            .ok_or(QuestError::UnknownQuest)?;
        match state.state {
            QuestState::Active => {}
            QuestState::Completed => return Err(QuestError::AlreadyCompleted),
            _ => return Err(QuestError::NotActive),
        }
        if !self.objectives_complete(quest_id, state) {
            return Err(QuestError::ObjectivesNotMet);
        }
        let mut removals = Vec::new();
        for o in &def.objectives {
            if o.kind == ObjectiveType::Deliver {
                if inventory.count_of(&o.target) < i64::from(o.required) {
                    return Err(QuestError::DeliverItemsMissing);
                }
                removals.push(ItemRemoval {
                    item_id: o.target.clone(),
                    qty: i64::from(o.required),
                });
            }
        }
        Ok(CompletionPlan {
            quest_id: quest_id.to_string(),
            removals,
        })
    }

    /// V1.2a-Abschluss („AUTORITATIV AUSFÜHREN", §27.26 Schritte 1–4) auf
    /// einem bereits geprüften Plan. Diese reine Funktion:
    ///
    ///   1. validiert den finalen, geschützten Zustand erneut (abschließende
    ///      Validierung direkt vor der Mutation),
    ///   2. entfernt die benötigten Deliver-/Questitems (nur benötigte
    ///      Menge, keine Teilentfernung; §§27.13/27.26),
    ///   3./4. Belohnungen: In V1.2a keine definierten Belohnungen — es wird
    ///      nichts vergeben (keine neuen Reward-Arten, §27.26).
    ///
    /// Der COMPLETED-Zustand wird hier in-memory erzeugt; die Persistenz
    /// (§27.26 Schritt 5) kommt anschließend über `persist_state` bzw.
    /// `complete_for_character`. Tritt irgendein Fehler auf, ist der Zustand
    /// unverändert (keine Teilentfernung, kein Teilerfolg).
    pub fn execute_completion(
        &self,
        plan: &CompletionPlan,
        state: &CharacterQuestState,
        inventory: &mut crate::inventory::InventoryState,
    ) -> Result<CharacterQuestState, QuestCompletionError> {
        // §27.26 Schritt 1: abschließende Validierung des geschützten Zustands.
        if self.find_definition(&plan.quest_id).is_none() {
            return Err(QuestError::UnknownQuest.into());
        }
        if state.state != QuestState::Active {
            return Err(QuestError::NotActive.into());
        }
        if !self.objectives_complete(&plan.quest_id, state) {
            return Err(QuestError::ObjectivesNotMet.into());
        }
        for r in &plan.removals {
            if inventory.count_of(&r.item_id) < r.qty {
                return Err(QuestError::DeliverItemsMissing.into());
            }
        }
        // §27.26 Schritt 2: benötigte Deliver-/Questitems entfernen.
        for r in &plan.removals {
            inventory.try_remove(&r.item_id, r.qty)?;
        }
        // §27.26 Schritt 3/4: Belohnungen — in V1.2a keine definierten.
        // §27.26 Schritt 5 (persistent COMPLETED) übernimmt der Aufrufer
        // (complete_for_character, atomare Abschluss-Transaktion);
        // Schritt 6/7 bleiben Aufgaben des Aufrufers (Locks freigeben,
        // Client bestätigen).
        Ok(self.complete(&plan.quest_id, state)?)
    }

    /// V1.2a.2 atomare Abschlussgrenze: plant, führt auf einem KLON aus und
    /// persistiert alle beteiligten Zustände in EINER MariaDB-Transaktion
    /// (docs/Quest-System.md §27.26 „atomare/transaktionale
    /// Sicherheitsgrenze"; Auditergebnis V1.2a.1). Ablauf innerhalb der
    /// Transaktion:
    ///
    ///   1. Inventar-Vollwrite des resultierenden Zustands (Deliver-Items
    ///      persistent entfernt; spätere Rewarditems können mitgeschrieben
    ///      werden — in V1.2a tragen die Definitionen keine Belohnungen),
    ///   2. DB-seitiger Guard `guarded_complete_quest` (ACTIVE→COMPLETED
    ///      nur, wenn die `quests`-Zeile noch ACTIVE ist; exakt 1 Zeile),
    ///   3. COMMIT — der einzige autoritative persistente Übergang.
    ///
    /// Inventory-/Reward-Writes dürfen VOR dem Guard liegen, weil bei
    /// fehlgeschlagenem Guard (0 Zeilen: Doppelabschluss/konkurrierender
    /// Prozess) die gesamte Transaktion verworfen wird — kein COMMIT.
    ///
    /// Erst NACH erfolgreichem COMMIT wird der vorbereitete Zustand in den
    /// autoritativen RAM (`inventory`) übernommen; bei jeder Abweichung
    /// bleibt alles unverändert (Quest ACTIVE, keine Items entfernt, keine
    /// Belohnung, kein COMPLETED — §27.26 „Fehlerfall/Rollback"). Die
    /// RAM-Übernahme erfolgt aus dem bereits validierten Arbeitszustand
    /// ohne neue Gameplayberechnung.
    ///
    /// Der Aufrufer hält währenddessen die vorhandene World-Serialisierung
    /// (das zentrale `Shared`-Mutex) — dadurch kann zwischen Prüfung,
    /// Entfernung und Persistenz keine konkurrierende Inventaroperation die
    /// geprüfte Grenze aufbrechen. Der DB-seitige Guard schützt darüber
    /// hinaus gegen konkurrierende Realm-Prozesse (der World-Mutex besitzt
    /// keine prozessübergreifende Autorität). Erst nach Ok übernimmt der
    /// Aufrufer `CompletedQuest` in die Spielerwelt.
    #[allow(dead_code)]
    pub async fn complete_for_character(
        &self,
        db: &Pool<MySql>,
        char_id: &str,
        quest_id: &str,
        state: CharacterQuestState,
        inventory: &mut crate::inventory::InventoryState,
    ) -> Result<CompletedQuest, QuestCompletionError> {
        let plan = self.plan_completion(quest_id, &state, inventory)?;
        let mut draft = inventory.clone();
        let completed = self.execute_completion(&plan, &state, &mut draft)?;

        let data = encode_state_for_db(&completed).map_err(QuestCompletionError::Persistence)?;
        let mut tx = db.begin().await.map_err(|e| {
            QuestCompletionError::Persistence(format!("Abschluss-Transaktion beginnen: {e}"))
        })?;

        // §27.26 Schritte 2–4, Teil 1: resultierendes Inventar (Deliver-Items
        // entfernt) innerhalb derselben Transaktion persistent schreiben.
        crate::db::write_inventory(&mut tx, char_id, &draft)
            .await
            .map_err(QuestCompletionError::Persistence)?;
        // Belohnungen EXP/Gold: V1.2a-Definitionen tragen keine Belohnungen —
        // es werden keine künstlichen Quest-EXP/-Gold-Rewards erzeugt. Die
        // transaktionskomponierbaren Pfade `write_progression`/`write_idia`
        // stehen für künftige Belohnungsdefinitionen bereit.

        // §27.26 Schritt 5 + Doppelabschluss-Schutz: DB-Guard für den
        // persistenten ACTIVE→COMPLETED-Übergang (auch gegen zweite
        // Realm-Prozesse). Liefert false bei 0 betroffenen Zeilen.
        let guard_ok =
            crate::db::guarded_complete_quest(&mut tx, char_id, &completed.quest_id, &data)
                .await
                .map_err(QuestCompletionError::Persistence)?;
        if !guard_ok {
            // Guard schlug fehl: Die Quest ist nicht mehr ACTIVE in der DB
            // (bereits COMPLETED durch einen konkurrierenden Prozess /
            // Doppelabschluss). Die Transaktion wird NICHT committet —
            // Inventar-/Quest-Writes verfallen mit dem Rollback (§27.26
            // Fehlerfall). Quest/RAM bleiben unverändert.
            log::warn!(
                "complete_for_character {char_id}/{quest_id}: \
                 ACTIVE→COMPLETED-Guard schlug fehl (keine ACTIVE-Zeile) — kein COMMIT"
            );
            return Err(QuestError::NotActive.into());
        }

        tx.commit().await.map_err(|e| {
            QuestCompletionError::Persistence(format!("Abschluss-Transaktion commit: {e}"))
        })?;
        // Erst nach erfolgreichem COMMIT in den autoritativen RAM übernehmen.
        *inventory = draft;
        Ok(CompletedQuest {
            state: completed,
            inventory: inventory.clone(),
        })
    }

    /// Lädt und rekonstruiert die persistierten Spieler-Questzustände eines
    /// Charakters aus der Tabelle `quests` (§13). Mindestens vorhandene
    /// ACTIVE-, COMPLETED- und FAILED-Zustände werden korrekt rekonstruiert.
    /// HIDDEN/AVAILABLE werden nicht aus DB-Zeilen als Wahrheit geladen,
    /// sondern abgeleitet (§27.5).
    pub async fn load_for_character(
        &self,
        db: &Pool<MySql>,
        char_id: &str,
    ) -> Result<Vec<CharacterQuestState>, String> {
        let rows = crate::db::load_quest_rows(db, char_id).await?;
        let mut out = Vec::new();
        for r in rows {
            let Some(st) = QuestState::from_db_value(r.state) else {
                log::warn!(
                    "Quest {}/{}: unbekannter Zustand {}; Zeile übersprungen",
                    char_id,
                    r.quest_id,
                    r.state
                );
                continue;
            };
            match decode_quest_data(&r.quest_id, st, r.data.as_deref()) {
                Ok(state) => out.push(state),
                Err(e) => log::warn!("Quest {}/{}: {e}", char_id, r.quest_id),
            }
        }
        Ok(out)
    }

    /// Stößt die Persistenz eines Spieler-Questzustands an (§12): Upsert in
    /// die Tabelle `quests` (Spalten state + data). HIDDEN/AVAILABLE werden
    /// hier bewusst nicht geschrieben.
    /// Wiederverbunden in V1.2 (Questdialog/Quests-Abschluss-Flow); in V1.1
    /// offengelegt, aber noch ohne Gameplay-Aufrufer.
    #[allow(dead_code)]
    pub async fn persist_state(
        &self,
        db: &Pool<MySql>,
        char_id: &str,
        state: &CharacterQuestState,
    ) -> Result<(), String> {
        // HIDDEN/AVAILABLE sind ABGELEITETE Zustände (docs/Quest-System.md
        // §27.5) und dürfen nie in die Tabelle `quests` geschrieben werden —
        // die Spalte `state` (Default 0) bekommt dadurch auch nie einen
        // abgeleiteten Zustand.
        let data = encode_state_for_db(state)?;
        crate::db::save_quest_state(db, char_id, &state.quest_id, state.state.db_value(), &data)
            .await
    }
}

fn prerequisites_met(
    def: &QuestDefinition,
    character_level: u32,
    completed_quests: &HashSet<String>,
) -> bool {
    character_level >= def.min_level && def.requires.iter().all(|q| completed_quests.contains(q))
}

/// The actually loaded Content-Registries (Monster-/NPC-`content_id` from
/// `monster_definitions` and Item-Definition-IDs) — the reference-resolvability
/// basis for Quest-Targets. Built at Realm-Start from the REAL registries, never
/// from a Clientclaim (§27.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentIndex {
    pub monsters: HashSet<String>,
    pub items: HashSet<String>,
}

/// Fehlerkatalog des Content-Loader: strukturiert, brauchable Diagnosegrunds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestLoadError {
    /// Lua-Load/Syntax/Runtime-/Sandboxerror of a Definition-Script (reason).
    Script(String),
    /// Definition invalid (missing field, unsupported Objective-type,
    /// invalid quantity).
    Invalid(String),
    /// Duplicate Quest-ID (within the batch or against the Registry).
    Duplicate(String),
    /// Content-Reference not resolvable against the loaded registries.
    UnresolvedRef(String),
}

impl std::fmt::Display for QuestLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuestLoadError::Script(m) => write!(f, "Definition-Script: {m}"),
            QuestLoadError::Invalid(m) => write!(f, "ungültige Questdefinition: {m}"),
            QuestLoadError::Duplicate(m) => write!(f, "doppelte Quest-ID: {m}"),
            QuestLoadError::UnresolvedRef(m) => write!(f, "unresolvable Content-Reference: {m}"),
        }
    }
}

/// Builds and FULLY validates one Questdefinition from the JSON a
/// Definition-Script returned. Follows the documented shape of
/// docs/quests_stories.md §3 (`id`, `title_key`, `description_key`,
/// `requirements = { level = N }`, `objectives = { { type=, target=,
/// required= }, .. }`).
///
/// Provisional decisions (the endgültige Lua-Quest-Format stays open, §27.9 —
/// smallest compatible proposal, see `.tmp/quest-definition-loading/`):
///   * `requirements.requires = { "quest_id", .. }` carries the documented
///     „vorherige Quest" (§14) onto the internal `requires`,
///   * `repeatable` (bool, default false) maps the §17-Testliste flag,
///   * `rewards` must be missing or an empty Table; a NON-EMPTY rewards
///     Table is rejected (Rewards stay open, §27.15/§27.26).
///
/// Rejections (with reason): missing/empty fields, UNKNOWN fields at any
/// level (Quest-Lua-Format V1 whitelist — a Typfehler never silently
/// changes Content), unsupported Objective-types (only V1 `kill`/`talk`/
/// `collect`/`deliver`), non-integer or < 1 quantities, non-empty `rewards`,
/// and Content-References that do not resolve against the actually loaded
/// registries. The existing `QuestDefinition.validate` is reused for
/// ID-/quantity-invariants.
fn build_quest_definition(
    json: &serde_json::Value,
    content: &ContentIndex,
) -> Result<QuestDefinition, QuestLoadError> {
    let Some(obj) = json.as_object() else {
        return Err(QuestLoadError::Invalid("Definition must be a Table/Object".into()));
    };
    // Quest-Lua-Format V1 (explicit contract): unknown fields are REJECTED
    // with a precise reason — a Typfehler never silently changes Content.
    let known_top: [&str; 9] = [
        "id",
        "giver",
        "turn_in",
        "title_key",
        "description_key",
        "requirements",
        "objectives",
        "repeatable",
        "rewards",
    ];
    for k in obj.keys() {
        if !known_top.iter().any(|f| *f == k.as_str()) {
            return Err(QuestLoadError::Invalid(
                format!(
                    "unknown field '{}' (Quest-Lua-Format V1 fields: {})",
                    k,
                    known_top.iter().map(|f| f.to_string()).collect::<Vec<String>>().join(", ")
                )
            ));
        }
    }
    // `rewards` (§23/§27.26): V1 supports ONLY the two documented, inventory-
    // independent kinds `exp` (Quest-EXP §§8–9) and `idia` (Geld). Missing or
    // EMPTY is allowed (no reward). Unknown/unsupported keys (e.g. item-,
    // ausrüstung-, Ruf-, Titel-, Skill-, Achievement-rewards) are REJECTED with
    // an understandable Ladefehler (no silent ignore, no invented Reward-Semantik,
    // no invented Preise/EXP-Mengen). Values must be non-negative integers.
    let rewards = RewardBundle {
        exp: 0,
        idia: 0,
    };
    let rewards = if let Some(rw) = obj.get("rewards") {
        let Some(rwo) = rw.as_object() else {
            return Err(QuestLoadError::Invalid("'rewards' must be a Table".into()));
        };
        for k in rwo.keys() {
            if k != "exp" && k != "idia" {
                return Err(QuestLoadError::Invalid(format!(
                    "unsupported reward kind '{}' (V1: exp, idia — item/other rewards remain open, §27.26)",
                    k
                )));
            }
        }
        let uint_field = |key: &str| -> Result<u64, QuestLoadError> {
            match rwo.get(key) {
                None => Ok(0),
                Some(v) => match v.as_i64() {
                    Some(n) if n >= 0 => match u64::try_from(n) {
                        Ok(m) => Ok(m),
                        Err(_) => Err(QuestLoadError::Invalid(format!("reward '{key}' too large (got {n})"))),
                    },
                    Some(n) => Err(QuestLoadError::Invalid(format!("reward '{key}' must be >= 0 (got {n})"))),
                    None => Err(QuestLoadError::Invalid(format!("reward '{key}' must be an Integer (got {v})"))),
                },
            }
        };
        RewardBundle {
            exp: uint_field("exp")?,
            idia: uint_field("idia")?,
        }
    } else {
        rewards
    };
    let str_field = |key: &str| -> Result<String, QuestLoadError> {
        match obj.get(key) {
            None => Err(QuestLoadError::Invalid(format!("field '{key}' missing"))),
            Some(v) => match v.as_str() {
                Some(s) if !s.is_empty() => Ok(s.to_string()),
                Some(_) => Err(QuestLoadError::Invalid(format!("field '{key}' must not be empty"))),
                None => Err(QuestLoadError::Invalid(format!("field '{key}' must be a String (got {v})"))),
            },
        }
    };
    let id = str_field("id")?;
    let title_key = str_field("title_key")?;
    let description_key = str_field("description_key")?;
    let giver = if obj.contains_key("giver") {
        let gid = str_field("giver")?;
        if !content.monsters.contains(&gid) {
            return Err(QuestLoadError::UnresolvedRef(format!(
                "Quest {id}: giver verweist auf keine geladene NPC-Content-ID"
            )));
        }
        Some(gid)
    } else {
        None
    };
    // Kein dokumentierter Fallback auf giver; die Referenz bleibt optional.
    let turn_in = if obj.contains_key("turn_in") {
        let tid = str_field("turn_in")?;
        if !content.monsters.contains(&tid) {
            return Err(QuestLoadError::UnresolvedRef(format!(
                "Quest {id}: turn_in verweist auf keine geladene NPC-Content-ID"
            )));
        }
        Some(tid)
    } else {
        None
    };

    let Some(req) = obj.get("requirements").and_then(|v| v.as_object()) else {
        return Err(QuestLoadError::Invalid("field 'requirements' missing or not a Table".into()));
    };
    for k in req.keys() {
        if k != "level" && k != "requires" {
            return Err(QuestLoadError::Invalid(
                format!("unknown field 'requirements.{}' (V1: level, requires)", k)
            ));
        }
    }
    let min_level = match req.get("level") {
        None => 0,
        Some(v) => match v.as_i64() {
            Some(n) => match u32::try_from(n) {
                Ok(m) => m,
                Err(_) => return Err(QuestLoadError::Invalid(
                    format!("'requirements.level' must be >= 0 (got {n})")
                )),
            },
            None => return Err(QuestLoadError::Invalid(
                format!("'requirements.level' must be an Integer (got {v})")
            )),
        },
    };
    let requires: Vec<String> = match req.get("requires") {
        None => Vec::new(),
        Some(v) => {
            let Some(arr) = v.as_array() else {
                return Err(QuestLoadError::Invalid("'requirements.requires' must be an Array".into()
                ));
            };
            let mut out = Vec::with_capacity(arr.len());
            for q in arr {
                match q.as_str() {
                    Some(s) if !s.is_empty() => out.push(s.to_string()),
                    _ => return Err(QuestLoadError::Invalid("'requirements.requires' entries must be non-empty Strings".into()
                    )),
                }
            }
            out
        }
    };

    let repeatable = match obj.get("repeatable") {
        None => false,
        Some(v) => match v.as_bool() {
            Some(b) => b,
            None => return Err(QuestLoadError::Invalid(
                format!("'repeatable' must be a Bool (got {v})")
            )),
        },
    };

    let Some(objs) = obj.get("objectives").and_then(|v| v.as_array()) else {
        return Err(QuestLoadError::Invalid("field 'objectives' missing or not an Array".into()
        ));
    };
    if objs.is_empty() {
        return Err(QuestLoadError::Invalid("Quest has no Objectives".into()));
    }
    let mut objectives = Vec::with_capacity(objs.len());
    for (idx, o) in objs.iter().enumerate() {
        let Some(oo) = o.as_object() else {
            return Err(QuestLoadError::Invalid(format!("Objective {} must be a Table", idx + 1)
            ));
        };
        for k in oo.keys() {
            if k != "id" && k != "type" && k != "target" && k != "required" {
                return Err(QuestLoadError::Invalid(
                    format!("Objective {}: unknown field '{}' (V1: id, type, target, required)", idx + 1, k)
                ));
            }
        }
        let str_at = |key: &str| -> Result<String, QuestLoadError> {
            match oo.get(key) {
                None => Err(QuestLoadError::Invalid(format!("Objective {}: field '{key}' missing", idx + 1))),
                Some(v) => match v.as_str() {
                    Some(s) if !s.is_empty() => Ok(s.to_string()),
                    Some(_) => Err(QuestLoadError::Invalid(format!("Objective {}: field '{key}' must not be empty", idx + 1))),
                    None => Err(QuestLoadError::Invalid(format!("Objective {}: field '{key}' must be a String (got {v})", idx + 1))),
                },
            }
        };
        let oid = str_at("id")?;
        let type_key = str_at("type")?;
        let kind = ObjectiveType::from_key(&type_key)
            .ok_or_else(|| QuestLoadError::Invalid(format!(
                "Objective {}: unsupported type (only kill/talk/collect/deliver)", idx + 1
            )))?;
        let target = str_at("target")?;
        let required = match oo.get("required") {
            None => return Err(QuestLoadError::Invalid(format!("Objective {}: 'required' missing", idx + 1)
            )),
            Some(v) => match v.as_i64() {
                Some(n) => match u32::try_from(n) {
                    Ok(m) if m >= 1 => m,
                    _ => return Err(QuestLoadError::Invalid(
                        format!("Objective {}: 'required' must be >= 1 (got {n})", idx + 1)
                    )),
                },
                None => return Err(QuestLoadError::Invalid(
                    format!("Objective {}: 'required' must be an Integer (got {v})", idx + 1)
                )),
            },
        };
        let resolvable = match kind {
            ObjectiveType::Kill | ObjectiveType::Talk => content.monsters.contains(&target),
            ObjectiveType::Collect | ObjectiveType::Deliver => content.items.contains(&target),
        };
        if !resolvable {
            return Err(QuestLoadError::UnresolvedRef(format!(
                "Objective {}: target '{}' does not resolve against the loaded Content ({})",
                idx + 1, target, kind.key()
            )));
        }
        objectives.push(QuestObjective {
            id: oid,
            kind,
            target,
            required,
        });
    }

    let def = QuestDefinition {
        giver,
        turn_in,
        id,
        title_key,
        description_key,
        objectives,
        min_level,
        requires,
        repeatable,
        rewards,
    };
    def.validate().map_err(|e| QuestLoadError::Invalid(format!("{e}")))?;
    Ok(def)
}

/// Nur Datenzustände (ACTIVE/COMPLETED/FAILED) sind persistierbar; die
/// abgeleiteten Zustände HIDDEN/AVAILABLE (docs/Quest-System.md §27.5)
/// werden bewusst ausgeschlossen. Wird vom `encode_state_for_db` und vom
/// zentralen Player-Persistenzpfad (crate::persist) zum Filtern der
/// Snapshot-Questzustände verwendet.
pub(crate) fn is_persistable(state: &CharacterQuestState) -> bool {
    !matches!(state.state, QuestState::Hidden | QuestState::Available)
}

/// Gemeinsamer Encoder für alle `quests.data`-Persistierungen: kodiert
/// ausschließlich persistierbare Zustände (ACTIVE/COMPLETED/FAILED) als
/// JSON-String. Abgeleitete Zustände (HIDDEN/AVAILABLE) werden abgelehnt,
/// damit die Spalte `state` (Default 0) nie einen abgeleiteten Zustand
/// erhält (§27.5). Wird von `persist_state`, dem atomaren Questabschluss
/// (`complete_for_character`) und dem zentralen Player-Persistenzpfad
/// (crate::persist) verwendet, damit alle denselben `data`-String erzeugen.
pub(crate) fn encode_state_for_db(state: &CharacterQuestState) -> Result<String, String> {
    if !is_persistable(state) {
        return Err(format!(
            "Quest {}: Zustand {:?} ist abgeleitet und nicht persistierbar",
            state.quest_id, state.state
        ));
    }
    let data = encode_quest_data(state);
    serde_json::to_string(&data).map_err(|e| format!("Quest-Daten kodieren: {e}"))
}

/// Serialisiert den V1-Spielerzustand in die `data`-JSON-Spalte der
/// Tabelle `quests`. Form gemäß docs/Quest-System.md §2/§27.6:
/// `objective_progress` enthält mehrere Fortschrittswerte; zusätzlich die
/// für den Questzustand erforderlichen Zeitstempel.
pub fn encode_quest_data(state: &CharacterQuestState) -> serde_json::Value {
    let mut progress = serde_json::Map::new();
    for p in &state.progress {
        progress.insert(p.objective_id.clone(), serde_json::json!(p.current));
    }
    serde_json::json!({
        "objective_progress": serde_json::Value::Object(progress),
        "started_at_ms": state.started_at_ms,
        "completed_at_ms": state.completed_at_ms,
    })
}

/// Rekonstruiert den V1-Spielerzustand aus der `data`-JSON-Spalte.
/// Fehlt `data` oder sind Felder unbekannt, entsteht ein gültiger
/// leerer Zustand (kein Datenverlust durch harte Fehler).
pub fn decode_quest_data(
    quest_id: &str,
    state: QuestState,
    data: Option<&str>,
) -> Result<CharacterQuestState, String> {
    let mut result = CharacterQuestState {
        quest_id: quest_id.to_string(),
        state,
        progress: Vec::new(),
        started_at_ms: None,
        completed_at_ms: None,
    };
    let Some(data) = data else {
        return Ok(result);
    };
    let value: serde_json::Value =
        serde_json::from_str(data).map_err(|e| format!("Quest-Daten dekodieren: {e}"))?;
    if let Some(progress) = value.get("objective_progress").and_then(|p| p.as_object()) {
        for (objective_id, current) in progress {
            let current = current.as_u64().unwrap_or(0) as u32;
            result.progress.push(CharacterObjectiveProgress {
                objective_id: objective_id.clone(),
                current,
            });
        }
    }
    result.started_at_ms = value.get("started_at_ms").and_then(|v| v.as_i64());
    result.completed_at_ms = value.get("completed_at_ms").and_then(|v| v.as_i64());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUEST_KILL: &str = "test_kill_wolves";
    const QUEST_TALK: &str = "test_talk_borin";
    const QUEST_COLLECT: &str = "test_collect_furs";
    const QUEST_DELIVER: &str = "test_deliver_furs";
    const QUEST_CHAIN_A: &str = "test_chain_a";
    const QUEST_CHAIN_B: &str = "test_chain_b";
    const QUEST_REPEATABLE: &str = "test_repeatable";
    const KILL_WOLVES: &str = "kill_wolves";
    const TALK_BORIN: &str = "talk_borin";
    const COLLECT_FURS: &str = "collect_furs";
    const DELIVER_FURS: &str = "deliver_furs";

    fn quest_def(
        id: &str,
        min_level: u32,
        requires: &[&str],
        objectives: Vec<QuestObjective>,
    ) -> QuestDefinition {
        QuestDefinition {
            id: id.to_string(),
            giver: None,
            turn_in: None,
            rewards: RewardBundle { exp: 0, idia: 0 },
            title_key: format!("quest.{id}.title"),
            description_key: format!("quest.{id}.description"),
            objectives,
            min_level,
            requires: requires.iter().map(|s| s.to_string()).collect(),
            repeatable: false,
        }
    }

    fn test_service() -> QuestService {
        let mut svc = QuestService::new();
        // Kill-Quest: „Töte 5 Wölfe."
        svc.register(quest_def(
            QUEST_KILL,
            1,
            &[],
            vec![QuestObjective {
                id: KILL_WOLVES.into(),
                kind: ObjectiveType::Kill,
                target: "wolf".into(),
                required: 5,
            }],
        ))
        .unwrap();
        // Talk-Quest: „Sprich mit Borin."
        svc.register(quest_def(
            QUEST_TALK,
            2,
            &[],
            vec![QuestObjective {
                id: TALK_BORIN.into(),
                kind: ObjectiveType::Talk,
                target: "npc_borin".into(),
                required: 1,
            }],
        ))
        .unwrap();
        // Collect-Quest: „Sammle/Besitze 8 Wolfsfelle."
        svc.register(quest_def(
            QUEST_COLLECT,
            1,
            &[],
            vec![QuestObjective {
                id: COLLECT_FURS.into(),
                kind: ObjectiveType::Collect,
                target: "wolf_fur".into(),
                required: 8,
            }],
        ))
        .unwrap();
        // Deliver-Quest: „Bringe Borin 8 Wolfsfelle."
        svc.register(quest_def(
            QUEST_DELIVER,
            1,
            &[],
            vec![QuestObjective {
                id: DELIVER_FURS.into(),
                kind: ObjectiveType::Deliver,
                target: "wolf_fur".into(),
                required: 8,
            }],
        ))
        .unwrap();
        // Questkette: A (keine Voraussetzung) → B (benötigt A, Level 3).
        svc.register(quest_def(
            QUEST_CHAIN_A,
            1,
            &[],
            vec![QuestObjective {
                id: "kill_a".into(),
                kind: ObjectiveType::Kill,
                target: "wolf".into(),
                required: 1,
            }],
        ))
        .unwrap();
        svc.register(quest_def(
            QUEST_CHAIN_B,
            3,
            &[QUEST_CHAIN_A],
            vec![QuestObjective {
                id: "kill_b".into(),
                kind: ObjectiveType::Kill,
                target: "wolf".into(),
                required: 2,
            }],
        ))
        .unwrap();
        // Ausdrücklich wiederholbare Quest.
        let mut rep = quest_def(
            QUEST_REPEATABLE,
            1,
            &[],
            vec![QuestObjective {
                id: "kill_rep".into(),
                kind: ObjectiveType::Kill,
                target: "wolf".into(),
                required: 1,
            }],
        );
        rep.repeatable = true;
        svc.register(rep).unwrap();
        svc
    }

    fn completed_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    // ── §17-Testliste: HIDDEN/AVAILABLE-Ableitung ────────────────────────

    #[test]
    fn derived_hidden_and_available_by_level() {
        let svc = test_service();
        let def = svc.find_definition(QUEST_TALK).unwrap();
        // Level 1 < min_level 2 → HIDDEN.
        assert_eq!(
            svc.derive_availability(def, 1, &completed_set(&[])),
            QuestState::Hidden
        );
        // Level 2 → AVAILABLE.
        assert_eq!(
            svc.derive_availability(def, 2, &completed_set(&[])),
            QuestState::Available
        );
    }

    #[test]
    fn derived_hidden_and_available_by_previous_quest() {
        let svc = test_service();
        let def = svc.find_definition(QUEST_CHAIN_B).unwrap();
        // Vorherige Quest A fehlt → HIDDEN trotz ausreichendem Level.
        assert_eq!(
            svc.derive_availability(def, 5, &completed_set(&[])),
            QuestState::Hidden
        );
        // A abgeschlossen + Level 3 → AVAILABLE.
        assert_eq!(
            svc.derive_availability(def, 3, &completed_set(&[QUEST_CHAIN_A])),
            QuestState::Available
        );
    }

    #[test]
    fn state_for_honors_persisted_states_and_derives_otherwise() {
        let svc = test_service();
        let def = svc.find_definition(QUEST_TALK).unwrap();
        let none = None;
        // Kein persistierter Zustand → abgeleitet (HIDDEN bei Level 1).
        assert_eq!(
            svc.state_for(def, none, 1, &completed_set(&[])),
            QuestState::Hidden
        );
        // Abgeleitet (AVAILABLE bei Level 2).
        assert_eq!(
            svc.state_for(def, none, 2, &completed_set(&[])),
            QuestState::Available
        );

        let active = CharacterQuestState {
            quest_id: QUEST_TALK.into(),
            state: QuestState::Active,
            progress: Vec::new(),
            started_at_ms: Some(1),
            completed_at_ms: None,
        };
        // ACTIVE-Zeile wird respektiert (auch wenn Voraussetzungen danach
        // nicht mehr erfüllt wären).
        assert_eq!(
            svc.state_for(def, Some(&active), 1, &completed_set(&[])),
            QuestState::Active
        );
    }

    // ── §17-Testliste: Annahme ───────────────────────────────────────────

    #[test]
    fn available_quest_can_be_accepted_and_creates_active() {
        let svc = test_service();
        let completed = completed_set(&[]);
        let accepted = svc.accept(QUEST_KILL, None, 1, &completed).unwrap();
        assert_eq!(accepted.quest_id, QUEST_KILL);
        assert_eq!(accepted.state, QuestState::Active);
        // ACTIVE beginnt mit Fortschritt 0 je Ziel.
        assert_eq!(
            accepted
                .progress
                .iter()
                .map(|p| (p.objective_id.as_str(), p.current))
                .collect::<Vec<_>>(),
            vec![(KILL_WOLVES, 0)]
        );
        assert!(accepted.started_at_ms.is_some());
        assert_eq!(accepted.completed_at_ms, None);
    }

    #[test]
    fn hidden_quest_cannot_be_accepted() {
        let svc = test_service();
        // min_level 2 nicht erreicht → HIDDEN → Annahme verweigert.
        assert_eq!(
            svc.accept(QUEST_TALK, None, 1, &completed_set(&[])),
            Err(QuestError::NotAvailable)
        );
        // Vorherige Quest fehlt → HIDDEN → Annahme verweigert.
        assert_eq!(
            svc.accept(QUEST_CHAIN_B, None, 5, &completed_set(&[])),
            Err(QuestError::NotAvailable)
        );
    }

    #[test]
    fn already_active_cannot_be_accepted_again() {
        let svc = test_service();
        let accepted = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        assert_eq!(
            svc.accept(QUEST_KILL, Some(&accepted), 1, &completed_set(&[])),
            Err(QuestError::AlreadyActive)
        );
    }

    #[test]
    fn completed_non_repeatable_cannot_be_accepted_again() {
        let svc = test_service();
        let mut active = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        // Quest vollständig: 5 Kills.
        for _ in 0..5 {
            svc.add_progress(QUEST_KILL, &mut active, KILL_WOLVES, 1)
                .unwrap();
        }
        assert!(svc.objectives_complete(QUEST_KILL, &active));
        let completed = svc.complete(QUEST_KILL, &active).unwrap();
        assert_eq!(completed.state, QuestState::Completed);
        assert_eq!(
            svc.accept(QUEST_KILL, Some(&completed), 1, &completed_set(&[])),
            Err(QuestError::AlreadyCompleted)
        );
    }

    #[test]
    fn repeatable_quest_can_be_accepted_after_completion() {
        let svc = test_service();
        // Erste Runde.
        let mut active = svc
            .accept(QUEST_REPEATABLE, None, 1, &completed_set(&[]))
            .unwrap();
        svc.add_progress(QUEST_REPEATABLE, &mut active, "kill_rep", 1)
            .unwrap();
        let completed = svc.complete(QUEST_REPEATABLE, &active).unwrap();
        assert_eq!(completed.state, QuestState::Completed);
        // Definition erlaubt ausdrücklich Wiederholbarkeit → erneute
        // Annahme erzeugt einen frischen ACTIVE-Zustand.
        let re_accepted = svc
            .accept(QUEST_REPEATABLE, Some(&completed), 1, &completed_set(&[]))
            .unwrap();
        assert_eq!(re_accepted.state, QuestState::Active);
        assert_eq!(
            re_accepted
                .progress
                .iter()
                .map(|p| p.current)
                .collect::<Vec<_>>(),
            vec![0]
        );
    }

    // ── §17-Testliste: Fortschritt ───────────────────────────────────────

    #[test]
    fn active_state_holds_progress() {
        let svc = test_service();
        let mut state = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        let d1 = svc
            .add_progress(QUEST_KILL, &mut state, KILL_WOLVES, 3)
            .unwrap();
        assert_eq!((d1.from, d1.to, d1.met), (0, 3, false));
        let d2 = svc
            .add_progress(QUEST_KILL, &mut state, KILL_WOLVES, 2)
            .unwrap();
        assert_eq!((d2.from, d2.to, d2.met), (3, 5, true));
        // Übererfüllung wird auf die Zielmenge geklemmt.
        let d3 = svc
            .add_progress(QUEST_KILL, &mut state, KILL_WOLVES, 99)
            .unwrap();
        assert_eq!((d3.from, d3.to, d3.met), (5, 5, false));
        let p = state
            .progress
            .iter()
            .find(|p| p.objective_id == KILL_WOLVES)
            .unwrap();
        assert_eq!(p.current, 5);
    }

    #[test]
    fn not_active_receives_no_progress() {
        let svc = test_service();
        // Abgeleitet AVAILABLE aber nicht angenommen (None) → kein Zustand,
        // dem Fortschritt zugeordnet werden könnte.
        let mut non_active = CharacterQuestState {
            quest_id: QUEST_KILL.into(),
            state: QuestState::Available,
            progress: Vec::new(),
            started_at_ms: None,
            completed_at_ms: None,
        };
        assert_eq!(
            svc.add_progress(QUEST_KILL, &mut non_active, KILL_WOLVES, 1),
            Err(QuestError::NotActive)
        );
        // COMPLETED-Zustand erhält ebenfalls keinen Fortschritt.
        let mut completed = CharacterQuestState {
            quest_id: QUEST_KILL.into(),
            state: QuestState::Completed,
            progress: vec![CharacterObjectiveProgress {
                objective_id: KILL_WOLVES.into(),
                current: 5,
            }],
            started_at_ms: Some(1),
            completed_at_ms: Some(2),
        };
        assert_eq!(
            svc.add_progress(QUEST_KILL, &mut completed, KILL_WOLVES, 1),
            Err(QuestError::NotActive)
        );
    }

    #[test]
    fn zero_delta_is_rejected() {
        let svc = test_service();
        let mut state = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        assert_eq!(
            svc.add_progress(QUEST_KILL, &mut state, KILL_WOLVES, 0),
            Err(QuestError::InvalidProgress)
        );
    }

    // ── §17-Testliste: Zielerfüllung / Abschluss ─────────────────────────

    #[test]
    fn fully_met_objectives_are_detected() {
        let svc = test_service();
        let mut state = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        assert!(!svc.objectives_complete(QUEST_KILL, &state));
        svc.add_progress(QUEST_KILL, &mut state, KILL_WOLVES, 5)
            .unwrap();
        assert!(svc.objectives_complete(QUEST_KILL, &state));
    }

    #[test]
    fn multi_objective_quest_needs_all_objectives() {
        let mut svc = test_service();
        svc.register(quest_def(
            "test_multi",
            1,
            &[],
            vec![
                QuestObjective {
                    id: "o1".into(),
                    kind: ObjectiveType::Kill,
                    target: "wolf".into(),
                    required: 1,
                },
                QuestObjective {
                    id: "o2".into(),
                    kind: ObjectiveType::Talk,
                    target: "npc_borin".into(),
                    required: 1,
                },
            ],
        ))
        .unwrap();
        let mut state = svc
            .accept("test_multi", None, 1, &completed_set(&[]))
            .unwrap();
        // Nur o1 erfüllt → Quest nicht komplett.
        svc.add_progress("test_multi", &mut state, "o1", 1).unwrap();
        assert!(!svc.objectives_complete("test_multi", &state));
        assert_eq!(
            svc.complete("test_multi", &state),
            Err(QuestError::ObjectivesNotMet)
        );
        // o2 zusätzlich erfüllt → Quest komplett.
        svc.add_progress("test_multi", &mut state, "o2", 1).unwrap();
        assert!(svc.objectives_complete("test_multi", &state));
    }

    #[test]
    fn incomplete_quest_cannot_be_completed() {
        let svc = test_service();
        let state = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        assert_eq!(
            svc.complete(QUEST_KILL, &state),
            Err(QuestError::ObjectivesNotMet)
        );
    }

    #[test]
    fn complete_active_quest_creates_completed() {
        let svc = test_service();
        let mut state = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        svc.add_progress(QUEST_KILL, &mut state, KILL_WOLVES, 5)
            .unwrap();
        let completed = svc.complete(QUEST_KILL, &state).unwrap();
        assert_eq!(completed.state, QuestState::Completed);
        assert!(completed.completed_at_ms.is_some());
        // Fortschritt bleibt nachvollziehbar (erfüllte Ziele).
        assert!(svc.objectives_complete(QUEST_KILL, &completed));
    }

    #[test]
    fn second_completion_is_rejected() {
        let svc = test_service();
        let mut state = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        svc.add_progress(QUEST_KILL, &mut state, KILL_WOLVES, 5)
            .unwrap();
        let completed = svc.complete(QUEST_KILL, &state).unwrap();
        assert_eq!(completed.state, QuestState::Completed);
        // Zweiter Abschluss derselben nicht wiederholbaren Quest wird
        // abgelehnt → kann keine zweite Belohnung erzeugen (§9, §14).
        assert_eq!(
            svc.complete(QUEST_KILL, &completed),
            Err(QuestError::AlreadyCompleted)
        );
    }

    #[test]
    fn complete_on_non_active_state_rejected() {
        let svc = test_service();
        let hidden = CharacterQuestState {
            quest_id: QUEST_KILL.into(),
            state: QuestState::Hidden,
            progress: Vec::new(),
            started_at_ms: None,
            completed_at_ms: None,
        };
        assert_eq!(
            svc.complete(QUEST_KILL, &hidden),
            Err(QuestError::NotActive)
        );
    }

    // ── V1.2a: Abschluss-Transaktion (docs/Quest-System.md §27.26) ────────

    /// Inventar mit `total` Wolfsfellen (normales, stapelbares Material;
    /// docs/Quest-System.md §27.17/§27.18).
    fn fur_inventory(total: i64) -> crate::inventory::InventoryState {
        let mut inv = crate::inventory::InventoryState::new(8);
        let mut def = crate::item::ItemDefinition::new(
            "wolf_fur",
            "Wolfsfell",
            crate::item::ItemCategory::RawMaterial,
        );
        def.max_stack = 100;
        inv.try_add(&def, total);
        inv
    }

    fn deliver_state(svc: &QuestService) -> CharacterQuestState {
        let mut state = svc
            .accept(QUEST_DELIVER, None, 2, &completed_set(&[]))
            .unwrap();
        svc.add_progress(QUEST_DELIVER, &mut state, DELIVER_FURS, 8)
            .unwrap();
        state
    }

    #[test]
    fn completion_plan_derives_deliver_removals_only() {
        let svc = test_service();
        let state = deliver_state(&svc);
        let plan = svc
            .plan_completion(QUEST_DELIVER, &state, &fur_inventory(8))
            .unwrap();
        assert_eq!(
            plan.removals,
            vec![ItemRemoval {
                item_id: "wolf_fur".into(),
                qty: 8,
            }]
        );
        // collect bleibt ein Besitznachweis → keine Entfernung im Plan.
        let mut col = svc
            .accept(QUEST_COLLECT, None, 1, &completed_set(&[]))
            .unwrap();
        svc.add_progress(QUEST_COLLECT, &mut col, COLLECT_FURS, 8)
            .unwrap();
        let plan = svc
            .plan_completion(QUEST_COLLECT, &col, &fur_inventory(8))
            .unwrap();
        assert!(plan.removals.is_empty());
    }

    #[test]
    fn missing_deliver_items_abort_completion_without_mutation() {
        let svc = test_service();
        let state = deliver_state(&svc);
        let inventory = fur_inventory(7); // 1 Wolfsfell zu wenig
        assert_eq!(
            svc.plan_completion(QUEST_DELIVER, &state, &inventory),
            Err(QuestError::DeliverItemsMissing)
        );
        // Zustand unverändert: nichts entfernt, Quest bleibt ACTIVE.
        assert_eq!(inventory.count_of("wolf_fur"), 7);
        assert_eq!(state.state, QuestState::Active);
    }

    #[test]
    fn completion_executes_deliver_removal_and_sets_completed() {
        let svc = test_service();
        let state = deliver_state(&svc);
        let plan = svc
            .plan_completion(QUEST_DELIVER, &state, &fur_inventory(12))
            .unwrap();
        let mut inventory = fur_inventory(12);
        let completed = svc
            .execute_completion(&plan, &state, &mut inventory)
            .unwrap();
        assert_eq!(completed.state, QuestState::Completed);
        assert!(completed.completed_at_ms.is_some());
        // Nur die benötigte Menge (8) wird entfernt; die übrigen 4 bleiben.
        assert_eq!(inventory.count_of("wolf_fur"), 4);
        // Doppelabschluss derselben Quest → kein zweiter Plan und keine
        // zweite Entfernung/Belohnung (§27.26 Doppelabschluss-Schutz).
        assert_eq!(
            svc.plan_completion(QUEST_DELIVER, &completed, &inventory),
            Err(QuestError::AlreadyCompleted)
        );
        // collect entfernt beim Abschluss nichts (Items bleiben beim Spieler).
        let mut col = svc
            .accept(QUEST_COLLECT, None, 1, &completed_set(&[]))
            .unwrap();
        svc.add_progress(QUEST_COLLECT, &mut col, COLLECT_FURS, 8)
            .unwrap();
        let plan = svc
            .plan_completion(QUEST_COLLECT, &col, &fur_inventory(8))
            .unwrap();
        let mut inv2 = fur_inventory(8);
        svc.execute_completion(&plan, &col, &mut inv2).unwrap();
        assert_eq!(inv2.count_of("wolf_fur"), 8);
    }

    #[test]
    fn completion_final_validation_prevents_all_negative_paths() {
        let svc = test_service();
        let inventory = fur_inventory(7);
        // Nicht erfüllte Objectives → kein Abschlussplan.
        let partial = svc
            .accept(QUEST_DELIVER, None, 2, &completed_set(&[]))
            .unwrap();
        assert_eq!(
            svc.plan_completion(QUEST_DELIVER, &partial, &inventory),
            Err(QuestError::ObjectivesNotMet)
        );
        // final validierte Deliver-Mengen fehlen → kein Teilerfolg.
        let state = deliver_state(&svc);
        let full_plan = svc
            .plan_completion(QUEST_DELIVER, &state, &fur_inventory(8))
            .unwrap();
        let mut inventory = fur_inventory(7);
        assert_eq!(
            svc.execute_completion(&full_plan, &state, &mut inventory),
            Err(QuestCompletionError::Quest(QuestError::DeliverItemsMissing))
        );
        assert_eq!(inventory.count_of("wolf_fur"), 7);
        // COMPLETED-Zustand kann nicht erneut ausgeführt werden (NotActive).
        let done = svc.complete(QUEST_DELIVER, &state).unwrap();
        assert_eq!(
            svc.execute_completion(&full_plan, &done, &mut inventory),
            Err(QuestCompletionError::Quest(QuestError::NotActive))
        );
    }

    // ── V1.2a.2: atomarer Abschluss — pure Kontrolllogik (ohne MariaDB) ──
    //
    // Integrations-/Crash-Eigenschaften (COMMIT-Atomizität, Rollback bei
    // fehlgeschlagenem Guard) werden von MariaDB/InnoDB selbst garantiert und
    // sind ohne echte DB nicht unit-testbar (keine künstliche MariaDB-
    // Simulation). Hier wird die PURE, DB-unkritische Kontrolllogik abge-
    // sichert: der Guard-Kontrakt (Spaltenwerte 1/2), der gemeinsame
    // data-Encoder sowie die Zuordnung abgeleiteter vs. Datenzustände.

    #[test]
    fn persisted_state_values_match_guard_contract() {
        // Die numerischen Werte von ACTIVE/COMPLETED (quest.rs) sind der
        // Vertrag, an dem der DB-seitige ACTIVE→COMPLETED-Guard gemessen
        // wird (db::guarded_complete_quest: `SET state = 2 ... AND
        // state = 1`). Ändern die Werte sich, schlägt dieser Test fehl.
        assert_eq!(QuestState::Active.db_value(), 1);
        assert_eq!(QuestState::Completed.db_value(), 2);
        assert_eq!(QuestState::Failed.db_value(), 3);
        // COMPLETED ist ein Datenzustand und damit persistierbar.
        let done = CharacterQuestState {
            quest_id: QUEST_DELIVER.into(),
            state: QuestState::Completed,
            progress: vec![],
            started_at_ms: None,
            completed_at_ms: Some(1),
        };
        assert!(is_persistable(&done));
    }

    fn state_with(state: QuestState) -> CharacterQuestState {
        CharacterQuestState {
            quest_id: QUEST_DELIVER.into(),
            state,
            progress: vec![],
            started_at_ms: None,
            completed_at_ms: None,
        }
    }

    #[test]
    fn encode_state_for_db_rejects_derived_accepts_data_states() {
        // Abgeleitete Zustände werden nie in die Spalte `quests.state`
        // geschrieben (wert 0 wäre kein gültiger Datenzustand, §27.5).
        assert!(encode_state_for_db(&state_with(QuestState::Hidden)).is_err());
        assert!(encode_state_for_db(&state_with(QuestState::Available)).is_err());
        // Datenzustände kodieren und round-trippen.
        for st in [
            QuestState::Active,
            QuestState::Completed,
            QuestState::Failed,
        ] {
            let data = encode_state_for_db(&state_with(st)).unwrap();
            let decoded = decode_quest_data(QUEST_DELIVER, st, Some(&data)).unwrap();
            assert_eq!(decoded.state, st);
            assert_eq!(decoded, state_with(st));
        }
    }

    #[test]
    fn accepted_state_is_immediately_persistable() {
        // docs/Player_Persistenz.md §10: AVAILABLE→ACTIVE wird unmittelbar
        // persistent gespeichert. Der frisch angenommene Zustand muss daher
        // ein persistierbarer Datenzustand sein und durch den gemeinsamen
        // Encoder verlustfrei roundtrippen — Voraussetzung für
        // `accept_for_character` (Schritt 2: ACTIVE-Upsert).
        let svc = test_service();
        let accepted = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        assert_eq!(accepted.state, QuestState::Active);
        assert!(is_persistable(&accepted));
        assert_eq!(accepted.progress.len(), 1);
        assert_eq!(accepted.progress[0].current, 0);
        assert!(accepted.started_at_ms.is_some());
        assert!(accepted.completed_at_ms.is_none());
        let data = encode_state_for_db(&accepted).unwrap();
        let decoded = decode_quest_data(QUEST_KILL, QuestState::Active, Some(&data)).unwrap();
        assert_eq!(decoded, accepted);
    }

    #[test]
    fn completed_quest_data_roundtrips_preserves_progress_and_timestamp() {
        // Die `data`-Zeile, die der atomare Abschluss per Guard in die
        // Tabelle `quests` schreibt, muss den vollen COMPLETED-Zustand
        // (Fortschritt + Zeitstempel) verlustfrei rekonstruierbar enthalten.
        let completed = CharacterQuestState {
            quest_id: QUEST_DELIVER.into(),
            state: QuestState::Completed,
            progress: vec![CharacterObjectiveProgress {
                objective_id: DELIVER_FURS.into(),
                current: 8,
            }],
            started_at_ms: Some(111),
            completed_at_ms: Some(222),
        };
        let data = encode_state_for_db(&completed).unwrap();
        let decoded = decode_quest_data(QUEST_DELIVER, QuestState::Completed, Some(&data)).unwrap();
        assert_eq!(decoded, completed);
    }

    // ── §17-Testliste: FAILED ────────────────────────────────────────────

    #[test]
    fn failed_is_an_existing_state_without_auto_transition() {
        let svc = test_service();
        // DB-Mapping: 3 = FAILED.
        assert_eq!(QuestState::from_db_value(3), Some(QuestState::Failed));
        // Persistierter FAILED-Zustand wird als Datenzustand rekonstruiert
        // und respektiert — kein automatischer Übergang, auch nicht nach
        // AVAILABLE, selbst wenn die Voraussetzungen inzwischen erfüllt sind.
        let failed = CharacterQuestState {
            quest_id: QUEST_KILL.into(),
            state: QuestState::Failed,
            progress: vec![CharacterObjectiveProgress {
                objective_id: KILL_WOLVES.into(),
                current: 2,
            }],
            started_at_ms: Some(1),
            completed_at_ms: None,
        };
        let def = svc.find_definition(QUEST_KILL).unwrap();
        assert_eq!(
            svc.state_for(def, Some(&failed), 99, &completed_set(&[])),
            QuestState::Failed
        );
        // Kein automatisierter FAILED-Übergang im normalen Ablauf: Eine
        // unvollständige ACTIVE-Quest bleibt ACTIVE (kein Timeout-Trigger).
        let active = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        let mut partial = active.clone();
        svc.add_progress(QUEST_KILL, &mut partial, KILL_WOLVES, 1)
            .unwrap();
        assert_eq!(partial.state, QuestState::Active);
        // Über die Zeit/„Ticks" hinweg gibt es keinen Übergang — das Modell
        // kennt keine Zeit-/Todes-/Logout-Failed-Ursachen (§27.5).
        assert_eq!(partial.state, QuestState::Active);
        // Wiederannahme nach FAILED ist in V1.1 nicht festgelegt (fail-closed).
        assert_eq!(
            svc.accept(QUEST_KILL, Some(&failed), 99, &completed_set(&[])),
            Err(QuestError::NotAvailable)
        );
    }

    // ── §17-Testliste: Persistenz-Serialisierung ─────────────────────────

    #[test]
    fn db_state_mapping_is_stable() {
        use QuestState::*;
        // Persistierbare Zustände (Tabelle `quests`, Spalte state TINYINT).
        assert_eq!(Active.db_value(), 1);
        assert_eq!(Completed.db_value(), 2);
        assert_eq!(Failed.db_value(), 3);
        // Abgeleitete Zustände werden nie persistiert (0 = Spalten-Default).
        assert_eq!(Hidden.db_value(), 0);
        assert_eq!(Available.db_value(), 0);
        // Unbekannte DB-Zahlen liefern keinen Zustand (fail-closed).
        assert_eq!(QuestState::from_db_value(0), None);
        assert_eq!(QuestState::from_db_value(99), None);
        assert_eq!(QuestState::from_db_value(-1), None);
        assert_eq!(QuestState::from_db_value(1), Some(Active));
        assert_eq!(QuestState::from_db_value(2), Some(Completed));
        assert_eq!(QuestState::from_db_value(3), Some(Failed));
    }

    #[test]
    fn only_data_states_are_persistable() {
        let base_progress = |quest_id: &str, state: QuestState| CharacterQuestState {
            quest_id: quest_id.into(),
            state,
            progress: Vec::new(),
            started_at_ms: None,
            completed_at_ms: None,
        };
        // Abgeleitete Zustände → nie persistierbar.
        assert!(!is_persistable(&base_progress(
            QUEST_KILL,
            QuestState::Hidden
        )));
        assert!(!is_persistable(&base_progress(
            QUEST_KILL,
            QuestState::Available
        )));
        // Datenzustände → persistierbar.
        assert!(is_persistable(&base_progress(
            QUEST_KILL,
            QuestState::Active
        )));
        assert!(is_persistable(&base_progress(
            QUEST_KILL,
            QuestState::Completed
        )));
        assert!(is_persistable(&base_progress(
            QUEST_KILL,
            QuestState::Failed
        )));
    }

    #[test]
    fn persistence_serialization_roundtrip() {
        let state = CharacterQuestState {
            quest_id: QUEST_DELIVER.into(),
            state: QuestState::Active,
            progress: vec![
                CharacterObjectiveProgress {
                    objective_id: DELIVER_FURS.into(),
                    current: 3,
                },
                CharacterObjectiveProgress {
                    objective_id: "other".into(),
                    current: 7,
                },
            ],
            started_at_ms: Some(1_700_000_000_123),
            completed_at_ms: None,
        };
        let json = encode_quest_data(&state);
        // Dokumentierte Form: objective_progress mit mehreren Werten (§2).
        assert_eq!(
            json["objective_progress"][DELIVER_FURS],
            serde_json::json!(3)
        );
        assert_eq!(json["objective_progress"]["other"], serde_json::json!(7));
        assert_eq!(
            json["started_at_ms"],
            serde_json::json!(1_700_000_000_123_i64)
        );
        assert!(json["completed_at_ms"].is_null());

        let encoded = serde_json::to_string(&json).unwrap();
        let decoded = decode_quest_data(QUEST_DELIVER, QuestState::Active, Some(&encoded)).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn persistence_decode_without_data_or_unknown_fields_is_safe() {
        // data NULL → leerer, gültiger Zustand.
        let empty = decode_quest_data(QUEST_KILL, QuestState::Active, None).unwrap();
        assert_eq!(empty.state, QuestState::Active);
        assert!(empty.progress.is_empty());
        assert_eq!(empty.started_at_ms, None);
        assert_eq!(empty.completed_at_ms, None);

        // Unbekannte Felder/fehlerhafte Zahlen werden toleriert, keine Panik.
        let quirky =
            r#"{"objective_progress":{"a":2,"b":"x","c":-1},"started_at_ms":42,"unknown":true}"#;
        let decoded = decode_quest_data(QUEST_KILL, QuestState::Failed, Some(quirky)).unwrap();
        assert_eq!(decoded.state, QuestState::Failed);
        assert_eq!(decoded.started_at_ms, Some(42));
        assert_eq!(
            decoded
                .progress
                .iter()
                .map(|p| (p.objective_id.as_str(), p.current))
                .collect::<Vec<_>>(),
            vec![("a", 2), ("b", 0), ("c", 0)]
        );
    }

    // ── §17-Testliste: sichere Ablehnung unbekannter IDs ─────────────────

    #[test]
    fn unknown_quest_id_is_rejected_everywhere() {
        let svc = test_service();
        assert_eq!(svc.find_definition("ghost_quest"), None);
        assert_eq!(
            svc.accept("ghost_quest", None, 99, &completed_set(&[])),
            Err(QuestError::UnknownQuest)
        );
        let mut state = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        assert_eq!(
            svc.add_progress("ghost_quest", &mut state, "x", 1),
            Err(QuestError::UnknownQuest)
        );
        assert_eq!(
            svc.complete("ghost_quest", &state),
            Err(QuestError::UnknownQuest)
        );
        assert!(!svc.objectives_complete("ghost_quest", &state));
    }

    #[test]
    fn unknown_objective_id_is_rejected() {
        let svc = test_service();
        let mut state = svc
            .accept(QUEST_KILL, None, 1, &completed_set(&[]))
            .unwrap();
        assert_eq!(
            svc.add_progress(QUEST_KILL, &mut state, "ghost_objective", 1),
            Err(QuestError::UnknownObjective)
        );
    }

    // ── §17-Testliste: Objective-Modell ──────────────────────────────────

    #[test]
    fn four_v1_objective_types_are_distinguished() {
        let svc = test_service();
        let by_id: BTreeMap<&str, &QuestDefinition> =
            svc.definitions().map(|d| (d.id.as_str(), d)).collect();
        let kinds: BTreeMap<&str, ObjectiveType> = by_id
            .iter()
            .map(|(id, def)| (*id, def.objectives[0].kind))
            .collect();
        assert_eq!(kinds.get(QUEST_KILL), Some(&ObjectiveType::Kill));
        assert_eq!(kinds.get(QUEST_TALK), Some(&ObjectiveType::Talk));
        assert_eq!(kinds.get(QUEST_COLLECT), Some(&ObjectiveType::Collect));
        assert_eq!(kinds.get(QUEST_DELIVER), Some(&ObjectiveType::Deliver));
        // Schlüssel-Roundtrip für spätere (nicht gebaute) Event-Anbindung.
        for kind in [
            ObjectiveType::Kill,
            ObjectiveType::Talk,
            ObjectiveType::Collect,
            ObjectiveType::Deliver,
        ] {
            assert_eq!(ObjectiveType::from_key(kind.key()), Some(kind));
        }
    }

    // ── Registrierung / Definitionen ─────────────────────────────────────

    // ── Quest V1 Content-Loader (docs/Quest-System.md §2/§27.9) ───────────
    //
    // The Definition-Scripts follow the documented shape of
    // docs/quests_stories.md §3 (provisional format, §27.9 — smallest
    // compatible proposal). Tests use temp-Dirs (short-lived, ai_jobs
    // „Temporäre Dateien"); no DB, no Service-Start.

    use std::fs;
    use std::path::Path;

    fn tmp_root(tag: impl AsRef<std::path::Path>) -> std::path::PathBuf {
        let tag_s = tag.as_ref().display().to_string();
        let base = std::env::temp_dir()
            .join(format!("pimmo-lua-quest-load-{tag_s}"))
            .join(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string());
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    /// Writes Definition-Scripts `(name, lua)` into `<root>/quests/`.
    fn write_scripts(root: &Path, scripts: &[(String, String)]) {
        let dir = root.join("quests");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, code) in scripts {
            std::fs::write(dir.join(format!("{name}.lua")), code).unwrap();
        }
    }

    /// Documented-shape Definition-Script (quests_stories.md §3): a
    /// `kill`-Quest for Content „wolf".
    const WOLF_QUEST_LUA: &str = r#"
quest = {
    id = "q_test_wolves",
    title_key = "quest.q_test_wolves.title",
    description_key = "quest.q_test_wolves.description",
    requirements = { level = 1 },
    objectives = {
        { id = "o_wolves", type = "kill", target = "wolf", required = 5 },
    },
}
return quest
"#;

    fn wolf_content() -> ContentIndex {
        let mut monsters = HashSet::new();
        monsters.insert("wolf".into());
        ContentIndex { monsters, items: HashSet::new() }
    }

    #[test]
    fn loader_turn_in_and_rewards_are_explicit_and_validated() {
        let root = tmp_root("turn-in-valid");
        let script = WOLF_QUEST_LUA.replace("quest = {",
            "quest = { giver = 'giver', turn_in = 'recipient', rewards = { exp = 11, idia = 7 },");
        let mut content = wolf_content();
        content.monsters.extend(["giver".into(), "recipient".into()]);
        write_scripts(&root, &[("valid".into(), script)]);
        let mut svc = QuestService::new();
        assert_eq!(svc.load_quest_definitions(&root, &content).unwrap(), 1);
        let def = svc.find_definition("q_test_wolves").unwrap();
        assert_eq!(def.giver.as_deref(), Some("giver"));
        assert_eq!(def.turn_in.as_deref(), Some("recipient"));
        assert_eq!(def.rewards, RewardBundle { exp: 11, idia: 7 });

        let root = tmp_root("turn-in-absent");
        write_scripts(&root, &[("absent".into(), WOLF_QUEST_LUA.replace(
            "quest = {", "quest = { giver = 'giver',"))]);
        let mut svc = QuestService::new();
        svc.load_quest_definitions(&root, &content).unwrap();
        let def = svc.find_definition("q_test_wolves").unwrap();
        assert_eq!(def.turn_in, None, "kein erfundener giver-Fallback");
        assert_eq!(def.rewards, RewardBundle::default());
    }

    #[test]
    fn loader_rejects_invalid_turn_in_and_reward_payloads_atomically() {
        for field in [
            "turn_in = ''", "turn_in = 'missing'", "turn_in = 5",
            "rewards = { exp = -1 }", "rewards = { idia = -1 }",
            "rewards = { exp = 0.5 }", "rewards = { idia = false }",
            "rewards = { exp = '11' }", "rewards = false",
            "rewards = { xp = 11 }", "rewards = { gold = 7 }",
            "rewards = { items = { 'wolf' } }", "rewards = { choice = 'wolf' }",
        ] {
            let root = tmp_root("turn-in-invalid");
            let bad = WOLF_QUEST_LUA.replace("quest = {", &format!("quest = {{ {field},"));
            let good = WOLF_QUEST_LUA.replace("q_test_wolves", "q_good");
            write_scripts(&root, &[("a_good".into(), good), ("z_bad".into(), bad)]);
            let mut svc = QuestService::new();
            assert!(svc.load_quest_definitions(&root, &wolf_content()).is_err(), "{field}");
            assert_eq!(svc.definitions().count(), 0, "keine Teilregistrierung: {field}");
        }
    }

    #[test]
    fn reward_numbers_must_fit_existing_signed_storage() {
        let valid = serde_json::json!({
            "id": "q", "title_key": "t", "description_key": "d",
            "requirements": {}, "objectives": [{"id": "k", "type": "kill", "target": "wolf", "required": 1}],
            "rewards": {"exp": i64::MAX, "idia": 0}
        });
        let def = build_quest_definition(&valid, &wolf_content()).unwrap();
        assert_eq!(def.rewards.exp, i64::MAX as u64);
        let mut invalid = valid;
        invalid["rewards"]["exp"] = serde_json::json!((i64::MAX as u64) + 1);
        assert!(build_quest_definition(&invalid, &wolf_content()).is_err());
        let mut invalid_def = def;
        invalid_def.rewards.exp = u64::MAX;
        assert!(invalid_def.validate().is_err(), "auch interne Registrierung validieren");
    }

    #[test]
    fn turn_in_reward_draft_reuses_progression_without_rested_or_mutation() {
        let cfg = crate::progression::ProgressionCfg::default();
        let before = crate::progression::Progression::new(
            1, 0, 0, 80, crate::class::ClassStatus::Adventurer, false,
        );
        let amount = crate::progression::exp_needed(&cfg, 1);
        let rewards = RewardBundle { exp: amount as u64, idia: 7 };
        let (draft, new_idia) = rewards.prepare(&before, 10, &cfg).unwrap();
        assert_eq!(draft.level, 2);
        assert_eq!(draft.exp, 0);
        assert_eq!(draft.rested_pool, 80, "Quest-EXP verbraucht kein Rested");
        assert_eq!(new_idia, 17);
        assert_eq!(before.level, 1, "Vorbereitung verändert den Eingang nicht");
        assert_eq!(before.free_attr_points, 0);
    }

    #[test]
    fn turn_in_reward_draft_rejects_overflow_without_partial_changes() {
        let cfg = crate::progression::ProgressionCfg::default();
        let before = crate::progression::Progression::new(
            1, 0, 0, 80, crate::class::ClassStatus::Adventurer, false,
        );
        let rewards = RewardBundle { exp: 11, idia: 7 };
        assert_eq!(rewards.prepare(&before, i64::MAX, &cfg), Err("quest_reward_overflow"));
        let mut exp_overflow = before.clone();
        exp_overflow.exp = i64::MAX;
        assert_eq!(rewards.prepare(&exp_overflow, 10, &cfg), Err("quest_reward_overflow"));
        let mut points_overflow = before.clone();
        points_overflow.free_attr_points = i32::MAX as u32;
        let level_up = RewardBundle { exp: crate::progression::exp_needed(&cfg, 1) as u64, idia: 0 };
        assert_eq!(level_up.prepare(&points_overflow, 10, &cfg), Err("quest_reward_overflow"));
        assert_eq!(points_overflow.free_attr_points, i32::MAX as u32);
        assert_eq!(before.exp, 0);
    }

    #[test]
    fn loader_giver_with_and_without_reference_remains_compatible() {
        for giver in [None, Some("wolf")] {
            let root = tmp_root("giver-valid");
            let code = match giver {
                Some(id) => WOLF_QUEST_LUA.replace("quest = {", &format!("quest = {{ giver = \"{id}\",")),
                None => WOLF_QUEST_LUA.to_owned(),
            };
            write_scripts(&root, &[("q".into(), code)]);
            let mut service = QuestService::new();
            assert_eq!(service.load_quest_definitions(&root, &wolf_content()).unwrap(), 1);
            assert_eq!(service.find_definition("q_test_wolves").unwrap().giver.as_deref(), giver);
        }
    }

    #[test]
    fn loader_giver_rejects_unknown_empty_and_non_string_atomically() {
        for value in ["\"missing\"", "\"\"", "7", "false", "{}"] {
            let root = tmp_root("giver-invalid");
            let invalid = WOLF_QUEST_LUA.replace("quest = {", &format!("quest = {{ giver = {value},"));
            write_scripts(&root, &[("a".into(), WOLF_QUEST_LUA.replace("q_test_wolves", "valid")),
                ("b".into(), invalid)]);
            let mut service = QuestService::new();
            assert!(service.load_quest_definitions(&root, &wolf_content()).is_err());
            assert_eq!(service.definitions().count(), 0);
        }
    }

    #[test]
    fn loader_loads_valid_definition_into_registry() {
        let root = tmp_root("valid");
        write_scripts(&root, &vec![(("q_test_wolves".into(), WOLF_QUEST_LUA.into()))]);
        let mut svc = QuestService::new();
        let n = svc.load_quest_definitions(&root, &wolf_content()).unwrap();
        assert_eq!(n, 1);
        let def = svc.find_definition("q_test_wolves").unwrap();
        assert_eq!(def.objectives[0].kind, ObjectiveType::Kill);
        assert_eq!(def.objectives[0].target, "wolf");
        assert_eq!(def.objectives[0].required, 5);
        assert_eq!(def.min_level, 1);
    }

    #[test]
    fn loader_rejects_duplicate_quest_ids_without_partial_load() {
        let root = tmp_root("dup");
        write_scripts(
            &root,
            &vec![(("a".into(), WOLF_QUEST_LUA.into())), (("b".into(), WOLF_QUEST_LUA.into()))],
        );
        let mut svc = QuestService::new();
        let err = svc.load_quest_definitions(&root, &wolf_content());
        assert!(matches!(err, Err(QuestLoadError::Duplicate(_))), "{err:?}");
        assert!(svc.find_definition("q_test_wolves").is_none(), "kein partial load");
    }

    #[test]
    fn loader_rejects_unsupported_objective_type() {
        let root = tmp_root("badkind");
        write_scripts(
            &root,
            &vec![(
                "escort".into(),
                r#"return {
    id = "q_escort",
    title_key = "t", description_key = "d",
    requirements = { level = 1 },
    objectives = { { id = "o1", type = "escort", target = "wolf", required = 1 } },
}"#.into(),
            )],
        );
        let mut svc = QuestService::new();
        let err = svc.load_quest_definitions(&root, &wolf_content());
        assert!(matches!(err, Err(QuestLoadError::Invalid(_))), "{err:?}");
        assert!(svc.find_definition("q_escort").is_none());
    }

    #[test]
    fn loader_rejects_invalid_quantity_and_missing_fields() {
        let root = tmp_root("badqty");
        write_scripts(
            &root,
            &vec![(
                "bad".into(),
                r#"return {
    id = "q_bad",
    title_key = "t", description_key = "d",
    requirements = { level = 1 },
    objectives = { { id = "o1", type = "kill", target = "wolf", required = 0 } },
}"#.into(),
            )],
        );
        let mut svc = QuestService::new();
        assert!(matches!(
            svc.load_quest_definitions(&root, &wolf_content()),
            Err(QuestLoadError::Invalid(_))
        ));
        assert!(svc.find_definition("q_bad").is_none());
    }

    #[test]
    fn loader_rejects_unresolvable_content_reference() {
        let root = tmp_root("badref");
        write_scripts(&root, &vec![("q".into(), WOLF_QUEST_LUA.into())]);
        // Content-Schicht WITHOUT „wolf" → the Target does not resolve.
        let empty = ContentIndex { monsters: HashSet::new(), items: HashSet::new() };
        let mut svc = QuestService::new();
        let err = svc.load_quest_definitions(&root, &empty);
        assert!(matches!(err, Err(QuestLoadError::UnresolvedRef(_))), "{err:?}");
        assert!(svc.find_definition("q_test_wolves").is_none());
    }

    #[test]
    fn loader_reports_lua_syntax_error_with_reason() {
        let root = tmp_root("syntax");
        write_scripts(
            &root,
            &vec![("broken".into(), "return { id = ((( ".into())],
        );
        let mut svc = QuestService::new();
        let err = svc.load_quest_definitions(&root, &wolf_content());
        match err {
            Err(QuestLoadError::Script(m)) => assert!(m.contains("SYNTAX_ERROR"), "{m}"),
            other => panic!("expected Script, got {other:?}"),
        }
        assert_eq!(svc.definitions().count(), 0);
    }

    #[test]
    fn loader_is_all_or_nothing_on_mixed_batch() {
        let root = tmp_root("mixed");
        write_scripts(
            &root,
            &vec![(("good".into(), WOLF_QUEST_LUA.into())),
                (
                    "bad".into(),
                    r#"return {
    id = "q_bad",
    title_key = "t", description_key = "d",
    requirements = { level = 1 },
    objectives = { { id = "o1", type = "kill", target = "dragon", required = 2 } },
}"#.into(),
                ),
            ],
        );
        let mut svc = QuestService::new();
        let err = svc.load_quest_definitions(&root, &wolf_content());
        assert!(matches!(err, Err(QuestLoadError::UnresolvedRef(_))), "{err:?}");
        // The VALID Definition is NOT registered behind the defective one.
        assert!(svc.find_definition("q_test_wolves").is_none());
    }

    #[test]
    fn loader_requires_resolve_against_known_quests() {
        let root = tmp_root("reqchain");
        write_scripts(
            &root,
            &[
                (
                    "a".into(),
                    r#"return {
    id = "q_a",
    title_key = "t", description_key = "d",
    requirements = { level = 1 },
    objectives = { { id = "o1", type = "kill", target = "wolf", required = 1 } },
}"#.into(),
                ),
                (
                    "b".into(),
                    r#"return {
    id = "q_b",
    title_key = "t", description_key = "d",
    requirements = { level = 3, requires = { "q_a" } },
    objectives = { { id = "o1", type = "kill", target = "wolf", required = 2 } },
}"#.into(),
                ),
            ],
        );
        let mut svc = QuestService::new();
        let n = svc.load_quest_definitions(&root, &wolf_content()).unwrap();
        assert_eq!(n, 2);
        let b = svc.find_definition("q_b").unwrap();
        assert_eq!(b.requires, vec!["q_a".to_string()]);
        assert_eq!(b.min_level, 3);
    }

    #[test]
    fn loader_rejects_unknown_fields_precisely() {
        let root = tmp_root("unkfield");
        write_scripts(
            &root,
            &[(
                "typo".into(),
                r#"return {
    id = "q_typo",
    titel_key = "t",
    description_key = "d",
    requirements = { level = 1 },
    objectives = { { id = "o1", type = "kill", target = "wolf", required = 1 } },
}"#.into(),
            )],
        );
        let mut svc = QuestService::new();
        let err = svc.load_quest_definitions(&root, &wolf_content());
        match err {
            Err(QuestLoadError::Invalid(m)) =>
                assert!(m.contains("unknown field 'titel_key'"), "{m}"),
            other => panic!("expected Invalid(unknown field), got {other:?}"),
        }
        assert!(svc.find_definition("q_typo").is_none());
    }

    #[test]
    fn loader_rejects_unsupported_reward_fields() {
        let root = tmp_root("rewards");
        write_scripts(
            &root,
            &[(
                "rew".into(),
                r#"return {
    id = "q_rew",
    title_key = "t", description_key = "d",
    requirements = { level = 1 },
    objectives = { { id = "o1", type = "kill", target = "wolf", required = 1 } },
    rewards = { xp = 500 },
}"#.into(),
            )],
        );
        let mut svc = QuestService::new();
        let err = svc.load_quest_definitions(&root, &wolf_content());
        match err {
            Err(QuestLoadError::Invalid(m)) =>
                assert!(m.contains("unsupported reward kind 'xp'"), "{m}"),
            other => panic!("expected Invalid(rewards), got {other:?}"),
        }
        assert!(svc.find_definition("q_rew").is_none());
    }

    #[test]
    fn loader_allows_missing_or_empty_rewards() {
        let root = tmp_root("norewards");
        write_scripts(
            &root,
            &[
                (
                    "empty".into(),
                    r#"return {
    id = "q_empty_rew",
    title_key = "t", description_key = "d",
    requirements = { level = 1 },
    objectives = { { id = "o1", type = "kill", target = "wolf", required = 1 } },
    rewards = {},
}"#.into(),
                ),
                (
                    "absent".into(),
                    r#"return {
    id = "q_no_rew",
    title_key = "t", description_key = "d",
    requirements = { level = 1 },
    objectives = { { id = "o1", type = "kill", target = "wolf", required = 1 } },
}"#.into(),
                ),
            ],
        );
        let mut svc = QuestService::new();
        let n = svc.load_quest_definitions(&root, &wolf_content()).unwrap();
        assert_eq!(n, 2, "missing/empty rewards stay zulässig");
    }

    #[test]
    fn invalid_definitions_are_rejected_on_register() {
        let mut svc = QuestService::new();
        assert!(matches!(
            svc.register(QuestDefinition {
                id: "no_objectives".into(),
                giver: None,
                turn_in: None,
                rewards: RewardBundle { exp: 0, idia: 0 },
                title_key: "t".into(),
                description_key: "d".into(),
                objectives: Vec::new(),
                min_level: 1,
                requires: Vec::new(),
                repeatable: false,
            }),
            Err(QuestError::InvalidDefinition(_))
        ));
        assert!(matches!(
            svc.register(quest_def(
                "dup_objectives",
                1,
                &[],
                vec![
                    QuestObjective {
                        id: "a".into(),
                        kind: ObjectiveType::Kill,
                        target: "wolf".into(),
                        required: 1,
                    },
                    QuestObjective {
                        id: "a".into(),
                        kind: ObjectiveType::Collect,
                        target: "fur".into(),
                        required: 2,
                    },
                ],
            )),
            Err(QuestError::InvalidDefinition(_))
        ));
        assert!(matches!(
            svc.register(quest_def(
                "zero_required",
                1,
                &[],
                vec![QuestObjective {
                    id: "a".into(),
                    kind: ObjectiveType::Kill,
                    target: "wolf".into(),
                    required: 0,
                }],
            )),
            Err(QuestError::InvalidDefinition(_))
        ));
    }
}
