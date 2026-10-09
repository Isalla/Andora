// player_trade — serverseitiger Spielerhandel: Dialog + Angebote.
//
// Baut auf dem bestehenden recovery-sicheren Zwei-Charakter-Persistenz-
// baustein auf (`persist::commit_trade`, docs/Player_Persistenz.md
// „Trade-Ausnahme"). Es gibt KEINE zweite Commit-/Recovery-Implementierung:
// der finale Austausch läuft ausschließlich über `TradeCommitRequest` und
// den bestehenden Commit-Pfad; Erfolg erst nach bestätigter Spool-
// Dauerhaftigkeit und gemeinsamer RAM-Übernahme.
//
// Verbindliche Spielregeln (docs/Handelssystem.md §15):
// - Handelsreichweite 5 m (vorhandener Interaktionsradius, Aufrufer reicht
//   `LOOT_PICKUP_RADIUS`).
// - Höchstens ein Handelsdialog je Charakter, einschließlich Einladung.
// - Einladungen verfallen nach 60 s (monotone Zeit, `Instant`).
// - Jede tatsächliche Angebotsänderung invalidiert beide Bestätigungen und
//   erhöht die Angebotsversion; Bestätigung gilt nur für die aktuelle Version.
// - Offene Dialoge reservieren weder Items noch Idia.
// - Beim Abschluss werden sämtliche Voraussetzungen erneut serverseitig
//   geprüft; der finale Austausch läuft über den freigegebenen Commit.
// - QuestItem-Kategorie ist zwischen Spielern immer gesperrt (auch nach
//   Questabbruch); zusätzlich gilt der bestehende ACTIVE-Questschutz
//   (`trade::active_quest_item_ids`, fail-closed bei unauflösbaren ACTIVE-
//   Definitionen).
// - Disconnect, Takeover, Tod und Reichweitenverlust brechen den noch nicht
//   verbindlich gestarteten Handel ab und informieren den Partner.
// - Sobald der Commit-Pfad vorbereiteten Inhalt verbindlich festhält, darf
//   Dialog-Cancel ihn nicht verwerfen; Pending/Wiederaufnahme/Recovery
//   folgen ausschließlich dessen bestehendem Vertrag.
// - Keine Gebühren, kein NPC-Buyback, kein Echtgeld, keine neue
//   Parental-Regel, keine Auktionshaus-Anbindung.
//
// Alle Funktionen laufen unter der World-Sperre des Aufrufers (keine Awaits,
// keine Zwischenänderungen). Offene Dialoge sind reiner Runtime-State und
// werden nie persistiert.
use std::time::{Duration, Instant};

use crate::persist::{TradeCommitRequest, TradeTransferRequest};
use crate::protocol::{s2c, Frame};
use crate::quest::QuestService;
use crate::world::World;

/// Einladungsfrist: Einladungen verfallen nach 60 Sekunden (monotone Zeit).
pub const INVITE_TTL: Duration = Duration::from_secs(60);
/// Obergrenze für Angebotszeilen je Seite (Rahmen gegen Riesen-Frames; die
/// harte Frame-Größe prüft weiterhin die Security-Schicht).
pub const MAX_OFFER_ITEMS: usize = 64;

/// Handelsaktion (`PLAYER_TALK`-ähnliches C2S-Feld `action`, exakt klein).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTradeAction {
    Request,
    Accept,
    Decline,
    Offer,
    Confirm,
    Cancel,
}

impl PlayerTradeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            PlayerTradeAction::Request => "request",
            PlayerTradeAction::Accept => "accept",
            PlayerTradeAction::Decline => "decline",
            PlayerTradeAction::Offer => "offer",
            PlayerTradeAction::Confirm => "confirm",
            PlayerTradeAction::Cancel => "cancel",
        }
    }

    /// Parst die Client-Aktion (exakte Kleinschreibung nach Trim; alles
    /// andere ist `None`, kein stiller Default).
    pub fn parse(raw: &str) -> Option<PlayerTradeAction> {
        match raw.trim() {
            "request" => Some(PlayerTradeAction::Request),
            "accept" => Some(PlayerTradeAction::Accept),
            "decline" => Some(PlayerTradeAction::Decline),
            "offer" => Some(PlayerTradeAction::Offer),
            "confirm" => Some(PlayerTradeAction::Confirm),
            "cancel" => Some(PlayerTradeAction::Cancel),
            _ => None,
        }
    }
}

/// Stabile Ablehnungs-/Abbruchgründe (`PLAYER_TRADE`-Feld `reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTradeReject {
    UnknownAction,
    UnknownPlayer,
    UnknownTarget,
    SelfTrade,
    OutOfRange,
    DialogBusy,
    NoDialog,
    NotParticipant,
    NotInvited,
    NotOpen,
    InviteExpired,
    StaleVersion,
    InvalidOffer,
    ItemUnavailable,
    NotEnoughItems,
    BoundItem,
    QuestItemProtected,
    QuestDataUnavailable,
    DefinitionUnavailable,
    InsufficientIdia,
    IdiaOverflow,
    CommitPending,
    CommitInProgress,
    CommitFailed,
    TradePending,
    ActorDead,
    PartnerUnavailable,
}

impl PlayerTradeReject {
    pub fn reason(self) -> &'static str {
        match self {
            PlayerTradeReject::UnknownAction => "unknown_action",
            PlayerTradeReject::UnknownPlayer => "unknown_player",
            PlayerTradeReject::UnknownTarget => "unknown_target",
            PlayerTradeReject::SelfTrade => "self_trade",
            PlayerTradeReject::OutOfRange => "out_of_range",
            PlayerTradeReject::DialogBusy => "dialog_busy",
            PlayerTradeReject::NoDialog => "no_dialog",
            PlayerTradeReject::NotParticipant => "not_participant",
            PlayerTradeReject::NotInvited => "not_invited",
            PlayerTradeReject::NotOpen => "not_open",
            PlayerTradeReject::InviteExpired => "invite_expired",
            PlayerTradeReject::StaleVersion => "stale_version",
            PlayerTradeReject::InvalidOffer => "invalid_offer",
            PlayerTradeReject::ItemUnavailable => "item_unavailable",
            PlayerTradeReject::NotEnoughItems => "not_enough_items",
            PlayerTradeReject::BoundItem => "bound_item",
            PlayerTradeReject::QuestItemProtected => "quest_item_protected",
            PlayerTradeReject::QuestDataUnavailable => "quest_data_unavailable",
            PlayerTradeReject::DefinitionUnavailable => "definition_unavailable",
            PlayerTradeReject::InsufficientIdia => "insufficient_idia",
            PlayerTradeReject::IdiaOverflow => "idia_overflow",
            PlayerTradeReject::CommitPending => "trade_commit_pending",
            PlayerTradeReject::CommitInProgress => "commit_in_progress",
            PlayerTradeReject::CommitFailed => "trade_commit_failed",
            PlayerTradeReject::TradePending => "trade_pending",
            PlayerTradeReject::ActorDead => "actor_dead",
            PlayerTradeReject::PartnerUnavailable => "partner_unavailable",
        }
    }
}

/// Angebotszeile: ausschließlich eigene Instanz-UUID mit Menge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedItem {
    pub item_uuid: String,
    pub count: i64,
}

/// Angebotsseite eines Charakters: Instanz-UUIDs mit Menge plus Idia.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DialogOffer {
    pub items: Vec<OfferedItem>,
    pub idia: i64,
}

/// Nachvollziehbare Dialogzustände. Terminale Zustände werden nicht
/// gespeichert: der Dialog wird entfernt und beide Seiten informiert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogState {
    /// Einladung versandt, noch nicht angenommen (verfällt nach 60 s).
    Invited,
    /// Angenommen: Angebote und Bestätigungen laufen.
    Open,
    /// Beide bestätigt, Commit-Versuch läuft (Version eingefroren).
    Committing,
    /// Commit-Dauerhaftigkeit unklar: bestehende Wiederherstellungspflicht
    /// des Commit-Pfads läuft; Cancel verwirft nichts.
    PendingCommit,
}

impl DialogState {
    pub fn as_str(self) -> &'static str {
        match self {
            DialogState::Invited => "invited",
            DialogState::Open => "open",
            DialogState::Committing => "committing",
            DialogState::PendingCommit => "pending",
        }
    }
}

/// Charaktergebundener Runtime-Handelsdialog (keine Persistenz offener
/// Dialoge). `parties[0]` ist der Einladende, `parties[1]` der Eingeladene;
/// `offers[i]`/`confirmed[i]` gehören zu `parties[i]`.
#[derive(Debug, Clone)]
pub struct PlayerTradeDialog {
    pub id: String,
    pub seq_no: u64,
    pub parties: [String; 2],
    pub state: DialogState,
    /// Eindeutige Angebotsversion: startet bei 0, jede tatsächliche
    /// Angebotsänderung erhöht um 1 und invalidiert beide Bestätigungen.
    pub version: u64,
    pub offers: [DialogOffer; 2],
    pub confirmed: [bool; 2],
    pub expires_at: Instant,
    /// Gesetzte Commit-Kennung während `Committing`/`PendingCommit`.
    pub commit_id: Option<String>,
}

/// Ergebnis einer Dialogoperation: Antwort an den Auslöser, Pushes an den
/// Partner (der Handler sendet sie mit `seq` 0) und optional eine
/// Commit-Absicht (der Handler führt sie über den bestehenden Commit-Pfad
/// aus und sendet erst danach das Endergebnis).
#[derive(Debug)]
pub struct DialogOutcome {
    pub actor_msg: Option<serde_json::Value>,
    pub partner_msgs: Vec<(String, serde_json::Value)>,
    pub commit: Option<TradeCommitRequest>,
}

/// Fehler einer Dialogoperation: Grund für den Auslöser plus optionale
/// Partner-Pushes (z. B. wenn die Operation den Dialog abgebrochen hat).
#[derive(Debug)]
pub struct DialogFailure {
    pub reason: PlayerTradeReject,
    pub dialog_id: Option<String>,
    pub partner_msgs: Vec<(String, serde_json::Value)>,
}

impl DialogFailure {
    pub fn reject(reason: PlayerTradeReject) -> Self {
        Self {
            reason,
            dialog_id: None,
            partner_msgs: Vec::new(),
        }
    }

    pub fn reject_on(reason: PlayerTradeReject, dialog_id: &str) -> Self {
        Self {
            reason,
            dialog_id: Some(dialog_id.to_string()),
            partner_msgs: Vec::new(),
        }
    }
}

/// Abbruchnlass für `abort_for` (Disconnect/Takeover/Tod) mit stabilem
/// `reason` für die Partnerbenachrichtigung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortReason {
    Disconnect,
    Takeover,
    Death,
}

impl AbortReason {
    fn reason(self) -> &'static str {
        match self {
            AbortReason::Disconnect => "disconnect",
            AbortReason::Takeover => "takeover",
            AbortReason::Death => "death",
        }
    }
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).hypot(ay - by)
}

/// Deterministische Commit-Kennung aus Dialognummer und Angebotsversion.
/// Charset `[A-Za-z0-9]`, deutlich unter 64 Zeichen: identische
/// Wiederholungen erzeugen dieselbe Kennung, sodass ein wiederholter
/// Aufruf keinen zweiten Commit anlegen kann (Receipt-Dedup des
/// bestehenden Commit-Pfads).
pub fn commit_id_for_dialog(seq_no: u64, version: u64) -> String {
    format!("ptd{seq_no}v{version}")
}

fn party_index(dialog: &PlayerTradeDialog, actor: &str) -> Option<usize> {
    dialog.parties.iter().position(|p| p == actor)
}

fn remove_dialog(world: &mut World, dialog_id: &str) -> Option<PlayerTradeDialog> {
    let dialog = world.player_trade_dialogs.remove(dialog_id)?;
    for party in &dialog.parties {
        if world
            .player_trade_by_char
            .get(party)
            .is_some_and(|v| v == dialog_id)
        {
            world.player_trade_by_char.remove(party);
        }
    }
    Some(dialog)
}

