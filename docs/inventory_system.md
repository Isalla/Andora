# Andora – Inventory-System V1

## Status

**Implementiert (Konzept – verbindlicher Stand).**

Rust-Umsetzung in `src/realm-rs/src/inventory.rs` (reine Zustandslogik, 18 Unit-Tests,
uncommittet): Grundinventar (Basis-Slots via `INVENTORY_BASE_SLOTS`, Default 8), ausrüstbare
Rucksäcke/Taschen (eigene Slots, benennbar, keine Kategoriebindung; optional
`INVENTORY_MAX_EQUIPPED_BAGS`), 21 Equipment-/Funktionsslots (`EquipSlot`), temporärer
Sicherheits-Puffer (verfällt beim Logout). Persistierung über Migration 016 und
`db.rs` (`load_inventory`/`save_inventory`, Transaktions-Vollwrite; Puffer wird nicht
persistiert). Anbindung: Laden + serverseitiges Entfernen defekten Equipments beim Einstieg
(`handle_hello`), Speichern beim Disconnect (`net.rs`); Config in `config.rs`.

Kein Client-UI, keine Protokoll-IDs (folgen mit dem Spiellayer). Loot-/Quest-Aufrufer
nutzen die Kern-API (`try_add`/`fits`/`capacity_for`) — im V1-Loop noch kein Aufrufer.

Verbindliche Lifecycle-Regeln: Abschnitt 11 legt den Sicherheits-Puffer als
ausschließlich temporären, nie persistierten Runtime-State fest; Abschnitt 16
regelt den Item-Instanz-Lifecycle und verwaiste Iteminstanzen. Für diese
Dokumentation wurde kein Produktionscode geändert.

