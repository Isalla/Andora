// handlers — Spielnachrichten (Port von src/realm handlers/*).
// HELLO: Handoff- UND Session-Prüfung (Zielkette Login → Realm; fail-closed,
// sonst wäre die Elternkontrolle über Session-Löschung umgehbar).
// MOVE: Speed-Cap. CHAT: Eltern-Gate + AOFB-Broadcast. HEARTBEAT: SYNC-ACK.
use std::sync::Arc;
use std::time::Instant;

use sqlx::{MySql, Pool};
use tokio::sync::mpsc;

use crate::auth_api::AuthApi;
use crate::combat::CombatState;
use crate::combat::ability::AbilityRegistry;
use crate::config::{CombatCfg, Config, NpcCfg};
use crate::db;
use crate::group::{self, GroupManager, SharedGroups};
use crate::npc::aggro_trigger;
use crate::parental::{self, SharedParental};
use crate::protocol::{s2c, Frame};
use crate::world::{apply_move, close_conn, ensure_visible, truncate_chat, Player, Shared, World};
use crate::attributes;

pub struct Ctx {
    pub cfg: Arc<Config>,
    pub db: Pool<MySql>,
    pub auth: AuthApi,
    pub shared: Shared,
    pub parental: SharedParental,
    pub registry: AbilityRegistry,
    pub groups: SharedGroups,
    /// Quest V1 (docs/Quest-System.md §27): zentrale serverseitige
    /// Quest-Komponente. Registerspieldefinitionen (internes V1.1-Format);
    /// Spielerzustand wird aus der Tabelle `quests` geladen/persistiert.
    pub quest: crate::quest::QuestService,
    /// Stufe-B-Persistenz: Spool + Status (Recovery/Ready/Degraded). Wird im
    /// HELLO-Guard (Recovering → Login blockiert) und beim finalen
    /// Disconnect-Save genutzt.
    pub persist: std::sync::Arc<crate::spool::PersistRuntime>,
}

fn get_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Einstiegsprüfung (fail-closed), sobald die Auth-API konfiguriert ist:
/// 1) gültiger, realm-gebundener Einmal-Handoff (wird verbraucht),
/// 2) gültige Session (Auth-API), deren account_id IDENTISCH dem
///    Handoff-Account ist.
/// Ohne beides verweigert der Realm den Einstieg — sonst könnte ein
/// Spieler seine Session löschen/lösen und die Elternkontrolle
/// (Playtime, Puffer, Force-Logout) umgehen.
/// Gibt das Account-ID zurück; bei Auth-API-Fehlern, ungültigem
/// Handoff, fehlender/ungültiger Session oder Account-Discrepanz Err.
/// Ohne konfigurierte Auth-API (Entwicklung/Test) ist das ein No-Op
/// (account_id 0, wie bisher).
pub async fn verify_entry(
    auth: &AuthApi,
    realm_id: u32,
    handoff: &str,
    session_id: &str,
) -> Result<u32, String> {
    if !auth.enabled() {
        return Ok(0);
    }
    if handoff.is_empty() {
        return Err("handoff required".into());
    }
    let h = auth.validate_handoff(handoff).await.map_err(|e| {
        log::error!("HELLO handoff validate: {e}");
        "handoff_unavailable".to_string()
    })?;
    let Some(h_account) = h.account_id.filter(|a| *a != 0) else {
        return Err("handoff_invalid".to_string());
    };
    if !h.valid || h.realm_id != Some(realm_id) {
        return Err("handoff_invalid".to_string());
    }
    if session_id.is_empty() {
        return Err("session required".into());
    }
    let s = auth.validate_session(session_id).await.map_err(|e| {
        log::error!("HELLO session validate: {e}");
        "session_unavailable".to_string()
    })?;
    if !s.valid || s.account_id != Some(h_account) {
        return Err("session_invalid".to_string());
    }
    Ok(h_account)
}

/// Einheitliche externe Ablehnung des Charakterzugriffs beim Realm-Einstieg.
/// Ungültige, nicht gefundene und fremde `character_id` sowie
/// Datenbankfehler sind für den Client bewusst nicht unterscheidbar, damit der
/// Ablehnungsfall keine Ownership-Information preisgibt. Intern unterscheiden
/// die Logzeilen den technischen Grund, ohne Session-ID, Token oder andere
/// Zugangsdaten zu übernehmen.
const CHARACTER_UNAVAILABLE: &str = "character unavailable";

/// Löst das Ergebnis des Charakter-Lookups fail-closed auf.
///
/// `Ok(Some(_))` wird durchgereicht; `Ok(None)` (kein Datensatz für `id` und
/// `account_id`) und `Err` (Datenbankfehler) ergeben beide dieselbe externe
/// Ablehnung. Kein Schreibzugriff, keine Charakteranlage.
pub fn resolve_character_lookup(
    lookup: Result<Option<db::Character>, String>,
) -> Result<db::Character, String> {
    match lookup {
        Ok(Some(c)) => Ok(c),
        Ok(None) => {
            log::info!("HELLO abgelehnt: kein Character für die angegebene character_id");
            Err(CHARACTER_UNAVAILABLE.to_string())
        }
        Err(e) => {
            log::error!("HELLO load character: {e}");
            Err(CHARACTER_UNAVAILABLE.to_string())
        }
    }
}

/// Bedingtes Parental-Cleanup nach einem Fehler **vor** `commit_login`.
///
/// Detacht **nur**, wenn für den Charakter kein Owner existiert. Grund: bei einem
/// Takeover existiert der Owner der verdrängten Sitzung, und `parental::attach`
/// hat den Elternzustand bereits ersetzt. Ein unbedingtes `detach` würde dort
/// die Elternkontrolle einer weiterhin verbundenen Sitzung entfernen — ein
/// Eingriff in den autoritativen Zustand, den AUTH-03A schützt.
///
/// Der World-Lock wird gescoped und **vor** dem `detach`-Await freigegeben;
/// es wird kein Lock über einen Await gehalten.
///
/// Gibt `true` zurück, wenn der Elternzustand entfernt wurde.
async fn detach_if_unowned(
    parental: &parental::SharedParental,
    shared: &crate::world::Shared,
    char_id: &str,
) -> bool {
    let has_owner = {
        let world = shared.lock().await;
        crate::world::conn_of(&world, char_id).is_some()
    };
    if has_owner {
        return false;
    }
    parental::detach(parental, char_id).await;
    true
}

/// Outcome-abhängige RAM-Anwendung nach der Login-Abrechnung.
///
/// **Rein:** kein Lock, kein DB-Zugriff, keine Persistenzwirkung. Liefert
/// `(neuer rested_pool, Reconciliation nötig)`.
///
/// Grundlagen (docs/Security.md `P-32`, docs/Login_Realm_Architektur.md):
/// * `Registered` — es existiert **kein** vorheriger autoritativer RAM-Player.
///   Der neu registrierte Player muss exakt den Wert tragen, der unmittelbar
///   zuvor gemeinsam mit `logout_at = NULL` abgerechnet wurde. Eine
///   Dirty-Markierung wäre hier falsch: sie könnte einen abweichenden
///   Kandidatenwert zurückschreiben.
/// * `Adopted` / `Takeover` — der bestehende RAM-Player bleibt autoritativ.
///   Ein **positives** Delta (der Offline-Zuwachs) wird additiv angewandt; ein
///   **negatives** Delta aus dem DB-Kandidaten (Kappung nach
///   `Erfahrung_und_Progressionssystem.md` §12.3) wird **nie** angewandt, weil
///   er den autoritativen RAM-Stand verschlechtern würde.
/// * `Refreshed` — keine erneute Gutschrift, keine Kandidatenkappung.
///
/// `delta > 0` ist die Mehrzahl der Fälle additiv und damit gegenüber einer
/// zwischenzeitlichen Spielmutation (Kill-EXP verbraucht den Pool über
/// `apply_progression`) verträglich. Die Signed-Form deckt zusätzlich den
/// Kappungsfall ab, in dem `credited < c.rested_pool`.
///
/// `true` als zweiter Rückgabewert bedeutet: die DB hält nach der Abrechnung
/// `credited`, der autoritative RAM-Player `pool` — der nächste Snapshot muss
/// den RAM-Stand in die DB bringen. Das dient **ausschließlich** der
/// Reconciliation des `rested_pool` und ist **kein** Retry für `logout_at`;
/// der Reset ist bereits in derselben Transaktion geschrieben.
fn apply_login_settlement(
    outcome: crate::world::CommitOutcome,
    ram_pool: i64,
    delta: i64,
    credited: i64,
) -> (i64, bool) {
    use crate::world::CommitOutcome as Outcome;
    let pool = match outcome {
        Outcome::Registered => credited,
        Outcome::Adopted | Outcome::Takeover { .. } if delta > 0 => ram_pool.saturating_add(delta),
        Outcome::Adopted | Outcome::Takeover { .. } | Outcome::Refreshed => ram_pool,
    };
    (pool, pool != credited)
}

