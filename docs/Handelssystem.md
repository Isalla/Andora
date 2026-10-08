# Andora – Sell-/Buyback-History (NPC-Händlerverkauf)

## Status

**Dokumentation (verbindliche Regel, noch nicht implementiert).**

Dieser Abschnitt definiert die Sell-/Buyback-History für an NPC-Händler
verkaufte Items. Er ergänzt die bestehenden autoritativen Regeln:

* `inventory_system.md` – Inventar, Persistenz, Sicherheits-Puffer (§10/§11),
  Item-Lifecycle (§16)
* `item_properties.md` – Item-Eigenschaften, Bindung, Kategorien
* `Player_Persistenz.md` – Dirty-State, periodischer Player-Save, finaler
  Disconnect-/Shutdown-Save
* `Quest-System.md` §27.19 – Questitem-Verkäufe nach Questabbruch
* `Datenbank_Architektur.md` – Realm-Isolation, statische Händler-Grunddaten

Dieser Abschnitt ändert keine bestehenden Regeln. Er erfindet keine neuen
Gameplayregeln für Verkaufspreise, Händlerkategorien, Bindungsmechaniken,
Handelssteuern, AH oder Remote-Händler.

---

## 1. Grundregel

Andora erhält eine Sell-/Buyback-History für an NPC-Händler verkaufte Items.

Die History ist:

* **spielergebunden** – sie gehört zum Charakter (Session-Zustand), nicht zu
  einem bestimmten NPC-Händler
* **nicht händlergebunden** – ein bei NPC A verkauftes Item ist bei jedem
  anderen geeigneten NPC-Händler rückkaufbar
* **ausschließlich Runtime-State** der laufenden Player-Session
* **maximal 20 Verkaufseinträge** groß

Beispiel:

```text
Spieler verkauft ein Schwert bei NPC Borin.
→ Eintrag in die persönliche Buyback-History.

Spieler öffnet NPC Hilda (anderer Händler, andere Region).
→ Das Schwert ist dort weiterhin rückkaufbar.

Händlerwechsel setzt die History NICHT zurück.
```

Die History ist ein Konzept des laufenden Sessions, nicht der Welt:
Ein Chefhändler in Stadt A und ein Klein-Händler in Dorf B teilen
dieselbe persönliche Buyback-History desselben Spielers.

---

## 2. Lebensdauer

Die Sell-/Buyback-History bleibt während der laufenden Session erhalten.

Sie wird **nicht** gelöscht durch:

* Schließen des Händlerfensters
* Wechsel zu einem anderen Händler
* normalen Gebietswechsel
* Tod des Spielers

**Beim Ende der Player-Session / Logout wird die gesamte History verworfen.**

Sie wird **nicht** persistent gespeichert. Insbesondere **nicht** durch:

* periodischen Player-Save (Dirty-State)
* Disconnect-Save
* Graceful-Shutdown-Save

Nach einer neuen Session beginnt die Buyback-History leer.

(Analog zur Behandlung des Sicherheits-Puffers: `inventory_system.md` §11 –
temporärer Runtime-State, kein persistenter Speicher.)

---

## 3. Maximale Größe / FIFO

Die History enthält maximal die letzten **20 Verkäufe**.

Beim 21. Verkauf wird der **älteste** Eintrag verdrängt.

**FIFO-Prinzip:** Ältester Verkauf zuerst heraus. Keine andere Sortier- oder
Prioritätslogik.

Inhalt eines Verkaufseintrags:

* Referenz auf die verkaufte Item-/Stack-Identität (`item_id` der Definition
  plus individuelle `item_uuid` der Instanz)
* tatsächlich verkaufte Menge (`count`)
* vom Spieler erhaltenen Verkaufswert (in Gold)

Die genaue Rust-Datenstruktur wird nicht in diesem Dokumentationsauftrag
festgelegt.

---

## 4. Item-Lifecycle

Ein an einen NPC-Händler verkauftes Item gilt **nicht** sofort als endgültig
vernichtet, solange es noch Bestandteil der Buyback-History ist.

Lebenszyklus des verkauften Items:

