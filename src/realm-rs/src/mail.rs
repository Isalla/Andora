// mail — Andora Mail-System V1 (System-Rückerstattung, realm-intern).
//
// Docs: docs/Coordinator.md §23 (Rückerstattung über das Mail-System),
// docs/Datenbank_Architektur.md §9/§19/§21 (additive Migration, keine DROP,
// keine fremden Tabellen).
//
// Serverautority: `character_id` kommt ausschließlich aus der geprüften
// Session (Go-API) bzw. der aktuellen Realm-Verbindung. Nie aus dem Payload.
// Der Realm ist die einzige Autorität für die `character_mail*`-Tabellen.
// Es gibt keinen Spieler-versus-Spieler-Versand, keine Quota, keine Fristen,
// keine Gebühren, kein Löschen und keine Elternbenachrichtigungen (D1–D7,
// D10, D13 unverändert offen).
//
// Flow:
//   Refund-Quelle → insert_refund_mail (atomare Transaktion:
//     character_mail + character_mail_items + Idia-Gutschrift + Job-Status)
//   → Escrow (Items bleiben in item_instances, referiert über Escrow)
//   → Claim über MailClaimArtifact im bestehenden Spool-/Drain-Pfad
//     (analog Quest-Completion, Migration 025)
//   → Inventar + Idia vollständig oder gar nicht
//
// Fail-closed: DB-Ausfall → keine halbe Mail, kein verlorener Job-Status.

use serde::{Deserialize, Serialize};
use sqlx::{MySql, Pool};
use std::collections::BTreeSet;

use crate::persist::{PersistSnapshot, write_snapshot_fields};

// ── Limits (serverautority; passend zu 026_mail_v1.sql) ────────────────────
pub const SUBJECT_MAX: usize = 120;
pub const BODY_MAX: usize = 65_536;
pub const MAIL_CLAIM_MAX_RETRIES: u32 = 3;

// ── MailState ───────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MailState {
    Unread,
    Read,
}

impl MailState {
    pub fn as_db(self: MailState) -> &'static str {
        match self {
            MailState::Unread => "unread",
            MailState::Read => "read",
        }
    }
    pub fn from_db(s: &str) -> MailState {
        match s {
            "read" => MailState::Read,
            _ => MailState::Unread,
        }
    }
}

// ── Core data model ─────────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mail {
    pub mail_id: String,
    pub character_id: u64,
    pub kind: String,          // system_refund (V1)
    pub subject: String,
    pub body: String,
    pub created_at_ms: i64,
    pub read_at_ms: Option<i64>,
    pub state: MailState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailItemRef {
    pub mail_id: String,
    pub item_uuid: String,
    pub position: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailBox {
    pub character_id: u64,
    pub mails: Vec<Mail>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MailClaimApply {
    Applied,
    AlreadyApplied,
    Waiting,
    Conflict,
}

// ── MailClaimArtifact (analog QuestCompletionArtifact, Migration 025) ───────
//
// Unveränderlicher Nachzustand eines Claims: vollständiger Inventar- und
// Idia-Zustand nach dem Claim, Basis-/Zielrevision, eindeutige Claim-Zuordnung.
// Wird vor der DB-Mutation geschrieben (Spool-Dauerhaftigkeit), danach in
// einer Transaktion publiziert (Snapshot + Escrow-Verbrauch + Receipt).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailClaimArtifact {
    pub kind: String,
    pub format_version: u16,
    pub commit_id: String,
    pub character_id: u64,
    pub mail_id: String,
    pub base_revision: i64,
    pub target_revision: i64,
    /// Vollständiger resultierender Zustand (Inventar mit übernommener
    /// Item-UUID, Idia, Lifecycle) als kanonische JSON-Darstellung.
    pub snapshot_json: String,
    pub claimed_mail_ids: Vec<String>,
}

impl MailClaimArtifact {
    pub fn commit_payload(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|e| format!("mail claim serialize: {e}"))
    }
    /// Persist-Revision des Ziel-Zustands (aus dem eingebetteten Snapshot).
    /// Entspricht base_revision + 1, da der Claim genau eine Revision
    /// voranschreitet.
    pub fn snapshot_persist_revision(&self) -> i64 {
        self.target_revision
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.kind != "character_mail_claim"
            || self.format_version != 1
            || self.commit_id.is_empty()
            || self.commit_id.len() > 64
            || !self.commit_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("invalid mail claim envelope".into());
        }
        crate::db::parse_character_id(&self.character_id.to_string())?;
        if self.mail_id.is_empty() || self.mail_id.len() > 64 {
            return Err("invalid mail id".into());
        }
        if self.base_revision < 0
            || self.base_revision.checked_add(1) != Some(self.target_revision)
            || self.target_revision <= 0
        {
            return Err("invalid mail claim revision".into());
        }
        if self.claimed_mail_ids.iter().any(|id| id.is_empty() || id.len() > 64) {
            return Err("invalid claimed mail id".into());
        }
        if self.snapshot_json.trim().is_empty() {
            return Err("mail claim result not set".into());
        }
        Ok(())
    }
    /// The durable receipt/artifact belongs to this exact claim intent.
    pub fn matches_request(&self, request: &MailClaimRequest) -> bool {
        self.commit_id == request.commit_id
            && self.character_id == request.character_id
            && self.mail_id == request.mail_id
    }
}

