# Quest-Protokoll V1.3 — Wire-Format und Validationsfeld

Status: **Protokoll-Schicht über dem bereits implementierten Server-Kern (V1.2a).**
Die Server-Logik (`npc_quest.rs`, `quest.rs`, `db.rs`, `spool.rs`, `persist.rs`) ist
vollständig implementiert und offline-geprüft (947/947 Tests). V1.3 definiert das
**Wire-Format** (C2S-Sending, S2C-Decoding) und die **Dispatch-Verdrahtung** in
`handlers.rs`. Es führt **keine** neuen Server-Semantik und **keine** Reward-Kind
über die bereits freigabten `exp`/`idia` (docs/Quest-Turn-In-Persistenz.md).

## 0. Nachrichten-IDs (Quelle: `src/realm-rs/src/protocol.rs`)

| Direction | ID | Name | Role |
|-----------|----|------|------|
| C2S | 6 | `NPC_TALK` | Quest-Offer/Accept/Turn-In-Absicht |
| S2C | 8 | `NPC_TEXT` | Quest-Antwort (Payload) |

Frame-Layout (beidseits identisch, `protocol.rs::Frame` / `protocol.gd::encode`):
`{"seq": <i64>, "type": <msg_id>, "data": { ... }}`. `seq` ist **nur** Korrelation
und wird zurückgespiegelt; es ist **kein** Idempotenz-Key (deterministischer
`receipt_id`/Commit-Key + Receipt-DedupProvide die Idempotenz, §2).

## 1. C2S (Godot → Rust)

### 1.1 `C2S_quest_turn_in` (NPC_TALK 6)

```json
{"npc_id": "<NPC-Instanz-ID>", "action": "quest_turn_in",
 "quest_id": "<Quest-ID>", "npc_content_id": "<stabile Content-ID>",
 "item_uuids": ["<Instanz-UUID>", ...]}
```

* `npc_id` — NPC-Instanz-ID (Server `World.npcs`-Lookup + Interaction/Range).
* `npc_content_id` — V1.3-Absicht; Server vergleicth gegen `Npc.content_id`
  (`def.turn_in`). **Kein** Fallback auf `giver` (§27.22).
* `item_uuids` — Absicht-List von **eigen** Instanz-UUIDs. **Open-Boundary:**
  die jetzige Server-Kern ist **Kill-Quest-only** (`objectives.all(Kill)`);
  Item-Removal (`InventoryState.try_remove`) per `item_uuids` ist **not yet
  implemented** im V1.2a-Kern und wird hier fail-closed (Ignored, no mutation).

### 1.2 `C2S_quest_accept` (NPC_TALK 6)

```json
{"npc_id": "<NPC-Instanz-ID>", "action": "quest_accept", "quest_id": "<Quest-ID>"}
```

* Offer-List (`action:"quest_offers"`) und Turn-In-List (`action:"quest_turn_ins"`)
  bleiben die V1.2a-List-actions; V1.3 **refines** das Payload-Shape, nicht die IDs.

## 2. Server-Side Validationspfad (`handle_npc_talk` → `npc_quest.rs`)

Dispatch (`handlers.rs:1571-1578`):

```rust
if action in {"quest_offers","quest_accept"} -> npc_quest::handle(...)
if action in {"quest_turn_ins","quest_turn_in"} -> npc_quest::handle_turn_in(...)
```

`handle_turn_in` Validation-Order (Fail-closed, **no partial mutation**):

1. **Ownership** (`by_conn`/`is_owner`) — lost → silent return, no frame.
2. **Character-Gate** (`persist.player_gate`) with `quest_io_timeout`
   → timeout = `quest_turn_in_busy` (no mutation).
3. **Interaction** (`npc_quest.rs::interaction`): `unknown_player`/`character_dead`/
   `unknown_npc`/`npc_unavailable`/`out_of_range`.
4. **Definition** `find_definition` → `unknown_quest`.
5. **turn_in Content-ID** `def.turn_in == Npc.content_id` → else `wrong_quest_turn_in`.
6. **Quest-State** ACTIVE required → else `quest_not_active`/`quest_already_completed`.
7. **Objective-Kind** all `Kill` → else `quest_turn_in_unsupported`.
8. **Objectives complete** (`objectives_complete`) → else `quest_objectives_not_met`.
9. **Reward prepare** (`RewardBundle.prepare`, overflow-checked) → else `quest_reward_overflow`.
10. **No second open order** (`quest_completion_pending`) → else `quest_turn_in_pending`.
11. **Durable publication** `finish_quest_completion` →
    `guarded_complete_quest` (ACTIVE→COMPLETED, DB-Guard) +
    `spool::publish_quest_completion` (Receipt `character_quest_commits`,
    Migration 025) + `InventoryState`/Snapshot write, **same transaction**.
    Uncertain/Conflict → `quest_turn_in_pending` (entry stays open, retry pass).

`receipt_id` ist **deterministisch** derselben String per Commit
(`quest_commit_id(pid, quest_id, base_revision)`) — Retry-recoverable, no recompute.

## 3. S2C (Rust → Godot) — Payload in NPC_TEXT 8

### 3.1 `S2C_quest_offer` (from `quest_offers`)

```json
{"ok": true, "action": "quest_offers", "npc_id": "...",
 "quests": [{"quest_id","title_key","description_key","state":"AVAILABLE","can_accept":true}]}
```

### 3.2 `S2C_quest_turn_in_result` (from `quest_turn_in`)

Success:
```json
{"ok": true, "action": "quest_turn_in", "npc_id": "...", "quest_id": "...",
 "state": "COMPLETED", "rewards": {"exp": <u64>, "idia": <u64>}}
```
Reject:
```json
{"ok": false, "action": "quest_turn_in", "npc_id": "...", "reason": "<stable>"}
```

**Reward-Field-Mapping (V1.3 ↔ V1.2a):** die V1.3-Illustration `rewards{experience,
gold, items}` **maps** to the implemented `rewards{exp, idia}`:
`experience = exp`, `gold = idia`. `items`-Rewards remain **open** (§27.26,
docs/Quest-Turn-In-Persistenz.md) — **not** invented, **not** sent. `receipt_id`
ist der Spool-/Receipt-Beleg (`character_quest_commits`); it is **not** embedded in
the NPC_TEXT frame (the frame carries `state`+`rewards` only).

## 4. Stable reject-reasons (Client-Feedback-Contract)

`unknown_quest`, `wrong_quest_giver`, `wrong_quest_turn_in`, `quest_already_active`,
`quest_already_completed`, `quest_not_active`, `quest_not_available`,
`quest_accept_pending`, `quest_turn_in_pending`, `quest_turn_in_busy`,
`quest_objectives_not_met`, `quest_turn_in_unsupported`, `quest_reward_overflow`,
`quest_persistence_failed`, `unknown_npc`, `npc_unavailable`, `character_dead`,
`out_of_range`.

## 5. Client-Helper-Contract (`shared/protocol.gd`)

* `encode_quest_turn_in_frame(seq, npc_id, quest_id, npc_content_id, item_uuids)` → C2S 6
* `encode_quest_accept_frame(seq, npc_id, quest_id)` → C2S 6
* `decode_quest_offer_frame(frame)` → `quests`-Array (else `[]`)
* `decode_quest_turn_in_frame(frame)` → validated `{npc_id,action,quest_id,item_uuids}` (else `{}`)
* `decode_quest_text_frame(frame)` → `quest_turn_in_result`-Payload (else `{}`)

Alle Decoders are seq-Korrelation-tolerant und decide by `type`+`action`.
