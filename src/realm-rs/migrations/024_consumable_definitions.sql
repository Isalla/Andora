-- 024_consumable_definitions.sql — realm_state: additive Konsumable-Effect
-- (docs/Attribute_und_Regeneration.md §9 „Consumable-Logik", docs/inventory_system.md §19).
--
-- 1) Zweck: A Heil-/Manatrank carries its effect magnitude SERVER-SIDE on the
--    static definition. Only an explicitly defined consumable may be used; a
--    definition without an effect (or an invalid one) is rejected without any
--    mutation. The magnitude is NEVER taken from the Client.
-- 2) Additive Schema: two NULLable columns on the EXISTING item_definitions
--    table. NULL = no effect (not a consumable). This is a pure extension —
--    no existing column or row is touched, no destructive statement.
-- 3) Value domain: heal_amount / mana_amount are absolute HP/Mana gained per
--    consumed unit (> 0 when present). item.rs validates the domain on load
--    (non-positive, both-at-once, or non-consumable category → rejected).
-- 4) The loader (db::load_item_definitions) SELECTs these columns, so this
--    migration MUST be applied before startup loads definitions. It is
--    registered here but NOT executed against a real DB by this job.
-- 5) No new seeds and no guessed test-item IDs are introduced: dev-content
--    (actual potion definitions with amounts) is a separate content step.

ALTER TABLE item_definitions ADD COLUMN IF NOT EXISTS heal_amount INT NULL;
ALTER TABLE item_definitions ADD COLUMN IF NOT EXISTS mana_amount INT NULL;