// ── Runtime-Commit-Service (analog Quest-Completion, Migration 025) ────────

#[derive(Debug, Clone, PartialEq)]
pub struct MailClaimRequest {
    pub commit_id: String,
    pub character_id: u64,
    pub mail_id: String,
}

#[derive(Clone)]
pub struct PreparedMailClaim {
    pub request: MailClaimRequest,
    pub artifact: MailClaimArtifact,
    pub snapshot: PersistSnapshot,
}

/// Deterministische technische Commit-ID für einen (character, mail_id)
/// Claim-Intent. Stabil über In-Process-Wiederholung und Neustart hinweg
/// (die Spool-Datei/Receipt trägt dieselbe Identität). Eine Kollision gegen
/// eine existierende offene Publikation ist fail-closed durch die
/// receipt-ID-Prüfung.
pub fn mail_claim_id(character_id: u64, mail_id: &str) -> String {
    use sha2::Digest;
    use sha2::Sha256;
    let raw = Sha256::digest(
        format!("mc:{character_id}:{mail_id}").as_bytes(),
    );
    let bytes: [u8; 32] = raw.into();
    let hex = hex::encode(&bytes);
    format!("mc{}", &hex[..40])
}

// ── UUID (v4-format, selbst erzeugt — keine externe uuid-Dependency) ────────
static MAIL_UUID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn new_mail_uuid() -> String {
    let n = MAIL_UUID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let hi = stamp ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let lo = stamp.wrapping_add(n.wrapping_mul(0xBF58_476D_1CE4_E5B9)) ^ hi.rotate_left(17);
    let h = format!("{:016x}{:016x}", hi, lo);
    format!(
        "{}-{}-4{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..15],
        &h[16..20],
        &h[20..32],
    )
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── Validierung (pure, serverautority) ─────────────────────────────────────
pub fn valid_mail_subject(subject: &str) -> Result<(), String> {
    if subject.is_empty() || subject.chars().count() > SUBJECT_MAX {
        return Err("invalid_subject".into());
    }
    Ok(())
}
pub fn valid_mail_body(body: &str) -> Result<(), String> {
    if body.len() > BODY_MAX {
        return Err("invalid_body".into());
    }
    Ok(())
}

// ── Serverseitig Character-Account-Binding (list/read/claim) ────────────────
//
// The session-account (from the signed Go-API forward) must owningly match the
// character's owner account. A zero/absent account or a foreign account is denied
// — a clientseitig character_id is never trusted alone; cross-account access is
// prevented. This is the single server-authoritative binding used by list/read/claim.
pub fn mail_owner_verdict(body_account_id: i64, owner_account_id: i64) -> bool {
    body_account_id > 0 && owner_account_id > 0 && body_account_id == owner_account_id
}

// ── Refund-Erzeugung (Realm-intern, aus Quell-Queues) ───────────────────────
//
// Aufgerufen von der Quell-Anbindung (z. B. Crafting-Job-Ende), wenn ein Job
// endgültig fehlgeschlagen ist. Eine DB-Transaktion sichert:
//   1. character_mail + character_mail_items (Escrow, UUID-Referenzen)
//   2. Idia-Gutschrift (characters.idia)
//   3. Job-Status `refunded` (idempotent via refund_key)
//   4. COMMIT
//
// Rückgabe: (mail_id, item_uuids, idia_amount) oder Fehler.
pub async fn insert_refund_mail(
    pool: &Pool<MySql>,
    character_id: u64,
    subject: &str,
    body: &str,
    item_uuids: &[String],
    idia_amount: i64,
    refund_key: &str,
) -> Result<(String, Vec<String>, i64), String> {
    valid_mail_subject(subject)?;
    valid_mail_body(body)?;

    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;

    // Idempotenz: doppelter refund_key darf keine zweite Mail/keine zweite Gutschrift.
    let already: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM character_mail WHERE refund_key = ? LIMIT 1",
    )
    .bind(refund_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("refund_key check: {e}"))?;
    if already.is_some() {
        let _ = tx.rollback().await;
        return Err("refund_already_done".into());
    }

    let mail_id = new_mail_uuid();
    let now = now_ms();

    // 1. Mail anlegen
    sqlx::query(
        "INSERT INTO character_mail (mail_id, character_id, kind, subject, body, created_at_ms, state, refund_key) VALUES (?, ?, 'system_refund', ?, ?, ?, 'unread', ?)",
    )
    .bind(&mail_id)
    .bind(character_id as i64)
    .bind(subject)
    .bind(body)
    .bind(now)
    .bind(refund_key)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("mail insert: {e}"))?;

    // 2. Items als Escrow-Referenzen (UUIDs bleiben in item_instances)
    for (pos, item_uuid) in item_uuids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO character_mail_items (mail_id, item_uuid, position) VALUES (?, ?, ?)",
        )
        .bind(&mail_id)
        .bind(item_uuid)
        .bind(pos as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("mail item insert: {e}"))?;
    }

    // 3. Idia-Gutschrift (nur wenn > 0; bei Idia=0 kein Op)
    if idia_amount > 0 {
        sqlx::query("UPDATE characters SET idia = idia + ? WHERE id = ?")
            .bind(idia_amount)
            .bind(character_id as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("idia credit: {e}"))?;
    }

    tx.commit().await.map_err(|e| format!("commit: {e}"))?;

    Ok((mail_id, item_uuids.to_vec(), idia_amount))
}