/// Gemeinsame Angebotssicht für beide Seiten: Dialogstand, Version,
/// Parteien, Angebote (nur angebotene Instanzen mit Definition und Menge,
/// angebotene Idia) und Bestätigungen. Keine fremden Inventar- oder
/// Kontodaten.
pub fn dialog_view(world: &World, dialog: &PlayerTradeDialog) -> serde_json::Value {
    let offers: Vec<serde_json::Value> = dialog
        .offers
        .iter()
        .map(|offer| {
            let items: Vec<serde_json::Value> = offer
                .items
                .iter()
                .map(|entry| {
                    let (item_id, name) = world
                        .players
                        .values()
                        .filter_map(|p| {
                            p.inventory
                                .base_slots
                                .iter()
                                .chain(p.inventory.bags.iter().flat_map(|b| &b.slots))
                                .filter_map(|s| s.as_ref())
                                .chain(p.inventory.equipped.values())
                                .find(|it| it.item_uuid == entry.item_uuid)
                        })
                        .next()
                        .and_then(|inst| {
                            world
                                .item_definitions
                                .get(&inst.item_id)
                                .map(|def| (inst.item_id.clone(), def.name.clone()))
                        })
                        .unwrap_or_default();
                    serde_json::json!({
                        "item_uuid": entry.item_uuid,
                        "item_id": item_id,
                        "name": name,
                        "count": entry.count,
                    })
                })
                .collect();
            serde_json::json!({"items": items, "idia": offer.idia})
        })
        .collect();
    serde_json::json!({
        "dialog_id": dialog.id,
        "state": dialog.state.as_str(),
        "version": dialog.version,
        "parties": dialog.parties,
        "offers": offers,
        "confirmed": dialog.confirmed,
    })
}

fn ok_action(
    action: PlayerTradeAction,
    dialog_id: &str,
    view: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({"ok": true, "action": action.as_str(), "dialog": view, "dialog_id": dialog_id})
}

fn push_event(event: &str, dialog_id: &str, view: Option<serde_json::Value>) -> serde_json::Value {
    let mut v = serde_json::json!({"ok": true, "event": event, "dialog_id": dialog_id});
    if let Some(view) = view {
        v["dialog"] = view;
    }
    v
}

fn push_abort(reason: &str, dialog_id: &str) -> serde_json::Value {
    serde_json::json!({"ok": true, "event": "aborted", "reason": reason, "dialog_id": dialog_id})
}

/// Sendet Partner-Pushes (`seq` 0) an noch vorhandene Spieler.
pub fn send_notes(world: &World, notes: &[(String, serde_json::Value)]) {
    for (to, payload) in notes {
        if let Some(player) = world.players.get(to) {
            player.send(&Frame::new(0, s2c::PLAYER_TRADE, payload.clone()));
        }
    }
}

/// Prüft ein Angebot gegen den aktuellen Serverzustand: ausschließlich
/// eigene Instanz-UUIDs mit Menge und angebotene Idia. Doppelte UUIDs,
/// ungültige Mengen und negative Idia werden abgelehnt; Bindung wird anhand
/// Definition UND Instanz geprüft; Equipment, Tascheninhalte außerhalb von
/// Basis/Taschen, Puffer und Fremd-UUIDs sind ausgeschlossen; die
/// QuestItem-Kategorie ist immer gesperrt, zusätzlich gilt der bestehende
/// ACTIVE-Questschutz (fail-closed).
fn validate_offer(
    world: &World,
    quests: &QuestService,
    owner: &str,
    offer: &DialogOffer,
) -> Result<(), PlayerTradeReject> {
    if offer.items.len() > MAX_OFFER_ITEMS {
        return Err(PlayerTradeReject::InvalidOffer);
    }
    if offer.idia < 0 {
        return Err(PlayerTradeReject::InvalidOffer);
    }
    let mut seen = std::collections::HashSet::new();
    for entry in &offer.items {
        if entry.count <= 0 {
            return Err(PlayerTradeReject::InvalidOffer);
        }
        if !seen.insert(entry.item_uuid.as_str()) {
            return Err(PlayerTradeReject::InvalidOffer);
        }
    }
    let player = world
        .players
        .get(owner)
        .ok_or(PlayerTradeReject::UnknownPlayer)?;
    if offer.idia > player.idia {
        return Err(PlayerTradeReject::InsufficientIdia);
    }
    let quest_items = crate::trade::active_quest_item_ids(player, quests)
        .map_err(|_| PlayerTradeReject::QuestDataUnavailable)?;
    for entry in &offer.items {
        let Some(inst) = player.inventory.owned_instance(&entry.item_uuid) else {
            return Err(PlayerTradeReject::ItemUnavailable);
        };
        if entry.count > inst.count {
            return Err(PlayerTradeReject::NotEnoughItems);
        }
        let Some(def) = world.item_definitions.get(&inst.item_id) else {
            return Err(PlayerTradeReject::DefinitionUnavailable);
        };
        if def.validate().is_err() || inst.validate(def).is_err() {
            return Err(PlayerTradeReject::InvalidOffer);
        }
        if def.binding_rule != crate::item::BindingRule::Tradeable || inst.is_bound() {
            return Err(PlayerTradeReject::BoundItem);
        }
        // QuestItem-Kategorie ist zwischen Spielern immer gesperrt — auch
        // nach Questabbruch (strenger als der NPC-Verkauf, der abgebrochene
        // Quests wieder freigibt).
        if def.category == crate::item::ItemCategory::QuestItem {
            return Err(PlayerTradeReject::QuestItemProtected);
        }
        if quest_items.contains(inst.item_id.as_str()) {
            return Err(PlayerTradeReject::QuestItemProtected);
        }
    }
    Ok(())
}

/// Lebendigkeit beider Parteien unter der World-Sperre. Liefert bei
/// Fehlstand den Abbruchgrund für den Auslöser.
fn ensure_live_parties(
    world: &World,
    dialog: &PlayerTradeDialog,
    actor: &str,
) -> Result<(), PlayerTradeReject> {
    let actor_alive = world.players.get(actor).is_some_and(|p| p.hp > 0);
    if !actor_alive {
        let missing = !world.players.contains_key(actor);
        return Err(if missing {
            PlayerTradeReject::UnknownPlayer
        } else {
            PlayerTradeReject::ActorDead
        });
    }
    let partner = &dialog.parties[1 - party_index(dialog, actor).unwrap_or(0)];
    if !world.players.get(partner).is_some_and(|p| p.hp > 0) {
        return Err(PlayerTradeReject::PartnerUnavailable);
    }
    Ok(())
}

fn ensure_range(
    world: &World,
    dialog: &PlayerTradeDialog,
    radius: f64,
) -> Result<(), PlayerTradeReject> {
    let [a, b] = &dialog.parties;
    let (Some(pa), Some(pb)) = (world.players.get(a), world.players.get(b)) else {
        return Err(PlayerTradeReject::PartnerUnavailable);
    };
    if dist(pa.x, pa.y, pb.x, pb.y) > radius {
        return Err(PlayerTradeReject::OutOfRange);
    }
    Ok(())
}

fn ensure_unfrozen(world: &World, dialog: &PlayerTradeDialog) -> Result<(), PlayerTradeReject> {
    if dialog
        .parties
        .iter()
        .any(|p| !world.economic_mutation_allowed(p))
    {
        return Err(PlayerTradeReject::CommitPending);
    }
    Ok(())
}

/// Bricht den Dialog des Auslösers bei Fehlstand (Reichweite/Leben)
/// ab und meldet das dem Partner. Gibt `true` zurück, wenn abgebrochen
/// wurde (der Aufrufer meldet dann den `reason` an den Auslöser).
fn abort_on_bad_stand(
    world: &mut World,
    dialog_id: &str,
    actor: &str,
    reason: PlayerTradeReject,
) -> (DialogFailure, bool) {
    let removed = remove_dialog(world, dialog_id);
    let mut failure = DialogFailure::reject_on(reason, dialog_id);
    if let Some(dialog) = removed {
        let other = dialog.parties.iter().find(|p| *p != actor).cloned();
        if let Some(other) = other {
            failure
                .partner_msgs
                .push((other, push_abort(reason.reason(), dialog_id)));
        }
        (failure, true)
    } else {
        (DialogFailure::reject(reason), false)
    }
}

/// Einladung versenden: reichweiten- und zustandsgeprüft, mit 60-s-Frist.
/// Höchstens ein Dialog je Charakter (Einladung zählt mit).
pub fn request_dialog(
    world: &mut World,
    now: Instant,
    radius: f64,
    actor: &str,
    target: &str,
) -> Result<DialogOutcome, DialogFailure> {
    if actor == target {
        return Err(DialogFailure::reject(PlayerTradeReject::SelfTrade));
    }
    let (actor_pos, actor_alive) = match world.players.get(actor) {
        Some(p) => ((p.x, p.y), p.hp > 0),
        None => return Err(DialogFailure::reject(PlayerTradeReject::UnknownPlayer)),
    };
    if !actor_alive {
        return Err(DialogFailure::reject(PlayerTradeReject::ActorDead));
    }
    let Some(partner) = world.players.get(target) else {
        return Err(DialogFailure::reject(PlayerTradeReject::UnknownTarget));
    };
    if partner.hp <= 0 {
        return Err(DialogFailure::reject(PlayerTradeReject::PartnerUnavailable));
    }
    if dist(actor_pos.0, actor_pos.1, partner.x, partner.y) > radius {
        return Err(DialogFailure::reject(PlayerTradeReject::OutOfRange));
    }
    if world.player_trade_by_char.contains_key(actor)
        || world.player_trade_by_char.contains_key(target)
    {
        return Err(DialogFailure::reject(PlayerTradeReject::DialogBusy));
    }
    if !world.economic_mutation_allowed(actor) || !world.economic_mutation_allowed(target) {
        return Err(DialogFailure::reject(PlayerTradeReject::CommitPending));
    }
    let seq_no = world.player_trade_next_id;
    world.player_trade_next_id = world.player_trade_next_id.wrapping_add(1);
    let id = format!("ptd{seq_no}");
    let dialog = PlayerTradeDialog {
        id: id.clone(),
        seq_no,
        parties: [actor.to_string(), target.to_string()],
        state: DialogState::Invited,
        version: 0,
        offers: [DialogOffer::default(), DialogOffer::default()],
        confirmed: [false, false],
        expires_at: now + INVITE_TTL,
        commit_id: None,
    };
    let view = dialog_view(world, &dialog);
    world.player_trade_dialogs.insert(id.clone(), dialog);
    world
        .player_trade_by_char
        .insert(actor.to_string(), id.clone());
    world
        .player_trade_by_char
        .insert(target.to_string(), id.clone());
    Ok(DialogOutcome {
        actor_msg: Some(ok_action(PlayerTradeAction::Request, &id, view.clone())),
        partner_msgs: vec![(target.to_string(), push_event("invited", &id, Some(view)))],
        commit: None,
    })
}

/// Läuft eine Einladung ab, wird sie entfernt (Auslöser: Tick oder nächste
/// Dialogoperation — auch ohne weitere Nachrichten).
fn expire_invite_if_due(
    world: &mut World,
    dialog_id: &str,
    now: Instant,
) -> Option<Vec<(String, serde_json::Value)>> {
    let expired = world
        .player_trade_dialogs
        .get(dialog_id)
        .is_some_and(|d| d.state == DialogState::Invited && now >= d.expires_at);
    if !expired {
        return None;
    }
    let dialog = remove_dialog(world, dialog_id)?;
    let mut notes = Vec::new();
    for party in &dialog.parties {
        notes.push((
            party.clone(),
            serde_json::json!({
                "ok": true, "event": "expired",
                "reason": PlayerTradeReject::InviteExpired.reason(),
                "dialog_id": dialog_id,
            }),
        ));
    }
    Some(notes)
}

fn resolve_open_dialog<'w>(
    world: &'w mut World,
    now: Instant,
    actor: &str,
    dialog_id: &str,
) -> Result<(usize, &'w mut PlayerTradeDialog), DialogFailure> {
    if let Some(notes) = expire_invite_if_due(world, dialog_id, now) {
        return Err(DialogFailure {
            reason: PlayerTradeReject::InviteExpired,
            dialog_id: Some(dialog_id.to_string()),
            partner_msgs: notes.into_iter().filter(|(to, _)| to != actor).collect(),
        });
    }
    let dialog = world
        .player_trade_dialogs
        .get_mut(dialog_id)
        .ok_or_else(|| DialogFailure::reject_on(PlayerTradeReject::NoDialog, dialog_id))?;
    let idx = party_index(dialog, actor)
        .ok_or_else(|| DialogFailure::reject_on(PlayerTradeReject::NotParticipant, dialog_id))?;
    Ok((idx, dialog))
}