/// HELLO: Einstieg mit Handoff-Token (Zielkette), Charakter laden,
/// Elternkontrolle anhängen, WELCOME + Nachbar-Spawns.
///
/// Reihenfolge (AUTH-03, docs/Login_Realm_Architektur.md „Verbindungs-
/// Einzigkeit und Takeover“): ALLE falliblen Schritte laufen VOR dem Commit.
/// Erst danach verändert `world::commit_login` die Verbindungszuordnung:
/// Erstlogin registriert den Player, ein zweiter Login übernimmt den
/// bestehenden RAM-Zustand (kein Überschreiben durch einen älteren
/// DB-Snapshot), entmachtet die alte `conn_id` und signalisiert erst danach
/// deren Closer. Gibt bei Ablehnung Err(reason) zurück (Verbindung schließen).
pub async fn handle_hello(
    ctx: &Ctx,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &serde_json::Value,
) -> Result<(), String> {
    let raw_char_id = get_str(data, "char_id");
    // Charakter-ID fail-closed validieren (docs/Login_Realm_Architektur.md
    // Abschnitt 6): ausschließlich die kanonische Dezimaldarstellung einer
    // positiven Datenbank-ID. Aliase ("1", "01", "+1", " 1") werden
    // abgelehnt, damit Gate-, World- und DB-Lookup nie unterschiedliche
    // Schlüssel bilden. Die Prüfung läuft VOR Persistence-Gate, World-Zugriff
    // und Datenbankzugriff.
    let char_id_db = db::parse_character_id(&raw_char_id).map_err(|reason| {
        log::info!("HELLO abgelehnt: character_id ungültig ({reason})");
        CHARACTER_UNAVAILABLE.to_string()
    })?;
    // Kanonische Darstellung: identischer Schlüssel für Gate, World und DB.
    let char_id = char_id_db.to_string();
    // Stufe B Login-Guard (docs/Player_Persistenz.md §34/§36/§37): Solange
    // der Realm im Startup-Recovery ist (Recovering), wird der HELLO-Einstieg
    // VERWEIGERT (Login blockiert — kein Spieler betritt einen Realm, dessen
    // Spool noch nicht vollständig auf die DB gedrained ist). Bei `Degraded`
    // ist der Einstieg erlaubt (Realm läuft, Drain retryt periodisch).
    if ctx.persist.status() == crate::spool::PersistStatus::Recovering {
        return Err("realm still recovering (Spool-Recovery) — retry later".into());
    }
    let lang = {
        let l = get_str(data, "lang");
        if l.is_empty() {
            "de".to_string()
        } else {
            l
        }
    };
    let session_id = get_str(data, "session_id");
    let handoff = get_str(data, "handoff_token");

    // Eintritt nur mit gültigem, realm-gebundenem Handoff UND gültiger,
    // account-identischer Session, sobald die Auth-API konfiguriert ist
    // (fail-closed; ohne wäre die Elternkontrolle umgehbar). Der Handoff
    // wird dabei verbraucht (einmalig).
    let account_id = verify_entry(&ctx.auth, ctx.cfg.realm_id, &handoff, &session_id).await?;

    // AUTH-03: Serialisierungsgrenze gegen den Logout-Commit einer ALTEN
    // Verbindung derselben `char_id`. Bestehendes per-player-Gate (identisch
    // zum Persistenz-Gate, `Spool::player_gate`), geholt VOR der ersten
    // World-Sperre und VOR dem ersten DB-Zugriff und gehalten bis über den
    // Commit. Damit ist die Reihenfolge garantiert:
    //   alter Logout-Write  <  Offline-Abrechnung(logout_at = NULL)  <  Commit
    // Der Login wartet also auf einen laufenden Logout-Write, berechnet die
    // Rested-Zeit aus dem echten Logout-Zeitpunkt der beendeten Sitzung und
    // setzt die Spalte danach selbst zurück — ein verspäteter Logout-Write der
    // alten Sitzung kann die aktive Sitzung nicht mehr markieren.
    let _logout_gate = ctx.persist.player_gate(&char_id).await.lock_owned().await;

    // AUTH-03 (Takeover): Die neue Verbindung darf erst übernehmen, wenn
    // ALLE falliblen Vorprüfungen erfolgreich waren. Der Ownership-Nachweis
    // gegen einen bereits aktiven RAM-Player wird deshalb VOR jedem
    // DB-Zugriff geprüft (fail-closed): ein fremder Account darf den
    // Charakter nicht einmal laden.
    {
        let world = ctx.shared.lock().await;
        crate::world::ensure_takeover_allowed(&world, &char_id, account_id)?;
    }

    // Charakter ausschließlich lesend laden und fail-closed auflösen: kein
    // Treffer (fehlend oder fremder Account) und Datenbankfehler führen beide
    // zur selben externen Ablehnung, damit keine Ownership-Information
    // preisgegeben wird. Es wird kein Charakter angelegt.
    let c = resolve_character_lookup(db::load_character(&ctx.db, account_id, char_id_db).await)?;

    // P-30 (docs/Player_Persistenz.md §33): Quarantäne-Sperre ist
    // **charakterbezogen**. Sie läuft unter dem bereits gehaltenen
    // `player_gate` (siehe oben), nach erfolgreichem Ownership-Load und vor
    // jedem RAM-Aufbau. Ein ungelöster Fall sperrt ausschließlich diesen
    // Charakter; andere Charaktere desselben Kontos bleiben spielbar.
    //
    // `CheckFailed` bedeutet: der Bestand war nicht zuverlässig lesbar. Dann
    // wird **fail-closed** für genau diesen Charakter abgelehnt — sonst könnte
    // bei einem Prüfversagen eine ältere DB-Zeile geladen werden. Das ist
    // keine Sanktion und keine dauerhafte Sperre.
    match ctx
        .persist
        .evaluate_character_availability(&c.id, c.persist_revision)
    {
        crate::spool::CharacterAvailability::Available => {}
        crate::spool::CharacterAvailability::SaveRecoveryPending => {
            return Err(crate::spool::SAVE_RECOVERY_PENDING.to_string());
        }
        crate::spool::CharacterAvailability::CheckFailed => {
            return Err(crate::spool::SAVE_RECOVERY_CHECK_FAILED.to_string());
        }
    }

    // AUTH-03 (Restzustand nach Disconnect): Liegt im Spool ein NEUERER
    // Snapshot als die gerade geladene DB-Zeile, ist der DB-Stand veraltet
    // (der Spool-Batch ist noch nicht gedraint). Dann darf KEIN aus der
    // DB-Zeile gebauter Player registriert werden — sonst würde der zuletzt
    // autoritative Zustand überschrieben. Fail-closed: Login abweisen, der
    // periodische Drain zieht nach, ein späterer Versuch gelingt.
    let pending = ctx.persist.pending_revision(&c.id);
    if crate::world::db_row_is_stale(c.persist_revision, pending.clone()) {
        if let Err(ref e) = pending {
            log::error!("HELLO pending revision {char_id}: {e}");
        }
        return Err("character state still in spool — retry later".into());
    }

    // P-30: Bereits DB-bestätigt abgelöste Fälle best-effort archivieren.
    // Notwendig für Altbestände und für Fälle, deren erfolgreicher Drain vor
    // Einführung der Archivlogik lag. Gate-frei, weil `player_gate` bereits
    // gehalten wird. Ein Archivierungsfehler ist **nur** eine Betreiberwarnung
    // und lässt den Charakter spielbar; die Sperrentscheidung oben steht bereits.
    let _ = ctx
        .persist
        .archive_resolved_quarantine_cases_inner(&c.id, c.persist_revision);

    let weapon_skill = db::load_weapon_skill(&ctx.db, &c.id, &ctx.cfg.combat.weapon_skill_id).await;
    let learned_abilities: std::collections::HashSet<String> =
        db::load_character_abilities(&ctx.db, &c.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();
    // Inventory V1: Inventar + Rucksäcke + Equipment laden; serverseitig
    // defektes Equipment (0 Haltbarkeit) beim Einstieg entfernen
    // (docs/inventory_system.md §10/§12). Fehler am Laden => leeres
    // Inventar (Fallback wie save_position, kein Login-Abbruch).
    let mut inventory = db::load_inventory(&ctx.db, &c.id, ctx.cfg.inventory.base_slots as usize)
        .await
        .unwrap_or_else(|e| {
            log::error!("HELLO load inventory: {e}");
            crate::inventory::InventoryState::new(ctx.cfg.inventory.base_slots as usize)
        });
    let broken_moved = {
        let world = ctx.shared.lock().await;
        inventory.remove_broken_equipment(&world.item_definitions)
    };
    // Rested-EXP (docs/Erfahrung_und_Progressionssystem.md §12): einmalige
    // Berechnung beim Login aus dem letzten Logout-Zeitpunkt. Hier wird
    // **nur berechnet** — der Write erfolgt als atomare Offline-Abrechnung
    // unmittelbar vor `commit_login` (siehe dort), damit ein fehlschlagender
    // Einstieg weder `logout_at` zurücksetzt noch eine Gutschrift bucht.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let credited = crate::progression::apply_offline_rested(
        &ctx.cfg.progression,
        c.level,
        c.rested_pool,
        c.logout_at,
        now_secs,
    );
    // Signed: `apply_offline_rested` kappt auf `max_pool`; ist der geladene
    // Pool bereits überfüllt, ist `credited < c.rested_pool` und `delta` negativ.
    let delta = credited - c.rested_pool;
    // Quest V1 (§13): persistierte Spieler-Questzustände (ACTIVE/COMPLETED/
    // FAILED) laden. Fehler am Laden => leerer Questzustand (Fallback wie
    // save_position, kein Login-Abbruch). HIDDEN/AVAILABLE sind abgeleitet
    // (§27.5) und liegen nie in der Tabelle `quests`.
    //
    // Sicherheitsaudit (V1.2a, §27.26-Doppelabschluss-Schutz): Ein nicht
    // zuverlässig geladener Questzustand darf NICHT als "Spieler hat keine
    // Quests" behandelt werden (der Spieler könnte eine bereits COMPLETED
    // Quest erneut annehmen oder eine zweite Belohnung erwirken). Deshalb
    // FAIL-CLOSED: Wie bei load_character wird der Einstieg abgelehnt,
    // statt mit leerem Questzustand weiterzuspielen.
    let quests: std::collections::BTreeMap<String, crate::quest::CharacterQuestState> = ctx
        .quest
        .load_for_character(&ctx.db, &c.id)
        .await
        .map_err(|e| {
            log::error!("HELLO load quests: {e}");
            "quest state unavailable"
        })?
        .into_iter()
        .map(|s| (s.quest_id.clone(), s))
        .collect();
    // `P-18`: Laufende Ability-Cooldowns laden. Sie sind absolute Ablaufzeitpunkte
    // und müssen Logout/Reconnect überdauern; Offline-Zeit zählt normal mit
    // (ein abgelaufener Zeitpunkt gibt die Fähigkeit sofort frei).
    //
    // FAIL-CLOSED wie der Questzustand: Ein nicht zuverlässig lesbarer
    // Cooldown-Stand darf NICHT als „der Charakter hat keine Cooldowns"
    // behandelt werden — das würde laufende Cooldowns zurücksetzen und die
    // Zusage aus docs/Player_Persistenz.md §23 aufheben. Daher wird der Einstieg
    // abgelehnt statt mit leerer Map fortzusetzen.
    let cooldowns: std::collections::BTreeMap<String, std::time::SystemTime> =
        db::load_character_cooldowns(&ctx.db, &c.id)
            .await
            .map_err(|e| {
                log::error!("HELLO load cooldowns: {e}");
                "cooldown state unavailable"
            })?;
    let me = Player {
        id: c.id.clone(),
        name: c.name.clone(),
        x: c.x,
        y: c.y,
        face: 0.0,
        ping_ms: 0,
        zone_id: 0,
        hp: c.hp,
        max_hp: c.hp,
        mana: c.mana,
        max_mana: c.mana_max,
        // `lang` wird zusätzlich an die ConnectionFields des Commits
        // übergeben (dort frisch gesetzt) — deshalb hier klonen.
        lang: lang.clone(),
        account_id,
        session_id: session_id.clone(),
        entities: Default::default(),
        last_activity: Instant::now(),
        tx: tx.clone(),
        char_class: c.char_class.clone(),
        class: c.class,
        faction_transition: c.faction_transition,
        level: c.level,
        armor: c.armor,
        exp: c.exp,
        free_attr_points: c.free_attr_points,
        // Ungecrediteter DB-Stand: der Kandidat ist noch nicht autoritativ,
        // die Gutschrift erfolgt nach der Abrechnung über
        // `apply_login_settlement` (bei `Registered` auf `credited` gesetzt).
        rested_pool: c.rested_pool,
        idia: c.idia,
        weapon_skill,
        combat: None,
        last_strike: None,
        effects: Vec::new(),
        cooldowns,
        active_cast: None,
        learned_abilities,
        attributes: attributes::Attributes {
            strength: c.strength,
            constitution: c.constitution,
            dexterity: c.dexterity,
            intelligence: c.intelligence,
            wisdom: c.wisdom,
            luck: c.luck,
            endurance: c.endurance,
        },
        max_hp_base: c.hp,
        max_mana_base: c.mana_max,
        sitting: false,
        hp_regen_bonus: 0.0,
        mana_regen_bonus: 0.0,
        hp_regen_carry: 0.0,
        mana_regen_carry: 0.0,
        inventory,
        quests,
        dirty: Default::default(),
        persist_generation: 0,
        persist_revision: c.persist_revision,
    };
    let mut me = me;
    attributes::recompute_max_resources(&mut me);

    // Elternkontrolle VOR dem Commit: BLOCKED am Login -> Einstieg verweigert.
    // Wichtig für Takeover: schlägt der Attach fehl, wurde noch NICHTS
    // übergeben — die bisherige Eigentümer-Verbindung bleibt unangetastet
    // (fail-closed, keine zwei Eigentümer).
    if let Err(reason) = parental::attach(&ctx.parental, tx, &c.id, account_id, &session_id).await {
        parental::detach(&ctx.parental, &c.id).await;
        return Err(reason);
    }

    // ── Offline-Abrechnung (docs/Login_Realm_Architektur.md, „Offline-
    // Abrechnung unmittelbar vor dem Commit") ───────────────────────────
    // Gutschrift und Zurücksetzen von `logout_at` werden gemeinsam und
    // unteilbar in EINER Transaktion geschrieben (`db::save_progression`).
    // Position: nach allen falliblen Vorprüfungen, unmittelbar vor dem Commit.
    // Schlägt sie fehl, wird der Login NICHT committet: `logout_at` und der
    // noch nicht konsumierte Offline-Zeitraum bleiben erhalten. Der Disconnect
    // kann das nicht reparieren (er setzt `logout_at = now`), deshalb Abbruch.
    // Kein World-Lock während des Awaits.
    if let Err(e) = db::save_progression(
        &ctx.db,
        &c.id,
        account_id,
        c.level,
        c.exp,
        c.free_attr_points,
        credited,
        None,
    )
    .await
    {
        log::error!("HELLO settlement {char_id}: {e}");
        detach_if_unowned(&ctx.parental, &ctx.shared, &c.id).await;
        return Err("character settlement unavailable".into());
    }

    // ── Commit (AUTH-03) ────────────────────────────────────────────────
    // EINZIGER Punkt, der `by_conn` (Berechtigung zur Spiellogik) verändert.
    // Alle falliblen Schritte (Auth, Charakterprüfung, Questzustand,
    // Elternkontrolle, Offline-Abrechnung) liegen davor. Der Aufruf ist unter
    // der World-Sperre atomar: beim Takeover wird die alte `conn_id` ZUERST
    // entmachtet, dann die neue eingesetzt; der bestehende RAM-Player bleibt
    // maßgeblich.
    let outcome = {
        let mut world = ctx.shared.lock().await;
        crate::world::commit_login(
            &mut world,
            conn_id,
            me,
            crate::world::ConnectionFields {
                tx: tx.clone(),
                session_id: session_id.clone(),
                lang: lang.clone(),
            },
        )
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(reason) => {
            // Commit abgelehnt: nur aufräumen, was diese Verbindung selbst
            // angehängt hat. Ein aktiver Eigentümer behält Player UND
            // Eltern-State.
            detach_if_unowned(&ctx.parental, &ctx.shared, &c.id).await;
            return Err(reason);
        }
    };

    // Der Login ist damit fachlich zustande gekommen (P-32). Die gebuchte
    // Gutschrift wird jetzt auf den **autoritativen** RAM-Player angewandt:
    // ein await-freier World-Lock-Block, der einzige RAM-Schreibpunkt nach
    // dem Commit. `reconcile` markiert die Progression dirty, damit der nächste
    // Snapshot den RAM-Stand in die DB bringt — es ist ausschließlich die
    // Reconciliation von `rested_pool`, kein Retry für `logout_at`.
    {
        let mut world = ctx.shared.lock().await;
        if let Some(p) = world.players.get_mut(&c.id) {
            let (pool, reconcile) = apply_login_settlement(outcome, p.rested_pool, delta, credited);
            p.rested_pool = pool;
            if reconcile {
                p.mark_dirty(crate::persist::PersistComponent::Progression);
            }
        }
    }
    if let crate::world::CommitOutcome::Takeover { old_conn_id } = outcome {
        // Eigentümerwechsel abgeschlossen: erst JETZT den vorhandenen Closer
        // der verdrängten Verbindung signalisieren (kein neuer Mechanismus).
        {
            let mut world = ctx.shared.lock().await;
            close_conn(&mut world, old_conn_id);
        }
        // Normales INFO-Ereignis, keine Roh-IP (docs/Security.md AUTH-03).
        crate::security::log_takeover(account_id, &c.id, old_conn_id, conn_id);
    }

    // Serverseitige Entfernung defekten Equipments beim Einstieg persistent
    // nachschreiben (Fehler nur loggen — kein Login-Abbruch). Nur beim
    // Erstlogin: bei einem Takeover ist der RAM-Inventarstand maßgeblich und
    // darf nicht durch den (älteren) DB-Stand zurückgeschrieben werden.
    if outcome == crate::world::CommitOutcome::Registered && broken_moved > 0 {
        log::info!("HELLO {char_id}: {broken_moved} defekte Equipment-Items entfernt");
        let inventory = {
            let world = ctx.shared.lock().await;
            world.players.get(&c.id).map(|p| p.inventory.clone())
        };
        if let Some(ref inv) = inventory {
            if let Err(e) = db::save_inventory(&ctx.db, &c.id, inv).await {
                log::error!("HELLO save inventory: {e}");
            }
        }
    }

    // WELCOME aus dem autoritativen RAM-Zustand (nicht aus dem DB-Kandidaten):
    // beim Takeover ist der RAM-Stand maßgeblich.
    let (pid, pname, px, py) = {
        let world = ctx.shared.lock().await;
        let p = world.players.get(&c.id).ok_or("gone".to_string())?;
        (p.id.clone(), p.name.clone(), p.x, p.y)
    };
    tx.send(
        Frame::new(
            seq,
            s2c::WELCOME,
            serde_json::json!({"you": {"id": pid, "name": pname, "x": px, "y": py}}),
        )
        .encode(),
    )
    .map_err(|_| "send failed".to_string())?;

    // Gebiets-Kollegen: Spieler spawnen (SPAWN + STATE) und umgekehrt.
    {
        let mut world = ctx.shared.lock().await;
        let (mx, my) = {
            let me = world.players.get(&c.id).ok_or("gone".to_string())?;
            (me.x, me.y)
        };
        let others: Vec<String> = world
            .players
            .values()
            .filter(|q| q.id != c.id && (q.x - mx).hypot(q.y - my) <= ctx.cfg.aofb_radius)
            .map(|q| q.id.clone())
            .collect();
        for oid in others {
            let other = world.players.remove(&oid).unwrap();
            let mut me = world.players.remove(&c.id).unwrap();
            ensure_visible(&mut me, &other);
            me.entities.insert(oid.clone());
            world.players.insert(oid, other);
            world.players.insert(c.id.clone(), me);
        }
    }

    // Gruppensystem §9: Reconnect — Mitgliedschaft/Leitung zurücksetzen,
    // Gruppe darüber informieren.
    let gid = {
        let mut groups = ctx.groups.lock().await;
        let gid = groups.group_of(&c.id);
        if gid.is_some() {
            groups.on_reconnect(&c.id, Instant::now());
        }
        gid
    };
    if let Some(gid) = gid {
        let world = ctx.shared.lock().await;
        let groups = ctx.groups.lock().await;
        broadcast_group_info(&world, &groups, gid);
    }
    Ok(())
}

