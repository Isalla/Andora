# Andora – Ausrüstungsführung

**Status: beschlossen als Zielregel. Umsetzung folgt (eigener Auftrag).
Noch nicht implementiert.**

Dieses Dokument regelt, **welche ausgerüstete Ausrüstung gerade wirkt**.
Die Slot-Zuordnung (`inventory_system.md` §7) bleibt unberührt.

## 1. Grundprinzip

> **Ein Gegenstand kann ausgerüstet sein, ohne geführt zu werden.**

Geführt wird genau eine **Last**:

* eine Nahkampfwaffe in der Hauptwaffe (mit oder ohne Schild), oder
* eine Distanzwaffe im Distanzwaffenslot.

Nur die geführte Last liefert ihre Kampf- und Nutzungswerte sowie Fähigkeiten,
deren Wirkung an die **aktive Führung** gebunden ist (Angriff, waffengebundene
Fähigkeiten und Boni, Schild-Rüstungswert). Dies betrifft **alle** geführten
Gegenstände, nicht nur den Schild.

**Ausnahme:** Normale **Körperrüstung** (Kopf, Schultern, Brust, Beine …) wirkt
unabhängig davon, welche Last geführt wird.

## 2. Führungswechsel

Wechseln der geführten Last — **auch bloßes Ablegen ohne neue Last** — löst
einen **Cooldown von 2 Sekunden** aus:

* Die **verlassene** Last wird **sofort** inaktiv.
* Die **neue** Last wird erst **nach 2 Sekunden** wirksam.
* Während des Cooldowns sind Angriffe und Fähigkeiten der wechselnden Waffe
  gesperrt. **Nicht betroffen:** Aktionen ohne Bindung an die neue Last
  (Bewegung, Tranknutzung, Inventarverwaltung).
* Ein erneuter Wechsel während des laufenden Cooldowns ist **erlaubt** und
  startet die 2 Sekunden **neu** (zeitverzögert, nicht blockiert).
* Normale **Körperrüstung** bleibt während des Wechsels durchgehend wirksam.

Beispiele:

* Bogen → Schwert + Schild: Bogen sofort inaktiv; Schildbonus erst nach 2 s.
* Schwert + Schild → Bogen: Schildbonus sofort weg; Bogen nutzbar erst nach 2 s.

## 3. Invariante

Aus §2 folgt ohne gesondertes Verbot: **Bogen und aktiver Schild sind niemals
gleichzeitig wirksam.** Ein Bogenschütze kann durch Wechseln zwischen Bogen und
Schild keinen durchgehenden Rüstungswert erzwingen.

## 4. Serverautorität

Geführte Last und Cooldown sind ausschließlich serverseitiger Zustand. Der
Client sendet Wechselabsichten; er kann weder die geführte Last eigenmächtig
bestimmen noch den Cooldown verkürzen. Ein Wechsel ist ohne serverseitige
Zustimmung wirkungslos.

Die vom Client angezeigte 2-Sekunden-Animation ist reine Nachvollziehbarkeit;
der maßgebliche Timer läuft beim Server.

## 5. Persistenz und Login

Drei getrennte Ebenen:

* **Gespeicherte Ausrüstung** — welche Gegenstände getragen werden
  (bestehender Ausrüstungszustand).
* **Aktive Führung** — welche Last gerade wirken soll. Diese Zuordnung
  (Slot-Referenz) wird als Teil des Ausrüstungszustands serverseitig gespeichert;
  ohne sie wäre die Führung bei mehreren Optionen (z. B. Bogen **oder**
  Schwert+Schild) nicht eindeutig.
* **Wechselzeit** — der 2-Sekunden-Timer. Er ist **flüchtiger Runtime-Zustand**
  und wird nicht persistiert.

**Beim Login:**

* Die geführte Last wird aus dem serverseitigen Ausrüstungszustand bestimmt;
  der Client kann sie nicht setzen.
* Ist die gespeicherte Referenz ungültig (Slot leer, Gegenstand defekt oder nach
  Regeländerung unzulässig), greift ein sicherer Standard: Hauptwaffe, sonst
  keine Last. Bestehende Defekt-Entfernung beim Einstieg bleibt gelten.
* Die Wirksamkeit der geführten Last beginnt erst nach einer
  **2-Sekunden-Einstiegssperre**. Ein Spieler kann einen laufenden
  Führungswechsel **nicht** durch Logout/Login abkürzen.
* **Körperrüstung** ist während der Sperre durchgehend aktiv.

## 6. Zweck

* Flüssiger Wechsel zwischen Nah- und Fernkampf ohne Inventarzugriff.
* Verhindert das Ansammeln gleichzeitig wirkender Vorteile aus Slots, die als
  Ausrüstung koexistieren dürfen (kein Rüstungspuschen über einen nur
  angelegten Schild; kein Bogen+Schild-Dauer-Duo).