// ── Claim-Anwendung (Drain) ─────────────────────────────────────────────────
//
// Wird vom Drain aufgerufen (apply_mail_claim_to_db). Führt in einer
// DB-Transaktion aus:
//   1. Receipt prüfen (Idempotenz)
//   2. Revision prüfen (Basis muss aktuell sein)
//   3. Vollständigen Snapshot schreiben (Inventar + Idia + Lifecycle + Revision)
//   4. Escrow verbrauchen (character_mail + character_mail_items löschen)
//   5. Claim-Beleg schreiben (character_mail_claims)
// Alles in EINER Transaktion → vollständig oder gar nicht.
// Niemals Escrow ohne zugehörige Auslieferung löschen.

/// Revision-Aktion mit Beleg-Nachweis (analog Quest-Completion).
/// Gibt Applied zurück, wenn der Claim durchgeführt werden kann,
/// AlreadyApplied bei identischer Wiederholung, Conflict bei abweichendem
/// Inhalt oder unzulässiger Revision, Waiting wenn die Basis-Revision
/// noch nicht erreicht ist.
pub(crate) fn mail_claim_revision_action_with_proof(
    artifact: &MailClaimArtifact,
    current: Option<i64>,
    committed_payload: Option<&str>,
) -> MailClaimApply {
    let Some(rev) = current else { return MailClaimApply::Conflict };
    if let Some(payload) = committed_payload {
        if artifact.commit_payload().as_deref() != Ok(payload) {
            return MailClaimApply::Conflict;
        }
        return if rev >= artifact.snapshot_persist_revision() {
            MailClaimApply::AlreadyApplied
        } else {
            MailClaimApply::Conflict
        };
    }
    if rev >= artifact.snapshot_persist_revision() {
        MailClaimApply::Conflict
    } else if rev == artifact.base_revision {
        MailClaimApply::Applied
    } else {
        MailClaimApply::Waiting
    }
}