/// MOVE: Position serverseitig validiert (Speed-Cap, keine Teleports).
pub async fn handle_move(shared: &Shared, conn_id: u64, data: &serde_json::Value, tick_ms: u64) {
    let (dx, dy) = match data.get("dir").and_then(|v| v.as_array()) {
        Some(a) if a.len() >= 2 => (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)),
        _ => (
            data.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
            data.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
        ),
    };
    let mut world = shared.lock().await;
    let pid = match world.by_conn.get(&conn_id) {
        Some(pid) => pid.clone(),
        None => return,
    };

    let moving = (dx.abs() > f64::EPSILON || dy.abs() > f64::EPSILON)
        && !crate::combat::effects::is_rooted(
            &world.players.get(&pid).map(|p| &p.effects).unwrap_or(&Vec::new()),
        );

    if let Some(me) = world.players.get_mut(&pid) {
        if !moving {
            return;
        }
        let (x, y) = apply_move(me.x, me.y, dx, dy, tick_ms);
        me.x = x;
        me.y = y;
        me.mark_dirty(crate::persist::PersistComponent::Position);
        me.last_activity = Instant::now();
    }

    // Cast-Unterbrechung durch Bewegung (Ability-System.md §3;
    // Kampfsystem.md §9: Zauber werden durch Bewegung unterbrochen).
    let had_cast = world.players.get(&pid).map(|p| p.active_cast.is_some()).unwrap_or(false);
    if moving && had_cast {
        if let Some(event) = crate::combat::ability::interrupt_cast(&mut world, &pid) {
            crate::combat::ability::broadcast_combat_event(&world, 0.0, 0.0, 0.0, &event);
        }
    }
}

/// ATTACK (Combat V1 + V2): Auto-Grundangriff starten oder beenden.
/// Payload: {target_id} = starten, {stop: true} = beenden.
/// Realm-autoritativ validiert: Ziel existiert, ist nicht selbst, lebt
/// und liegt in Waffen-Reichweite. Ziel kann ein Spieler (V1) oder ein
/// NPC/Monster (V2) sein. NPCs werden nur angegriffen, wenn sie
/// `attackable` sind und nicht in Evade/Return (Combat V2, §18/§20).
/// Gültige Starts bewaffnen den Angriff; der erste Schlag folgt sofort
/// (Duration als abgelaufen gesetzt), alle weiteren im Duration-Takt
/// (siehe combat::combat_tick). Ein gültiger Angriff auf einen NPC löst
/// dessen (defensives) Aggro aus (§21 Aggro-Formen).
pub async fn handle_attack(
    shared: &Shared,
    conn_id: u64,
    data: &serde_json::Value,
    cfg: &CombatCfg,
    npc_cfg: &NpcCfg,
) {
    let mut world = shared.lock().await;
    let pid = match world.by_conn.get(&conn_id) {
        Some(pid) => pid.clone(),
        None => return,
    };
    // Stop: bewaffneten Angriff beenden (immer erlaubt).
    if data.get("stop").and_then(|v| v.as_bool()).unwrap_or(false) {
        if let Some(me) = world.players.get_mut(&pid) {
            me.combat = None;
        }
        return;
    }
    let target_id = data.get("target_id").and_then(|v| v.as_str()).unwrap_or("");
    if target_id.is_empty() || target_id == pid {
        return;
    }
    // Ist das Ziel ein NPC? (Target-Namespace: "npc_<spawn_id>").
    let is_npc = world.npcs.contains_key(target_id);
    // Validierung (immutable): Ziel existiert, nicht tot, in Reichweite,
    // Angreifer lebt. NPCs zusätzlich: attackable + nicht in Evade/Return.
    // Serverautorität V1: Ein serverseitig toter Angreifer (hp <= 0) darf
    // keinen gültigen Angriff ausführen — auch wenn ein manipulierter Client
    // lokal 140000 HP anzeigt und weiter Angriffe sendet. Die Aktion wird
    // verworfen (+ Auffälligkeit geloggt); der Raid/Dungeon läuft für alle
    // anderen normal weiter (kein Instanzabbruch).
    let attacker_dead = world.players.get(&pid).is_some_and(|me| me.hp <= 0);
    if attacker_dead {
        let me = world.players.get(&pid);
        crate::security::log_reject(
            me,
            conn_id,
            &crate::security::RejectInfo {
                reason: "attacker_dead".into(),
                msg_type: crate::protocol::c2s::ATTACK,
                detail: format!("target={target_id}"),
            },
            0,
        );
        return;
    }
    // Validierung (immutable): Ziel existiert, nicht tot, in Reichweite,
    // Angreifer lebt. NPCs zusätzlich: attackable + nicht in Evade/Return.
    let valid = {
        let me = match world.players.get(&pid) {
            Some(me) => me,
            None => return,
        };
        if me.hp <= 0 {
            false
        } else if is_npc {
            world.npcs.get(target_id).is_some_and(|n| {
                n.status == crate::npc::NpcStatus::Alive
                    && n.effective_attackable()
                    && (me.x - n.x).hypot(me.y - n.y) <= cfg.weapon_range
            })
        } else {
            world
                .players
                .get(target_id)
                .is_some_and(|t| t.hp > 0 && (me.x - t.x).hypot(me.y - t.y) <= cfg.weapon_range)
        }
    };
    if !valid {
        return; // kein Kampfzustand, kein Schaden.
    }
    if is_npc {
        // Defensives/soziales Aggro auslösen (§21) — NPC verteidigt sich
        // bzw. die feste Gruppe/Fraktion steigt ein.
        aggro_trigger(&mut world, target_id, &pid, npc_cfg);
    }
    let now = Instant::now();
    if let Some(me) = world.players.get_mut(&pid) {
        // Serverautomatik: Der Angriffstakt ist eine Eigenschaft des
        // Charakters, nicht der Absicht und nicht des Zieles. Maßgeblich ist
        // der zuletzt TATSÄCHLICH ausgeführte Schlag (`last_strike`, nur vom
        // Tick geschrieben); ein erneutes Eintreffen derselben Absicht — mit
        // gleicher oder neuer `seq`, gleiches oder anderes Ziel, nach Stop
        // oder nach Zieltod — setzt ihn nicht zurück und schaltet keinen
        // zusätzlichen Sofortschlag frei. Ohne bisherigen Schlag (erstmalige
        // Aktivierung in dieser RAM-Existenz) gilt der dokumentierte
        // Sofortschlag: die Duration gilt als bereits abgelaufen.
        //
        // Die Absicht selbst bleibt vollständig gewahrt: Ziel, Reichweite und
        // Lebendigkeit wurden oben bereits validiert, `combat` wird hier
        // bewaffnet; `seq` ist an dieser Stelle nicht verfügbar und nicht
        // erforderlich (Korrelation, keine Berechtigung).
        let cadence_from = me.last_strike.unwrap_or_else(|| {
            now.checked_sub(std::time::Duration::from_millis(cfg.weapon_duration_ms))
                .unwrap_or(now)
        });
        me.combat = Some(CombatState {
            target_id: target_id.to_string(),
            last_attack: cadence_from,
        });
    }
}

/// ABILITY (Combat V3): Fähigkeit auslösen.
/// Payload: {ability_id, target_id?, x?, y?}
/// Realm-autoritativ: Cast-Management, Mana, Cooldown, Effekte.
pub async fn handle_ability(
    ctx: &Ctx,
    conn_id: u64,
    data: &serde_json::Value,
) {
    let pid = {
        let world = ctx.shared.lock().await;
        match world.by_conn.get(&conn_id) {
            Some(pid) => pid.clone(),
            None => return,
        }
    };

    let ability_id = get_str(data, "ability_id");
    if ability_id.is_empty() {
        return;
    }
    let target_id = {
        let t = get_str(data, "target_id");
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    };
    let ground_x = data.get("x").and_then(|v| v.as_f64());
    let ground_y = data.get("y").and_then(|v| v.as_f64());

    let mut world = ctx.shared.lock().await;
    let now = std::time::Instant::now();
    let wall_now = std::time::SystemTime::now();

    // Sende-Resultat an den Casting-Spieler zurück.
    let events = crate::combat::ability::start_ability(
        &mut world,
        &ctx.registry,
        &pid,
        &ability_id,
        target_id.as_deref(),
        ground_x,
        ground_y,
        now,
        wall_now,
    );

    let (caster_x, caster_y) = world
        .players
        .get(&pid)
        .map(|p| (p.x, p.y))
        .unwrap_or((0.0, 0.0));
    // Serverautorität V1: Ablehnungen (u. a. toter Caster — ein manipulierter
    // Client ignoriert ggf. den Todeszustand und castet weiter) verändern den
    // Zustand nicht; sie werden nur verworfen (+ Auffälligkeit geloggt).
    for event in &events {
        if let crate::combat::events::CombatEvent::AbilityResult {
            outcome: crate::combat::events::AbilityOutcome::Failed,
            reason: Some(reason),
            ability_id,
            ..
        } = event
        {
            crate::security::log_reject(
                world.players.get(&pid),
                conn_id,
                &crate::security::RejectInfo {
                    reason: reason.clone().into(),
                    msg_type: crate::protocol::c2s::ABILITY,
                    detail: format!("ability={ability_id}"),
                },
                0,
            );
            break;
        }
    }
    for event in &events {
        crate::combat::ability::broadcast_combat_event(
            &world, caster_x, caster_y, ctx.cfg.aofb_radius, event,
        );
    }
}

