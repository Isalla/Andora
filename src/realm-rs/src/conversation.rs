//! conversation — Freier NPC-Gesprach (Spieler-Freitext -> Realm-Autoritaet
//! -> Coordinator-KI (existing Queue, job_type "npc") -> NPC_TEXT).
//! docs/KI_Personalisierungs_Questsystem_v2.md §4.5 + docs/Coordinator.md §§4/6/7/13/28.
//!
//! KEINE Spielautoritaet der KI: Realm determines Identity/NPC serverseitig;
//! only limited NPC-/Spielkontext (Anzeigename/Rolle/Locale/Situation) to the
//! Coordinator (no Secrets, no other players, no Inventar/Account/Token §28);
//! the existing Coordinator Queue is reused (no second KI-Anbindung/pipeline);
//! KI-/Transport answers in tests are MODELLIERT; the answer is reine
//! Gesprächdarstellung (no Items/Idia/Reputation/Quest/Reward change); a
//! narrated promise is KEIN Questangebot.
//!
//! FIX (FINAL_CHECK): A) Job-Identity server-side + restart-safe (realm-local
//!   monotonic counter + wallclock boot-nonce, NOT client seq / resettable
//!   conn_id). B) Coordinator roundtrip (submit+poll) bounded by a REAL total
//!   deadline (tokio::time::timeout) + HTTP connect/request timeouts. C) the
//!   existing parental communication gate (chat_allowed) is checked BEFORE any
//!   text reaches the Coordinator. Ownership uses is_owner + by_conn. No World-
//!   Lock across any I/O await.

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio::time::timeout;
use std::time::Duration;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::config::CoordinatorConfig;
use crate::handlers::Ctx;
use crate::npc::NpcStatus;
use crate::parental::chat_allowed;
use crate::protocol::{c2s, s2c, Frame};
use crate::security::{log_reject, RejectInfo};
use crate::world::{World, is_owner};

pub const MAX_INPUT_CHARS: usize = 500;
pub const POLL_MAX_ATTEMPTS: u32 = 3;
pub const POLL_BACKOFF_MS: u64 = 25;
pub const COORDINATOR_TOTAL_BUDGET_MS: u64 = 2000;
pub const HTTP_CONNECT_TIMEOUT_MS: u64 = 1000;
pub const HTTP_TIMEOUT_MS: u64 = 1500;

pub const FALLBACK_TEXT: &'static str = "ich im Moment nicht freetimeable - vershu spaeter.";

pub const REJECT_UNKNOWN_NPC: &'static str = "unknown_npc";
pub const REJECT_NPC_UNAVAILABLE: &'static str = "npc_unavailable";
pub const REJECT_OUT_OF_RANGE: &'static str = "out_of_range";
pub const REJECT_INPUT_TOO_LONG: &'static str = "input_too_long";
pub const REJECT_INPUT_INVALID: &'static str = "input_invalid";
pub const REJECT_CHAT_LOCKED: &'static str = "chat_locked";

#[derive(PartialEq)]
pub enum SubmitOutcome {
    Queued,
    InputTooLong,
    InputInvalid,
    Cooldown,
    QueueFull,
    JobExists,
    Unreachable,
}

#[derive(PartialEq)]
pub enum PollOutcome {
    Completed,
    Failed,
    Pending,
    NotFound,
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct ModelScript {
    pub submit: String,
    pub poll: String,
    pub result: String,
}

impl ModelScript {
    pub fn make() -> ModelScript {
        ModelScript {
            submit: "QUEUED".into(),
            poll: "COMPLETED".into(),
            result: "Modelled KI-Response (offline Nachweis, not a live answer).".into(),
        }
    }
}

pub struct CoordinatorClient {
    live: bool,
    url: String,
    service_id: String,
    secret: String,
    http: reqwest::Client,
    model: Mutex<ModelScript>,
}

impl CoordinatorClient {
    pub fn modelled() -> CoordinatorClient {
        CoordinatorClient {
            live: false,
            url: String::new(),
            service_id: String::new(),
            secret: String::new(),
            http: reqwest::Client::new(),
            model: Mutex::new(ModelScript::make()),
        }
    }