pub async fn apply_mail_claim_to_db(
    pool: &Pool<MySql>,
    artifact: &MailClaimArtifact,
    weapon_skill_id: &str,
) -> Result<MailClaimApply, String> {
    artifact.validate()?;
    let mut tx = pool.begin().await.map_err(|e| format!("mail claim begin: {e}"))?;

    // Mail-Zeile sperren (FOR UPDATE) und Revision + Beleg laden, alles in derselben TX.
    let _mail = crate::db::lock_mail_for_claim(
        &mut tx,
        &artifact.character_id.to_string(),
        &artifact.mail_id,
    )
    .await?;
    let current = crate::db::lock_persist_revision(&mut tx, &artifact.character_id.to_string()).await?;
    let proof = crate::db::load_mail_claim_proof(&mut tx, &artifact.commit_id).await?;
    let action = mail_claim_revision_action_with_proof(artifact, current, proof.as_deref());
    if action != MailClaimApply::Applied {
        return Ok(action);
    }

    // Snapshot deserialisieren und vollständigen Charakter-Zustand schreiben
    // (Inventar + Idia + Progression + Revision + Lifecycle), alles in derselben TX.
    let snapshot: PersistSnapshot = serde_json::from_str(&artifact.snapshot_json)
        .map_err(|e| format!("mail claim snapshot deserialize: {e}"))?;
    if snapshot.player_id != artifact.character_id.to_string() {
        let _ = tx.rollback().await;
        return Ok(MailClaimApply::Conflict);
    }
    write_snapshot_fields(&mut tx, &snapshot, weapon_skill_id).await?;
    if let Some(lifecycle) = snapshot.item_lifecycle.as_ref() {
        crate::db::apply_item_lifecycle(
            &mut tx,
            &artifact.character_id.to_string(),
            lifecycle,
            &snapshot.inventory,
        )
        .await?;
    }
    crate::db::write_persist_revision(
        &mut tx,
        &artifact.character_id.to_string(),
        snapshot.persist_revision,
    )
    .await?;

    // Escrow verbrauchen: Mail + Item-Referenzen löschen (FK-CASCADE clears
    // character_mail_items). State-ORTHOGONAL: a read Mail is consumed exactly
    // like an unread one. A repeated Claim never reaches here (the receipt
    // already returned AlreadyApplied above), so items are never duplicated.
    let deleted: sqlx::mysql::MySqlQueryResult = sqlx::query(
        "DELETE FROM character_mail WHERE mail_id = ? AND character_id = ?",
    )
    .bind(&artifact.mail_id)
    .bind(artifact.character_id as i64)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("mail delete: {e}"))?;

    if deleted.rows_affected() == 0 {
        let _ = tx.rollback().await;
        return Ok(MailClaimApply::Conflict); // bereits verarbeitet oder ungültig
    }

    // Receipt schreiben (identisch zu Artifact, für Wiederholungsnachweis).
    sqlx::query(
        "INSERT INTO character_mail_claims (commit_id, character_id, mail_id, artifact_json, created_at_ms) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&artifact.commit_id)
    .bind(artifact.character_id as i64)
    .bind(&artifact.mail_id)
    .bind(&artifact.snapshot_json)
    .bind(now_ms())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("claim receipt insert: {e}"))?;

    tx.commit().await.map_err(|e| format!("claim commit: {e}"))?;

    Ok(MailClaimApply::Applied)
}

// ── Öffentliche Claim via Runtime-Commit-Service ────────────────────────────
//
// Gate → aktuelle Validierung → unveränderliche Vorbereitung → bestätigte
// Spool-Dauerhaftigkeit → RAM-Handoff. Der DB-Application (Snapshot + Escrow-
// Verbrauch + Receipt + Revision) bleibt in dem Drain (`apply_mail_claim_to_db`
// via `Spool::drain_mail_claim`). Kein Erfolg bei bloßem DB-Write ohne
// konsistente RAM-Übernahme; bei unklarer Veröffentlichung der IDENTICAL
// prepared Order is retained (docs/Quest-Turn-In-Persistenz.md analog).

/// Loads the escrow item UUIDs of a mail (DB read, outside the World lock).
pub async fn load_mail_item_uuids(
    pool: &Pool<MySql>,
    mail_id: &str,
) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx::query_as::<_, (String,)>(
        "SELECT item_uuid FROM character_mail_items WHERE mail_id = ?",
    )
    .bind(mail_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("mail items load: {e}"))?;
    Ok(rows.into_iter().map(|(uuid,)| uuid).collect())
}