/// CHAT: Eltern-Gate, 240-Zeichen-Cap, AOFB-Broadcast (+ Echo an selbst).
pub async fn handle_chat(
    parental: &SharedParental,
    shared: &Shared,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &serde_json::Value,
    aofb_radius: f64,
) {
    let pid = {
        let world = shared.lock().await;
        match world.by_conn.get(&conn_id) {
            Some(pid) => pid.clone(),
            None => return,
        }
    };
    if !parental::chat_allowed(parental, &pid).await {
        let _ = tx.send(
            Frame::new(
                seq,
                s2c::PARENTAL_RESULT,
                serde_json::json!({"ok": false, "reason": "chat_locked"}),
            )
            .encode(),
        );
        return;
    }
    let text = data.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let text = truncate_chat(text);
    if text.is_empty() {
        return;
    }
    let channel = data
        .get("channel")
        .and_then(|v| v.as_str())
        .unwrap_or("local");
    let world = shared.lock().await;
    let Some(me) = world.players.get(&pid) else {
        return;
    };
    let payload = Frame::new(
        0,
        s2c::CHAT,
        serde_json::json!({"from": me.name, "channel": channel, "text": text}),
    )
    .encode();
    for o in world.players.values() {
        if (o.x - me.x).hypot(o.y - me.y) > aofb_radius {
            continue;
        }
        let _ = o.tx.send(payload.clone());
    }
    let _ = tx.send(payload);
}

// ── Gruppensystem V1 (docs/Gruppensystem.md §§1–9) ──────────────────────

/// GROUP_INFO-Payload: Mitgliedsdaten (id, name, class, level, hp, mp,
/// online, is_leader, in_range, effects) + leader_id. in_range = online
/// UND innerhalb cfg.range um den aktuellen Leiter (§3, Mittelpunkt).
fn group_info_json(
    world: &World,
    groups: &GroupManager,
    group_id: u64,
) -> Option<serde_json::Value> {
    let group = groups.get_group(group_id)?;
    let leader_pos = world.players.get(&group.leader_id).map(|p| (p.x, p.y));
    let members: Vec<serde_json::Value> = group
        .members
        .values()
        .map(|m| {
            let p = world.players.get(&m.player_id);
            let (x, y) = p.map(|p| (p.x, p.y)).unwrap_or((0.0, 0.0));
            let in_range = leader_pos.is_some_and(|(lx, ly)| {
                (lx - x).hypot(ly - y) <= groups.cfg.range
            });
            let effects: Vec<&str> = p
                .map(|p| {
                    p.effects
                        .iter()
                        .filter(|e| e.kind.tickable())
                        .map(|e| e.kind.key())
                        .collect()
                })
                .unwrap_or_default();
            serde_json::json!({
                "id": m.player_id,
                "name": p.map(|p| p.name.as_str()).unwrap_or(""),
                "class": p.map(|p| p.char_class.as_str()).unwrap_or(""),
                "level": p.map(|p| p.level).unwrap_or(0),
                "hp": p.map(|p| p.hp).unwrap_or(0),
                "mp": p.map(|p| p.mana).unwrap_or(0),
                "online": m.online,
                "is_leader": group.leader_id == m.player_id,
                "in_range": m.online && in_range,
                "effects": effects,
            })
        })
        .collect();
    Some(serde_json::json!({
        "group_id": group.id,
        "leader_id": group.leader_id,
        "members": members,
    }))
}

fn broadcast_group_info(world: &World, groups: &GroupManager, group_id: u64) {
    let Some(info) = group_info_json(world, groups, group_id) else {
        return;
    };
    let frame = Frame::new(0, s2c::GROUP_INFO, info).encode();
    for pid in groups.member_ids(group_id) {
        if let Some(p) = world.players.get(&pid) {
            let _ = p.tx.send(frame.clone());
        }
    }
}

fn group_toast(world: &World, groups: &GroupManager, group_id: u64, text: &str) {
    let frame = Frame::new(
        0,
        s2c::GROUP_TOAST,
        serde_json::json!({"text": text, "kind": "group"}),
    )
    .encode();
    for pid in groups.member_ids(group_id) {
        if let Some(p) = world.players.get(&pid) {
            let _ = p.tx.send(frame.clone());
        }
    }
}

fn send_invite_s2c(world: &World, target_id: &str, group_id: u64, from_id: &str, from_name: &str) {
    if let Some(p) = world.players.get(target_id) {
        let _ = p.tx.send(
            Frame::new(
                0,
                s2c::GROUP_INVITE_S2C,
                serde_json::json!({
                    "group_id": group_id,
                    "from_id": from_id,
                    "from_name": from_name,
                }),
            )
            .encode(),
        );
    }
}

/// §2: Leiter lädt einen (online) Spieler ein. S2C-Einladung an Ziel.
pub async fn handle_group_invite(ctx: &Ctx, conn_id: u64, data: &serde_json::Value) {
    let world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let mut groups = ctx.groups.lock().await;
    let Some(gid) = groups.group_of(&pid) else {
        return;
    };
    if !groups.is_leader(&pid) {
        return;
    }
    let target_id = get_str(data, "target_id");
    if target_id.is_empty()
        || target_id == pid
        || !world.players.contains_key(&target_id)
    {
        return;
    }
    if groups.invite(gid, &pid, &target_id, Instant::now()).is_ok() {
        let leader_name = world
            .players
            .get(&pid)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        send_invite_s2c(&world, &target_id, gid, &pid, &leader_name);
        let text = format!("{leader_name} hat {target_id} eingeladen");
        group_toast(&world, &groups, gid, &text);
    }
}

/// §2: Einladung annehmen/ablehnen. {group_id, accept}
pub async fn handle_group_invite_react(ctx: &Ctx, conn_id: u64, data: &serde_json::Value) {
    let world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let mut groups = ctx.groups.lock().await;
    let group_id = data.get("group_id").and_then(|v| v.as_u64()).unwrap_or(0);
    if group_id == 0 {
        return;
    }
    let accept = data.get("accept").and_then(|v| v.as_bool()).unwrap_or(false);
    if accept {
        if groups.accept_invite(group_id, &pid, Instant::now()).is_ok() {
            group_toast(&world, &groups, group_id, "Ein Spieler ist der Gruppe beigetreten");
            broadcast_group_info(&world, &groups, group_id);
        }
    } else if groups.reject_invite(group_id, &pid).is_ok() {
        group_toast(&world, &groups, group_id, "Eine Einladung wurde abgelehnt");
    }
}

/// §3: Mitglied schlägt Spieler vor (Leiter entscheidet). {target_id}
pub async fn handle_group_suggest(ctx: &Ctx, conn_id: u64, data: &serde_json::Value) {
    let world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
let mut groups = ctx.groups.lock().await;
    // Nur Leiter lädt ein. Ein Gruppenloser gründet die Gruppe mit der
    // ersten Einladung; ein (Nicht-Leiter-)Mitglied darf nicht einladen.
    let gid = if groups.is_leader(&pid) {
        match groups.group_of(&pid) {
            Some(gid) => gid,
            None => return,
        }
    } else {
        if groups.group_of(&pid).is_some() {
            return;
        }
        match groups.create_group(&pid, Instant::now()) {
            Ok(gid) => gid,
            Err(_) => return,
        }
    };
    let target_id = get_str(data, "target_id");
    if target_id.is_empty() || target_id == pid || !world.players.contains_key(&target_id) {
        return;
    }
    if groups.propose(gid, &pid, &target_id).is_ok() {
        let member_name = world
            .players
            .get(&pid)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let text = format!("{member_name} schlägt {target_id} vor");
        group_toast(&world, &groups, gid, &text);
    }
}

/// §3: Leiter entscheidet über Vorschlag. {target_id, accept}
pub async fn handle_group_suggest_decide(ctx: &Ctx, conn_id: u64, data: &serde_json::Value) {
    let world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let mut groups = ctx.groups.lock().await;
    let Some(gid) = groups.group_of(&pid) else {
        return;
    };
    if !groups.is_leader(&pid) {
        return;
    }
    let target_id = get_str(data, "target_id");
    let accept = data.get("accept").and_then(|v| v.as_bool()).unwrap_or(false);
    if target_id.is_empty() {
        return;
    }
    if accept {
        if !world.players.contains_key(&target_id) {
            groups.reject_proposal(gid, &pid, &target_id).ok();
            return;
        }
        if groups.approve_proposal(gid, &pid, &target_id, Instant::now()).is_ok() {
            let leader_name = world
                .players
                .get(&pid)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            send_invite_s2c(&world, &target_id, gid, &pid, &leader_name);
            let text = format!("{leader_name} schickt {target_id} eine Einladung");
            group_toast(&world, &groups, gid, &text);
        }
    } else if groups.reject_proposal(gid, &pid, &target_id).is_ok() {
        let text = format!("Der Vorschlag für {target_id} wurde abgelehnt");
        group_toast(&world, &groups, gid, &text);
    }
}

/// §2: Mitglied verlässt Gruppe (Leiter muss erst übertragen).
pub async fn handle_group_leave(ctx: &Ctx, conn_id: u64, _data: &serde_json::Value) {
    let world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let mut groups = ctx.groups.lock().await;
    let Some(gid) = groups.group_of(&pid) else {
        return;
    };
    if let Err(group::GroupError::LeaderCannotLeave) = groups.leave(gid, &pid, Instant::now()) {
        group_toast(&world, &groups, gid, "Leiter muss zuerst die Leitung übertragen");
        return;
    }
    let name = world
        .players
        .get(&pid)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let text = format!("{name} hat die Gruppe verlassen");
    group_toast(&world, &groups, gid, &text);
    broadcast_group_info(&world, &groups, gid);
}

/// §2: Leiter entfernt Mitglied. {target_id}
pub async fn handle_group_kick(ctx: &Ctx, conn_id: u64, data: &serde_json::Value) {
    let world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let mut groups = ctx.groups.lock().await;
    let Some(gid) = groups.group_of(&pid) else {
        return;
    };
    if !groups.is_leader(&pid) {
        return;
    }
    let target_id = get_str(data, "target_id");
    if target_id.is_empty()
        || groups.kick(gid, &pid, &target_id, Instant::now()).is_err()
    {
        return;
    }
    let text = format!("{target_id} wurde aus der Gruppe entfernt");
    group_toast(&world, &groups, gid, &text);
    broadcast_group_info(&world, &groups, gid);
}

/// §2: Leiter überträgt Leitung. {target_id}
pub async fn handle_group_transfer(ctx: &Ctx, conn_id: u64, data: &serde_json::Value) {
    let world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let mut groups = ctx.groups.lock().await;
    let Some(gid) = groups.group_of(&pid) else {
        return;
    };
    if !groups.is_leader(&pid) {
        return;
    }
    let target_id = get_str(data, "target_id");
    if target_id.is_empty() || groups.transfer_leader(gid, &pid, &target_id).is_err() {
        return;
    }
    let text = format!("{target_id} ist jetzt Gruppenleiter");
    group_toast(&world, &groups, gid, &text);
    broadcast_group_info(&world, &groups, gid);
}

/// PICKUP {loot_id} (Loot System V1): Boden-Loot-Drop aufnehmen.
/// Lock-Reihenfolge wie im Tick-Loop: erst World (shared), dann Groups.
pub async fn handle_pickup(ctx: &Ctx, conn_id: u64, data: &serde_json::Value) {
    let loot_id = get_str(data, "loot_id");
    if loot_id.is_empty() {
        return;
    }
    let mut world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let groups = ctx.groups.lock().await;
    let _ = crate::loot::attempt_pickup(
        &mut world,
        &pid,
        &groups,
        &loot_id,
        Instant::now(),
        &ctx.cfg.loot,
    );
}

/// SPEND_ATTRIBUTE {attribute} (Serverautorität V1): einen freien
/// Attributpunkt serverautoritativ ausgeben.
///
/// Der Client übermittelt NUR die Aktion (welches Attribut). Der Server:
/// 1. bestimmt den Charakter aus dem serverseitigen Zustand,
/// 2. prüft die verfügbaren Punkte (free_attr_points, RAM — nie Clientwerte),
/// 3. erhöht das Attribut intern um genau +1,
/// 4. reduziert den freien Punkt,
/// 5. sendet ATTRIBUTE_RESULT mit dem neuen Stand,
/// 6. persistiert über die bestehende Spool-Architektur (dirty-Flag).
/// Ein lokal manipuliertes `strength = 999` ist bedeutungslos.
pub async fn handle_spend_attribute(
    ctx: &Ctx,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &serde_json::Value,
) {
    let attribute = get_str(data, "attribute");
    let mut world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let Some(me) = world.players.get_mut(&pid) else {
        return;
    };
    match crate::security::spend_attribute_point(me, &attribute) {
        Ok(()) => {
            let me = &world.players[&pid];
            let a = &me.attributes;
            let _ = tx.send(
                Frame::new(
                    seq,
                    s2c::ATTRIBUTE_RESULT,
                    serde_json::json!({
                        "ok": true, "attribute": attribute.trim().to_lowercase(),
                        "strength": a.strength, "constitution": a.constitution,
                        "dexterity": a.dexterity, "intelligence": a.intelligence,
                        "wisdom": a.wisdom, "luck": a.luck, "endurance": a.endurance,
                        "free_attr_points": me.free_attr_points,
                    }),
                )
                .encode(),
            );
        }
        Err(e) => {
            let reason = e.reason();
            crate::security::log_reject(
                world.players.get(&pid),
                conn_id,
                &crate::security::RejectInfo {
                    reason: reason.into(),
                    msg_type: crate::protocol::c2s::SPEND_ATTRIBUTE,
                    detail: format!("attribute={attribute}"),
                },
                0,
            );
            let _ = tx.send(
                Frame::new(
                    seq,
                    s2c::ATTRIBUTE_RESULT,
                    serde_json::json!({"ok": false, "reason": reason}),
                )
                .encode(),
            );
        }
    }
}