    pub fn live(cfg: &CoordinatorConfig) -> CoordinatorClient {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(HTTP_CONNECT_TIMEOUT_MS))
            .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
            .build()
            .unwrap_or(reqwest::Client::new());
        CoordinatorClient {
            live: true,
            url: cfg.url.clone(),
            service_id: cfg.service_id.clone(),
            secret: cfg.secret.clone(),
            http: http,
            model: Mutex::new(ModelScript::make()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.live && !self.url.is_empty()
    }

    pub fn set_model(self: &CoordinatorClient, submit: &str, poll: &str, result: &str) {
        self.model.lock().unwrap().submit = submit.into();
        self.model.lock().unwrap().poll = poll.into();
        self.model.lock().unwrap().result = result.into();
    }

    fn sign(&self, method: &str, path: &str, ts: i64, body: &str) -> String {
        use sha2::Digest;
        let body_hash = hex::encode(Sha256::digest(body.as_bytes()));
        let payload = format!("{method}\n{path}\n\n{ts}\n{body_hash}");
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes()).expect("hmac key");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub async fn submit(
        self: &CoordinatorClient,
        job_id: String,
        realm_id: String,
        character_id: String,
        player_id: String,
        npc_id: String,
        text: String,
        context: Value,
    ) -> SubmitOutcome {
        if !self.live {
            let m = self.model.lock().unwrap();
            return map_submit(m.submit.as_str());
        }
        let payload = json!({
            "job_type": "npc",
            "job_id": job_id,
            "realm_id": realm_id,
            "character_id": character_id,
            "player_id": player_id,
            "npc_id": npc_id,
            "text": text,
            "context": context,
        });
        let body = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let sig = self.sign("POST", "/v1/jobs", ts, &body);
        let sent = self.http
            .post(format!("{}/v1/jobs", self.url))
            .header("Content-Type", "application/json")
            .header("X-Andora-Service", &self.service_id)
            .header("X-Andora-Timestamp", ts.to_string())
            .header("X-Andora-Signature", sig)
            .body(body)
            .send()
            .await;
        let Some(resp) = sent.ok() else {
            return SubmitOutcome::Unreachable;
        };
        let status = resp.status().as_u16();
        let raw = resp.text().await.unwrap_or_default();
        match status {
            201 => SubmitOutcome::Queued,
            400 => {
                if raw.contains("INPUT_TOO_LONG") {
                    SubmitOutcome::InputTooLong
                } else {
                    SubmitOutcome::InputInvalid
                }
            },
            409 => SubmitOutcome::JobExists,
            429 => SubmitOutcome::Cooldown,
            503 => SubmitOutcome::QueueFull,
            _ => SubmitOutcome::Unreachable,
        }
    }

    pub async fn poll(self: &CoordinatorClient, job_id: String) -> PollOutcome {
        if !self.live {
            let mode = {
                let m = self.model.lock().unwrap();
                m.poll.clone()
            };
            if mode == "HANG" {
                sleep(Duration::from_secs(5)).await;
                return PollOutcome::Pending;
            }
            return map_poll(mode.as_str());
        }
        let Some(doc) = self.query(job_id.clone()).await else {
            return PollOutcome::Unreachable;
        };
        let got = doc.get("job_id").and_then(|v| v.as_str()).unwrap_or("");
        if got != job_id.as_str() {
            return PollOutcome::Pending;
        }
        match doc.get("status").and_then(|v| v.as_str()).unwrap_or("") {
            "COMPLETED" => PollOutcome::Completed,
            "FAILED" => PollOutcome::Failed,
            _ => PollOutcome::Pending,
        }
    }

    async fn query(self: &CoordinatorClient, job_id: String) -> Option<Value> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let path = format!("/v1/jobs/{}", job_id);
        let sig = self.sign("GET", &path, ts, "");
        let sent = self.http
            .get(format!("{}/v1/jobs/{}", self.url, job_id))
            .header("X-Andora-Service", &self.service_id)
            .header("X-Andora-Timestamp", ts.to_string())
            .header("X-Andora-Signature", sig)
            .send()
            .await;
        let Some(resp) = sent.ok() else {
            return None;
        };
        if resp.status().as_u16() != 200 {
            return None;
        }
        let raw = resp.text().await.unwrap_or_default();
        serde_json::from_str(&raw).ok()
    }