Die bestehende Item-System-V1-Architektur (`item_properties.md`, Abschnitt „Item System V1 – Implementierungsstand") ist verbindlich und wird hier nicht verändert.

---

## 1. Grundprinzip

Das Inventory-System verwaltet:

* Grundinventar
* mehrere ausrüstbare Rucksäcke / Taschen
* Equipment- und Funktionsslots
* einen temporären Sicherheits-Puffer für Ausnahmefälle

Verbindlich gilt:

> **1 Item oder 1 Stack = exakt 1 Inventarslot.**

Die alte Item-„size"-Mechanik ist verworfen.
Items belegen niemals mehrere Inventarslots aufgrund ihrer Größe.

Numerische Quality und Rarity sind getrennte Eigenschaften gemäß Item System V1
(vereinbart in `item_properties.md`, Abschnitt „Numerische Qualität" und
„Seltenheiten"). Alte Dokumentation, die Quality und Rarity koppelt oder
daraus Item-Größe ableitet, wurde hiermit korrigiert.

---

## 2. Grundinventar und Rucksäcke

Der Charakter besitzt ein Grundinventar und mehrere ausrüstbare Rucksack- / Taschenplätze.

### Grundinventar

Das Grundinventar hat eine feste Anzahl von Slots
(derzeitiger Inhaltswert: **8 Slots**).

Die konkrete Startgröße ist ein Content-Wert; sie wird nicht als Architekturregel
im Inventory-Kern interpretiert und kann bei Bedarf über Konfiguration angepasst werden.

### Ausrüstbare Rucksäcke / Taschen

Jeder ausgerüstete Rucksack:

* besitzt eine eigene Anzahl von Slots,
* stellt einen eigenen Inventarbereich dar,
* kann vom Spieler individuell benannt werden.

Beispiele für Namen:

* Ausrüstung
* Rohstoffe
* Zu verkaufen
* Tränke

Der Name ist ausschließlich Organisations- / Anzeigedaten.
Eine Tasche namens „Rohstoffe" darf trotzdem beliebige normale Items enthalten.
Es gibt **keine** serverseitige Item-Kategoriebeschränkung aufgrund des Taschennamens.

### Gesamtkapazität

Die Gesamtkapazität ergibt sich aus:

```text
Grundinventar (Slots)
+ Summe (Slots aller ausrüsteten Rucksäcke)
= Gesamtkapazität
```

### Kein Level-Hardcoding für Rucksackgrößen

Die alten fest an Charakterlevel gekoppelten Rucksackgrößen
(4/8/12/16/20/24/30/38/42/48/52/64) werden **nicht** als harte Rust-Logik
übernommen.

Tier- und Progressionsgrenzen sollen nicht im Inventory-Kern hartcodiert werden.

Rucksackgrößen und deren Erwerb sind Content- und Progressionsfragen
(siehe `Tier-Progression.md`).

---

## 3. Client-Darstellung

Der Realm verwaltet Inventarbereiche, Slots und Inhalte.

Die Darstellung ist Sache des Godot-Clients.

Der Client soll später beispielsweise:

* Rucksäcke als Tabs / Reiter darstellen können,
* mehrere Rucksäcke an ein gemeinsames Inventarfenster binden können,
* gegebenenfalls mehrere Inventarfenster verwenden können.

Diese UI-Gruppierung ist **keine** Realm-Server-Architektur.

---

## 4. Stacks

Die Stackregeln stammen aus Item System V1
(siehe `item_properties.md`, Abschnitt „Kategorien und Stack-Regeln").

Beim Hinzufügen eines stackbaren Items gilt grundsätzlich:

1. Vorhandene passende Stacks auffüllen.
2. Verbleibende Menge auf freie Slots verteilen.
3. Wenn nicht alles untergebracht werden kann, darf der Rest nicht verschwinden
   (die normale Aufnahme schlägt fehl; siehe Abschnitt 5).

Individuelle Items mit `item_uuid` bzw. individuellen dynamischen Eigenschaften
werden **nicht** mit normalen identischen Items zusammengestackt.

Es wird **keine** parallele Stack-Architektur im Inventory-System erfunden.

---

## 5. Volles Inventar

Normales Looten darf den temporären Sicherheits-Puffer **nicht** verwenden.

Ist das normale Inventar voll und kann auch kein vorhandener Stack aufgefüllt werden:

* Schlägt die normale Aufnahme fehl.
* Das World-Loot-Item bleibt in der Welt.

Es gibt kein:

* automatisches Postfach,
* versteckten Überlaufspeicher,
* automatisches temporäres Loot-Inventar.

---

## 6. Questbelohnungen

Quests folgen dem normalen Inventarprinzip.

Vor Abschluss einer Quest mit Itembelohnung muss geprüft werden,
ob die benötigten Items normal aufgenommen werden können.

Ist nicht genügend Platz vorhanden:

* Kann die Quest nicht abgeschlossen werden.
* Die Belohnung wird **nicht** in den Sicherheits-Puffer gelegt.
* Der Spieler muss zuerst Inventarplatz schaffen.

---

## 7. Equipment- und Funktionsslots

Für V1 sind folgende **21 Slots** vorgesehen:

| Nr | Slot |
|----|------|
| 1 | Kopf |
| 2 | Ohrring 1 |
| 3 | Ohrring 2 |
| 4 | Hals |
| 5 | Schultern |
| 6 | Arme |
| 7 | Hände |
| 8 | Brust |
| 9 | Taille |
| 10 | Beine |
| 11 | Füße |
| 12 | Rücken |
| 13 | Hauptwaffe |
| 14 | Nebenhand |
| 15 | Ring 1 |
| 16 | Ring 2 |
| 17 | Lichtquelle |
| 18 | Distanzwaffe |
| 19 | Köcher / Munition |
| 20 | Essen |
| 21 | Trinken |

Anmerkungen:

* „Lichtquelle" soll technisch nicht ausschließlich auf Fackeln beschränkt sein.
  Später können beispielsweise Fackeln, Laternen oder andere passende
  Lichtquellen verwendet werden.
* Der Distanzwaffen-Slot ist vom Haupt- / Nebenhand-System getrennt.
* Der Köcher- / Munitionsslot ist für die zur Distanzwaffe gehörende
  Munition vorgesehen.

Die konkrete Prüfung, welche Item-Kategorien / Definitionen in welchen
Equipment-Slot passen, muss später mit Item System und Equipment-Logik
verbunden werden. Es werden keine neuen Itemtypen erfunden.

---

## 8. Essen und Trinken

Essen und Trinken besitzen eigene Funktionsslots (Slots 20 und 21).

Diese Slots existieren, damit Nahrung / Getränke später automatisch
konsumiert werden können.

**Wichtig:**

* Die eigentliche Auto-Consume- / Regenerationslogik gehört **nicht** zu Inventory V1.
* Inventory V1 stellt lediglich die Slots und ihren Zustand bereit.

Normale Tränke bleiben normale Inventargegenstände.

Heil- / Manatränke:

* Besitzen **keinen** eigenen Auto-Consume-Slot.
* Werden **manuell** benutzt.
* Dürfen **nicht** automatisch aufgrund niedriger HP / Mana konsumiert werden.

(Regenerative Wirkung von Essen / Getränken mit Dauerwirkung versus
direkte Heil- / Manatränke: konzeptionelle Abgrenzung in
`Attribute_und_Regeneration.md`, Abschnitt 9.)

---

## 9. Equipment und Inventar

Ein ausgerüstetes Item befindet sich **nicht** gleichzeitig in einem
normalen Inventarslot.

Beim normalen manuellen Ablegen / Ausziehen muss das Inventory-System
entsprechend einen Zielplatz verwalten und prüfen.

Equipment-Boni dürfen ausschließlich von tatsächlich aktiven und
funktionsfähigen ausgerüsteten Items stammen.

---

## 10. Temporärer Sicherheits-Puffer

Es gibt einen kleinen temporären Sicherheits-Puffer für serverseitige
Ausnahmefälle.

Dieser Puffer ist ausdrücklich **keine** zusätzliche Inventarkapazität.

Er darf **nicht** verwendet werden für:

* normales Looten,
* Questbelohnungen bei vollem Inventar,
* Crafting-Ausgaben als normale Umgehung,
* zusätzliche reguläre Lagerkapazität.

### Wichtiger Anwendungsfall

Ein ausgerüstetes Item erreicht 0 Haltbarkeit und muss aus dem aktiven
Equipment entfernt werden.

Dadurch wird verhindert, dass ein kaputtes Item weiterhin Statuswerte
oder andere Equipment-Effekte liefert
(siehe Abschnitt 12).

Verbindliche Regel:

* Ist normaler Inventarplatz vorhanden, wird das Item
  **automatisch** in das normale Inventar gelegt.
* Ist das normale Inventar voll, wird das Item
  **automatisch** in den temporären Sicherheits-Puffer gelegt.

Die automatische Ablegung gilt ausschließlich für diesen Ausnahmefall
(serverseitige Entfernung aus dem Equipment). Das Hinauslegen aus dem
Puffer ins normale Inventar folgt den Regeln aus Abschnitt 11.

---

## 11. Verhalten des Sicherheits-Puffers

Items im Sicherheits-Puffer werden **niemals** automatisch in frei werdende
Inventarslots verschoben.

Auch wenn später ein Inventarslot frei wird, bleibt das Item im Puffer.

Der Spieler muss:

1. selbst Platz im normalen Inventar schaffen,
2. das Item im Puffer bewusst anklicken,
3. dadurch die Übertragung ins normale Inventar auslösen.

Der Server prüft beim Klick erneut, ob Platz vorhanden ist.

### Begründung

Ein Spieler könnte beispielsweise beim Händler durch Doppelklick Items
verkaufen. Würde ein Puffer-Item automatisch in einen gerade frei
gewordenen Slot springen, könnte ein weiterer Klick versehentlich
das wertvolle Item verkaufen.

> **Puffer → Inventar ausschließlich durch bewusste Spieleraktion.**

Items im Puffer können dort **nicht**:

* benutzt,
* ausgerüstet,
* verkauft,
* gehandelt,
* gecraftet / verarbeitet

werden.

Der Puffer ist **temporär**.

> **Beim Logout verbleibende Items im Puffer gehen verloren.**

Dies muss in der Dokumentation ausdrücklich als bewusstes
Sicherheits- / Ausnahmesystem beschrieben werden.

### Persistenz-Ausschluss (verbindlich)

Der Sicherheits-Puffer ist ausschließlich temporärer Runtime-State innerhalb
einer laufenden Player-Session. Er ist **kein** persistenter Inventarspeicher.

Der Puffer wird deshalb durch **keinen** Player-Save geschrieben:

* periodischer Player-Save (Dirty-State / periodischer Flush,
  `Player_Persistenz.md`)
* Disconnect-Save
* Graceful-Shutdown-Save

Ein erfolgreicher normaler Player-Save verändert diese Regel nicht: Auch nach
einem erfolgreichen Speichern bleibt der Pufferinhalt Teil des flüchtigen
Runtime-Zustands und gelangt nicht in die persistierte Inventarrepräsentation.

Abgrenzung zur Player-Persistenz (`Player_Persistenz.md` §6): Die Komponente
„inventory dirty" umfasst ausschließlich die persistenten Inventarbestandteile
(Grundinventar, Rucksäcke / Bag-Slots, Equipment). Der Sicherheits-Puffer fällt
ausdrücklich **nicht** unter die Dirty- / Persistenzpflicht.

### Session-Ende – Verwerfen (verbindlich)

Beim Ende der Player-Session verbleibende Buffer-Items werden **verworfen**.
Dies entspricht der bestehenden Gameplayregel:

> **Beim Logout verbleibende Items im Puffer gehen verloren.**

Es darf **keinen** Mechanismus geben, durch den verworfene Buffer-Items bei
einem späteren Login aus alten DB-Daten wiederhergestellt werden. Ein Puffer-Item,
das einmal dem Session-Ende zum Opfer gefallen ist, bleibt endgültig vernichtet
(bewusster Gameplay-Verlust gemäß dieser Regel, kein verlorener persistenter
Besitz).

Die RAM-seitige Entsorgung ist bereits vorhanden: `drop_buffer()` (`inventory.rs`)
im Disconnect-Pfad (`net.rs`). Die unten beschriebene DB-Inkonsistenz muss
bereinigt werden, damit die „keine Wiederherstellung"-Regel auch technisch gilt.

### Bekannte DB-Inkonsistenz (in einem späteren Coding-Auftrag zu bereinigen)

Der Read-only-Audit hat folgende technische Inkonsistenz festgestellt:

* `write_inventory` (`db.rs`) persistiert den Puffer **nicht** (Puffer wird im
  Transaktions-Vollwrite übergangen; dieser Zustand entspricht dem vorgesehenen
  Persistenz-Ausschluss).
* `load_inventory` (`db.rs`) kann jedoch vorhandene `inventory_buffer`-Zeilen
  laden, sodass alte Puffer-Einträge bei einem späteren Login wieder erscheinen
  können. Das widerspricht der verbindlichen Regel „keine Wiederherstellung
  verworfener Buffer-Items".
* `wipe_logout_buffer` (`db.rs`) existiert derzeit als `#[allow(dead_code)]`-
  Funktion ohne aktiven vollständigen Pfad: In V1 werden `inventory_buffer`-Zeilen
  nie geschrieben, daher wäre der Wipe ein No-Op; die Funktion steht bisher nur
  als Entwurf bereit.

Diese Punkte müssen bei der späteren Implementierung bereinigt werden – Zielzustand:
ein verworfenes Puffer-Item ist endgültig weg und kann nicht aus `inventory_buffer`
wiederbelebt werden.

Für diesen Dokumentationsauftrag wird **keine Migration und kein Rust-Code**
geschrieben; die konkrete technische Bereinigung (z. B. Pufferzeilen gar nicht
mehr laden/erzeugen oder sauber wippen) bleibt dem Coding-Auftrag vorbehalten.

---

## 12. Haltbarkeit

Item System V1 definiert bereits:

> 0 Haltbarkeit = Item liefert keine Gameplay-Werte.

Für die Inventory- / Equipment-Integration gilt zusätzlich:

Ein Item mit 0 Haltbarkeit darf nicht als aktives Equipment weiterwirken.

Falls das bestehende Item-System aktuell lediglich die Werte deaktiviert
und das Item technisch ausgerüstet lässt, wird die gewünschte spätere
Inventory- / Equipment-Integration hier dokumentiert:

* Ein defektes Item (0 Haltbarkeit) muss beim Ablegen / Entfernen
  aus dem Equipment-Slot wie ein normaler Gegenstand behandelt werden:
  Es benötigt einen Zielplatz im Inventar.
* Kann kein Zielplatz bereitgestellt werden, greift der Sicherheits-Puffer
  (Abschnitt 10).
* Der Zustand „defekt" bleibt an der Item-Instanz erhalten
  (siehe `item_properties.md`, Abschnitt „Haltbarkeit").

In diesem Dokumentationsauftrag wird **kein Rust-Code** geändert.

---

## 13. Nicht Teil von Inventory V1

Nicht jetzt auszuarbeiten oder zu implementieren:

* Loot-System
* Händler-System
* Auktionshaus
* Crafting-Ausführung
* automatische Sortierung
* automatische Zuordnung nach Taschennamen
* Godot-Fenster- / Tab-Implementierung
* vollständige Auto-Essen- / Auto-Trinken-Logik
* neue Progressions- / Tierregeln

Es werden nur die notwendigen Integrationspunkte dokumentiert.

---

## 14. Alte Dokumentation – Korrekturen und Korrekturbedarf

Die folgenden veralteten Aussagen wurden im Rahmen dieser Aktualisierung
beseitigt oder korrigiert (vor allem in dieser Datei, teilweise auch
in angrenzenden Dokumenten):

| Veraltete Aussage | Status |
|---|---|
| Item size / Items belegen mehrere Slots | **Verworfen.** 1 Item = 1 Slot. |
| Quality = Rarity (Quality 1 Gray … 5 Purple) | **Verworfen.** Numerische Quality und Rarity sind getrennt (Item System V1). |
| Höhere Rarity benötigt größere Items / mehr Slots | **Verworfen.** Keine Item-Größe. |
| Feste Level → Rucksackgröße (4/8/12/…/64) | **Nicht als harte Inventory-Kern-Logik.** Content-/Progressionsfrage. |
| Crafting mit „proper sizing calculations" | **Verworfen.** Keine Item-Größe, keine Multi-Slot-Berechnung. |

Bestehende sinnvolle Inhalte bleiben erhalten, sofern sie den neuen
verbindlichen Entscheidungen nicht widersprechen.

---

## 15. Integrierte Systeme – Bezugspunkte

| System | Bezug |
|---|---|
| Item System V1 | Stack-Regeln, Gewicht, Bindung, Haltbarkeit, Kategorien, Seltenheiten, numerische Quality (`item_properties.md`) |
| Attribute und Regeneration | Essen / Getränke / Tränke – konzeptionelle Abgrenzung (`Attribute_und_Regeneration.md`, Abschnitt 9) |
| Tier-Progression | Rucksackgrößen und deren Erwerb als Content-Frage (`Tier-Progression.md`) |
| Quest-System | Questbelohnungen folgen dem normalen Inventarprinzip |
| Loot-System | World-Loot bleibt bei vollem Inventar in der Welt (nicht Teil von V1) |
| Coordinator / Mail-System | Rückerstattungen über das Realm-Mail-System; kein Bestandteil von Inventory V1 (`Coordinator.md`) |

---

## 16. Item-Instanz-Lifecycle und verwaiste Instanzen

Dieser Abschnitt regelt die persistente Repräsentation von Item-Instanzen
(`item_instances`), insbesondere deren Entfernung, wenn die zugehörige
Spiel-Instanz endgültig vernichtet wurde.

### Grundregel

Eine endgültig vernichtete Iteminstanz darf nicht unbegrenzt als verwaiste
persistente `item_instance`-Zeile in MariaDB verbleiben.

> Wenn eine Iteminstanz endgültig keinen gültigen persistenten Besitzer / keine
> gültige persistente Platzierung mehr besitzt und laut Gameplay vernichtet
> wurde, muss auch ihre persistente Repräsentation kontrolliert entfernt werden.

### Bevorzugt: Lifecycle-basiertes Entfernen

Bevorzugt wird **Lifecycle-basiertes Entfernen beim endgültigen Item-Untergang**
(d. h. zum Zeitpunkt des Lebenszyklus-Endes des Items, nicht durch pauschale
Rückwärts-Scans).

Beispielhafte Lifecycle-Endpunkte ohne neue Gameplayregeln:

* Eine Iteminstanz verliert ihre letzte gültige Platzierung und wurde laut
  Gameplay vernichtet (z. B. verbraucht oder entfernt).
* Ein Puffer-Item, das beim Session-Ende gemäß Abschnitt 11 verworfen wird –
  die Instanz ist damit endgültig vernichtet.

Die konkrete Umsetzung (Zeitpunkt, Transaktionsgrenze) wird im Implementierungs-
auftrag gegen den vorhandenen Code entschieden (vorhandene Bausteine: `write_*`-
Transaktionshelfer, `wipe_logout_buffer` als Entwurf, `db.rs`).

### Kein pauschaler periodischer SQL-Garbage-Collector (V1)

Für V1 wird **kein** pauschaler periodischer SQL-GC als Grundlösung festgelegt
(also kein regelmäßiger Voll-Scan über sämtliche `item_instances`-Zeilen mit
automatischer Löschung).

Begründung analog `Runtime_Lifecycle_Cleanup.md` §7: bevorzugt werden
Lifecycle-basierte Entfernungen statt periodischer Voll-Scans. Ein periodischer
Sweep darf später allenfalls als begrenzter Sicherheitsmechanismus erwogen
werden, nicht als primäre Säuberungslogik.

### Safety-/Diagnose-Mechanismus (getrennt)

Ein späterer Diagnose- / Safety-Mechanismus darf verwaiste Iteminstanzen
**erkennen** und melden (Monitoring-/Telemetrieebene).

Das ist **getrennt** vom normalen Item-Lifecycle: Seine Ergebnisse führen
nicht automatisch zu Löschungen im normalen Spielablauf. In diesem
Dokumentationsauftrag wird **keine** automatische Löschlogik für einen solchen
Safety-Mechanismus entworfen.

### Abgrenzung zur Player-Persistenz

* Persistente Inventarbestandteile → Dirty-State bzw. periodischer / finaler
  Player-Save (`Player_Persistenz.md`). Der Sicherheits-Puffer ist ausdrücklich
  ausgeschlossen (Abschnitt 11).
* Der Questabschluss mit seinen beteiligten persistenten Änderungen bleibt eine
  sofortige atomare Transaktion (`Quest-System.md` §27.26) und wird von den
  Regeln dieses Abschnitts **nicht** verändert.
* Dieser Abschnitt legt keine neuen Regeln für Inventarkapazität, Loot,
  Questbelohnungen, Itemhandel, Haltbarkeit, Puffer-Größe oder die Wiederher-
  stellung verlorener Puffer-Items fest.
