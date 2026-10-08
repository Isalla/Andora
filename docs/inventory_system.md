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

**Instanzgenaue Kernoperationen eingebaut:** `try_take_instance` und
`try_insert_instance` in `src/realm-rs/src/inventory.rs`; API-/UUID-Vertrag
in Abschnitt 17. Verifikation: 12 neue Produktionstests, vollständige
Realm-Suite offline 710/710 bestanden (698 als berichteter Vorlaufstand).
Clippy `--offline --all-targets --message-format=json` auf Basis und neuem
Stand: jeweils 102 Diagnosen, keine neuen/entfallenen (Vergleich nach
Code, Meldung, Quelldatei und primärem Quelltext, unabhängig von Zeilennummern).

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

Die konkrete Umsetzung (Zeitpunkt, Transaktionsgrenze) ist für den
Single-Process-Realm entschieden und in Abschnitt 18 beschrieben
(revisionsgebundene Finalisierung im Drain, selbe Transaktion wie Inventar,
Idia und `persist_revision`; vorhandene Bausteine: `write_*`-
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

---

## 17. Instanzgenaue Kernoperationen

Diese reine RAM-API ist die Inventargrundlage für spätere NPC-Verkäufe und
Buyback. Sie vergibt **keine** Handels-, Bindungs- oder Questfreigabe und
führt weder Währungsänderungen noch History-/DB-Operationen aus. Der Aufrufer
muss seine Fachprüfungen und die Dirty-Markierung selbst vornehmen.
Bestehende Loot-, `try_add`-/`try_remove`-, Equip- und Pufferpfade verwenden
weiter ihre bisherigen Operationen.

### Entnehmen

`try_take_instance(uuid, qty) -> Result<ItemInstance, InventoryError>`:

* Nur ein eindeutig identifizierter Stack im Grundinventar oder in den
  Inhalts-Slots ausgerüsteter Taschen ist zugänglich. Equipment, die
  Taschencontainer selbst und der Sicherheits-Puffer sind keine Entnahmeorte.
* Vollentnahme liefert die Originalinstanz einschließlich ihrer UUID.
* Teilentnahme: Rest behält die Original-UUID; der entnommene Teil erhält
  eine neue UUID aus der vorhandenen Erzeugung. Nur Mengen/Teil-UUID ändern
  sich; Bindung, Haltbarkeit, Hersteller und sämtliche Modifier bleiben erhalten.
* Unbekannte/leere UUID, Menge <= 0, Menge über dem gewählten Stack oder
  eine mehrdeutige UUID werden ohne Mutation abgelehnt. Es wird nicht aus
  anderen Stacks derselben Definition ergänzt.

### Vollständiges Wiedereinsetzen

`try_insert_instance(def, incoming) -> Result<InstanceInsertOutcome, InventoryError>`:

* Definition und vollständige Instanz werden validiert; insbesondere muss
  `count` innerhalb `1..=max_stack` liegen. Die geliehene Eingabe bleibt erhalten.
* Zuerst passende normale Stacks auffüllen: Basis-Slots, dann Tascheninhalte.
  Es gilt die bestehende Plain-Stack-Regel (gleiche Itemdefinition, keine
  individuellen Modifier und keine Haltbarkeit), zusätzlich müssen Bindung
  **und Hersteller** übereinstimmen. Inkompatible Eigenschaften verschwinden
  niemals durch Verschmelzung.
* Ein verbleibender Teil belegt einen freien normalen Slot mit der
  eingehenden UUID und allen Eigenschaften. 1 Item/Stack = 1 Slot;
  Equipment und Puffer liefern keine zusätzliche Aufnahmekapazität.
* Die gesamte Änderung wird auf einem Arbeitsklon vorbereitet. Fehlt Platz
  für den Rest, bleibt auch jeder zuvor probeweise aufgefüllte Stack unverändert.
* Das Ergebnis meldet `merged_count`. Nur wenn die **gesamte** Eingabe in
  bestehende Stacks verschmolzen ist, liefert `retired_uuid` die aufgegebene
  eingehende UUID. Andernfalls ist `retired_uuid = None` und diese UUID bleibt
  am neuen Reststack erhalten. Bestehende Ziel-UUIDs bleiben erhalten.
* UUID-Kollisionen werden im gesamten eigenen Inventar einschließlich
  Equipment und Puffer geprüft. Die Gesamtmenge derselben Definition im
  normalen Inventar und die Additionen beim Stacken müssen in i64 darstellbar
  sein; Überlauf wird ohne Mutation abgelehnt.

### Lifecycle-Anschluss

`retired_uuid` ist ein ausdrücklicher Hinweis für die spätere
revisionsgebundene Instanzfinalisierung, **kein** sofortiger DB-Löschauftrag.
Diese API prüft nur inventarlokale UUID-Eindeutigkeit. Globale Eigentumsprüfung,
History-/Spool-Anschluss und der endgültige persistente Instanz-Lifecycle
bleiben beim späteren Spiellayer. Die Operationen selbst markieren keinen
Player-Dirty-State.

---

## 18. Revisionsgebundene Instanzfinalisierung (eingebaut, Single-Process-Realm)

Dieser Abschnitt beschreibt die eingebaute Umsetzung der §16-Regel für den
Single-Process-Realm (genau ein Realm-Serverprozess je RealmDB,
`Login_Realm_Architektur.md` Abschnitt „Realm-Server“,
`Datenbank_Architektur.md` §17).

**Neubewertung des bisherigen Zuständigkeitsblockers:** Die Finalisierung war
zuvor blockiert, weil bei mehreren gleichzeitig autoritativen Prozessen
derselben RealmDB unklar blieb, welcher Prozess eine abgekoppelte UUID
verbindlich finalisieren darf (fremder RAM-Bestand könnte die UUID noch
führen). Unter dem verbindlichen Single-Process-Betriebsvertrag entfällt
dieser Blocker: Der eine Realm-Prozess besitzt den maßgeblichen RAM-Bestand
seiner RealmDB; Zuordnung und Finalisierung liegen allein bei ihm. Das ist ein
Betriebsvertrag, kein technischer Doppelstartschutz — gegen vertragswidrige
parallele Starts wird keine Sicherheit behauptet; ein solcher Betrieb ist
kein geprüfter Zustand.

**Trennung (verbindlich):**

* Runtime-History (`SellHistory`, `src/realm-rs/src/item_lifecycle.rs`):
  ausschließlich Runtime-State der laufenden Session (max. 20 Einträge, FIFO),
  nicht persistent, Verwerfen am Session-Ende. Takeover und RAM-Übernahme
  innerhalb des laufenden Prozesses erhalten sie.
* Dauerhafte Lifecycle-Metadaten (`ItemLifecycle` → Snapshot-Sicht →
  `item_instance_finalizations`, Migration 021): ermöglichen nur Zuordnung und
  kontrollierte Finalisierung abgekoppelter UUIDs.

**Anschlussstellen (ohne Händlerhandler, Preise oder Angebote):**
`try_take_instance` (Vollentnahme) und `try_insert_instance` (`retired_uuid`
bei Vollverschmelzung) liefern die Abkopplungen; die Verrechnung
(`reconcile_after_take`/`reconcile_after_insert`, Buyback-Aufhebung vor dem
Snapshot) steht für den späteren Händler-Spiellayer bereit. Die
Dirty-Markierung bleibt Aufgabe des aufrufenden Spiellayers (§17): Ohne
Dirty-State entsteht kein Snapshot und keine Finalisierung. Ein Teilstack-Rest
behält seine UUID und ist dadurch vor Finalisierung geschützt.

**Snapshot-/Drain-Pfad (abwärtskompatibel):** Der Snapshot trägt die
Lifecycle-Sicht (`item_lifecycle`, neues optionales Feld neben `cooldowns`).
Neues Format (`Some`, auch leer) schreibt die Metadaten vollständig neu —
nach Zusammenführung mit den gespeicherten Pflichten in derselben Transaktion
(`merge_pending`): Ein `Some(empty)` nach Neustart löscht erhaltene
Konflikt-/Finalisierungspflichten nicht; eine Wiedereinsetzung hebt nur die
passende Pflicht auf. Altformat (`None`, Feld fehlt) lässt Metadaten und
Instanzen unberührt. Unbestätigte Entfernungen bleiben in neueren Snapshots
erhalten, bis der DB-Commit bestätigt ist; bei der Bestätigung hebt eine zuvor
erfasste Wiedereinsetzung die Entfernung auf (keine versehentliche Freigabe
neuer Änderungen).

**Transaktion:** Inventar, Idia, Lifecycle-Metadaten, zulässige
Instanzentfernung und `persist_revision` werden in derselben DB-Transaktion
angewendet (`persist::apply_snapshot_to_db`, docs/Player_Persistenz.md §30).
Referenzierte oder widersprüchlich zugeordnete Instanzen (Platzierungs- oder
Pufferzeile, auch fremder Charaktere) werden nicht gelöscht; ihre Metadaten
bleiben zur erneuten Prüfung erhalten.

**Startup/Shutdown:** Die Startup-Finalisierung alter Runtime-Metadaten läuft
ausschließlich bei vollständig abgeschlossener Spool-Recovery; bei
unvollständiger Recovery keine widersprüchliche Bereinigung und keine
vorzeitige Freigabe. Wird die Recovery erst später durch den periodischen
Drainer vollständig, holt dieser die Finalisierung über denselben gemeinsamen
Mechanismus (`finalize_startup_lifecycle`) genau einmal nach — READY gilt
erst nach erfolgreichem Abschluss aller notwendigen Schritte; bei Fehler
bleibt die Pflicht erhalten (DEGRADED, kontrollierter Retry im Folgetick),
und nach Erfolg findet im laufenden Betrieb keine erneute pauschale
Startup-Finalisierung statt. Session-Ende und Shutdown fließen vor dem
maßgeblichen (finalen, erzwungenen) Snapshot ein; bei Savefehler bleiben die
Lifecycle-Pflichten im erhaltenen RAM (§16-Regel).

**Grenzen (ausdrücklich):** FakeDb-/Harness-Tests belegen die
Entscheidungslogik, kein SQL-/FK-/Rollback-Verhalten und keine echte
MariaDB-Semantik. Die fünf Harness-Ablauftests sind ausdrücklich Modelle:
Revisionsvergleich mit Skip/Supersede und Commit-Atomarität sind darin
nachgebaut; einzige Produktionsfunktion unter Test ist
`deletable_candidates` (Merge: `merge_pending`). Das produktive
Revisions-Gating liegt in `spool.rs`, die Transaktionsatomarität und der
erfolgreiche DB-Durchlauf der Startup-Finalisierung sind offline
prinzipbedingt unbelegt („offline" allein beweist keine Untestbarkeit —
unbelegt bleibt unbelegt benannt). Es wird keine universelle Crash- oder
Mehrprozessgarantie behauptet. Eigene Runtime-Kennung (`rt-<pid>-<nanos>`)
dient nur der Zuordnung, nicht der Korrektheit.

**Mitführungsgrenze (benannt, keine Löschfrist erfunden):**
Referenziert-blockierte Pflichten können ohne feste Grenze in RAM und DB
verbleiben — es gibt bewusst kein Cap, keine TTL und keine automatische
Zwangslöschung. Die Metadatentabelle wächst nur um tatsächlich abgekoppelte
UUIDs; verwaiste Zeilen (Instanz bereits weg) bereinigt die
Startup-Finalisierung. Charakterweise Pflichten sind per SELECT auf
`item_instance_finalizations` einsehbar (Operator-Diagnose, kein
Automatik-Eingriff).