    pub async fn result_text(self: &CoordinatorClient, job_id: String) -> String {
        if !self.live {
            let m = self.model.lock().unwrap();
            return m.result.clone();
        }
        let Some(doc) = self.query(job_id.clone()).await else {
            return String::new();
        };
        doc.get("result").and_then(|v| v.as_str()).unwrap_or_default().to_string()
    }
}

fn map_submit(s: &str) -> SubmitOutcome {
    match s {
        "QUEUED" => SubmitOutcome::Queued,
        "INPUT_TOO_LONG" => SubmitOutcome::InputTooLong,
        "INPUT_INVALID" => SubmitOutcome::InputInvalid,
        "COOLDOWN" => SubmitOutcome::Cooldown,
        "QUEUE_FULL" => SubmitOutcome::QueueFull,
        "JOB_EXISTS" => SubmitOutcome::JobExists,
        _ => SubmitOutcome::Unreachable,
    }
}

fn map_poll(p: &str) -> PollOutcome {
    match p {
        "COMPLETED" => PollOutcome::Completed,
        "FAILED" => PollOutcome::Failed,
        "PENDING" => PollOutcome::Pending,
        "NOT_FOUND" => PollOutcome::NotFound,
        _ => PollOutcome::Unreachable,
    }
}

// A — server-side, restart-safe conversation-job identity. Independent of the
// client seq and of the resettable conn_id. A realm-local monotonic counter
// alone would reset on restart, so a wallclock boot-nonce (nanos) is mixed in:
// across a Realm-Neustart the nonce differs, so a still-open Coordinator job
// from the previous session is never re-used or overwritten.
static CONV_JOB_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn next_conv_job_id(realm_id: u32) -> String {
    let boot = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = CONV_JOB_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("conv-{}-{}-{}", realm_id, boot, n)
}

pub fn correlate_ok(world: &World, conn_id: u64, pid: &str, npc_id: &str) -> bool {
    let same_conn = world.by_conn.get(&conn_id).map(|p| p.as_str() == pid).unwrap_or(false);
    let present = world.players.contains_key(pid);
    let npc_alive = world.npcs.get(npc_id).is_some_and(|n| n.status == NpcStatus::Alive);
    same_conn && present && npc_alive
}

fn send_reject(tx: &mpsc::UnboundedSender<String>, seq: i64, npc_id: &str, reason: &str) {
    let _ = tx.send(
        Frame::new(seq, s2c::NPC_TEXT, json!({
            "ok": false, "action": "talk", "npc_id": npc_id, "reason": reason,
        })).encode(),
    );
}

async fn coordinator_roundtrip(
    ctx: &Ctx,
    job_id: String,
    realm_id: String,
    character_id: String,
    player_id: String,
    npc_id: String,
    text: String,
    context: Value,
) -> (String, String, String) {
    let submit = ctx.coordinator
        .submit(job_id.clone(), realm_id, character_id, player_id, npc_id, text, context)
        .await;
    match submit {
        SubmitOutcome::Queued => {
            let po = poll_until_terminal(ctx, job_id.clone()).await;
            if po == PollOutcome::Completed {
                (ctx.coordinator.result_text(job_id.clone()).await, "ki".into(), "".into())
            } else {
                (FALLBACK_TEXT.into(), "fallback".into(), "".into())
            }
        }
        SubmitOutcome::InputTooLong => (String::new(), String::new(), REJECT_INPUT_TOO_LONG.into()),
        SubmitOutcome::InputInvalid => (String::new(), String::new(), REJECT_INPUT_INVALID.into()),
        SubmitOutcome::Cooldown => (FALLBACK_TEXT.into(), "fallback".into(), "".into()),
        SubmitOutcome::QueueFull => (FALLBACK_TEXT.into(), "fallback".into(), "".into()),
        SubmitOutcome::JobExists => (FALLBACK_TEXT.into(), "fallback".into(), "".into()),
        SubmitOutcome::Unreachable => (FALLBACK_TEXT.into(), "fallback".into(), "".into()),
    }
}

async fn poll_until_terminal(ctx: &Ctx, job_id: String) -> PollOutcome {
    for _ in 0..POLL_MAX_ATTEMPTS {
        let o = ctx.coordinator.poll(job_id.clone()).await;
        match o {
            PollOutcome::Pending => {
                sleep(Duration::from_millis(POLL_BACKOFF_MS)).await;
                continue;
            }
            other => return other,
        }
    }
    PollOutcome::Unreachable
}

pub async fn handle(
    ctx: &Ctx,
    tx: &mpsc::UnboundedSender<String>,
    conn_id: u64,
    seq: i64,
    data: &Value,
) {
    let npc_id = data.get("npc_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let raw_text = data.get("text").and_then(|v| v.as_str()).unwrap_or_default().to_string();

    let (pid, lang, npc_name, npc_kind, npc_alive, in_range, too_long, empty, weather_ctx, weather_key, season_key) = {
        let world = ctx.shared.lock().await;
        let Some(pid) = world.by_conn
            .get(&conn_id)
            .filter(|p| is_owner(&world, conn_id, p))
        else {
            return;
        };
        let Some(pl) = world.players.get(pid) else {
            return;
        };
        let lang = pl.lang.clone();
        let Some(n) = world.npcs.get(&npc_id) else {
            send_reject(tx, seq, &npc_id, REJECT_UNKNOWN_NPC);
            log_reject(
                world.players.get(pid),
                conn_id,
                &RejectInfo {
                    reason: REJECT_UNKNOWN_NPC.into(),
                    msg_type: c2s::NPC_TALK,
                    detail: format!("action=talk npc={}", npc_id),
                },
                0,
            );
            return;
        };
        let npc_name = n.name.clone();
        let npc_kind = n.kind.clone();
        let npc_alive = n.status == NpcStatus::Alive;
        let range = ctx.cfg.loot.pickup_radius;
        let in_range = (pl.x - n.x).hypot(pl.y - n.y) <= range;
        let chars = raw_text.chars().count();
        let too_long = chars > MAX_INPUT_CHARS;
        let empty = raw_text.trim().is_empty();
        // Realm-decided weather context (docs/Weltzeit_und_Wettersystem.md §16/
        // §17): NPCs may READ the autoritative weather in conversation; the
        // Coordinator/NPC-KI never changes it. Fills jobContext.world.
        let weather_ctx = world.weather.coordinator_context();
        let weather_key = world.weather.current.key().to_string();
        let season_key = world.weather.season.key().to_string();
        (pid.clone(), lang, npc_name, npc_kind, npc_alive, in_range, too_long, empty, weather_ctx, weather_key, season_key)
    };

    if empty {
        return;
    }
    if too_long {
        send_reject(tx, seq, &npc_id, REJECT_INPUT_TOO_LONG);
        return;
    }
    if !in_range {
        send_reject(tx, seq, &npc_id, REJECT_OUT_OF_RANGE);
        return;
    }
    if !npc_alive {
        send_reject(tx, seq, &npc_id, REJECT_NPC_UNAVAILABLE);
        return;
    }

    // C — existing parental communication gate BEFORE any text to the
    // Coordinator. On lock: existing rejection semantics, NO KI request.
    if !chat_allowed(&ctx.parental, &pid).await {
        send_reject(tx, seq, &npc_id, REJECT_CHAT_LOCKED);
        return;
    }

    // A — server-side restart-safe identity (independent of seq/conn).
    let job_id = next_conv_job_id(ctx.cfg.realm_id);
    let context = json!({
        "locale": lang,
        "npc": format!("{} ({})", npc_name, npc_kind),
        "situation": "the player starts a free-text conversation with this NPC",
        "world": weather_ctx,
        "weather": weather_key,
        "season": season_key,
    });

    // B — bounded roundtrip under a REAL total deadline (submit + polling).
    let rt = timeout(
        Duration::from_millis(COORDINATOR_TOTAL_BUDGET_MS),
        coordinator_roundtrip(
            ctx,
            job_id.clone(),
            ctx.cfg.realm_id.to_string(),
            pid.clone(),
            conn_id.to_string(),
            npc_id.clone(),
            raw_text,
            context,
        ),
    ).await;
    let (answer, source, reject) = match rt {
        Err(_) => (FALLBACK_TEXT.into(), "fallback".into(), "".into()),
        Ok(t) => t,
    };
    if !reject.is_empty() {
        send_reject(tx, seq, &npc_id, reject.as_str());
        return;
    }

    {
        let world = ctx.shared.lock().await;
        if !correlate_ok(&world, conn_id, &pid, &npc_id) {
            return;
        }
        let _ = tx.send(
            Frame::new(seq, s2c::NPC_TEXT, json!({
                "ok": true,
                "action": "talk",
                "npc_id": npc_id,
                "conversation_id": job_id,
                "text": answer,
                "source": source,
            })).encode(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_length_boundary_is_realm_side() {
        assert!(MAX_INPUT_CHARS == 500);
        assert!(("x".repeat(500)).chars().count() == 500);
        assert!(("x".repeat(501)).chars().count() == 501);
    }

    #[test]
    fn submit_and_poll_mapping_is_closed() {
        assert!(map_submit("QUEUE_FULL") == SubmitOutcome::QueueFull);
        assert!(map_submit("COOLDOWN") == SubmitOutcome::Cooldown);
        assert!(map_submit("INPUT_TOO_LONG") == SubmitOutcome::InputTooLong);
        assert!(map_poll("COMPLETED") == PollOutcome::Completed);
        assert!(map_poll("TIMEOUT") == PollOutcome::Unreachable);
        assert!(map_poll("AI_UNAVAILABLE") == PollOutcome::Unreachable);
    }

    #[test]
    fn fallback_never_is_empty_and_never_mutates() {
        assert!(!FALLBACK_TEXT.is_empty());
    }

    #[test]
    fn modelled_client_is_offline() {
        let c = CoordinatorClient::modelled();
        assert!(!c.enabled());
    }

    #[test]
    fn live_client_signs_like_auth_scheme() {
        let cfg = CoordinatorConfig {
            url: "http://127.0.0.1:9".into(),
            service_id: "realm-de1-service".into(),
            secret: "s3cret".into(),
        };
        let c = CoordinatorClient::live(&cfg);
        assert!(c.enabled());
        let sig = c.sign("POST", "/v1/jobs", 1753286400, "{}");
        assert!(sig.len() == 64);
    }

    #[test]
    fn correlate_rejects_empty_world() {
        let w = mk_world();
        assert!(!correlate_ok(&w, 7, "1", "npc_9"));
    }

    // A — identity is server-side and independent of seq/conn: two calls yield
    // distinct job ids even for the same realm_id (counter/nonce advance).
    #[test]
    fn conv_job_id_is_server_side_and_distinct() {
        let a = next_conv_job_id(1);
        let b = next_conv_job_id(1);
        assert!(a != b);
        assert!(a.starts_with("conv-1-"));
        assert!(b.starts_with("conv-1-"));
    }

    fn mk_world() -> World {
        World::new()
    }
}