/// AUCTION_BUY {auction_id} (Serverautorität V1, Anschluss-Stub): Der Client
/// übermittelt NUR die gewünschte Auktion. Der Server ermittelt selbst
/// Existenz, Aktivität, Preis, Käufer-Gold (RAM), Eigentum und Gültigkeit.
///
/// V1-Stand: Es existiert noch kein Auktionshaus-State im Realm (keine
/// auction-Tabellen/kein AH-Modul — siehe docs/Auktionshaus und Marktplatz).
/// Deshalb wird JEDER Kauf aktuell mit `auction_unavailable` abgelehnt
/// (fail-closed, kein Gold-/Item-Transfer), statt Clientwerten zu vertrauen.
/// Die reine Validierungslogik (Preis/Gold/Eigentum aus Serverwerten) liegt
/// in `crate::security::validate_auction_buy` und ist unit-getestet; sobald
/// das Auktionshaus-State existiert, wird der Lookup dort angeschlossen
/// (Aufrufstelle unten markiert) — kein Protokollumbau nötig.
pub async fn handle_auction_buy(
    ctx: &Ctx,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &serde_json::Value,
) {
    let auction_id = data
        .get("auction_id")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let world = ctx.shared.lock().await;
    let Some(pid) = world.by_conn.get(&conn_id).cloned() else {
        return;
    };
    let player = world.players.get(&pid);
    // V1: kein AH-State → Angebot serverseitig unbekannt → ablehnen.
    // Später: `let offer = ah_state.lookup(auction_id)` hier einsetzen und
    // `validate_auction_buy(&pid, player.idia, offer.as_ref())` prüfen.
    let err = crate::security::validate_auction_buy(
        &pid,
        player.map(|p| p.idia).unwrap_or(0),
        None,
    )
    .expect_err("ohne AH-State ist kein Kauf gültig");
    crate::security::log_reject(
        player,
        conn_id,
        &crate::security::RejectInfo {
            reason: err.reason().into(),
            msg_type: crate::protocol::c2s::AUCTION_BUY,
            detail: format!("auction_id={auction_id}"),
        },
        0,
    );
    let _ = tx.send(
        Frame::new(
            seq,
            s2c::CHAT,
            serde_json::json!({"from": "", "channel": "system",
                "text": "Auktionshaus ist derzeit nicht verfügbar."}),
        )
        .encode(),
    );
}

/// HEARTBEAT → SYNC-ACK (plus Ping-/Aktivitäts-Update).
pub async fn handle_heartbeat(
    shared: &Shared,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &serde_json::Value,
) {
    {
        let mut world = shared.lock().await;
        if let Some(pid) = world.by_conn.get(&conn_id).cloned() {
            if let Some(me) = world.players.get_mut(&pid) {
                me.last_activity = Instant::now();
                if let Some(ping) = data.get("ping_ms").and_then(|v| v.as_f64()) {
                    if ping >= 0.0 && ping < 10000.0 {
                        me.ping_ms = ping.round() as u32;
                    }
                }
            }
        }
    }
    let _ = tx.send(Frame::new(seq, s2c::SYNC, serde_json::json!({"ack_seq": seq})).encode());
}

#[cfg(test)]
mod tests {
    // Lokal-HTTP-Stub für die signierten Auth-API-Aufrufe: canned
    // JSON-Responses für /handoff/validate und /session/validate,
    // konfigurierbar pro Test (Signature wird im Test nicht geprüft).

    use super::*;
    use crate::auth_api::AuthApi;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct StubState {
        handoff_resp: String,
        handoff_status: u16,
        session_resp: String,
        session_status: u16,
        /// Canned-Antwort für `POST /parental/status` (Elternkontrolle am
        /// Login). Vorher lieferte der Stub für diesen Pfad `unknown path`,
        /// wodurch `parental::attach` immer mit `status_unavailable` scheiterte
        /// und der Zustand nach einem Attach nicht beobachtbar war.
        parental_resp: String,
        parental_status: u16,
    }

    async fn start_stub(handoff_resp: &str, session_resp: &str) -> AuthApi {
        start_stub_status(handoff_resp, 200, session_resp, 200).await
    }

    /// Stub mit zusätzlich konfigurierbarer Elternkontroll-Antwort.
    async fn start_stub_with_parental(
        handoff_resp: &str,
        session_resp: &str,
        parental_resp: &str,
    ) -> AuthApi {
        start_stub_full(handoff_resp, 200, session_resp, 200, parental_resp, 200).await
    }

    async fn start_stub_status(
        handoff_resp: &str,
        handoff_status: u16,
        session_resp: &str,
        session_status: u16,
    ) -> AuthApi {
        start_stub_full(
            handoff_resp,
            handoff_status,
            session_resp,
            session_status,
            PARENTAL_ON,
            200,
        )
        .await
    }

    /// Elternkontrolle aktiv, Chat gesperrt: `parental::chat_allowed` ist dann
    /// genau dann `false`, wenn ein Zustand für den Charakter existiert —
    /// damit ist das Anlegen bzw. Entfernen des Zustands beobachtbar.
    const PARENTAL_ON: &str = r#"{"enabled":true,"chat_allowed":false}"#;