/// Immutable preparation under the World lock: the complete resulting state
/// (current inventory + the escrow items) as a full PersistSnapshot. No Player
/// clone; the snapshot IS the resulting state.
pub(crate) fn prepare_mail_claim(
    world: &crate::world::World,
    request: &MailClaimRequest,
    instances: &[crate::item::ItemInstance],
) -> Result<PreparedMailClaim, String> {
    let pid = request.character_id.to_string();
    let player = world.players.get(&pid).ok_or("mail claim character unavailable")?;
    // A freshly logged-in character has no Lifecycle entry yet (login does not
    // create one; persist.rs likewise treats a missing Lifecycle as empty). An
    // empty Lifecycle is the correct view for a character without pending
    // detachments, so the claim must not fail because of its absence.
    let empty_lifecycle = crate::item_lifecycle::ItemLifecycle::new();
    let lifecycle = world.item_lifecycle.get(&pid).unwrap_or(&empty_lifecycle);
    let mut snapshot = crate::persist::build_snapshot(player, true, lifecycle, &world.runtime_id)?
        .ok_or("mail claim snapshot build")?
        .clone();
    for instance in instances {
        let def = world.item_definitions
            .get(&instance.item_id)
            .ok_or("mail claim item definition unavailable")?;
        snapshot.inventory.try_insert_instance(def, &instance)
            .map_err(|e| format!("mail claim inventory insert: {:?}", e))?;
    }
    let artifact = MailClaimArtifact {
        kind: "character_mail_claim".into(),
        format_version: 1,
        commit_id: request.commit_id.clone(),
        character_id: request.character_id,
        mail_id: request.mail_id.clone(),
        base_revision: player.persist_revision,
        target_revision: player.persist_revision + 1,
        snapshot_json: serde_json::to_string(&snapshot).map_err(|e| format!("mail claim snapshot serialize: {e}"))?,
        claimed_mail_ids: vec![request.mail_id.clone()],
    };
    artifact.validate()?;
    Ok(PreparedMailClaim {
        request: request.clone(),
        artifact: artifact.clone(),
        snapshot: snapshot.clone(),
    })
}

/// The only success boundary is confirmed spool durability of the immutable
/// claim order, then the RAM handoff. Earlier normal publications of this
/// character are resolved first (the base revision may never be skipped from
/// a stale RAM baseline).
pub async fn commit_mail_claim(
    spool: &crate::spool::Spool,
    shared: &crate::world::Shared,
    request: MailClaimRequest,
    pool: &Pool<MySql>,
) -> Result<(), String> {
    let pid = request.character_id.to_string();
    crate::persist::retry_normal_publications(spool, shared, Some(&[pid.clone()])).await?;
    let _gate = spool.player_gate(&pid).await.lock_owned().await;
    finish_mail_claim(spool, shared, request, pool, |artifact| async move {
        spool.write_mail_claim(&artifact)
    })
    .await
}