/// Einladung annehmen: nur der Eingeladene, nur im Zustand `Invited`,
/// nur in Reichweite und nur vor Fristablauf.
pub fn accept_dialog(
    world: &mut World,
    now: Instant,
    radius: f64,
    actor: &str,
    dialog_id: &str,
) -> Result<DialogOutcome, DialogFailure> {
    let idx = {
        let (idx, dialog) = resolve_open_dialog(world, now, actor, dialog_id)?;
        if dialog.state != DialogState::Invited {
            return Err(DialogFailure::reject_on(
                PlayerTradeReject::NotOpen,
                dialog_id,
            ));
        }
        if idx != 1 {
            return Err(DialogFailure::reject_on(
                PlayerTradeReject::NotInvited,
                dialog_id,
            ));
        }
        idx
    };
    let _ = idx;
    if let Err(reason) = ensure_live_parties(
        world,
        world.player_trade_dialogs.get(dialog_id).unwrap(),
        actor,
    ) {
        let (failure, _) = abort_on_bad_stand(world, dialog_id, actor, reason);
        return Err(failure);
    }
    if ensure_range(
        world,
        world.player_trade_dialogs.get(dialog_id).unwrap(),
        radius,
    )
    .is_err()
    {
        let (failure, _) =
            abort_on_bad_stand(world, dialog_id, actor, PlayerTradeReject::OutOfRange);
        return Err(failure);
    }
    if let Err(reason) = ensure_unfrozen(world, world.player_trade_dialogs.get(dialog_id).unwrap())
        .map_err(|_| PlayerTradeReject::CommitPending)
    {
        let _ = reason;
        return Err(DialogFailure::reject_on(
            PlayerTradeReject::CommitPending,
            dialog_id,
        ));
    }
    world.player_trade_dialogs.get_mut(dialog_id).unwrap().state = DialogState::Open;
    let dialog = world.player_trade_dialogs.get(dialog_id).unwrap();
    let view = dialog_view(world, dialog);
    let partner = dialog.parties[0].clone();
    Ok(DialogOutcome {
        actor_msg: Some(ok_action(
            PlayerTradeAction::Accept,
            dialog_id,
            view.clone(),
        )),
        partner_msgs: vec![(partner, push_event("opened", dialog_id, Some(view)))],
        commit: None,
    })
}

/// Einladung ablehnen: nur der Eingeladene im Zustand `Invited`.
pub fn decline_dialog(
    world: &mut World,
    now: Instant,
    actor: &str,
    dialog_id: &str,
) -> Result<DialogOutcome, DialogFailure> {
    {
        let (idx, dialog) = resolve_open_dialog(world, now, actor, dialog_id)?;
        if dialog.state != DialogState::Invited {
            return Err(DialogFailure::reject_on(
                PlayerTradeReject::NotOpen,
                dialog_id,
            ));
        }
        if idx != 1 {
            return Err(DialogFailure::reject_on(
                PlayerTradeReject::NotInvited,
                dialog_id,
            ));
        }
    }
    let dialog = remove_dialog(world, dialog_id).unwrap();
    let partner = dialog.parties[0].clone();
    Ok(DialogOutcome {
        actor_msg: Some(serde_json::json!({
            "ok": true, "action": PlayerTradeAction::Decline.as_str(),
            "event": "declined", "dialog_id": dialog_id,
        })),
        partner_msgs: vec![(
            partner,
            serde_json::json!({"ok": true, "event": "declined", "dialog_id": dialog_id}),
        )],
        commit: None,
    })
}

/// Angebot setzen: ausschließlich eigene Instanz-UUIDs mit Menge plus
/// angebotene Idia. Jede tatsächliche Änderung erhöht die Angebotsversion
/// und invalidiert BEIDE Bestätigungen; identische Wiederholung ist ein
/// No-Op ohne Versionswechsel.
pub fn set_offer(
    world: &mut World,
    quests: &QuestService,
    now: Instant,
    radius: f64,
    actor: &str,
    dialog_id: &str,
    offer: &DialogOffer,
) -> Result<DialogOutcome, DialogFailure> {
    let idx = {
        let (idx, dialog) = resolve_open_dialog(world, now, actor, dialog_id)?;
        if dialog.state != DialogState::Open {
            let reason = if dialog.state == DialogState::Invited {
                PlayerTradeReject::NotOpen
            } else {
                PlayerTradeReject::CommitInProgress
            };
            return Err(DialogFailure::reject_on(reason, dialog_id));
        }
        idx
    };
    {
        let dialog = world.player_trade_dialogs.get(dialog_id).unwrap();
        if let Err(reason) = ensure_live_parties(world, dialog, actor) {
            let fatal = reason;
            let (failure, _) = abort_on_bad_stand(world, dialog_id, actor, fatal);
            return Err(failure);
        }
        if ensure_range(world, dialog, radius).is_err() {
            let (failure, _) =
                abort_on_bad_stand(world, dialog_id, actor, PlayerTradeReject::OutOfRange);
            return Err(failure);
        }
        if ensure_unfrozen(world, dialog).is_err() {
            return Err(DialogFailure::reject_on(
                PlayerTradeReject::CommitPending,
                dialog_id,
            ));
        }
    }
    if let Err(reason) = validate_offer(world, quests, actor, offer) {
        return Err(DialogFailure::reject_on(reason, dialog_id));
    }
    {
        let dialog = world.player_trade_dialogs.get_mut(dialog_id).unwrap();
        if dialog.offers[idx] != *offer {
            dialog.offers[idx] = offer.clone();
            dialog.version = dialog.version.wrapping_add(1);
            dialog.confirmed = [false, false];
        }
    }
    let dialog = world.player_trade_dialogs.get(dialog_id).unwrap();
    let view = dialog_view(world, dialog);
    let partner = dialog.parties[1 - idx].clone();
    Ok(DialogOutcome {
        actor_msg: Some(ok_action(PlayerTradeAction::Offer, dialog_id, view.clone())),
        partner_msgs: vec![(partner, push_event("updated", dialog_id, Some(view)))],
        commit: None,
    })
}

/// Baut die Commit-Absicht aus dem aktuellen Dialogzustand: beide
/// Bestätigungen für die aktuelle Version, beide Eigentümer, Reichweite,
/// Freeze-Freiheit, Eigentum/Bindung/Questschutz pro Angebot und geprüfte
/// Idia-Arithmetik. Instanzgenaue Entnahme/Einsetzung, Split/Merge und
/// Kapazität prüft der bestehende Commit-Pfad atomar bei der Vorbereitung.
fn build_commit_request(
    world: &World,
    quests: &QuestService,
    dialog: &PlayerTradeDialog,
) -> Result<TradeCommitRequest, PlayerTradeReject> {
    ensure_unfrozen(world, dialog)?;
    for (i, party) in dialog.parties.iter().enumerate() {
        let player = world
            .players
            .get(party)
            .ok_or(PlayerTradeReject::PartnerUnavailable)?;
        if player.hp <= 0 {
            return Err(PlayerTradeReject::PartnerUnavailable);
        }
        validate_offer(world, quests, party, &dialog.offers[i])?;
    }
    let mut order = [0usize, 1usize];
    if dialog.parties[order[0]] > dialog.parties[order[1]] {
        order.swap(0, 1);
    }
    let balances = |i: usize| -> Result<i64, PlayerTradeReject> {
        let me = &dialog.parties[i];
        let other = &dialog.parties[1 - i];
        let player = world
            .players
            .get(me)
            .ok_or(PlayerTradeReject::PartnerUnavailable)?;
        let after = i128::from(player.idia) - i128::from(dialog.offers[i].idia)
            + i128::from(dialog.offers[1 - i].idia);
        if after < 0 {
            return Err(PlayerTradeReject::InsufficientIdia);
        }
        if after > i64::MAX as i128 {
            return Err(PlayerTradeReject::IdiaOverflow);
        }
        let _ = (me, other);
        Ok(after as i64)
    };
    let after0 = balances(0)?;
    let after1 = balances(1)?;
    let after_sorted = if order[0] == 0 {
        [after0, after1]
    } else {
        [after1, after0]
    };
    let mut transfers = Vec::new();
    for (i, party) in dialog.parties.iter().enumerate() {
        for entry in &dialog.offers[i].items {
            transfers.push(TradeTransferRequest {
                source: party.clone(),
                item_uuid: entry.item_uuid.clone(),
                count: entry.count,
            });
        }
    }
    transfers.sort_by(|a, b| a.source.cmp(&b.source).then(a.item_uuid.cmp(&b.item_uuid)));
    Ok(TradeCommitRequest {
        commit_id: commit_id_for_dialog(dialog.seq_no, dialog.version),
        characters: [
            dialog.parties[order[0]].clone(),
            dialog.parties[order[1]].clone(),
        ],
        idia: after_sorted,
        transfers,
    })
}

/// Verbindliche Nachprüfung unmittelbar vor der Commit-Vorbereitung
/// (Aufruf durch `persist::commit_validated_trade` unter den erworbenen
/// Charakter-Gates und derselben World-Sperre, unter der `prepare_trade`
/// die verbindlichen Nachzustände baut; synchron, ohne Await).
///
/// Prüft erneut den Dialogstand (Version, beide Bestätigungen, Parteien,
/// Commit-Bindung), die aktuellen Eigentümer (online, lebend, in
/// Handelsreichweite) sowie je Angebot Eigentum, Mengen, Idia, Bindung
/// anhand Definition UND Instanz, QuestItem-Sperre und ACTIVE-Questschutz
/// (einschließlich `quest_data_unavailable`). Inventarmechanik
/// (Eigentumsplatzierung, Idia-Erhalt, vollständige Kapazität, Split/Merge)
/// prüft `prepare_trade` atomar in derselben Sperre unmittelbar danach.
/// Fehler lehnen ohne wirtschaftliche Mutation und ohne vorbereiteten Trade
/// ab; die Meldungen enthalten keine Spielerdaten (nur stabile Gründe).
pub fn validate_binding_preparation(
    world: &World,
    quests: &QuestService,
    radius: f64,
    dialog_id: &str,
    version: u64,
    request: &TradeCommitRequest,
) -> Result<(), String> {
    let dialog = world
        .player_trade_dialogs
        .get(dialog_id)
        .ok_or_else(|| "trade dialog gone".to_string())?;
    if dialog.state != DialogState::Committing {
        return Err("trade dialog not committing".to_string());
    }
    if dialog.commit_id.as_deref() != Some(request.commit_id.as_str()) {
        return Err("trade commit ID mismatch".to_string());
    }
    if dialog.version != version {
        return Err("trade dialog version changed".to_string());
    }
    if !(dialog.confirmed[0] && dialog.confirmed[1]) {
        return Err("trade dialog not confirmed".to_string());
    }
    let mut parties = dialog.parties.clone();
    parties.sort();
    if parties[0] != request.characters[0] || parties[1] != request.characters[1] {
        return Err("trade dialog parties changed".to_string());
    }
    for party in &dialog.parties {
        let player = world
            .players
            .get(party)
            .ok_or_else(|| "trade party unavailable".to_string())?;
        if player.hp <= 0 {
            return Err("trade party not alive".to_string());
        }
    }
    let [a, b] = &dialog.parties;
    let (pa, pb) = (&world.players[a.as_str()], &world.players[b.as_str()]);
    if dist(pa.x, pa.y, pb.x, pb.y) > radius {
        return Err("trade parties out of range".to_string());
    }
    for (i, party) in dialog.parties.iter().enumerate() {
        validate_offer(world, quests, party, &dialog.offers[i])
            .map_err(|r| format!("trade offer rejected: {}", r.reason()))?;
    }
    Ok(())
}
/// Bestätigen: gilt ausschließlich für die angegebene (aktuelle)
/// Angebotsversion. Bei beidseitiger Bestätigung wird die Version
/// eingefroren und die Commit-Absicht zurückgegeben; wiederholte
/// Nachrichten erzeugen keinen zweiten Commit (deterministische
/// Commit-Kennung + Receipt-Dedup des bestehenden Pfads).
pub fn confirm_dialog(
    world: &mut World,
    quests: &QuestService,
    now: Instant,
    radius: f64,
    actor: &str,
    dialog_id: &str,
    version: u64,
) -> Result<DialogOutcome, DialogFailure> {
    let idx = {
        let (idx, dialog) = resolve_open_dialog(world, now, actor, dialog_id)?;
        if dialog.state != DialogState::Open {
            let reason = if dialog.state == DialogState::Invited {
                PlayerTradeReject::NotOpen
            } else {
                PlayerTradeReject::CommitInProgress
            };
            return Err(DialogFailure::reject_on(reason, dialog_id));
        }
        if version != dialog.version {
            return Err(DialogFailure::reject_on(
                PlayerTradeReject::StaleVersion,
                dialog_id,
            ));
        }
        idx
    };
    {
        let dialog = world.player_trade_dialogs.get(dialog_id).unwrap();
        if let Err(reason) = ensure_live_parties(world, dialog, actor) {
            let fatal = reason;
            let (failure, _) = abort_on_bad_stand(world, dialog_id, actor, fatal);
            return Err(failure);
        }
        if ensure_range(world, dialog, radius).is_err() {
            let (failure, _) =
                abort_on_bad_stand(world, dialog_id, actor, PlayerTradeReject::OutOfRange);
            return Err(failure);
        }
    }
    let request = {
        let dialog = world.player_trade_dialogs.get(dialog_id).unwrap();
        match build_commit_request(world, quests, dialog) {
            Ok(request) => request,
            Err(reason) => {
                return match reason {
                    PlayerTradeReject::OutOfRange
                    | PlayerTradeReject::ActorDead
                    | PlayerTradeReject::PartnerUnavailable
                    | PlayerTradeReject::UnknownPlayer => {
                        let (failure, _) = abort_on_bad_stand(world, dialog_id, actor, reason);
                        Err(failure)
                    }
                    _ => Err(DialogFailure::reject_on(reason, dialog_id)),
                };
            }
        }
    };
    world
        .player_trade_dialogs
        .get_mut(dialog_id)
        .unwrap()
        .confirmed[idx] = true;
    let both = {
        let dialog = world.player_trade_dialogs.get(dialog_id).unwrap();
        dialog.confirmed[0] && dialog.confirmed[1]
    };
    if !both {
        let dialog = world.player_trade_dialogs.get(dialog_id).unwrap();
        let view = dialog_view(world, dialog);
        let partner = dialog.parties[1 - idx].clone();
        return Ok(DialogOutcome {
            actor_msg: Some(ok_action(
                PlayerTradeAction::Confirm,
                dialog_id,
                view.clone(),
            )),
            partner_msgs: vec![(partner, push_event("confirmed", dialog_id, Some(view)))],
            commit: None,
        });
    }
    {
        let dialog = world.player_trade_dialogs.get_mut(dialog_id).unwrap();
        dialog.state = DialogState::Committing;
        dialog.commit_id = Some(request.commit_id.clone());
    }
    Ok(DialogOutcome {
        actor_msg: None,
        partner_msgs: Vec::new(),
        commit: Some(request),
    })
}