    #[allow(clippy::too_many_arguments)]
    async fn start_stub_full(
        handoff_resp: &str,
        handoff_status: u16,
        session_resp: &str,
        session_status: u16,
        parental_resp: &str,
        parental_status: u16,
    ) -> AuthApi {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let state = StubState {
            handoff_resp: handoff_resp.to_string(),
            session_resp: session_resp.to_string(),
            session_status,
            handoff_status,
            parental_resp: parental_resp.to_string(),
            parental_status,
        };
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let (r, mut w) = tokio::io::split(sock);
                let h_resp = state.handoff_resp.clone();
                let h_status = state.handoff_status;
                let s_resp = state.session_resp.clone();
                let s_status = state.session_status;
                let p_resp = state.parental_resp.clone();
                let p_status = state.parental_status;
                tokio::spawn(async move {
                    let mut br = tokio::io::BufReader::new(r);
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let n = br.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if String::from_utf8_lossy(&buf).contains("\r\n\r\n") || buf.len() > 8192 {
                            break;
                        }
                    }
                    let reqhead = String::from_utf8_lossy(&buf);
                    let path = reqhead
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("");
                    let (resp, status) = if path == "/handoff/validate" {
                        (h_resp, h_status)
                    } else if path == "/session/validate" {
                        (s_resp, s_status)
                    } else if path == "/parental/status" {
                        (p_resp, p_status)
                    } else {
                        (r#"{"error":"unknown path"}"#.to_string(), 200)
                    };
                    let raw = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        resp.len(),
                        resp
                    );
                    let _ = w.write_all(raw.as_bytes()).await;
                });
            }
        });
        api(&format!("http://{addr}"))
    }

    fn api(url: &str) -> AuthApi {
        AuthApi::new(&crate::config::AuthApiConfig {
            url: url.to_string(),
            service_id: "realm-de1-service".into(),
            secret: "s3cret".into(),
        })
        .expect("authapi client")
    }

    const HANDOFF_OK: &str = r#"{"valid":true,"account_id":7,"realm_id":42}"#;
    const HANDOFF_DEAD: &str = r#"{"valid":false,"account_id":null,"realm_id":null}"#;
    const HANDOFF_RELM: &str = r#"{"valid":true,"account_id":7,"realm_id":9}"#;
    const SESSION_OK: &str = r#"{"valid":true,"account_id":7,"expires_at":"2030-01-01T00:00:00Z"}"#;
    const SESSION_DEAD: &str = r#"{"valid":false,"account_id":null,"expires_at":null}"#;
    const SESSION_OTHER: &str =
        r#"{"valid":true,"account_id":8,"expires_at":"2030-01-01T00:00:00Z"}"#;

    fn npc_cfg() -> NpcCfg {
        NpcCfg {
            social_aggro_radius: 15.0,
            no_link_ms: 5000,
            return_speed: 5.0,
            persist_interval_ms: 30000,
        }
    }

    /// Testkontext für die fail-closed-Prüfungen des HELLO-Einstiegs: Auth-API
    /// bewusst deaktiviert (`verify_entry` -> Konto 0), Datenbank als nicht
    /// erreichbarer Lazy-Pool. Ein gültiger Canonical-`char_id` erreicht damit
    /// nachweisbar Gate und World-Logik; jeder DB-Zugriff schlägt fehl.
    /// Gespiegelt aus `net::tests::test_ctx`.
    async fn test_ctx() -> std::sync::Arc<Ctx> {
        use std::collections::HashMap;
        let dir = std::env::temp_dir().join(format!(
            "andora-realm-hello-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let env = HashMap::<String, String>::new();
        let cfg = std::sync::Arc::new(crate::config::Config {
            realm_id: 1,
            ws_port: 3001,
            health_port: 3002,
            ws_bind_host: String::new(),
            health_bind_host: String::new(),
            tick_ms: 100,
            aofb_radius: 20.0,
            render_cap: 64,
            ollama_url: String::new(),
            auth_api: crate::config::AuthApiConfig {
                url: String::new(),
                service_id: String::new(),
                secret: String::new(),
            },
            realm_db: crate::config::DbConfig {
                host: "127.0.0.1".into(),
                port: 3306,
                user: "u".into(),
                password: "p".into(),
                database: "realm_state_test".into(),
            },
            migrations_dir: String::new(),
            allow_destructive: false,
            combat: crate::config::combat_config(&env),
            npc: crate::config::npc_config(&env),
            group: crate::config::group_config(&env),
            inventory: crate::config::inventory_config(&env),
            loot: crate::config::loot_config(&env),
            progression: crate::config::progression_config(&env),
            persist: crate::config::persist_config(&env),
            security: crate::config::security_config(&env),
        });
        let db = sqlx::mysql::MySqlPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(300))
            .connect_lazy("mysql://u:p@127.0.0.1:3306/realm_state_test")
            .unwrap();
        let auth = AuthApi::new(&cfg.auth_api).unwrap();
        test_ctx_with_auth(cfg, db, dir, auth).await
    }

    /// Testkontext mit frei wählbarer Auth-API. Notwendig für die Elternkontrolle:
    /// `parental::attach` legt den Zustand nur bei **aktivierter** Auth-API an
    /// (`account_id != 0`), sonst kehrt es sofort zurück.
    async fn test_ctx_with_auth(
        cfg: std::sync::Arc<crate::config::Config>,
        db: sqlx::MySqlPool,
        dir: std::path::PathBuf,
        auth: AuthApi,
    ) -> std::sync::Arc<Ctx> {
        let _ = &dir;
        let shared = crate::world::new_shared();
        let parental = crate::parental::new_shared(auth.clone());
        let groups = crate::group::new_shared_groups(cfg.group.clone());
        let persist = std::sync::Arc::new(
            crate::spool::PersistRuntime::new(&dir, &cfg.combat.weapon_skill_id).unwrap(),
        );
        std::sync::Arc::new(Ctx {
            cfg,
            db,
            auth,
            shared,
            parental,
            registry: crate::combat::ability::AbilityRegistry::new(),
            groups,
            quest: crate::quest::QuestService::new(),
            persist,
        })
    }

    fn p30_temp_dir(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d =
            std::env::temp_dir().join(format!("realmrs-p30-{tag}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn p30_snapshot(id: &str, rev: i64) -> crate::persist::PersistSnapshot {
        crate::persist::PersistSnapshot {
            player_id: id.to_string(),
            persist_revision: rev,
            captured_at_ms: 1_700_000_000_000 + rev,
            x: 1.0,
            y: 1.0,
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
            learned_abilities: vec![],
            // `P-18`: `None` = Altformat ohne Cooldown-Feld; der Cooldown-Bestand
            // bleibt bei diesen Fixtures unberührt.
            cooldowns: None,
            inventory: crate::inventory::InventoryState::default(),
            generation: 0,
            dirty: crate::persist::PersistDirty::default(),
        }
    }

    fn hello_frame(char_id: &str) -> serde_json::Value {
        serde_json::json!({
            "t": "hello",
            "char_id": char_id,
            "lang": "de",
            "session_id": "sess-1",
            "handoff_token": "h",
        })
    }

    /// RAM-Player **mit** Owner-Zuordnung. `insert_foreign_player` setzt nur
    /// `players`; für den Owner-Nachweis braucht es zusätzlich `by_conn`, weil
    /// `world::conn_of` ausschließlich `by_conn` auswertet.
    async fn insert_owned_player(ctx: &Ctx, conn_id: u64, player_id: &str) {
        insert_foreign_player(ctx).await;
        ctx.shared
            .lock()
            .await
            .by_conn
            .insert(conn_id, player_id.to_string());
    }

    /// Minimales `db::Character` für die Auflösungslogik.
    fn character_stub(id: &str) -> db::Character {
        db::Character {
            id: id.to_string(),
            name: "hero".into(),
            x: 0.0,
            y: 0.0,
            level: 1,
            exp: 0,
            free_attr_points: 0,
            rested_pool: 0,
            logout_at: None,
            idia: 0,
            persist_revision: 0,
            hp: 100,
            char_class: "Adventurer".into(),
            class: crate::class::ClassStatus::Adventurer,
            faction_transition: false,
            armor: 0,
            mana: 50,
            mana_max: 50,
            race: "Mensch".into(),
            strength: 10,
            dexterity: 10,
            intelligence: 10,
            constitution: 10,
            wisdom: 10,
            luck: 10,
            endurance: 10,
        }
    }

    /// RAM-Player mit fremder `account_id` unter der `char_id` "1" — damit
    /// wird der Ownership-Check für den Account 0 (deaktivierte Auth-API)
    /// unterscheidbar.
    async fn insert_foreign_player(ctx: &Ctx) {
        let (ptx, _prx) = mpsc::unbounded_channel();
        let mut world = ctx.shared.lock().await;
        world.players.insert(
            "1".to_string(),
            crate::world::Player {
                id: "1".into(),
                name: "hero".into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 100,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-other".into(),
                entities: Default::default(),
                last_activity: std::time::Instant::now(),
                tx: ptx,
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
                cooldowns: Default::default(),
                active_cast: None,
                learned_abilities: Default::default(),
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
        );
    }

    // ---- P-31: fail-closed Charakter-Lookup (kein Create im Login-Pfad) ----

    /// Die kanonische Dezimaldarstellung einer positiven DB-ID wird akzeptiert;
    /// der Wert ist der i32-Wert der Spalte `characters.id INT` (1..=2147483647).
    #[test]
    fn character_id_accepts_canonical_positive_db_id() {
        for (raw, expected) in [
            ("1", 1i32),
            ("7", 7),
            ("42", 42),
            ("1000", 1000),
            ("2147483646", 2147483646),
            ("2147483647", 2147483647),
        ] {
            assert_eq!(
                db::parse_character_id(raw),
                Ok(expected),
                "kanonische ID {raw} muss akzeptiert werden"
            );
            // Nach erfolgreicher Prüfung genau eine kanonische Darstellung:
            // der Gate-/World-/DB-Schlüssel ist `value.to_string()`.
            assert_eq!(
                expected.to_string(),
                raw,
                "Schlüssel muss der kanonischen Darstellung entsprechen"
            );
        }
    }

    /// 0, negative Werte, Nicht-Zahlen, Whitespace, Vorzeichen, nichtkanonische
    /// Aliase und Werte außerhalb des DB-ID-Typs werden abgelehnt. Aliase
    /// derselben ID erzeugen damit keinen zweiten Gate-/Lookup-Schlüssel.
    #[test]
    fn character_id_rejects_non_canonical_zero_negative_and_overflow() {
        // 0 und negative Werte (Positivitätsgrenze).
        assert_eq!(db::parse_character_id("0"), Err("not-positive"));
        // Nicht-Zahlen, Whitespace, Vorzeichen, Dezimalstellen.
        for raw in [
            "abc", "1a", "a1", "1.0", "0x1", "1e3", "١", "1_0", "NaN", "inf", "--1", "1\n", "1\t",
        ] {
            assert_eq!(
                db::parse_character_id(raw),
                Err("not-canonical"),
                "{raw:?} muss nichtkanonisch abgelehnt werden"
            );
        }
        // Whitespace und Vorzeichen (auch mit führendem Null-Alias).
        for raw in [
            "", " ", " 1", "1 ", " 1 ", "\t1", "+1", "-1", "+0", "-0", " -1", "+ 1", "++1", "--1",
        ] {
            let reason = db::parse_character_id(raw);
            assert!(
                matches!(
                    reason,
                    Err("not-canonical") | Err("empty") | Err("not-positive")
                ),
                "{raw:?} muss abgelehnt werden, war {reason:?}"
            );
        }
        // Nichtkanonische Aliase derselben ID.
        for raw in ["01", "007", "000", "0001", "041"] {
            assert_eq!(
                db::parse_character_id(raw),
                Err("not-canonical"),
                "Alias {raw:?} muss abgelehnt werden"
            );
        }
        // Überlauf und Werte außerhalb des Spaltentyps (MariaDB INT, signed).
        for raw in [
            "2147483648",
            "2147483649",
            "4294967295",
            "4294967296",
            "9223372036854775807",
            "18446744073709551615",
            "99999999999999999999",
        ] {
            assert_eq!(
                db::parse_character_id(raw),
                Err("out-of-range"),
                "{raw} liegt außerhalb des DB-ID-Typs"
            );
        }
        // Sehr lange Ziffernfolgen: kein Panik-Verhalten.
        let long = "1".repeat(512);
        assert_eq!(db::parse_character_id(&long), Err("out-of-range"));
        let zeros = format!("0{}", "0".repeat(512));
        assert_eq!(db::parse_character_id(&zeros), Err("not-canonical"));
    }

    /// Der Ablehnungsgrund enthält niemals die rohe, untrusted Eingabe.
    #[test]
    fn character_id_rejection_reason_does_not_echo_input() {
        for raw in [" 1\n", "abc", "+1", "2147483648", "0", ""] {
            let reason = db::parse_character_id(raw).unwrap_err();
            assert!(
                !reason.contains(raw.trim()) || raw.is_empty(),
                "Grund {reason:?} darf die Eingabe {raw:?} nicht wiedergeben"
            );
            assert!(
                reason.len() <= 16,
                "Grund muss ein statisches Kurzwort sein"
            );
        }
    }

    /// NotFound und DB-Fehler werden intern unterschieden, extern aber
    /// identisch abgelehnt: kein Ownership-Leak, kein Fallback-Create.
    #[test]
    fn character_lookup_maps_not_found_and_db_error_to_same_external_rejection() {
        // Treffer wird durchgereicht.
        let found = resolve_character_lookup(Ok(Some(character_stub("1")))).unwrap();
        assert_eq!(found.id, "1");
        // Kein Treffer (fehlend ODER fremder Account: die Abfrage filtert nach
        // id UND account_id) -> generische Ablehnung.
        let not_found = resolve_character_lookup(Ok(None)).unwrap_err();
        // DB-Fehler -> dieselbe generische Ablehnung.
        let db_error =
            resolve_character_lookup(Err("Charakter laden: pool timeout".into())).unwrap_err();
        assert_eq!(not_found, db_error);
        assert_eq!(not_found, CHARACTER_UNAVAILABLE);
        // Kein Zeichen des Charakters und kein Create-Hinweis im Fehler.
        assert!(!not_found.contains("create"), "kein Create-Fallback");
        assert!(
            !not_found.contains("char_id"),
            "keine ID-Information im Fehler"
        );
    }

    /// Ungültige, nichtkanonische und überlaufende `char_id` werden abgelehnt,
    /// BEVOR Gate, World und Datenbank berührt werden: mit einem RAM-Player
    /// unter "1" (fremder Account) ist der Ownership-Fehler nur erreichbar,
    /// wenn die ID die Vorprüfung passiert.
    #[tokio::test]
    async fn hello_rejects_non_canonical_char_id_before_gate_world_and_db() {
        for raw in [
            "",
            "0",
            "-1",
            "+1",
            "01",
            "007",
            " 1",
            "1 ",
            "1.0",
            "abc",
            "1a",
            "2147483648",
            "99999999999",
            "1\n",
            "  1",
        ] {
            let ctx = test_ctx().await;
            insert_foreign_player(&ctx).await;
            let (tx, mut _rx) = mpsc::unbounded_channel();
            let err = handle_hello(&ctx, &tx, 42, 1, &hello_frame(raw))
                .await
                .unwrap_err();
            assert_eq!(
                err, CHARACTER_UNAVAILABLE,
                "char_id {raw:?} muss generisch abgelehnt werden"
            );
            // Kein Takeover/Ownership-Fehler -> World-Logik wurde nicht erreicht.
            assert_ne!(err, "character is owned by another account");
            // Kein Player-Zustand und keine Verbindungszuordnung entstanden.
            let world = ctx.shared.lock().await;
            assert_eq!(world.players.len(), 1, "nur der vorbestehende Player");
            assert!(world.by_conn.is_empty(), "keine by_conn-Zuordnung");
            assert_eq!(
                world.players.get("1").unwrap().account_id,
                7,
                "Ownership des vorbestehenden Players unverändert"
            );
        }
    }

    /// Eine gültige kanonische `char_id` passiert die Vorprüfung und erreicht
    /// nachweisbar die Ownership-Prüfung gegen den RAM-World-Zustand. Der
    /// Alias "01" erreicht sie nicht — derselbe numerische Wert erzeugt also
    /// genau einen Gate-/Lookup-Schlüssel.
    #[tokio::test]
    async fn hello_with_canonical_char_id_reaches_ownership_check() {
        let ctx = test_ctx().await;
        ctx.persist.set_status(crate::spool::PersistStatus::Ready);
        insert_foreign_player(&ctx).await;
        let (tx, _rx) = mpsc::unbounded_channel();
        // Kanonisch: erreicht die World-/Takeover-Logik.
        assert_eq!(
            handle_hello(&ctx, &tx, 42, 1, &hello_frame("1"))
                .await
                .unwrap_err(),
            "character is owned by another account"
        );
        // Alias: wird vorher fail-closed abgelehnt.
        assert_eq!(
            handle_hello(&ctx, &tx, 42, 1, &hello_frame("01"))
                .await
                .unwrap_err(),
            CHARACTER_UNAVAILABLE
        );
        // Der vorbestehende Player bleibt unangetastet (kein RAM-Überschreiben,
        // keine Entmachtung eines fremden Players).
        let world = ctx.shared.lock().await;
        assert_eq!(world.players.len(), 1);
        assert_eq!(world.players.get("1").unwrap().account_id, 7);
        assert_eq!(world.by_conn.len(), 0);
    }

    // ===== P-30: charakterbezogene Quarantäne-Sperre im Eintrittspfad =====

    /// Die HELLO-Entscheidung wird ausschließlich aus der serverseitigen
    /// Bestandsbewertung abgeleitet und in stabile Gründe übersetzt. Ein
    /// nicht kanonischer Einzelfall sperrt niemanden, ein Scanfehler sperrt
    /// ausschließlich den angefragten Charakter.
    #[test]
    fn p30_availability_maps_to_stable_server_side_reasons() {
        use crate::spool::CharacterAvailability as A;
        let dir = p30_temp_dir("p30h1");
        let s = std::sync::Arc::new(crate::spool::PersistRuntime::new(&dir, "ws").unwrap());
        // Leerer Bestand.
        assert_eq!(s.evaluate_character_availability("42", 1), A::Available);
        // Kanonischer, ungelöster Fall.
        std::fs::write(
            dir.join("quarantine")
                .join("open")
                .join("1700000000000-42-r17.json--malformed.json"),
            "{x}",
        )
        .unwrap();
        assert_eq!(
            s.evaluate_character_availability("42", 16),
            A::SaveRecoveryPending
        );
        // Anderer Charakter bleibt spielbar.
        assert_eq!(s.evaluate_character_availability("7", 16), A::Available);
        // DB-bestätigt abgelöst.
        assert_eq!(s.evaluate_character_availability("42", 17), A::Available);
        // Scanfehler ⇒ CheckFailed, keine Zuordnung.
        std::fs::remove_dir_all(dir.join("quarantine").join("open")).unwrap();
        assert_eq!(s.evaluate_character_availability("42", 1), A::CheckFailed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Die Ablehnungsgründe sind exakt und tragen keine Leerzeichen, keine ID,
    /// keine Revision, keinen Pfad und keinen Rohfehler.
    #[test]
    fn p30_rejection_reasons_are_exact_and_data_free() {
        assert_eq!(crate::spool::SAVE_RECOVERY_PENDING, "save_recovery_pending");
        assert_eq!(
            crate::spool::SAVE_RECOVERY_CHECK_FAILED,
            "save_recovery_check_failed"
        );
        for grund in [
            crate::spool::SAVE_RECOVERY_PENDING,
            crate::spool::SAVE_RECOVERY_CHECK_FAILED,
        ] {
            assert!(!grund.contains(' '), "keine Leerzeichen: {grund}");
            assert!(!grund.contains('/'), "kein Pfad: {grund}");
            assert!(!grund.contains(".json"), "kein Dateiname: {grund}");
            assert!(
                !grund.chars().any(|c| c.is_ascii_digit()),
                "keine ID: {grund}"
            );
        }
    }

    /// Ein einzelner nicht kanonischer Einzelfall erzeugt **keine**
    /// Charaktersperre und **keine** Konto-/Realm-Sperre.
    #[test]
    fn p30_single_non_canonical_file_blocks_nobody() {
        use crate::spool::CharacterAvailability as A;
        let dir = p30_temp_dir("p30h2");
        let s = std::sync::Arc::new(crate::spool::PersistRuntime::new(&dir, "ws").unwrap());
        let open = dir.join("quarantine").join("open");
        std::fs::write(open.join("handgelegt.json"), "{y}").unwrap();
        for pid in ["7", "8", "9"] {
            assert_eq!(
                s.evaluate_character_availability(pid, 1),
                A::Available,
                "{pid} bleibt spielbar"
            );
        }
        assert_eq!(s.unattributed_quarantine_count(), Some(1));
        // Der Einzelfall bleibt zur Analyse erhalten.
        assert!(open.join("handgelegt.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ein Scanfehler sperrt weder Konto noch Realm: nur der angefragte
    /// Charakter wird fail-closed abgelehnt, der Realm-Zustand bleibt Ready.
    #[test]
    fn p30_scan_error_is_character_scoped_and_never_global() {
        use crate::spool::CharacterAvailability as A;
        let dir = p30_temp_dir("p30h3");
        let s = std::sync::Arc::new(crate::spool::PersistRuntime::new(&dir, "ws").unwrap());
        s.set_status(crate::spool::PersistStatus::Ready);
        std::fs::remove_dir_all(dir.join("quarantine").join("open")).unwrap();
        assert_eq!(s.evaluate_character_availability("8", 1), A::CheckFailed);
        // Kein globaler Sperrzustand: Runtime bleibt Ready (Login möglich).
        assert_eq!(s.status(), crate::spool::PersistStatus::Ready);
        assert_eq!(s.unattributed_quarantine_count(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// HELLO hält das per-character Gate über die Bestandsprüfung hinweg:
    /// während das Gate gehalten wird, sieht `pending_revision` einen
    /// wartenden Batch und der Charakter wäre fail-closed. Kein Sleep: der
    /// Nachweis läuft über eine kontrollierte Reihenfolge.
    #[tokio::test]
    async fn p30_hello_holds_gate_while_pending_spool_batch_is_visible() {
        let dir = p30_temp_dir("p30h4");
        let s = std::sync::Arc::new(crate::spool::PersistRuntime::new(&dir, "ws").unwrap());
        let snap = p30_snapshot("42", 18);
        s.spool().write_batch(&snap).unwrap();
        let guard = s.player_gate("42").await.lock_owned().await;
        // Unter dem Gate: der wartende Batch ist sichtbar ⇒ fail-closed.
        assert_eq!(s.pending_revision("42"), Ok(Some(18)));
        assert!(crate::world::db_row_is_stale(17, s.pending_revision("42")));
        // Ein zweiter Versuch am selben Gate wartet (nicht reentrant).
        let s2 = s.clone();
        let waiter = tokio::spawn(async move {
            let _held = s2.player_gate("42").await.lock_owned().await;
            "durch"
        });
        // Deterministisch: der Wartende kann erst nach dem Freigeben laufen.
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished(), "Gate ist nicht reentrant");
        drop(guard);
        assert_eq!(waiter.await.unwrap(), "durch");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ein anderer Charakter desselben Kontos ist **nicht** vom Gate oder vom
    /// Quarantänefall des gesperrten Charakters betroffen.
    #[tokio::test]
    async fn p30_other_character_of_same_account_is_playable() {
        let dir = p30_temp_dir("p30h5");
        let s = std::sync::Arc::new(crate::spool::PersistRuntime::new(&dir, "ws").unwrap());
        std::fs::write(
            dir.join("quarantine")
                .join("open")
                .join("1700000000000-8-r30.json--malformed.json"),
            "{x}",
        )
        .unwrap();
        use crate::spool::CharacterAvailability as A;
        let a = s.player_gate("8").await;
        let b = s.player_gate("9").await;
        assert!(
            !a.same_gate(&b),
            "verschiedene Charaktere, verschiedene Gates"
        );
        let _held = a.lock_owned().await;
        assert_eq!(
            s.evaluate_character_availability("8", 1),
            A::SaveRecoveryPending
        );
        assert_eq!(s.evaluate_character_availability("9", 1), A::Available);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Erreicht die kanonische ID Gate, World- und DB-Schritt, und ist der
    /// Charakterzugriff nicht verfügbar, endet der Einstieg fail-closed: kein
    /// Player im World-State, keine Verbindungszuordnung, kein Progressionseintrag.
    ///
    /// Der Test deckt den **Err-Zweig** von `resolve_character_lookup` ab (der
    /// Test-Pool erreicht kein MariaDB: `pool timed out while waiting for an
    /// open connection`). Der **Ok(None)-Zweig** — fehlender oder fremder
    /// Datensatz — ist in `character_lookup_maps_not_found_and_db_error_to_
    /// same_external_rejection` auf der Entscheidungslogik abgedeckt. Die
    /// MariaDB-Integration (Treffer mit passendem `account_id`, NotFound ohne
    /// INSERT) bleibt eine ausdrücklich benannte Testlücke.
    // ---- P-32: atomare Offline-Abrechnung vor dem Owner-Commit ----

    /// N3 — die outcome-abhängige RAM-Anwendung ist **rein** und damit ohne
    /// MariaDB vollständig prüfbar. `true` als zweiter Wert bedeutet: die DB
    /// hält nach der Abrechnung einen anderen Wert als der RAM-Player und die
    /// Progression wird dirty markiert, damit der nächste Snapshot den
    /// RAM-Stand in die DB bringt (Reconciliation, **kein** `logout_at`-Retry).
    #[test]
    fn login_settlement_is_outcome_dependent() {
        use crate::world::CommitOutcome as O;
        let takeover = O::Takeover { old_conn_id: 7 };

        // (1) Registered: exakt `credited`, kein Delta, keine Reconciliation —
        // auch wenn der Kandidatenwert abweicht (darf nicht zurückgeschrieben
        // werden).
        assert_eq!(
            apply_login_settlement(O::Registered, 90, 0, 90),
            (90, false)
        );
        assert_eq!(
            apply_login_settlement(O::Registered, 50, -100, 100),
            (100, false)
        );
        assert_eq!(
            apply_login_settlement(O::Registered, 7, 40, 47),
            (47, false)
        );

        // (2)/(3) Adopted/Takeover, positives Delta: additiv, dirty nur bei
        // verbleibender Abweichung zum DB-Wert.
        assert_eq!(apply_login_settlement(O::Adopted, 50, 40, 90), (90, false));
        assert_eq!(apply_login_settlement(takeover, 50, 40, 90), (90, false));
        assert_eq!(apply_login_settlement(O::Adopted, 60, 40, 90), (100, true));
        assert_eq!(apply_login_settlement(takeover, 60, 40, 90), (100, true));

        // (4)/(5) Adopted/Takeover, negatives Delta (Kappung aus dem
        // DB-Kandidaten): RAM bleibt autoritativ, Reconciliation markiert.
        assert_eq!(
            apply_login_settlement(O::Adopted, 50, -100, 100),
            (50, true)
        );
        assert_eq!(apply_login_settlement(takeover, 50, -100, 100), (50, true));

        // (6) RAM bereits gleich dem DB-Wert: keine unnötige Mutation.
        assert_eq!(apply_login_settlement(O::Adopted, 90, 0, 90), (90, false));
        assert_eq!(apply_login_settlement(takeover, 90, 0, 90), (90, false));

        // (7) Refreshed: keine zweite Gutschrift, keine Kandidatenkappung;
        // eine bestehende Divergenz wird nicht dauerhaft stehen gelassen.
        assert_eq!(apply_login_settlement(O::Refreshed, 30, 40, 90), (30, true));
        assert_eq!(apply_login_settlement(O::Refreshed, 90, 0, 90), (90, false));
    }

    /// N3 (6) am echten Player: `reconcile == false` bedeutet keine
    /// `mark_dirty`, also keine Generationserhöhung und kein Dirty-Bit.
    #[tokio::test]
    async fn login_settlement_marks_dirty_only_on_divergence() {
        let (ptx, _prx) = mpsc::unbounded_channel();
        let mut player = crate::world::Player {
            id: "1".into(),
            name: "hero".into(),
            x: 0.0,
            y: 0.0,
            face: 0.0,
            ping_ms: 0,
            zone_id: 0,
            hp: 100,
            max_hp: 100,
            lang: "de".into(),
            account_id: 7,
            session_id: "sess-1".into(),
            entities: Default::default(),
            last_activity: std::time::Instant::now(),
            tx: ptx,
            char_class: "Adventurer".into(),
            class: crate::class::ClassStatus::Adventurer,
            faction_transition: false,
            level: 1,
            exp: 0,
            free_attr_points: 0,
            rested_pool: 90,
            idia: 0,
            armor: 0,
            weapon_skill: 1,
            combat: None,
            last_strike: None,
            mana: 50,
            max_mana: 50,
            effects: Vec::new(),
            cooldowns: Default::default(),
            active_cast: None,
            learned_abilities: Default::default(),
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
        };

        // RAM (90) == DB (90): keine Reconciliation, keine Dirty-Markierung.
        let (pool, reconcile) = apply_login_settlement(
            crate::world::CommitOutcome::Adopted,
            player.rested_pool,
            0,
            90,
        );
        assert_eq!((pool, reconcile), (90, false));
        if reconcile {
            player.mark_dirty(crate::persist::PersistComponent::Progression);
        }
        assert_eq!(player.persist_generation, 0, "keine Generationserhöhung");
        assert!(!player
            .dirty
            .is_dirty(crate::persist::PersistComponent::Progression));

        // RAM (50) != DB (100) nach negativer Kandidatenkorrektur: Reconciliation.
        player.rested_pool = 50;
        let (pool, reconcile) = apply_login_settlement(
            crate::world::CommitOutcome::Adopted,
            player.rested_pool,
            -100,
            100,
        );
        assert_eq!((pool, reconcile), (50, true));
        player.rested_pool = pool;
        player.mark_dirty(crate::persist::PersistComponent::Progression);
        assert_eq!(player.rested_pool, 50, "RAM bleibt autoritativ");
        assert_eq!(player.persist_generation, 1);
        assert!(player
            .dirty
            .is_dirty(crate::persist::PersistComponent::Progression));
    }

    /// N2a/N2b — das bedingte Parental-Cleanup läuft **nur** ohne Owner.
    #[tokio::test]
    async fn parental_cleanup_runs_only_without_owner() {
        let ctx = test_ctx().await;
        // N2a: kein Owner im World-State -> Cleanup wird ausgeführt.
        assert!(detach_if_unowned(&ctx.parental, &ctx.shared, "1").await);
        // N2b: Owner vorhanden -> KEIN Cleanup (Takeover-Fall: der Elternzustand
        // gehört der weiterhin verbundenen Sitzung).
        insert_owned_player(&ctx, 77, "1").await;
        assert!(!detach_if_unowned(&ctx.parental, &ctx.shared, "1").await);
    }

    /// N2c — mit aktivem `parental::attach`: der Zustand existiert danach nur
    /// ohne Owner nicht mehr. Beobachtung über den bestehenden `pub`-Accessor
    /// `parental::chat_allowed` (Elternkontrolle aktiv + Chat gesperrt ⇒
    /// `false`, solange ein Zustand existiert).
    #[tokio::test]
    async fn parental_state_removed_only_without_owner_after_attach() {
        let auth = start_stub_with_parental(HANDOFF_OK, SESSION_OK, PARENTAL_ON).await;
        let dir = std::env::temp_dir().join(format!(
            "andora-realm-parental-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut env = std::collections::HashMap::<String, String>::new();
        let cfg = std::sync::Arc::new(crate::config::Config {
            realm_id: 1,
            ws_port: 3001,
            health_port: 3002,
            ws_bind_host: String::new(),
            health_bind_host: String::new(),
            tick_ms: 100,
            aofb_radius: 20.0,
            render_cap: 64,
            ollama_url: String::new(),
            auth_api: crate::config::AuthApiConfig {
                url: String::new(),
                service_id: String::new(),
                secret: String::new(),
            },
            realm_db: crate::config::DbConfig {
                host: "127.0.0.1".into(),
                port: 3306,
                user: "u".into(),
                password: "p".into(),
                database: "realm_state_test".into(),
            },
            migrations_dir: String::new(),
            allow_destructive: false,
            combat: crate::config::combat_config(&env),
            npc: crate::config::npc_config(&env),
            group: crate::config::group_config(&env),
            inventory: crate::config::inventory_config(&env),
            loot: crate::config::loot_config(&env),
            progression: crate::config::progression_config(&env),
            persist: crate::config::persist_config(&env),
            security: crate::config::security_config(&env),
        });
        let db = sqlx::mysql::MySqlPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(300))
            .connect_lazy("mysql://u:p@127.0.0.1:3306/realm_state_test")
            .unwrap();
        let ctx = test_ctx_with_auth(cfg, db, dir, auth).await;
        let _ = &mut env;

        let (tx, _rx) = mpsc::unbounded_channel();
        // Echter Attach: legt den Elternzustand an (Auth aktiv, Konto 7).
        crate::parental::attach(&ctx.parental, &tx, "1", 7, "sess-1")
            .await
            .expect("attach mit Stub-Antwort muss erfolgreich sein");
        assert!(
            !crate::parental::chat_allowed(&ctx.parental, "1").await,
            "Zustand nach attach vorhanden"
        );

        // Ohne Owner: Cleanup entfernt den Zustand.
        assert!(detach_if_unowned(&ctx.parental, &ctx.shared, "1").await);
        assert!(
            crate::parental::chat_allowed(&ctx.parental, "1").await,
            "Zustand nach Cleanup entfernt"
        );

        // Zweiter Attach, diesmal mit vorhandenem Owner: Zustand bleibt.
        crate::parental::attach(&ctx.parental, &tx, "1", 7, "sess-1")
            .await
            .expect("attach");
        insert_owned_player(&ctx, 77, "1").await;
        assert!(!detach_if_unowned(&ctx.parental, &ctx.shared, "1").await);
        assert!(
            !crate::parental::chat_allowed(&ctx.parental, "1").await,
            "Zustand bleibt bei vorhandenem Owner erhalten"
        );
    }

    /// N1-Struktur: der Settlement-Aufruf und die Fehler-Rückkehr liegen VOR
    /// dem Commit-Block. Ohne Datenbank ist der Settlement-Fehlerpfad selbst
    /// nicht auslösbar (`load_character` ist der erste DB-Zugriff und scheitert
    /// am nicht erreichbaren Pool) — das ist eine MariaDB-Testlücke und wird
    /// hier nur als Reihenfolge-Invariante belegt.
    #[test]
    fn settlement_precedes_commit_in_source_order() {
        let src = include_str!("handlers.rs");
        let settle = src
            .find("HELLO settlement {char_id}")
            .expect("Settlement-Fehlerbehandlung vorhanden");
        let commit = src
            .find("crate::world::commit_login(")
            .expect("Commit-Aufruf vorhanden");
        assert!(
            settle < commit,
            "Settlement muss vor dem Commit liegen (Zeilenlage)"
        );
    }

    #[tokio::test]
    async fn hello_fails_closed_when_character_lookup_is_unavailable() {
        let ctx = test_ctx().await;
        ctx.persist.set_status(crate::spool::PersistStatus::Ready);
        let (tx, _rx) = mpsc::unbounded_channel();
        // Auth deaktiviert -> Konto 0; kein RAM-Player für "1" -> kein
        // Ownership-Konflikt; der DB-Lookup schlägt fehl (kein MariaDB im Test).
        assert_eq!(
            handle_hello(&ctx, &tx, 42, 1, &hello_frame("1"))
                .await
                .unwrap_err(),
            CHARACTER_UNAVAILABLE
        );
        let world = ctx.shared.lock().await;
        assert!(world.players.is_empty(), "kein Player im World-State");
        assert!(world.by_conn.is_empty(), "kein Realm-Commit");
    }

    #[tokio::test]
    async fn entry_requires_handoff_and_matching_session() {
        let auth = start_stub(HANDOFF_OK, SESSION_OK).await;
        assert_eq!(verify_entry(&auth, 42, "h", "s").await, Ok(7));
    }

    #[tokio::test]
    async fn entry_fails_without_handoff_or_session() {
        let auth = start_stub(HANDOFF_OK, SESSION_OK).await;
        assert_eq!(
            verify_entry(&auth, 42, "", "s").await,
            Err("handoff required".into())
        );
        assert_eq!(
            verify_entry(&auth, 42, "h", "").await,
            Err("session required".into())
        );
    }

    #[tokio::test]
    async fn entry_fails_on_dead_tokens_or_mismatched_account() {
        let a = start_stub(HANDOFF_DEAD, SESSION_OK).await;
        assert_eq!(
            verify_entry(&a, 42, "h", "s").await,
            Err("handoff_invalid".into())
        );
        let a = start_stub(HANDOFF_RELM, SESSION_OK).await;
        assert_eq!(
            verify_entry(&a, 42, "h", "s").await,
            Err("handoff_invalid".into())
        );
        let a = start_stub(HANDOFF_OK, SESSION_DEAD).await;
        assert_eq!(
            verify_entry(&a, 42, "h", "s").await,
            Err("session_invalid".into())
        );
        let a = start_stub(HANDOFF_OK, SESSION_OTHER).await;
        // Session gültig, gehört aber zu einem anderen Account.
        assert_eq!(
            verify_entry(&a, 42, "h", "s").await,
            Err("session_invalid".into())
        );
    }

    #[tokio::test]
    async fn entry_fails_closed_on_authapi_transport_errors() {
        // Kein Listener: Connect-Refusal auf 127.0.0.1 (reservierter Port
        // 1 ist garantiert unverbindbar).
        let auth = api("http://127.0.0.1:1");
        assert_eq!(
            verify_entry(&auth, 42, "h", "s").await,
            Err("handoff_unavailable".into())
        );
        // Auth-API erreichbar, aber Session-Call mit HTTP-Fehler
        // (z. B. 503): Einstieg bleibt verwehrt.
        let auth = start_stub_status(HANDOFF_OK, 200, SESSION_DEAD, 503).await;
        assert_eq!(
            verify_entry(&auth, 42, "h", "s").await,
            Err("session_unavailable".into())
        );
    }

    #[test]
    fn entry_noop_without_authapi() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let auth = api("");
            assert_eq!(verify_entry(&auth, 42, "h", "s").await, Ok(0));
        });
    }

    #[tokio::test]
    async fn attack_start_stop_and_validation() {
        let shared = crate::world::new_shared();
        let cfg = CombatCfg {
            weapon_skill_id: "schwerter".into(),
            weapon_damage: 10,
            weapon_duration_ms: 2000,
            weapon_range: 2.0,
            hit_miss_permille: 100,
            hit_dodge_permille: 100,
            hit_parry_permille: 50,
            hit_block_permille: 100,
            hit_crit_permille: 100,
            hit_crit_mult_percent: 150,
            hit_block_reduce_percent: 50,
            armor_pct_per_point: 2,
            armor_cap_tank: 50,
            armor_cap_mage: 20,
            armor_cap_default: 30,
            skill_hit_bonus_permille: 5,
        };
        // Welt mit zwei Spielern aufbauen (a bei 0,0; b bei 1,0 → in Reichweite).
        {
            let mut w = shared.lock().await;
            let (ta, _) = mpsc::unbounded_channel();
            let (tb, _) = mpsc::unbounded_channel();
            w.players.insert(
                "a".into(),
                crate::world::Player {
                    id: "a".into(),
                    name: "a".into(),
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
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: ta,
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
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
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
            );
            w.players.insert(
                "b".into(),
                crate::world::Player {
                    id: "b".into(),
                    name: "b".into(),
                    x: 1.0,
                    y: 0.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 100,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 0,
                    session_id: String::new(),
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: tb,
                    char_class: "Mage".into(),
                    class: crate::class::ClassStatus::Mage,
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
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
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
            );
            w.by_conn.insert(7, "a".into());
        }

        // Ungültig: Ziel existiert nicht → kein Kampfzustand.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "ghost"}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        assert!(shared.lock().await.players["a"].combat.is_none());

        // Ungültig: sich selbst anvisieren → kein Kampfzustand.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "a"}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        assert!(shared.lock().await.players["a"].combat.is_none());

        // Gültig: b in Reichweite → bewaffnet.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "b"}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        {
            let w = shared.lock().await;
            let c = w.players["a"].combat.as_ref().expect("bewaffnet");
            assert_eq!(c.target_id, "b");
        }

        // Stop → Kampf beendet.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"stop": true}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        assert!(shared.lock().await.players["a"].combat.is_none());
    }

    #[tokio::test]
    async fn attack_out_of_range_is_rejected() {
        let shared = crate::world::new_shared();
        let cfg = crate::config::combat_config(&Default::default()); // Reichweite 2 m
        {
            let mut w = shared.lock().await;
            let (ta, _) = mpsc::unbounded_channel();
            let (tb, _) = mpsc::unbounded_channel();
            w.players.insert(
                "a".into(),
                crate::world::Player {
                    id: "a".into(),
                    name: "a".into(),
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
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: ta,
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
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
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
            );
            w.players.insert(
                "b".into(),
                crate::world::Player {
                    id: "b".into(),
                    name: "b".into(),
                    x: 50.0,
                    y: 0.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 100,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 0,
                    session_id: String::new(),
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: tb,
                    char_class: "Mage".into(),
                    class: crate::class::ClassStatus::Mage,
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
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
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
            );
            w.by_conn.insert(7, "a".into());
        }
        // b steht 50 m entfernt → kein gültiger Start.
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "b"}),
            &cfg,
            &npc_cfg(),
        )
        .await;
        assert!(shared.lock().await.players["a"].combat.is_none());
    }

    #[tokio::test]
    async fn movement_marks_position_dirty_but_stop_does_not() {
        let shared = crate::world::new_shared();
        {
            let mut w = shared.lock().await;
            let (ta, _) = mpsc::unbounded_channel();
            w.players.insert(
                "a".into(),
                Player {
                    id: "a".into(),
                    name: "a".into(),
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
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: ta,
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
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
                    sitting: false,
                    attributes: Default::default(),
                    max_hp_base: 100,
                    max_mana_base: 50,
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
            );
            w.by_conn.insert(7, "a".into());
        }
        // Gültige Bewegung → serverseitig neue Position + Position-dirty.
        handle_move(&shared, 7, &serde_json::json!({"dir": [1.0, 0.0]}), 1000).await;
        {
            let w = shared.lock().await;
            let p = &w.players["a"];
            assert!(p.x > 0.0, "Bewegung ändert die Position");
            assert!(p.dirty.is_dirty(crate::persist::PersistComponent::Position));
            assert_eq!(p.persist_generation, 1);
        }
        // Stopp (Richtung 0) → keine RAM-Mutation, kein neuer Dirty-Write.
        {
            let mut w = shared.lock().await;
            w.players
                .get_mut("a")
                .unwrap()
                .dirty
                .clear(crate::persist::PersistComponent::Position);
        }
        handle_move(&shared, 7, &serde_json::json!({"dir": [0.0, 0.0]}), 1000).await;
        {
            let w = shared.lock().await;
            let p = &w.players["a"];
            assert!(!p.dirty.is_dirty(crate::persist::PersistComponent::Position));
            assert_eq!(p.persist_generation, 1);
        }
    }

    // AUTH-03: Nach der Entmachtung erreicht kein Frame der alten Verbindung
    // die Spiellogik mehr — der Player-Zustand bleibt unverändert.
    #[tokio::test]
    async fn frames_of_displaced_connection_do_not_change_player_state() {
        let shared = crate::world::new_shared();
        {
            let mut w = shared.lock().await;
            let (ta, _ra) = mpsc::unbounded_channel();
            let old = crate::world::Player {
                id: "hero".into(),
                name: "hero".into(),
                x: 1.0,
                y: 2.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp: 100,
                max_hp: 100,
                lang: "de".into(),
                account_id: 7,
                session_id: "sess-1".into(),
                entities: Default::default(),
                last_activity: Instant::now(),
                tx: ta,
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
                effects: std::vec::Vec::new(),
                cooldowns: std::collections::BTreeMap::new(),
                active_cast: None,
                learned_abilities: std::collections::HashSet::new(),
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
                persist_revision: 3,
            };
            w.players.insert("hero".into(), old);
            w.by_conn.insert(7, "hero".into());
        }
        // Takeover durch conn 8 (wie commit_login es im HELLO tut).
        let (new_tx, _rx) = mpsc::unbounded_channel();
        {
            let mut w = shared.lock().await;
            let outcome = crate::world::commit_login(
                &mut w,
                8,
                crate::world::Player {
                    id: "hero".into(),
                    name: "hero".into(),
                    x: 900.0,
                    y: 900.0,
                    face: 0.0,
                    ping_ms: 0,
                    zone_id: 0,
                    hp: 1,
                    max_hp: 100,
                    lang: "de".into(),
                    account_id: 7,
                    session_id: "sess-2".into(),
                    entities: Default::default(),
                    last_activity: Instant::now(),
                    tx: new_tx,
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
                    effects: std::vec::Vec::new(),
                    cooldowns: std::collections::BTreeMap::new(),
                    active_cast: None,
                    learned_abilities: std::collections::HashSet::new(),
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
                crate::world::ConnectionFields {
                    tx: mpsc::unbounded_channel().0,
                    session_id: "sess-2".into(),
                    lang: "de".into(),
                },
            )
            .unwrap();
            assert_eq!(
                outcome,
                crate::world::CommitOutcome::Takeover { old_conn_id: 7 }
            );
        }
        // Alte Verbindung (7) ist entmachtet: MOVE/ATTACK verändern nichts …
        handle_move(
            &shared,
            7,
            &serde_json::json!({"dir": [1.0, 0.0], "x": 500.0, "y": 500.0}),
            100,
        )
        .await;
        handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "hero"}),
            &crate::config::combat_config(&Default::default()),
            &npc_cfg(),
        )
        .await;
        let w = shared.lock().await;
        let p = &w.players["hero"];
        assert_eq!((p.x, p.y), (1.0, 2.0), "RAM-Zustand wurde verändert");
        assert_eq!(p.hp, 100);
        assert!(p.combat.is_none());
        assert!(!p.dirty.any());
        assert_eq!(p.persist_generation, 0);
        assert_eq!(p.persist_revision, 3);
        // … und die neue Verbindung ist die einzige berechtigte.
        assert!(!w.by_conn.contains_key(&7));
        assert!(crate::world::is_owner(&w, 8, "hero"));
        assert_eq!(w.by_conn.len(), 1);
    }
}