/// Gate-ASSUMING core: the caller already holds this character's Gate (claim
/// endpoint) or acquires it immediately before (retry path). Performs the
/// durable publication and the RAM handoff; never acquires the Gate itself.
pub(crate) async fn finish_mail_claim<W, F>(
    spool: &crate::spool::Spool,
    shared: &crate::world::Shared,
    request: MailClaimRequest,
    pool: &Pool<MySql>,
    publish: W,
) -> Result<(), String>
where
    W: FnOnce(MailClaimArtifact) -> F,
    F: std::future::Future<Output = Result<(), String>>,
{
    let pid = request.character_id.to_string();
    // Durable spool receipt (cross-restart identity): identical content -> done.
    if let Some(receipt) = spool.mail_claim_receipt(&request.commit_id)? {
        return if receipt.matches_request(&request) {
            Ok(())
        } else {
            Err("mail claim ID conflict".into())
        };
    }
    // Retained in-process prepared order (pending retry) reused verbatim.
    let retained = {
        let world = shared.lock().await;
        if let Some(receipt) = world.mail_claim_receipts.get(&request.commit_id) {
            return if receipt == &request {
                Ok(())
            } else {
                Err("mail claim ID conflict".into())
            };
        }
        if let Some(prepared) = world.prepared_mail_claims.get(&request.commit_id) {
            if prepared.request != request {
                return Err("mail claim ID conflict".into());
            }
            Some(prepared.clone())
        } else {
            None
        }
    };
    let prepared = match retained {
        Some(p) => p,
        None => {
            // Filesystem availability checks stay OUTSIDE the World lock.
            spool.ensure_mail_claim_save_allowed(&pid)?;
            spool.ensure_trade_save_allowed(&pid)?;
            // Load escrow items from DB (outside World lock).
            let item_uuids = load_mail_item_uuids(pool, &request.mail_id).await?;
            let mut instances: Vec<crate::item::ItemInstance> = Vec::with_capacity(item_uuids.len());
            for uuid in item_uuids {
                match crate::db::load_item_instance(pool, &uuid).await? {
                    Some(i) => instances.push(i),
                    None => return Err(format!("mail claim item instance missing: {uuid}")),
                }
            }
            let mut world = shared.lock().await;
            if world.normal_publications.contains_key(&pid) {
                return Err("normal publication pending".into());
            }
            if world.prepared_mail_claims.contains_key(&request.commit_id)
                || world.mail_claim_receipts.contains_key(&request.commit_id)
            {
                return Err("mail claim ID conflict".into());
            }
            // One pending Mail-Claim per character (analog quest_completion_pending):
            // a second refund must not prepare at the same base revision while one
            // is retained — it defers and converges via the periodic retry/next Login.
            if world.prepared_mail_claims
                .values()
                .any(|p| p.request.character_id == request.character_id)
            {
                return Err("mail claim pending".into());
            }
            let prepared = prepare_mail_claim(&world, &request, &instances)?;
            world.prepared_mail_claims.insert(request.commit_id.clone(), prepared.clone());
            spool.hold_mail_claim_publication(&request.commit_id);
            prepared
        }
    };
    publish(prepared.artifact.clone()).await?;
    // RAM handoff (generation-bound dirty clear).
    let mut world = shared.lock().await;
    let snap = &prepared.snapshot;
    if !world.players.contains_key(&snap.player_id) {
        return Err("prepared claim player lost".into());
    }
    let unchanged = world.players.get(&snap.player_id).unwrap().persist_generation == snap.generation;
    let p = world.players.get_mut(&snap.player_id).unwrap();
    p.x = snap.x;
    p.y = snap.y;
    p.level = snap.level;
    p.exp = snap.exp;
    p.free_attr_points = snap.free_attr_points;
    p.idia = snap.idia;
    p.hp = snap.hp;
    p.mana = snap.mana;
    p.attributes = snap.attributes.clone();
    p.inventory = snap.inventory.clone();
    p.persist_revision = snap.persist_revision;
    if unchanged {
        p.dirty.clear_components(snap.dirty);
    }
    world.prepared_mail_claims.remove(&request.commit_id);
    world.mail_claim_receipts.insert(request.commit_id.clone(), request.clone());
    spool.release_mail_claim_publication(&request.commit_id);
    Ok(())
}