/// Abbrechen: bricht den noch nicht verbindlich gestarteten Handel ab und
/// informiert den Partner. Während `Committing`/`PendingCommit` wird nichts
/// verworfen (bestehende Wiederherstellungspflicht des Commit-Pfads).
pub fn cancel_dialog(
    world: &mut World,
    now: Instant,
    actor: &str,
    dialog_id: &str,
) -> Result<DialogOutcome, DialogFailure> {
    let (idx, state) = {
        let (idx, dialog) = resolve_open_dialog(world, now, actor, dialog_id)?;
        (idx, dialog.state)
    };
    match state {
        DialogState::Committing | DialogState::PendingCommit => {
            let dialog = world.player_trade_dialogs.get(dialog_id).unwrap();
            let view = dialog_view(world, dialog);
            let _ = idx;
            Ok(DialogOutcome {
                actor_msg: Some(serde_json::json!({
                    "ok": true,
                    "action": PlayerTradeAction::Cancel.as_str(),
                    "event": "commit_in_progress",
                    "reason": PlayerTradeReject::CommitInProgress.reason(),
                    "cancel_deferred": true,
                    "dialog": view,
                    "dialog_id": dialog_id,
                })),
                partner_msgs: Vec::new(),
                commit: None,
            })
        }
        DialogState::Invited | DialogState::Open => {
            let dialog = remove_dialog(world, dialog_id).unwrap();
            let partner = dialog
                .parties
                .iter()
                .find(|p| *p != actor)
                .cloned()
                .unwrap();
            Ok(DialogOutcome {
                actor_msg: Some(serde_json::json!({
                    "ok": true, "action": PlayerTradeAction::Cancel.as_str(),
                    "event": "cancelled", "dialog_id": dialog_id,
                })),
                partner_msgs: vec![(
                    partner,
                    serde_json::json!({"ok": true, "event": "cancelled", "dialog_id": dialog_id}),
                )],
                commit: None,
            })
        }
    }
}

/// Bricht alle nicht verbindlich gestarteten Dialoge eines Charakters ab
/// (Disconnect/Takeover/Tod) und meldet sie dem Partner. Dialoge, deren
/// Inhalt der Commit-Pfad bereits verbindlich festhält (`Committing` mit
/// vorbereitetem Eintrag, `PendingCommit`), bleiben erhalten.
pub fn abort_for(
    world: &mut World,
    char_id: &str,
    reason: AbortReason,
) -> Vec<(String, serde_json::Value)> {
    let dialog_id = match world.player_trade_by_char.get(char_id).cloned() {
        Some(id) => id,
        None => return Vec::new(),
    };
    let Some(dialog) = world.player_trade_dialogs.get(&dialog_id).cloned() else {
        world.player_trade_by_char.remove(char_id);
        return Vec::new();
    };
    let retained = matches!(
        dialog.state,
        DialogState::Committing | DialogState::PendingCommit
    ) || dialog
        .commit_id
        .as_ref()
        .is_some_and(|id| world.prepared_trades.contains_key(id));
    if retained {
        return Vec::new();
    }
    remove_dialog(world, &dialog_id);
    dialog
        .parties
        .iter()
        .filter(|p| *p != char_id)
        .map(|p| (p.clone(), push_abort(reason.reason(), &dialog_id)))
        .collect()
}

/// Bricht abgelaufene Einladungen (60 s, monotone Zeit) auch ohne weitere
/// Nachrichten ab und meldet sie beiden Seiten.
pub fn sweep_expired(world: &mut World, now: Instant) -> Vec<(String, serde_json::Value)> {
    let expired: Vec<String> = world
        .player_trade_dialogs
        .iter()
        .filter(|(_, d)| d.state == DialogState::Invited && now >= d.expires_at)
        .map(|(id, _)| id.clone())
        .collect();
    let mut notes = Vec::new();
    for id in expired {
        if let Some(dialog) = remove_dialog(world, &id) {
            for party in &dialog.parties {
                notes.push((
                    party.clone(),
                    serde_json::json!({
                        "ok": true, "event": "expired",
                        "reason": PlayerTradeReject::InviteExpired.reason(),
                        "dialog_id": id,
                    }),
                ));
            }
        }
    }
    notes
}

/// Bricht Dialoge mit fehlenden/toten oder außer Reichweite geratenen
/// Parteien ab (Bewegungs-/Todes-/Login-Pfade rufen gezielt auf; der
/// Server-Tick kehrt hierüber zusätzlich).
pub fn sweep_stand(world: &mut World, radius: f64) -> Vec<(String, serde_json::Value)> {
    let ids: Vec<String> = world.player_trade_dialogs.keys().cloned().collect();
    let mut notes = Vec::new();
    for id in ids {
        let Some(dialog) = world.player_trade_dialogs.get(&id).cloned() else {
            continue;
        };
        if matches!(
            dialog.state,
            DialogState::Committing | DialogState::PendingCommit
        ) {
            continue;
        }
        let alive = dialog
            .parties
            .iter()
            .all(|p| world.players.get(p).is_some_and(|pl| pl.hp > 0));
        let in_range = dialog.parties.iter().all(|p| world.players.contains_key(p)) && {
            let [a, b] = &dialog.parties;
            match (world.players.get(a), world.players.get(b)) {
                (Some(pa), Some(pb)) => dist(pa.x, pa.y, pb.x, pb.y) <= radius,
                _ => false,
            }
        };
        if alive && in_range {
            continue;
        }
        let reason = if alive {
            PlayerTradeReject::OutOfRange.reason()
        } else {
            PlayerTradeReject::PartnerUnavailable.reason()
        };
        remove_dialog(world, &id);
        for party in &dialog.parties {
            if world.players.contains_key(party) {
                notes.push((party.clone(), push_abort(reason, &id)));
            }
        }
    }
    notes
}

/// Server-Tick für Handelsdialoge: Einladungsfristen plus Standprüfung
/// (Reichweite/Leben/Anwesenheit). Reine World-Mutation; der Aufrufer
/// sendet die Notizen (`send_notes`).
pub fn player_trade_tick(
    world: &mut World,
    radius: f64,
    now: Instant,
) -> Vec<(String, serde_json::Value)> {
    let mut notes = sweep_expired(world, now);
    notes.extend(sweep_stand(world, radius));
    notes
}

/// Abschluss nach bestätigter Spool-Dauerhaftigkeit und gemeinsamer
/// RAM-Übernahme: Der Handler ruft dies erst nach `Ok` aus dem bestehenden
/// Commit-Pfad auf. Entfernt den Dialog, gibt Slots frei und liefert je
/// Partei die Erfolgsdaten (eigener neuer Idia-Stand; keine fremden
/// Kontodaten).
pub fn commit_succeeded(
    world: &mut World,
    dialog_id: &str,
) -> Option<(String, Vec<(String, i64)>)> {
    let dialog = remove_dialog(world, dialog_id)?;
    let commit_id = dialog.commit_id.clone().unwrap_or_default();
    let mut balances = Vec::new();
    for party in &dialog.parties {
        let idia = world.players.get(party).map(|p| p.idia).unwrap_or(0);
        balances.push((party.clone(), idia));
    }
    Some((commit_id, balances))
}

/// Unklare Veröffentlichung wird als pending behandelt: keine falsche
/// Erfolgs- oder Rücknahmemeldung; der Dialog bleibt bis zum Abschluss der
/// bestehenden Wiederherstellung bestehen.
pub fn commit_uncertain(world: &mut World, dialog_id: &str) -> Option<Vec<String>> {
    let dialog = world.player_trade_dialogs.get_mut(dialog_id)?;
    if dialog.state != DialogState::Committing {
        return None;
    }
    dialog.state = DialogState::PendingCommit;
    Some(dialog.parties.to_vec())
}

/// Abgelehnter Abschluss verändert weder Inventar, Idia, Lifecycle noch
/// Dirty-State (garantiert der bestehende Commit-Pfad): der Dialog bleibt
/// mit Angeboten, Version und Bestätigungen geöffnet. Ist eine Partei
/// inzwischen weg, wird der Dialog entfernt und die verbliebene Seite
/// informiert.
pub fn commit_failed(
    world: &mut World,
    actor: &str,
    dialog_id: &str,
) -> Option<(serde_json::Value, Vec<(String, serde_json::Value)>)> {
    let dialog = world.player_trade_dialogs.get(dialog_id)?.clone();
    let both_present = dialog.parties.iter().all(|p| world.players.contains_key(p));
    if !both_present {
        remove_dialog(world, dialog_id);
        let mut partners = Vec::new();
        for party in &dialog.parties {
            if party != actor && world.players.contains_key(party) {
                partners.push((party.clone(), push_abort("partner_unavailable", dialog_id)));
            }
        }
        let actor_msg = serde_json::json!({
            "ok": false,
            "action": PlayerTradeAction::Confirm.as_str(),
            "reason": PlayerTradeReject::PartnerUnavailable.reason(),
            "dialog_id": dialog_id,
        });
        return Some((actor_msg, partners));
    }
    {
        let dialog = world.player_trade_dialogs.get_mut(dialog_id)?;
        dialog.state = DialogState::Open;
        dialog.commit_id = None;
    }
    let dialog = world.player_trade_dialogs.get(dialog_id)?;
    let view = dialog_view(world, dialog);
    let actor_msg = serde_json::json!({
        "ok": false,
        "action": PlayerTradeAction::Confirm.as_str(),
        "reason": PlayerTradeReject::CommitFailed.reason(),
        "dialog": view,
        "dialog_id": dialog_id,
    });
    let mut partners = Vec::new();
    for party in &dialog.parties {
        if party != actor {
            let mut v = push_event("failed", dialog_id, Some(view.clone()));
            v["reason"] =
                serde_json::Value::String(PlayerTradeReject::CommitFailed.reason().into());
            partners.push((party.clone(), v));
        }
    }
    Some((actor_msg, partners))
}