```text
Inventar
   ↓
Verkauf (Item verlässt das Inventar)
   ↓
Buyback-History (temporärer Runtime-State)
   ↓
   ├── Rückkauf → Item kehrt ins Inventar zurück
   │
   └── Endgültige Entfernung aus der History
       → Item gilt als endgültig vernichtet
```

Endgültige Entfernung aus der History erfolgt insbesondere:

* **Verdrängung:** Der Eintrag wird durch einen neueren Verkauf aus den
  20 Plätzen verdrängt (FIFO).
* **Session-Ende:** Logout, Disconnect oder Graceful Shutdown verwirft die
  gesamte History.

Erst dann gilt das darin befindliche Item gemäß der bestehenden
Item-Instanz-Lifecycle-Regel (`inventory_system.md` §16, Umsetzung §18) als
endgültig vernichtet: Die persistente `item_instances`-Zeile wird
kontrolliert entfernt — revisionsgebunden im Drain, in derselben Transaktion
wie Inventar, Idia und `persist_revision`, und nur wenn die UUID weder im
Snapshot-Inventar steht noch in der DB referenziert ist. Die Abkopplung
erfolgt über die Inventar-Anschlussstellen (`try_take_instance`,
`retired_uuid` aus `try_insert_instance`); die Sell-/Buyback-History selbst
bleibt dabei reiner Runtime-State (Abschnitt 10) und ist kein
Finalisierungsnachweis. Details regelt `inventory_system.md` §18; Händlerhandler,
Preise und Angebote folgen mit dem späteren Händler-Spiellayer.

---

## 5. Rückkauf

Ein Item kann aus der persönlichen Buyback-History bei einem **geeigneten**
normalen NPC-Händler zurückgekauft werden.

„Geeignet" bedeutet: Der NPC-Händler besitzt eine Händlerfunktion, die
Rückkäufe erlaubt (genaue Kategorisierung bleibt dem Implementierungsauftrag
vorbehalten; in diesem Dokumentationsauftrag wird keine Händlerkategorie
definiert).

**Rückkaufpreis:**

Der Rückkaufpreis entspricht dem Verkaufspreis, den der Spieler beim
ursprünglichen Verkauf erhalten hat.

Es wird **keine zusätzliche Rückkaufgebühr** festgelegt.

Keine konkreten Verkaufspreise, Rabatte, Ruf- oder Fraktionspreise werden in
diesem Dokumentationsauftrag erfasst (diese bleiben Autorität der späteren
Content-/Implementierungsebene).

Der Eintrag in der Buyback-History muss den für den Rückkauf benötigten
ursprünglichen Verkaufswert erhalten können (Feld wie `sell_gold_value`
im Eintrag).

---

## 6. Stacks

Bei verkauften Stack-Items muss die **tatsächlich verkaufte Menge** Teil
des Buyback-Eintrags sein.

Der Buyback-Eintrag muss die vollständige Identität und Menge des verkauften
Stacks erhalten können:

* Item-Definition (`item_id` der Definition)
* individuelle Instanz (`item_uuid`)
* tatsächlich verkaufte Stückzahl (`count`)
* erhaltenen Verkaufswert in Gold

Wird ein Stack teilweise verkauft (der Spieler behält einen Teil), enthält
der Buyback-Eintrag nur den verkauften Teil – nicht den Rest, der im
Inventar verblieben ist.

---

## 7. Verhalten bei vollem Inventar

Beim **Rückkauf** eines Items aus der Buyback-History gilt dasselbe Prinzip
wie beim normalen Loot: Ist nicht genügend Inventarkapazität vorhanden:

* Schlägt der Rückkauf **vollständig** fehl.
* Es wird **kein Gold** abgezogen.
* Das Item bleibt **unverändert** in der Buyback-History.
* Es gibt **keinen Teilrückkauf** aufgrund fehlenden Platzes.
* Der **Sicherheits-Puffer** (`inventory_system.md` §10/§11) wird **nicht**
  verwendet – Puffer und Buyback-History sind völlig unabhängige Systeme.

Der Spieler muss zuerst Inventarplatz schaffen und den Rückkauf erneut
versuchen.

---

## 8. Atomarität

Ein erfolgreicher Rückkauf darf **keinen** inkonsistenten Zustand hinterlassen:

| Fehlerfall | Erlaubt? |
|---|---|
| Item im Inventar, kein Gold abgezogen | **Nein** |
| Gold abgezogen, Item nicht im Inventar | **Nein** |
| Item dupliziert (Buyback-Eintrag bleibt zusätzlich) | **Nein** |
| Buyback-Eintrag ohne erfolgreichen Rückkauf entfernt | **Nein** |
| Gold ohne Item in der Buyback-History | **Nein** |

Der Rückkauf muss als **atomarer Vorgang** (RAM-Änderung + DB-Transaktion)
statisch garantiert werden.

Die konkrete technische Transaktions-/RAM-Implementierung (Transaktionsgrenze
im DB-Bereich, Analogie zum atomaren Questabschluss in `Quest-System.md`
§27.26) bleibt einem späteren Coding-Auftrag vorbehalten.

---

## 9. Abgrenzung zum Inventory-Buffer

Inventory-Buffer und Buyback-History sind **zwei unterschiedliche**
temporäre Player-Systeme:

| Eigenschaft | Inventory-Buffer | Buyback-History |
|---|---|---|
| Zweck | Technischer Sicherheitsbereich (§10/§11 `inventory_system.md`) | Gameplay-/Komfortfunktion für versehentlich verkaufte Items |
| Inhalt | Defekte Equipment-Items bei vollem Inventar | Verkaufte Items, die vom Spieler zurückgekauft werden können |
| Persistenz | Keine (wird nicht gespeichert) | Keine (wird nicht gespeichert) |
| Session-Ende | Verworfen | Verworfen |
| Aktive Nutzung | Kein aktiver Spielerzugriff (nur automatisches Ablegen/Entfernen) | Aktiver Rückkauf über Händlerfenster |

Die Buyback-History darf den Inventory-Buffer **nicht** verwenden.
Die beiden Systeme bleiben in Implementation und Semantik vollständig
getrennt.

---

## 10. Player-Persistenz

Die Sell-/Buyback-History ist **ausdrücklich keine** persistente
Player-Komponente:

* Sie wird **nicht** Teil des Dirty-State / 15-Minuten-Player-Saves
  (`Player_Persistenz.md`).
* Sie wird **nicht** beim Graceful Shutdown persistiert.
* Sie wird **nicht** beim Disconnect-Save persistiert.

Persistente Änderungen, die durch den eigentlichen Verkauf oder den
Rückkauf entstehen (Inventaränderung, Goldänderung, Item-Instanz-Lifecycle),
müssen später technisch korrekt behandelt werden – die Sell-/Buyback-History
selbst gehört aber nicht zur persistenzpflichtigen Datenmenge.

---

## 11. Nicht festgelegt (in diesem Dokumentationsauftrag)

In diesem Abschnitt werden **keine** neuen Regeln festgelegt für:

* konkrete Händlerpreise / Verkaufsformeln
* Händlerkategorien (sofern noch nicht separat dokumentiert)
* Ruf- / Fraktionsrabatte
* Verkaufsverbote (Soulbound, Questitems – siehe `Quest-System.md` §27.19)
* Remote-Händler
* Auktionshaus (`Auktionshaus und Marktplatz`)
* Handelssteuern
* Client-UI oder Protokoll-Messages
* maximale Buyback-Preise oder Mengen

Bestehende Regeln hierzu bleiben unverändert autoritativ.

---

## 12. Bezug zu bestehender Dokumentation

| Dokument | Bezug |
|---|---|
| `inventory_system.md` §11 | Sicherheits-Puffer als analoger temporärer Runtime-State (nicht persistiert) |
| `inventory_system.md` §16 | Item-Instanz-Lifecycle: Endgültig vernichtete Iteminstanzen → kontrollierte Persistenzentfernung |
| `item_properties.md` | Verkäufe als Item-Usage-Art; Bindungsregeln für Verkäufe |
| `Quest-System.md` §27.19 | Questitems nach Abbruch: an geeignete NPC-Händler verkäuflich, ohne konkrete Preisvorgaben |
| `Player_Persistenz.md` | Sell-/Buyback-History ausdrücklich nicht Teil der Dirty-/Persistenzkomponenten |
| `Datenbank_Architektur.md` | Händler-Grunddaten als statische Realm-Definitionen (Realm-DB); Verkäufe als realmgebundene Operation |