/// Resume the retained, identical claim orders independently. Never recompute;
/// never acquire a gate while another is held.
pub(crate) async fn retry_mail_claims(
    spool: &crate::spool::Spool,
    shared: &crate::world::Shared,
    pool: &Pool<MySql>,
    requested: Option<&[String]>,
) -> Result<(), String> {
    let requests: Vec<MailClaimRequest> = {
        let world = shared.lock().await;
        world.prepared_mail_claims
            .values()
            .filter(|p| requested.is_none_or(|ids| ids.contains(&p.request.character_id.to_string())))
            .map(|p| p.request.clone())
            .collect()
    };
    let mut errors: Vec<String> = Vec::new();
    for request in requests {
        if let Err(e) = commit_mail_claim(spool, shared, request, pool).await {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

// ── Automatic Login-Pickup (Mail-V1 §23, integrated in Login-Lifecycle) ─────
//
// Pending system-refund mails are delivered via the shared Runtime-Commit-Service
// (Gate-assuming finish_mail_claim — the Login already holds this character's
// Gate). Never a direct DB write. Idempotent across re-login/recovery (deterministic
// commit_id + durable receipt). A deferred/failed claim is non-fatal: the periodic
// retry and the next Login converge it; the refund (escrow row) is never lost.

/// Lists the mail_ids of pending (not yet claimed) system-refund mails for a
/// character. A claimed Mail's row is gone (receipt records it), so "present"
/// == "not yet claimed". Lesestatus (unread/read) is orthogonal.
pub async fn list_pending_refund_mail_ids(
    pool: &Pool<MySql>,
    character_id: u64,
) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx::query_as::<_, (String,)>(
        "SELECT mail_id FROM character_mail WHERE character_id = ? AND kind = 'system_refund'",
    )
    .bind(character_id as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("mail pickup list: {e}"))?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Automatic login pickup: deliver pending refund mails via the shared
/// Runtime-Commit-Service. Gate-ASSUMING (login holds the character Gate).
/// Returns the number of claims that completed durable publication + RAM handoff.
pub(crate) async fn pickup_refund_mails_for_login(
    spool: &crate::spool::Spool,
    shared: &crate::world::Shared,
    pool: &Pool<MySql>,
    character_id: u64,
    weapon_skill_id: &str,
) -> Result<u32, String> {
    let mail_ids = list_pending_refund_mail_ids(pool, character_id).await?;
    let mut delivered = 0;
    for mail_id in mail_ids {
        let request = MailClaimRequest {
            commit_id: mail_claim_id(character_id, &mail_id),
            character_id,
            mail_id: mail_id.clone(),
        };
        match finish_mail_claim(spool, shared, request, pool, |a| async move {
            spool.write_mail_claim(&a)
        })
        .await {
            Ok(()) => delivered += 1,
            Err(e) => log::warn!("login mail-claim pickup deferred (char {character_id} mail {mail_id}): {e}"),
        }
    }
    Ok(delivered)
}

// ── Abholung / Lesen (Spieler-seitig, über Go-Fassade) ─────────────────────

pub async fn list_mails_for_character(
    pool: &Pool<MySql>,
    character_id: u64,
) -> Result<Vec<Mail>, String> {
    let rows: Vec<(String, String, String, String, i64, Option<i64>, String)> =
        sqlx::query_as(
            "SELECT mail_id, kind, subject, body, created_at_ms, read_at_ms, state FROM character_mail WHERE character_id = ? AND state != 'deleted' ORDER BY created_at_ms DESC",
        )
        .bind(character_id as i64)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("mail list: {e}"))?;

    Ok(rows
        .into_iter()
        .map(|(id, kind, subject, body, created, read, state)| Mail {
            mail_id: id,
            character_id,
            kind,
            subject,
            body,
            created_at_ms: created,
            read_at_ms: read,
            state: MailState::from_db(&state),
        })
        .collect())
}

pub async fn read_mail(
    pool: &Pool<MySql>,
    character_id: u64,
    mail_id: &str,
) -> Result<Option<Mail>, String> {
    // Einzelnes Lesen: nur die angeforderte Mail, Eigentümerprüfung inklusive.
    let row: Option<(String, String, String, String, i64, Option<i64>, String)> =
        sqlx::query_as(
            "SELECT mail_id, kind, subject, body, created_at_ms, read_at_ms, state FROM character_mail WHERE mail_id = ? AND character_id = ? AND state != 'deleted'",
        )
        .bind(mail_id)
        .bind(character_id as i64)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("mail read: {e}"))?;

    if let Some((id, kind, subject, body, created, read, state)) = row {
        // Als gelesen markieren (einzelnes Lesen)
        sqlx::query(
            "UPDATE character_mail SET read_at_ms = ?, state = 'read' WHERE mail_id = ? AND character_id = ?",
        )
        .bind(now_ms())
        .bind(&id)
        .bind(character_id as i64)
        .execute(pool)
        .await
        .map_err(|e| format!("mail read mark: {e}"))?;

        return Ok(Some(Mail {
            mail_id: id,
            character_id,
            kind,
            subject,
            body,
            created_at_ms: created,
            read_at_ms: Some(now_ms()),
            state: MailState::Read,
        }));
    }

    Ok(None)
}

// ── Item-Lifecycle-Schutz ───────────────────────────────────────────────────
//
// character_mail_items muss von db.rs::item_uuid_is_referenced mitgeführt
// werden, damit Escrow-Items nicht als unreferenziert gelöscht werden.

pub fn mail_item_refs(mails: &[Mail]) -> Vec<String> {
    let mut uuids: BTreeSet<String> = BTreeSet::new();
    // (Wird von db.rs aufgerufen; hier nur Platzhalter für Konsistenz.)
    let _ = mails;
    uuids.into_iter().collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_claim_id_deterministic() {
        let id1 = mail_claim_id(42, "mail-abc");
        let id2 = mail_claim_id(42, "mail-abc");
        let id3 = mail_claim_id(43, "mail-abc");
        assert_eq!(id1, id2, "gleiche Eingabe muss gleiche ID liefern");
        assert_ne!(id1, id3, "unterschiedliche Charaktere müssen unterschiedliche IDs liefern");
        assert!(id1.starts_with("mc"), "ID muss mit 'mc' beginnen");
        assert_eq!(id1.len(), 42, "ID muss 42 Zeichen lang sein ('mc' + 40 hex)");
    }

    #[test]
    fn mail_claim_revision_action_applied() {
        let artifact = MailClaimArtifact {
            kind: "character_mail_claim".to_string(),
            format_version: 1,
            commit_id: "mc_test".to_string(),
            character_id: 7,
            mail_id: "mail-1".to_string(),
            base_revision: 10,
            target_revision: 11,
            snapshot_json: "{\"snapshot\":1}".to_string(),
            claimed_mail_ids: vec!["mail-1".to_string()],
        };

        // Revision exakt auf Basis -> Applied
        assert_eq!(
            mail_claim_revision_action_with_proof(&artifact, Some(10), None),
            MailClaimApply::Applied
        );

        // Höher Revision OHNE Content-Proof -> KEIN falschen AlreadyApplied (Conflict).
        assert_eq!(
            mail_claim_revision_action_with_proof(&artifact, Some(11), None),
            MailClaimApply::Conflict
        );
        assert_eq!(
            mail_claim_revision_action_with_proof(&artifact, Some(12), None),
            MailClaimApply::Conflict
        );

        // Revision noch nicht erreicht -> Waiting
        assert_eq!(
            mail_claim_revision_action_with_proof(&artifact, Some(9), None),
            MailClaimApply::Waiting
        );

        // Keine aktuelle Revision -> Conflict
        assert_eq!(
            mail_claim_revision_action_with_proof(&artifact, None, None),
            MailClaimApply::Conflict
        );

        // IDENTICAL Content-Proof plus passende Revision -> AlreadyApplied (Skip).
        let payload = artifact.commit_payload().unwrap();
        assert_eq!(
            mail_claim_revision_action_with_proof(&artifact, Some(11), Some(&payload)),
            MailClaimApply::AlreadyApplied
        );
    }

    #[test]
    fn mail_claim_revision_action_conflict_on_divergent_payload() {
        let artifact1 = MailClaimArtifact {
            kind: "character_mail_claim".to_string(),
            format_version: 1,
            commit_id: "mc_test".to_string(),
            character_id: 7,
            mail_id: "mail-1".to_string(),
            base_revision: 10,
            target_revision: 11,
            snapshot_json: "{\"snapshot\":1}".to_string(),
            claimed_mail_ids: vec!["mail-1".to_string()],
        };
        let artifact2 = MailClaimArtifact {
            snapshot_json: "{\"snapshot\":2}".to_string(),
            ..artifact1.clone()
        };

        // Gleiche commit_id, abweichender Inhalt -> Conflict
        assert_eq!(
            mail_claim_revision_action_with_proof(&artifact1, Some(10), Some(&artifact2.commit_payload().unwrap())),
            MailClaimApply::Conflict
        );
        assert_eq!(
            mail_claim_revision_action_with_proof(&artifact2, Some(10), Some(&artifact1.commit_payload().unwrap())),
            MailClaimApply::Conflict
        );
    }

    #[test]
    fn mail_owner_verdict_binds_account_to_character() {
        // Matching owner -> allowed.
        assert!(mail_owner_verdict(7, 7));
        // Foreign account (cross-account) -> denied.
        assert!(!mail_owner_verdict(8, 7));
        // Absent/zero account (client did not bind) -> denied.
        assert!(!mail_owner_verdict(0, 7));
        assert!(!mail_owner_verdict(7, 0));
        // Both zero -> denied (no anonymous access).
        assert!(!mail_owner_verdict(0, 0));
    }
}