/// Baut die stabile Fehlerantwort für den Auslöser (`seq` spiegelt der
/// Handler zurück).
pub fn failure_payload(failure: &DialogFailure, action: PlayerTradeAction) -> serde_json::Value {
    let mut v = serde_json::json!({"ok": false, "reason": failure.reason.reason()});
    v["action"] = serde_json::Value::String(action.as_str().to_string());
    if let Some(id) = &failure.dialog_id {
        v["dialog_id"] = serde_json::Value::String(id.clone());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::time::Instant;
    use tokio::sync::mpsc;

    use crate::item::{BindingState, ItemCategory, ItemModifiers};
    use crate::quest::{
        CharacterQuestState, ObjectiveType, QuestDefinition, QuestObjective, QuestService,
        QuestState,
    };

    const RADIUS: f64 = 5.0;

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

    fn potion_def() -> crate::item::ItemDefinition {
        let mut d =
            crate::item::ItemDefinition::new("hp_potion", "Heiltrank", ItemCategory::Potion);
        d.max_stack = 20;
        d
    }

    fn sword_def() -> crate::item::ItemDefinition {
        crate::item::ItemDefinition::new("eisenschwert", "Eisenschwert", ItemCategory::Weapon)
    }

    fn bop_def() -> crate::item::ItemDefinition {
        let mut d =
            crate::item::ItemDefinition::new("bop_schwert", "BoP-Schwert", ItemCategory::Weapon);
        d.binding_rule = crate::item::BindingRule::BindOnPickup;
        d
    }

    fn relic_def() -> crate::item::ItemDefinition {
        let mut d =
            crate::item::ItemDefinition::new("quest_relic", "Questrelikt", ItemCategory::QuestItem);
        d.max_stack = 10;
        d
    }

    fn defs() -> HashMap<String, crate::item::ItemDefinition> {
        let mut m = HashMap::new();
        m.insert("hp_potion".into(), potion_def());
        m.insert("eisenschwert".into(), sword_def());
        m.insert("bop_schwert".into(), bop_def());
        m.insert("quest_relic".into(), relic_def());
        m
    }

    /// Welt mit zwei Nachbarn (Abstand 3 m) plus Definitionen, ohne Items.
    fn world_pair() -> (World, QuestService) {
        let mut w = World::new();
        let (a, _) = trader("1", 0.0, 0.0, 100);
        let (b, _) = trader("2", 3.0, 0.0, 50);
        w.players.insert("1".into(), a);
        w.players.insert("2".into(), b);
        w.by_conn.insert(7, "1".into());
        w.by_conn.insert(8, "2".into());
        w.item_definitions = defs();
        (w, QuestService::new())
    }

    /// Legt eine Instanz mit fester UUID in den ersten freien Basisslot.
    fn place(w: &mut World, char: &str, uuid: &str, item_id: &str, count: i64) {
        let mut inst = crate::item::ItemInstance::new(uuid, item_id, ItemModifiers::default());
        inst.count = count;
        let inv = &mut w.players.get_mut(char).unwrap().inventory;
        let slot = inv.base_slots.iter_mut().find(|s| s.is_none()).unwrap();
        *slot = Some(inst);
    }

    fn offer(items: &[(&str, i64)], idia: i64) -> DialogOffer {
        DialogOffer {
            items: items
                .iter()
                .map(|(u, c)| OfferedItem {
                    item_uuid: (*u).to_string(),
                    count: *c,
                })
                .collect(),
            idia,
        }
    }

    fn collect_def(target: &str) -> QuestDefinition {
        QuestDefinition {
            id: "q_collect".into(),
            title_key: "t".into(),
            description_key: "d".into(),
            objectives: vec![QuestObjective {
                id: "o_collect".into(),
                kind: ObjectiveType::Collect,
                target: target.into(),
                required: 3,
            }],
            min_level: 1,
            requires: vec![],
            repeatable: false,
        }
    }

    fn set_quest(w: &mut World, char: &str, qid: &str, state: QuestState) {
        w.players.get_mut(char).unwrap().quests.insert(
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

    /// Offener Dialog a→b (Einladung + Annahme), gibt die Dialog-ID zurück.
    fn open_dialog(w: &mut World, _quests: &QuestService, now: Instant) -> String {
        let out = request_dialog(w, now, RADIUS, "1", "2").expect("request gelingt");
        let id = out.actor_msg.as_ref().unwrap()["dialog_id"]
            .as_str()
            .unwrap()
            .to_string();
        accept_dialog(w, now, RADIUS, "2", &id).expect("accept gelingt");
        id
    }

    fn reason_of(f: DialogFailure) -> &'static str {
        f.reason.reason()
    }

    #[test]
    fn action_parse_is_explicit() {
        assert_eq!(
            PlayerTradeAction::parse("request"),
            Some(PlayerTradeAction::Request)
        );
        assert_eq!(
            PlayerTradeAction::parse("accept"),
            Some(PlayerTradeAction::Accept)
        );
        assert_eq!(
            PlayerTradeAction::parse("decline"),
            Some(PlayerTradeAction::Decline)
        );
        assert_eq!(
            PlayerTradeAction::parse("offer"),
            Some(PlayerTradeAction::Offer)
        );
        assert_eq!(
            PlayerTradeAction::parse("confirm"),
            Some(PlayerTradeAction::Confirm)
        );
        assert_eq!(
            PlayerTradeAction::parse("cancel"),
            Some(PlayerTradeAction::Cancel)
        );
        assert_eq!(PlayerTradeAction::parse("REQUEST"), None);
        assert_eq!(PlayerTradeAction::parse(""), None);
        assert_eq!(PlayerTradeAction::parse("handel"), None);
        assert_eq!(PlayerTradeAction::Request.as_str(), "request");
        assert_eq!(commit_id_for_dialog(7, 3), "ptd7v3");
    }

    #[test]
    fn protocol_ids_whitelist_and_rare_classification() {
        use crate::protocol::{c2s, s2c};
        assert_eq!(c2s::PLAYER_TRADE, 21);
        assert_eq!(s2c::PLAYER_TRADE, 22);
        assert!(crate::security::is_known_c2s(c2s::PLAYER_TRADE));
        assert!(matches!(
            crate::security::MsgClass::of(c2s::PLAYER_TRADE),
            crate::security::MsgClass::Rare
        ));
    }

    #[test]
    fn request_accept_happy_path_and_busy_rules() {
        let (mut w, _quests) = world_pair();
        let t0 = Instant::now();
        let out = request_dialog(&mut w, t0, RADIUS, "1", "2").expect("request gelingt");
        let id = out.actor_msg.unwrap()["dialog_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(out.partner_msgs.len(), 1);
        assert_eq!(out.partner_msgs[0].0, "2");
        assert_eq!(out.partner_msgs[0].1["event"], "invited");
        // Beide Seiten sind belegt (Einladung zählt mit), auch umgekehrt.
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "2", "1").unwrap_err()),
            "dialog_busy"
        );
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "1", "2").unwrap_err()),
            "dialog_busy"
        );
        // Dritter Charakter ohne Dialog kann niemanden Belegten einladen.
        let (c, _) = trader("3", 1.0, 0.0, 0);
        w.players.insert("3".into(), c);
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "3", "1").unwrap_err()),
            "dialog_busy"
        );
        // Nur der Eingeladene nimmt an; der Einladende nutzt cancel.
        assert_eq!(
            reason_of(accept_dialog(&mut w, t0, RADIUS, "1", &id).unwrap_err()),
            "not_invited"
        );
        let out = accept_dialog(&mut w, t0, RADIUS, "2", &id).expect("accept gelingt");
        assert_eq!(out.actor_msg.unwrap()["dialog"]["state"], "open");
        assert_eq!(out.partner_msgs[0].0, "1");
        assert_eq!(out.partner_msgs[0].1["event"], "opened");
        // Nach Annahme kein zweiter Dialog.
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "1", "3").unwrap_err()),
            "dialog_busy"
        );
    }

    #[test]
    fn request_guards_self_unknown_range_dead_frozen() {
        let (mut w, _quests) = world_pair();
        let t0 = Instant::now();
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "1", "1").unwrap_err()),
            "self_trade"
        );
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "1", "ghost").unwrap_err()),
            "unknown_target"
        );
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "ghost", "2").unwrap_err()),
            "unknown_player"
        );
        w.players.get_mut("2").unwrap().x = 500.0;
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "1", "2").unwrap_err()),
            "out_of_range"
        );
        w.players.get_mut("2").unwrap().x = 3.0;
        w.players.get_mut("2").unwrap().hp = 0;
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "1", "2").unwrap_err()),
            "partner_unavailable"
        );
        w.players.get_mut("2").unwrap().hp = 100;
        w.players.get_mut("1").unwrap().hp = 0;
        assert_eq!(
            reason_of(request_dialog(&mut w, t0, RADIUS, "1", "2").unwrap_err()),
            "actor_dead"
        );
    }

    #[test]
    fn invite_expiry_lazy_and_sweep() {
        let (mut w, _quests) = world_pair();
        let t0 = Instant::now();
        let out = request_dialog(&mut w, t0, RADIUS, "1", "2").expect("request gelingt");
        let id = out.actor_msg.unwrap()["dialog_id"]
            .as_str()
            .unwrap()
            .to_string();
        // Vor Fristablauf: Annahme möglich, kein Sweep-Befund.
        assert!(sweep_expired(&mut w, t0 + INVITE_TTL - Duration::from_secs(1)).is_empty());
        // Nach 60 s: Tick-Sweep meldet beiden Seiten und gibt Slots frei.
        let notes = sweep_expired(&mut w, t0 + INVITE_TTL + Duration::from_secs(1));
        assert_eq!(notes.len(), 2);
        assert!(notes
            .iter()
            .all(|(_, v)| v["event"] == "expired" && v["reason"] == "invite_expired"));
        assert!(w.player_trade_dialogs.is_empty());
        assert!(w.player_trade_by_char.is_empty());
        // Erneute Einladung, dann faule Fristprüfung bei der Operation.
        let out = request_dialog(&mut w, t0, RADIUS, "1", "2").expect("request gelingt");
        let id2 = out.actor_msg.unwrap()["dialog_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(id, id2);
        let late = t0 + INVITE_TTL + Duration::from_secs(1);
        assert_eq!(
            reason_of(accept_dialog(&mut w, late, RADIUS, "2", &id2).unwrap_err()),
            "invite_expired"
        );
        assert!(w.player_trade_dialogs.is_empty());
    }

    #[test]
    fn offer_bumps_version_and_resets_both_confirmations() {
        let (mut w, quests) = world_pair();
        let t0 = Instant::now();
        place(&mut w, "1", "pot-a", "hp_potion", 10);
        place(&mut w, "2", "sword-b", "eisenschwert", 1);
        let id = open_dialog(&mut w, &quests, t0);
        set_offer(
            &mut w,
            &quests,
            t0,
            RADIUS,
            "1",
            &id,
            &offer(&[("pot-a", 3)], 10),
        )
        .expect("angebot a");
        assert_eq!(w.player_trade_dialogs[&id].version, 1);
        set_offer(
            &mut w,
            &quests,
            t0,
            RADIUS,
            "2",
            &id,
            &offer(&[("sword-b", 1)], 0),
        )
        .expect("angebot b");
        assert_eq!(w.player_trade_dialogs[&id].version, 2);
        confirm_dialog(&mut w, &quests, t0, RADIUS, "1", &id, 2).expect("confirm a");
        assert!(w.player_trade_dialogs[&id].confirmed[0]);
        // Identische Wiederholung: No-Op ohne Versionswechsel.
        set_offer(
            &mut w,
            &quests,
            t0,
            RADIUS,
            "1",
            &id,
            &offer(&[("pot-a", 3)], 10),
        )
        .expect("identisch");
        assert_eq!(w.player_trade_dialogs[&id].version, 2);
        assert!(w.player_trade_dialogs[&id].confirmed[0]);
        // Tatsächliche Änderung: Version +1, BEIDE Bestätigungen weg.
        set_offer(
            &mut w,
            &quests,
            t0,
            RADIUS,
            "1",
            &id,
            &offer(&[("pot-a", 4)], 10),
        )
        .expect("änderung");
        let d = &w.player_trade_dialogs[&id];
        assert_eq!(d.version, 3);
        assert_eq!(d.confirmed, [false, false]);
    }

    #[test]
    fn stale_version_confirm_is_rejected() {
        let (mut w, quests) = world_pair();
        let t0 = Instant::now();
        place(&mut w, "1", "pot-a", "hp_potion", 10);
        let id = open_dialog(&mut w, &quests, t0);
        set_offer(
            &mut w,
            &quests,
            t0,
            RADIUS,
            "1",
            &id,
            &offer(&[("pot-a", 3)], 0),
        )
        .expect("angebot");
        assert_eq!(
            reason_of(confirm_dialog(&mut w, &quests, t0, RADIUS, "1", &id, 0).unwrap_err()),
            "stale_version"
        );
        assert!(!w.player_trade_dialogs[&id].confirmed[0]);
        confirm_dialog(&mut w, &quests, t0, RADIUS, "1", &id, 1).expect("aktuell");
        assert!(w.player_trade_dialogs[&id].confirmed[0]);
    }

    #[test]
    fn offer_validation_rejects_shapes() {
        let (mut w, quests) = world_pair();
        let t0 = Instant::now();
        place(&mut w, "1", "pot-a", "hp_potion", 10);
        let id = open_dialog(&mut w, &quests, t0);
        // Doppelte UUID.
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 1), ("pot-a", 1)], 0)
                )
                .unwrap_err()
            ),
            "invalid_offer"
        );
        // Menge 0.
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 0)], 0)
                )
                .unwrap_err()
            ),
            "invalid_offer"
        );
        // Negatives Idia.
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 1)], -5)
                )
                .unwrap_err()
            ),
            "invalid_offer"
        );
        // Fremd-UUID (gehört b).
        place(&mut w, "2", "pot-b", "hp_potion", 5);
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-b", 1)], 0)
                )
                .unwrap_err()
            ),
            "item_unavailable"
        );
        // Unbekannte UUID.
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("nichts", 1)], 0)
                )
                .unwrap_err()
            ),
            "item_unavailable"
        );
        // Mehr als der Stack hergibt.
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 11)], 0)
                )
                .unwrap_err()
            ),
            "not_enough_items"
        );
        // Mehr Idia als Bestand.
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 1)], 500)
                )
                .unwrap_err()
            ),
            "insufficient_idia"
        );
        // Fehlgeschlagene Angebote verändern weder Version noch Bestand.
        assert_eq!(w.player_trade_dialogs[&id].version, 0);
        assert_eq!(
            w.players["1"].inventory.base_slots[0]
                .as_ref()
                .unwrap()
                .count,
            10
        );
    }

    #[test]
    fn equipment_buffer_binding_quest_guards() {
        let (mut w, mut quests) = world_pair();
        let t0 = Instant::now();
        place(&mut w, "1", "sword-a", "eisenschwert", 1);
        place(&mut w, "1", "bop-a", "bop_schwert", 1);
        place(&mut w, "1", "relic-a", "quest_relic", 2);
        place(&mut w, "1", "pot-a", "hp_potion", 10);
        let id = open_dialog(&mut w, &quests, t0);
        // Ausgerüstetes Exemplar ist kein Angebot.
        {
            let pos = w
                .players
                .get("1")
                .unwrap()
                .inventory
                .base_slots
                .iter()
                .position(|s| s.as_ref().is_some_and(|it| it.item_uuid == "sword-a"))
                .unwrap();
            let taken = w.players.get_mut("1").unwrap().inventory.base_slots[pos]
                .take()
                .unwrap();
            w.players
                .get_mut("1")
                .unwrap()
                .inventory
                .equipped
                .insert(crate::inventory::EquipSlot::MainHand, taken);
        }
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("sword-a", 1)], 0)
                )
                .unwrap_err()
            ),
            "item_unavailable"
        );
        // Puffer-Exemplar ist kein Angebot.
        {
            let p = w.players.get_mut("1").unwrap();
            let mut buf =
                crate::item::ItemInstance::new("buf-a", "hp_potion", ItemModifiers::default());
            buf.count = 2;
            p.inventory.buffer.push(Some(buf));
        }
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("buf-a", 2)], 0)
                )
                .unwrap_err()
            ),
            "item_unavailable"
        );
        // Definitionsbindung (BindOnPickup-Regel) sperrt, …
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("bop-a", 1)], 0)
                )
                .unwrap_err()
            ),
            "bound_item"
        );
        // … ebenso gebundener Instanzzustand bei freier Definition.
        w.players
            .get_mut("1")
            .unwrap()
            .inventory
            .base_slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|it| it.item_uuid == "pot-a")
            .unwrap()
            .binding = BindingState::Bound;
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 1)], 0)
                )
                .unwrap_err()
            ),
            "bound_item"
        );
        // QuestItem-Kategorie ist immer gesperrt — auch ganz ohne Quest.
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("relic-a", 1)], 0)
                )
                .unwrap_err()
            ),
            "quest_item_protected"
        );
        // ACTIVE-Questschutz für normale Kategorien …
        quests.register(collect_def("hp_potion")).unwrap();
        set_quest(&mut w, "1", "q_collect", QuestState::Active);
        w.players
            .get_mut("1")
            .unwrap()
            .inventory
            .base_slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|it| it.item_uuid == "pot-a")
            .unwrap()
            .binding = BindingState::Tradeable;
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 1)], 0)
                )
                .unwrap_err()
            ),
            "quest_item_protected"
        );
        // … und unauflösbare ACTIVE-Definition bleibt fail-closed.
        set_quest(&mut w, "1", "ghost_quest", QuestState::Active);
        assert_eq!(
            reason_of(
                set_offer(
                    &mut w,
                    &quests,
                    t0,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("bop-a", 1)], 0)
                )
                .unwrap_err()
            ),
            "quest_data_unavailable"
        );
    }

    #[test]
    fn decline_cancel_and_deferred_cancel() {
        let (mut w, quests) = world_pair();
        let t0 = Instant::now();
        // decline durch den Eingeladenen.
        let out = request_dialog(&mut w, t0, RADIUS, "1", "2").expect("request");
        let id = out.actor_msg.unwrap()["dialog_id"]
            .as_str()
            .unwrap()
            .to_string();
        let out = decline_dialog(&mut w, t0, "2", &id).expect("decline");
        assert_eq!(out.partner_msgs[0].0, "1");
        assert_eq!(out.partner_msgs[0].1["event"], "declined");
        assert!(w.player_trade_dialogs.is_empty());
        // cancel durch den Einladenden im Zustand Invited.
        let out = request_dialog(&mut w, t0, RADIUS, "1", "2").expect("request");
        let id = out.actor_msg.unwrap()["dialog_id"]
            .as_str()
            .unwrap()
            .to_string();
        let out = cancel_dialog(&mut w, t0, "1", &id).expect("cancel");
        assert_eq!(out.partner_msgs[0].1["event"], "cancelled");
        assert!(w.player_trade_dialogs.is_empty());
        // cancel während Committing verwirft nichts (deferred).
        let id = open_dialog(&mut w, &quests, t0);
        {
            let d = w.player_trade_dialogs.get_mut(&id).unwrap();
            d.state = DialogState::Committing;
            d.commit_id = Some(commit_id_for_dialog(d.seq_no, d.version));
        }
        let out = cancel_dialog(&mut w, t0, "1", &id).expect("deferred");
        assert_eq!(out.actor_msg.unwrap()["cancel_deferred"], true);
        assert!(out.partner_msgs.is_empty());
        assert!(w.player_trade_dialogs.contains_key(&id));
    }

    #[test]
    fn abort_disconnect_takeover_death_retains_binding() {
        // Eigene Welt mit beobachtbaren Partnerkanälen.
        let mut w = World::new();
        let (a, mut ra) = trader("1", 0.0, 0.0, 100);
        let (b, _) = trader("2", 3.0, 0.0, 50);
        w.players.insert("1".into(), a);
        w.players.insert("2".into(), b);
        w.by_conn.insert(7, "1".into());
        w.by_conn.insert(8, "2".into());
        w.item_definitions = defs();
        let quests = QuestService::new();
        let t0 = Instant::now();
        // Disconnect bricht ab und informiert den Partner.
        let _ = open_dialog(&mut w, &quests, t0);
        let notes = abort_for(&mut w, "1", AbortReason::Disconnect);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, "2");
        assert_eq!(notes[0].1["reason"], "disconnect");
        assert!(w.player_trade_dialogs.is_empty());
        // Takeover-Pfad (commit_login) bricht ebenfalls ab und sendet an a.
        let id = open_dialog(&mut w, &quests, t0);
        let (cand, _) = trader("2", 3.0, 0.0, 50);
        let (ntx, _) = mpsc::unbounded_channel::<String>();
        let outcome = crate::world::commit_login(
            &mut w,
            9,
            crate::world::Player { tx: ntx, ..cand },
            crate::world::ConnectionFields {
                tx: mpsc::unbounded_channel().0,
                session_id: "s".into(),
                lang: "de".into(),
            },
        )
        .expect("takeover");
        assert!(matches!(
            outcome,
            crate::world::CommitOutcome::Takeover { old_conn_id: 8 }
        ));
        assert!(!w.player_trade_dialogs.contains_key(&id));
        let frame: crate::protocol::Frame =
            serde_json::from_str(&ra.try_recv().expect("Partnernote an a")).expect("Frame");
        assert_eq!(frame.msg_type, crate::protocol::s2c::PLAYER_TRADE);
        assert_eq!(frame.data["event"], "aborted");
        assert_eq!(frame.data["reason"], "takeover");
        // Tod bricht ab (Note an den Partner).
        let _ = open_dialog(&mut w, &quests, t0);
        crate::combat::ability::on_death(&mut w, "1", true);
        assert!(w.player_trade_dialogs.is_empty());
        // Verbindlich gestarteter Dialog bleibt erhalten (Retention).
        let id = open_dialog(&mut w, &quests, t0);
        {
            let d = w.player_trade_dialogs.get_mut(&id).unwrap();
            d.state = DialogState::Committing;
            d.commit_id = Some(commit_id_for_dialog(d.seq_no, d.version));
        }
        assert!(abort_for(&mut w, "1", AbortReason::Disconnect).is_empty());
        assert!(w.player_trade_dialogs.contains_key(&id));
    }

    #[test]
    fn range_loss_sweep_aborts_and_notifies() {
        let (mut w, quests) = world_pair();
        let t0 = Instant::now();
        let _ = open_dialog(&mut w, &quests, t0);
        w.players.get_mut("2").unwrap().x = 50.0;
        let notes = sweep_stand(&mut w, RADIUS);
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|(_, v)| v["reason"] == "out_of_range"));
        assert!(w.player_trade_dialogs.is_empty());
        // Toter Partner: Überlebender wird informiert.
        w.players.get_mut("2").unwrap().x = 3.0;
        w.players.get_mut("2").unwrap().hp = 100;
        let _ = open_dialog(&mut w, &quests, t0);
        w.players.get_mut("2").unwrap().hp = 0;
        let notes = sweep_stand(&mut w, RADIUS);
        assert!(notes
            .iter()
            .any(|(to, v)| to == "1" && v["reason"] == "partner_unavailable"));
        assert!(w.player_trade_dialogs.is_empty());
    }

    // --- Commit-Pfad-Tests (modellierter Datei-Spool im Tempdir, KEINE
    // MariaDB; der Cashflow läuft über den bestehenden Commit-Pfad). ---

    static NEXT_TMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn modeled_runtime(tag: &str) -> (crate::spool::PersistRuntime, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "ptd-{tag}-{}-{}",
            std::process::id(),
            NEXT_TMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        (
            crate::spool::PersistRuntime::new(&path, "sword").unwrap(),
            path,
        )
    }

    async fn shared_pair() -> (crate::world::Shared, QuestService) {
        let shared = crate::world::new_shared();
        let (mut a, _) = trader("1", 0.0, 0.0, 100);
        let (mut b, _) = trader("2", 3.0, 0.0, 50);
        let mut pot =
            crate::item::ItemInstance::new("pot-a", "hp_potion", ItemModifiers::default());
        pot.count = 10;
        a.inventory.base_slots[0] = Some(pot);
        let mut sword =
            crate::item::ItemInstance::new("sword-b", "eisenschwert", ItemModifiers::default());
        sword.count = 1;
        b.inventory.base_slots[0] = Some(sword);
        let mut world = shared.lock().await;
        world.item_definitions = defs();
        world.players.insert("1".into(), a);
        world.players.insert("2".into(), b);
        world.by_conn.insert(7, "1".into());
        world.by_conn.insert(8, "2".into());
        drop(world);
        (shared, QuestService::new())
    }

    /// Fährt einen Dialog bis zur Commit-Absicht (beide bestätigt) und gibt
    /// Dialog-ID plus Request zurück. Der Aufrufer führt den bestehenden
    /// Commit-Pfad aus.
    async fn drive_to_commit(
        shared: &crate::world::Shared,
        quests: &QuestService,
        now: Instant,
    ) -> (String, crate::persist::TradeCommitRequest) {
        let mut world = shared.lock().await;
        let out = request_dialog(&mut world, now, RADIUS, "1", "2").expect("request");
        let id = out.actor_msg.unwrap()["dialog_id"]
            .as_str()
            .unwrap()
            .to_string();
        accept_dialog(&mut world, now, RADIUS, "2", &id).expect("accept");
        set_offer(
            &mut world,
            quests,
            now,
            RADIUS,
            "1",
            &id,
            &offer(&[("pot-a", 3)], 10),
        )
        .expect("angebot a");
        set_offer(
            &mut world,
            quests,
            now,
            RADIUS,
            "2",
            &id,
            &offer(&[("sword-b", 1)], 0),
        )
        .expect("angebot b");
        let version = world.player_trade_dialogs[&id].version;
        assert_eq!(version, 2);
        confirm_dialog(&mut world, quests, now, RADIUS, "1", &id, version).expect("confirm a");
        let out =
            confirm_dialog(&mut world, quests, now, RADIUS, "2", &id, version).expect("confirm b");
        let request = out.commit.expect("Commit-Absicht liegt vor");
        assert_eq!(
            world.player_trade_dialogs[&id].state,
            DialogState::Committing
        );
        drop(world);
        (id, request)
    }

    fn snapshot(
        w: &World,
    ) -> (
        crate::inventory::InventoryState,
        crate::inventory::InventoryState,
        i64,
        i64,
        bool,
        u64,
    ) {
        (
            w.players["1"].inventory.clone(),
            w.players["2"].inventory.clone(),
            w.players["1"].idia,
            w.players["2"].idia,
            w.players["1"].dirty.any() || w.players["2"].dirty.any(),
            w.players["1"].persist_generation + w.players["2"].persist_generation,
        )
    }

    #[tokio::test]
    async fn commit_success_swaps_via_existing_path_without_second_commit() {
        let (runtime, path) = modeled_runtime("success");
        let (shared, quests) = shared_pair().await;
        let now = Instant::now();
        let (id, request) = drive_to_commit(&shared, &quests, now).await;
        let commit_id = request.commit_id.clone();
        crate::persist::commit_trade(runtime.spool(), &shared, request)
            .await
            .expect("Commit über bestehenden Pfad");
        // Erfolg erst nach Dauerhaftigkeit + RAM-Übernahme: Tausch und
        // Bilanzen stehen, Revisionen +1, Receipt liegt vor.
        {
            let mut world = shared.lock().await;
            assert_eq!(world.players["1"].idia, 90);
            assert_eq!(world.players["2"].idia, 60);
            assert_eq!(world.players["1"].inventory.count_of("eisenschwert"), 1);
            assert_eq!(world.players["1"].inventory.count_of("hp_potion"), 7);
            assert_eq!(world.players["2"].inventory.count_of("hp_potion"), 3);
            assert_eq!(world.players["1"].persist_revision, 1);
            assert_eq!(world.players["2"].persist_revision, 1);
            assert!(world.trade_receipts.contains_key(&commit_id));
            let out = commit_succeeded(&mut world, &id).expect("Abschluss");
            assert_eq!(out.0, commit_id);
            assert!(world.player_trade_dialogs.is_empty());
            assert!(world.player_trade_by_char.is_empty());
        }
        // Wiederholte Bestätigung (auch gleiche seq-Ebene): kein Dialog mehr,
        // kein Zweiteffekt — Receipt-Dedup, kein zweiter Commit.
        {
            let mut world = shared.lock().await;
            let before = snapshot(&world);
            assert_eq!(
                reason_of(
                    confirm_dialog(&mut world, &quests, now, RADIUS, "2", &id, 2).unwrap_err()
                ),
                "no_dialog"
            );
            assert_eq!(snapshot(&world), before);
            assert_eq!(world.players["1"].persist_revision, 1);
        }
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn commit_failures_leave_no_partial_mutation() {
        // (a) Inzwischen fehlendes Item.
        {
            let (_, path) = modeled_runtime("missing");
            let (shared, quests) = shared_pair().await;
            let now = Instant::now();
            {
                let mut world = shared.lock().await;
                let out = request_dialog(&mut world, now, RADIUS, "1", "2").expect("request");
                let id = out.actor_msg.unwrap()["dialog_id"]
                    .as_str()
                    .unwrap()
                    .to_string();
                accept_dialog(&mut world, now, RADIUS, "2", &id).expect("accept");
                set_offer(
                    &mut world,
                    &quests,
                    now,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 3)], 10),
                )
                .expect("angebot");
                // Item verschwindet nach dem Angebot (keine Reservierung).
                world.players.get_mut("1").unwrap().inventory.base_slots[0] = None;
                let before = snapshot(&world);
                assert_eq!(
                    reason_of(
                        confirm_dialog(&mut world, &quests, now, RADIUS, "1", &id, 1).unwrap_err()
                    ),
                    "item_unavailable"
                );
                assert_eq!(snapshot(&world), before);
                assert!(world.player_trade_dialogs.contains_key(&id));
            }
            std::fs::remove_dir_all(path).unwrap();
        }
        // (b) Volles Empfängerinventar: Commit scheitert, nichts mutiert.
        {
            let (runtime, path) = modeled_runtime("full");
            let (shared, quests) = shared_pair().await;
            let now = Instant::now();
            {
                let mut world = shared.lock().await;
                let out = request_dialog(&mut world, now, RADIUS, "1", "2").expect("request");
                let id = out.actor_msg.unwrap()["dialog_id"]
                    .as_str()
                    .unwrap()
                    .to_string();
                accept_dialog(&mut world, now, RADIUS, "2", &id).expect("accept");
                // Alle 8 Slots von b mit artfremden Schwertern füllen; b
                // bietet nichts an und soll 3 Tränke empfangen.
                for i in 0..8u32 {
                    let mut s = crate::item::ItemInstance::new(
                        &format!("fill-{i}"),
                        "eisenschwert",
                        ItemModifiers::default(),
                    );
                    s.count = 1;
                    world.players.get_mut("2").unwrap().inventory.base_slots[i as usize] = Some(s);
                }
                set_offer(
                    &mut world,
                    &quests,
                    now,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 3)], 0),
                )
                .expect("angebot");
                set_offer(&mut world, &quests, now, RADIUS, "2", &id, &offer(&[], 0))
                    .expect("leer");
                let version = world.player_trade_dialogs[&id].version;
                confirm_dialog(&mut world, &quests, now, RADIUS, "1", &id, version)
                    .expect("confirm a");
                let out = confirm_dialog(&mut world, &quests, now, RADIUS, "2", &id, version)
                    .expect("confirm b");
                let request = out.commit.expect("Commit-Absicht");
                let before = snapshot(&world);
                drop(world);
                let err = crate::persist::commit_trade(runtime.spool(), &shared, request).await;
                assert!(err.is_err(), "volles Inventar scheitert");
                let mut world = shared.lock().await;
                assert_eq!(snapshot(&world), before);
                let (actor_msg, _) = commit_failed(&mut world, "2", &id).expect("zurück auf offen");
                assert_eq!(actor_msg["reason"], "trade_commit_failed");
                assert_eq!(world.player_trade_dialogs[&id].state, DialogState::Open);
            }
            std::fs::remove_dir_all(path).unwrap();
        }
        // (c) Idia-Unterdeckung nach dem Angebot.
        {
            let (_, path) = modeled_runtime("idia");
            let (shared, quests) = shared_pair().await;
            let now = Instant::now();
            {
                let mut world = shared.lock().await;
                let out = request_dialog(&mut world, now, RADIUS, "1", "2").expect("request");
                let id = out.actor_msg.unwrap()["dialog_id"]
                    .as_str()
                    .unwrap()
                    .to_string();
                accept_dialog(&mut world, now, RADIUS, "2", &id).expect("accept");
                set_offer(
                    &mut world,
                    &quests,
                    now,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 3)], 10),
                )
                .expect("angebot");
                world.players.get_mut("1").unwrap().idia = 5;
                let before = snapshot(&world);
                assert_eq!(
                    reason_of(
                        confirm_dialog(&mut world, &quests, now, RADIUS, "1", &id, 1).unwrap_err()
                    ),
                    "insufficient_idia"
                );
                assert_eq!(snapshot(&world), before);
            }
            std::fs::remove_dir_all(path).unwrap();
        }
        // (d) Idia-Überlauf in der Abschlussarithmetik.
        {
            let (_, path) = modeled_runtime("overflow");
            let (shared, quests) = shared_pair().await;
            let now = Instant::now();
            {
                let mut world = shared.lock().await;
                let out = request_dialog(&mut world, now, RADIUS, "1", "2").expect("request");
                let id = out.actor_msg.unwrap()["dialog_id"]
                    .as_str()
                    .unwrap()
                    .to_string();
                accept_dialog(&mut world, now, RADIUS, "2", &id).expect("accept");
                set_offer(&mut world, &quests, now, RADIUS, "1", &id, &offer(&[], 0))
                    .expect("leer a");
                set_offer(&mut world, &quests, now, RADIUS, "2", &id, &offer(&[], 5))
                    .expect("5 idia");
                world.players.get_mut("1").unwrap().idia = i64::MAX;
                let before = snapshot(&world);
                let version = world.player_trade_dialogs[&id].version;
                // Schon die erste Bestätigung baut die Abschlussarithmetik:
                // MAX - 0 + 5 läuft über.
                assert_eq!(
                    reason_of(
                        confirm_dialog(&mut world, &quests, now, RADIUS, "1", &id, version)
                            .unwrap_err()
                    ),
                    "idia_overflow"
                );
                assert_eq!(snapshot(&world), before);
            }
            std::fs::remove_dir_all(path).unwrap();
        }
        // (e) Questschutz nach dem Angebot (ACTIVE schützt den Trank).
        {
            let (_, path) = modeled_runtime("quest");
            let (shared, mut quests) = shared_pair().await;
            quests.register(collect_def("hp_potion")).unwrap();
            let now = Instant::now();
            {
                let mut world = shared.lock().await;
                let out = request_dialog(&mut world, now, RADIUS, "1", "2").expect("request");
                let id = out.actor_msg.unwrap()["dialog_id"]
                    .as_str()
                    .unwrap()
                    .to_string();
                accept_dialog(&mut world, now, RADIUS, "2", &id).expect("accept");
                set_offer(
                    &mut world,
                    &quests,
                    now,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 3)], 0),
                )
                .expect("angebot");
                set_quest(&mut world, "1", "q_collect", QuestState::Active);
                let before = snapshot(&world);
                assert_eq!(
                    reason_of(
                        confirm_dialog(&mut world, &quests, now, RADIUS, "1", &id, 1).unwrap_err()
                    ),
                    "quest_item_protected"
                );
                assert_eq!(snapshot(&world), before);
            }
            std::fs::remove_dir_all(path).unwrap();
        }
        // (f) Bindung nach dem Angebot (Equip-bindet → Bound).
        {
            let (_, path) = modeled_runtime("bound");
            let (shared, quests) = shared_pair().await;
            let now = Instant::now();
            {
                let mut world = shared.lock().await;
                let out = request_dialog(&mut world, now, RADIUS, "1", "2").expect("request");
                let id = out.actor_msg.unwrap()["dialog_id"]
                    .as_str()
                    .unwrap()
                    .to_string();
                accept_dialog(&mut world, now, RADIUS, "2", &id).expect("accept");
                set_offer(
                    &mut world,
                    &quests,
                    now,
                    RADIUS,
                    "1",
                    &id,
                    &offer(&[("pot-a", 3)], 0),
                )
                .expect("angebot");
                world.players.get_mut("1").unwrap().inventory.base_slots[0]
                    .as_mut()
                    .unwrap()
                    .binding = BindingState::Bound;
                let before = snapshot(&world);
                assert_eq!(
                    reason_of(
                        confirm_dialog(&mut world, &quests, now, RADIUS, "1", &id, 1).unwrap_err()
                    ),
                    "bound_item"
                );
                assert_eq!(snapshot(&world), before);
            }
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    #[tokio::test]
    async fn uncertain_publication_stays_pending_and_recovers() {
        let (runtime, path) = modeled_runtime("pending");
        let (shared, quests) = shared_pair().await;
        let now = Instant::now();
        let (id, request) = drive_to_commit(&shared, &quests, now).await;
        let commit_id = request.commit_id.clone();
        // Unklare Veröffentlichung: Sync-Fehler, vorbereitete Daten bleiben
        // exakt erhalten (Vertrag des bestehenden Commit-Pfads).
        let err = crate::persist::commit_trade_with(
            runtime.spool(),
            &shared,
            request.clone(),
            |_| async { Err("publication failed".into()) },
        )
        .await;
        assert!(err.is_err());
        {
            let world = shared.lock().await;
            assert!(world.prepared_trades.contains_key(&commit_id));
            assert!(!world.economic_mutation_allowed("1"));
            assert_eq!(world.players["1"].idia, 100);
            assert_eq!(world.players["1"].persist_revision, 0);
        }
        // Als pending behandeln: keine Erfolgs-/Rücknahmemeldung, Dialog
        // bleibt; Cancel verwirft nichts; Disconnect verweigert den
        // Cleanup (bestehende Wiederherstellungspflicht).
        {
            let mut world = shared.lock().await;
            let parties = commit_uncertain(&mut world, &id).expect("pending");
            assert_eq!(parties.len(), 2);
            assert_eq!(
                world.player_trade_dialogs[&id].state,
                DialogState::PendingCommit
            );
            let out = cancel_dialog(&mut world, now, "1", &id).expect("deferred");
            assert_eq!(out.actor_msg.unwrap()["cancel_deferred"], true);
            assert!(abort_for(&mut world, "1", AbortReason::Disconnect).is_empty());
            assert!(crate::world::disconnect_conn(&mut world, 8).is_none());
        }
        // Wiederherstellung ausschließlich über den bestehenden Vertrag:
        // identischer Retry löst die retained Vorbereitung auf.
        crate::persist::commit_trade(runtime.spool(), &shared, request)
            .await
            .expect("Retry löst retained Vorbereitung auf");
        {
            let mut world = shared.lock().await;
            assert_eq!((world.players["1"].idia, world.players["2"].idia), (90, 60));
            assert_eq!(world.players["2"].inventory.count_of("hp_potion"), 3);
            assert!(world.trade_receipts.contains_key(&commit_id));
            assert!(!world.prepared_trades.contains_key(&commit_id));
            commit_succeeded(&mut world, &id).expect("Abschluss");
            assert!(world.player_trade_dialogs.is_empty());
        }
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn disconnect_during_commit_window_aborts_cleanly() {
        let (runtime, path) = modeled_runtime("window");
        let (shared, quests) = shared_pair().await;
        let now = Instant::now();
        // Beide bestätigt, Commit noch nicht gestartet (Fenster).
        let (id, request) = drive_to_commit(&shared, &quests, now).await;
        {
            let mut world = shared.lock().await;
            // Disconnect des Partners: Dialog bleibt (Committing-Retention),
            // Spieler wird entfernt.
            let notes = {
                let n = abort_for(&mut world, "2", AbortReason::Disconnect);
                crate::world::disconnect_conn(&mut world, 8).expect("cleanup");
                n
            };
            assert!(notes.is_empty());
            assert!(world.player_trade_dialogs.contains_key(&id));
            assert!(!world.players.contains_key("2"));
        }
        // Der Commit scheitert kontrolliert (Charakter weg), ohne Mutation.
        let err = crate::persist::commit_trade(runtime.spool(), &shared, request).await;
        assert!(err.is_err());
        {
            let mut world = shared.lock().await;
            assert_eq!(world.players["1"].inventory.count_of("hp_potion"), 10);
            assert_eq!(world.players["1"].idia, 100);
            // Überlebender wird informiert, Dialog ist weg.
            let (actor_msg, partners) = commit_failed(&mut world, "1", &id).expect("Abbruch");
            assert_eq!(actor_msg["reason"], "partner_unavailable");
            assert!(partners.is_empty());
            assert!(world.player_trade_dialogs.is_empty());
        }
        std::fs::remove_dir_all(path).unwrap();
    }

    /// Verbindliche Nachprüfung unter Gates+Sperre: Ändert sich eine
    /// relevante Voraussetzung zwischen erster Bestätigung und verbindlicher
    /// Vorbereitung, lehnt der bestehende Commit-Pfad ohne wirtschaftliche
    /// Mutation und ohne vorbereiteten Trade ab (ereignisgesteuert: die
    /// Zustandsänderung liegt zwischen Confirm und Vorbereitung; kein Sleep,
    /// keine nachgebaute Pipeline — der Aufruf nutzt
    /// `commit_trade_with_validation` mit dem echten Spool-Write).
    #[tokio::test]
    async fn binding_preparation_rejects_changed_preconditions() {
        async fn rejected_case(tag: &str, mutate: impl FnOnce(&mut World), expected: &str) {
            let (runtime, path) = modeled_runtime(tag);
            let (shared, quests) = shared_pair().await;
            let now = Instant::now();
            let (id, request) = drive_to_commit(&shared, &quests, now).await;
            let version = {
                let world = shared.lock().await;
                world.player_trade_dialogs[&id].version
            };
            {
                let mut world = shared.lock().await;
                mutate(&mut world);
            }
            let spool = runtime.spool();
            let err = crate::persist::commit_trade_with_validation(
                spool,
                &shared,
                request.clone(),
                |w| validate_binding_preparation(w, &quests, RADIUS, &id, version, &request),
                |artifact| async move { spool.write_trade(&artifact) },
            )
            .await
            .unwrap_err();
            assert_eq!(err, expected);
            {
                let world = shared.lock().await;
                // Kein vorbereiteter Trade, keine wirtschaftliche Mutation,
                // Dialog bleibt verbindlich wartend (kein stilles Verwerfen).
                assert!(!world.prepared_trades.contains_key(&request.commit_id));
                assert!(!world.trade_receipts.contains_key(&request.commit_id));
                assert_eq!(world.players["1"].persist_revision, 0);
                assert_eq!(world.players["2"].persist_revision, 0);
                let dialog = &world.player_trade_dialogs[&id];
                assert_eq!(dialog.state, DialogState::Committing);
                assert_eq!(
                    dialog.commit_id.as_deref(),
                    Some(request.commit_id.as_str())
                );
            }
            std::fs::remove_dir_all(path).unwrap();
        }
        // (a) Reichweitenverlust nach Confirm.
        rejected_case(
            "vp-range",
            |w| {
                w.players.get_mut("2").unwrap().x = 50.0;
            },
            "trade parties out of range",
        )
        .await;
        // (b) Tod nach Confirm.
        rejected_case(
            "vp-death",
            |w| {
                w.players.get_mut("2").unwrap().hp = 0;
            },
            "trade party not alive",
        )
        .await;
        // (c) Inzwischen fehlendes Item (keine Reservierung).
        rejected_case(
            "vp-missing",
            |w| {
                w.players.get_mut("1").unwrap().inventory.base_slots[0] = None;
            },
            "trade offer rejected: item_unavailable",
        )
        .await;
        // (d) Idia-Unterdeckung nach Angebot.
        rejected_case(
            "vp-idia",
            |w| {
                w.players.get_mut("1").unwrap().idia = 5;
            },
            "trade offer rejected: insufficient_idia",
        )
        .await;
        // (e) Bindung nach Angebot.
        rejected_case(
            "vp-bound",
            |w| {
                w.players.get_mut("1").unwrap().inventory.base_slots[0]
                    .as_mut()
                    .unwrap()
                    .binding = BindingState::Bound;
            },
            "trade offer rejected: bound_item",
        )
        .await;
        // (f) ACTIVE-Questschutz nach Angebot.
        {
            let (runtime, path) = modeled_runtime("vp-quest");
            let (shared, mut quests) = shared_pair().await;
            quests.register(collect_def("hp_potion")).unwrap();
            let now = Instant::now();
            let (id, request) = drive_to_commit(&shared, &quests, now).await;
            let version = {
                let world = shared.lock().await;
                world.player_trade_dialogs[&id].version
            };
            {
                let mut world = shared.lock().await;
                set_quest(&mut world, "1", "q_collect", QuestState::Active);
            }
            let spool = runtime.spool();
            let err = crate::persist::commit_trade_with_validation(
                spool,
                &shared,
                request.clone(),
                |w| validate_binding_preparation(w, &quests, RADIUS, &id, version, &request),
                |artifact| async move { spool.write_trade(&artifact) },
            )
            .await
            .unwrap_err();
            assert_eq!(err, "trade offer rejected: quest_item_protected");
            {
                let world = shared.lock().await;
                assert!(!world.prepared_trades.contains_key(&request.commit_id));
                assert_eq!(world.players["1"].inventory.count_of("hp_potion"), 10);
            }
            std::fs::remove_dir_all(path).unwrap();
        }
        // (g) Unauflösbare ACTIVE-Definition bleibt fail-closed.
        {
            let (runtime, path) = modeled_runtime("vp-questfail");
            let (shared, quests) = shared_pair().await;
            let now = Instant::now();
            let (id, request) = drive_to_commit(&shared, &quests, now).await;
            let version = {
                let world = shared.lock().await;
                world.player_trade_dialogs[&id].version
            };
            {
                let mut world = shared.lock().await;
                set_quest(&mut world, "1", "ghost_quest", QuestState::Active);
            }
            let spool = runtime.spool();
            let err = crate::persist::commit_trade_with_validation(
                spool,
                &shared,
                request.clone(),
                |w| validate_binding_preparation(w, &quests, RADIUS, &id, version, &request),
                |artifact| async move { spool.write_trade(&artifact) },
            )
            .await
            .unwrap_err();
            assert_eq!(err, "trade offer rejected: quest_data_unavailable");
            std::fs::remove_dir_all(path).unwrap();
        }
        // (h) Verfälschte Versionsannahme wird erkannt (Plumbing-Nachweis).
        {
            let (runtime, path) = modeled_runtime("vp-version");
            let (shared, quests) = shared_pair().await;
            let now = Instant::now();
            let (id, request) = drive_to_commit(&shared, &quests, now).await;
            let spool = runtime.spool();
            let err = crate::persist::commit_trade_with_validation(
                spool,
                &shared,
                request.clone(),
                |w| validate_binding_preparation(w, &quests, RADIUS, &id, 999, &request),
                |artifact| async move { spool.write_trade(&artifact) },
            )
            .await
            .unwrap_err();
            assert_eq!(err, "trade dialog version changed");
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn accept_reports_actor_dead() {
        let (mut w, _quests) = world_pair();
        let t0 = Instant::now();
        let out = request_dialog(&mut w, t0, RADIUS, "1", "2").expect("request");
        let id = out.actor_msg.unwrap()["dialog_id"]
            .as_str()
            .unwrap()
            .to_string();
        w.players.get_mut("2").unwrap().hp = 0;
        let failure = accept_dialog(&mut w, t0, RADIUS, "2", &id).unwrap_err();
        assert_eq!(failure.reason.reason(), "actor_dead");
        // Dialog abgebrochen, Partner informiert.
        assert!(w.player_trade_dialogs.is_empty());
        assert_eq!(failure.partner_msgs.len(), 1);
        assert_eq!(failure.partner_msgs[0].0, "1");
    }
}