## 7. Nicht Teil dieses Dokuments

* Slot-Zulässigkeit und Kombinationsregeln (`inventory_system.md` §7).
* Reichweiten- und Angriffsdetails (`Kampfsystem.md`); das Prinzip
  „Fernkampf nur bis zu einer Distanz, darunter Nahkampfwaffe" folgt dort.
* Handgehaltene Lichtquellen (Fackel) — Expansion 1.
* Dual-Wielding-Freigabe je Klasse (Teil B, Klassensystem).
* Balancing: 2 Sekunden sind Ausgangswert; Anpassung nach Spieltests ohne
  Architekturumbau.

## 8. Kodierungshinweis (BESTAND)

Der Bestand bietet die Führung **noch nirgends** an. Die Umsetzung berührt
deshalb mehr als einen einzigen Haken:

- **Ausrüstung wirkt heute nicht im Kampf.** Der Welt-Tick nutzt
  `cfg.weapon_damage`, `cfg.weapon_duration_ms` und `cfg.weapon_range` aus
  `CombatCfg` (`src/realm-rs/src/config.rs`, `combat/mod.rs`) – realmweite
  Platzhalterwerte, nicht die Werte der ausgerüsteten Waffe. Die Rüstung des
  Ziels kommt aus dem persistierten Feld `characters.combat_armor`
  (`src/realm-rs/src/db.rs`) bzw. `npcs.armor`, nicht aus ausgerüsteter Rüstung.
- **Definitions-Werte sind vorhanden, aber unausgewertet.** `base_damage`,
  `duration_ms`, `range`, `armor_value`, `attribute_bonuses` und `resistances`
  werden in `item.rs` validiert und über `load_item_definitions` geladen; kein
  Kampf-, Attributs- oder Regenerationspfad liest sie. `stats_active()` /
  `is_broken()` (`item.rs`) werden nur in Tests ausgewertet.
  → Die Werte-aus-der-Definition-Pipeline ist die Voraussetzung; die geführte
  Last ist der **Filter davor**, nicht ihr Ersatz. Beide gehören klar getrennt
  implementiert.
- **Kein Führungszustand.** `Player` kennt nur
  `inventory.equipped: HashMap<EquipSlot, ItemInstance>` und `combat:
  Option<CombatState>` (Ziel + letzter Angriff). Es gibt weder ein Feld für die
  geführte Last noch einen Führungs-Timer.
- **Timer-Vorbild:** `combat/cooldowns.rs` (`start`, `is_ready`,
  `to_epoch_ms`/`from_epoch_ms`, `reset_non_persistent`) sowie
  `Player.last_strike` (RAM-only, ohne Snapshot) zeigen die im Projekt übliche
  Form eines Runtime-Timers. Der Führungs-Coldown ist wie in §5 vorgesehen
  **nicht** zu persistieren: kein Snapshot-Feld, keine Migration, beim Login neu
  zu beginnen (§5 Einstiegssperre).
- **Persistenz der aktiven Führung:** `character_equipment(char_id, slot,
  item_uuid)` (Migration 016) speichert nur Slot → Instanz. Die zusätzliche
  Slot-Referenz „das ist die aktive Last" ist datenmäßig noch nicht vorhanden;
  ihre Form (Spalte, Marker-Zeile, getrennte Tabelle) entscheidet der
  Umsetzungsauftrag und bleibt additiv.
- **Protokoll:** `equip`/`unequip` über C2S 22 `INVENTORY` sind bestehende
  Absichten (`src/realm-rs/src/inventory_actions.rs`, `shared/protocol.gd`).
  Der Führungswechsel ist eine **neue Absicht** ohne Gegenwert vom Client
  (Serverautorität, §4); Reason-Namen gehören in die bestehende stabile Liste
  von `InventoryReject::reason()` bzw. das Gegenergebnis. Ability-Pfad:
  waffengebundene Fähigkeiten müssen die Führung prüfen (§1), der bestehende
  Cooldown-Block `combat/ability.rs` prüft sie nicht.
- **Client (Teststand):** `testclient/tc_inventory.gd` zeigt 21
  Equipment-Slots, aber keine geführte Last und keine 2-Sekunden-Rückmeldung.

---

*Verwandte Dokumente: `inventory_system.md` (§7 Slot-Zulässigkeit und
Kombinationsregeln, §9 Equipment-Boni nur aus aktiven Items, §12 Haltbarkeit),
`item_properties.md` (Ausrüstungs-Achsen, Waffen- und Rüstungswerte),
`Kampfsystem.md` (Grundangriff, Rüstungsreduktion), `Ability-System.md`
(waffengebundene Fähigkeiten), `Serverautoritaet_und_Anti-Manipulation_V1.md`
(Aktion statt Endwert).*
