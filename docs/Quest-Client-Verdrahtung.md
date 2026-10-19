# Quest-Client-Verdrahtung (Godot 3.6) — V1.3

Direct Reflex des Server-Antwortformats (docs/Quest-Protokoll-V1.3.md). **Verdrahten**,
not "Entwerfen": no new UI-Design, no dedicated `tc_quest.gd` scene.

## 1. `shared/protocol.gd` (Quest-Helper-Layer)

Adds the 5 Quest-Codec functions (see Quest-Protokoll-V1.3.md §5). They mirror
`protocol.rs` IDs (`C2S.NPC_TALK=6`, `S2C.NPC_TEXT=8`). Decoders are tolerant to
`seq` and decide by `type`+`action`; non-matching → `[]`/`{}`.

## 2. `testclient/tc_main.gd` (Dispatch)

* **C2S** `_on_merchant_requested(npc_id)` (`:544-571`): builds the NPC_TALK
  Quest-Absicht, `net.send(Protocol.C2S.NPC_TALK, payload)`, then
  `merchant.quest_sent(payload, sent_seq)` (seq-Korrelation to the panel).
* **S2C** NPC_TEXT-Branch (`:889-902`):
  ```gdscript
  elif msg_type == Protocol.S2C.NPC_TEXT:
      if str(data.get("action","")).begins_with("quest_"):
          if ok and (state=="ACTIVE" or quests non-empty):
              merchant.on_quest_text(data, seq)   # offer/accept/turn_in result
          else:
              merchant.on_npc_text(data)          # merchant fallback
          # reject path -> merchant.on_reject(action, reason)
  ```
  `seq` is checked **inside** `on_quest_text` (stale/foreign answers change nothing).

## 3. `testclient/tc_merchant.gd` (QuestPanel)

State: `_quests`, `_selected_quest`, `_quest_action`, `_quest_seq`, `_quest_busy`,
`_turn_in_view`. Buttons (built in `_build()`):

| Button | Handler | C2S action |
|--------|---------|-----------|
| `tc_q_load` (Offers) | `_do_quest_offers` | `quest_offers` |
| `tc_q_turn_ins` (Preview) | `_do_quest_turn_ins` | `quest_turn_ins` |
| `tc_q_accept` | `_do_quest_accept` | `quest_accept` |
| `tc_q_turn_in` | `_do_quest_turn_in` | `quest_turn_in` |

Flow:
1. `_quest_request(action, quest_id)` emits `merchant_send(payload)` (Absicht only),
   sets `_quest_busy`/`_quest_seq`, disables buttons while pending.
2. `quest_sent(payload, seq)` records the seq (send-fail → `tc_q_send_failed`).
3. `on_quest_text(data, seq)` (the **only** truth-source):
   * `quest_offers`/`quest_turn_ins` → render `_box_quests` buttons from `title_key`
     (i18n `quest.*.title`), set `_turn_in_view` for preview.
   * `_select_quest(index)` → description from `description_key`, enable
     `tc_q_accept` iff `state==AVAILABLE && can_accept`; in turn-in-view enable
     `tc_q_turn_in` iff `objectives_met && can_turn_in`; reward preview via
     `tc_q_reward_preview{exp,idia}`.
   * `quest_accept` + `state==ACTIVE` → `tc_q_active`, mark local `state=ACTIVE`.
   * `quest_turn_in` + `state==COMPLETED` → `tc_q_completed`, mark `COMPLETED`.
   * `ok=false` → `tc_q_pending`/`tc_q_turn_in_pending` (pending), else
     `tc_q_turn_in_locked` (`quest_turn_in_unsupported`) or `tc_q_error{reason}`.

**No local award:** EXP/Idia/Inventory are **never** applied client-side; only the
server-confirmed `state`+`rewards` are displayed. The client shows and forwards;
the Server alone decides (Prices, Range, Inventory-Rules, Completion).

## 4. i18n Keys (`i18n/de.json`, `i18n/en.json`)

Per-Quest keys referenced by `title_key`/`description_key` from the Quest-Content:
`quest.<id>.title`, `quest.<id>.description`, `quest.<id>.objectives.<obj>`,
`quest.<id>.reward.exp`, `quest.<id>.reward.idia`, plus `quest.turn_in`.
The 20 `tc_q_*` UI-Keys already exist in both locales.

## 5. Testdaten (`.tmp/quest-v13-protokoll/`)

* `quest_content/quests/tc_q_wolves.lua` — Mock Quest-Content (V1 Lua-Format, NOT a
  DB-Seed, NOT loaded at Realm-Start).
* `test-item-uuids.txt` — sample Instanz-UUIDs (Turn-In-Absicht Wire-Shape).
* `frame-*.json` — C2S/S2C Frame-Dumps in the **real** action-based wire format.
