# Experience-, Level- und Progressionssystem

## Status

**Status:** 🟡 Konzept

Dieses Dokument definiert die verbindliche Grundlage der normalen Charakterprogression von Andora: Erfahrungspunkte, Charakterlevel, Attributpunkte, EXP-Quellen, Level-Cap, Gruppen-EXP, Entdeckungs-EXP, Quest-EXP, Todesfolgen und Rested EXP.

Sammeln und Crafting besitzen eigene Progressionen (siehe `Handwerks_und_Sammelsystem.md`, `Crafting.md`). Die Unterwelt-Progression von Expansion 1 ist ein separates System (siehe `exp1_Unterwelt.md`, Abschnitt 5).

### Implementierungsstand (Rust-Realm)

- EXP wird gespeichert (`characters.exp`, Migration 001) und bei Gegner-Toden verteilt: 100 % an die berechtigten Gruppenmitglieder bzw. Claim-Spieler (Gruppensystem V1 §7, Migration 013, `combat/mod.rs` `award_monster_exp`), Persistenz bei Disconnect (`db::save_exp`).
- **Nicht eingebaut:** EXP-Kurve/Levelaufstieg, Attributpunkte durch Levelaufstieg, Level-Cap-Verhalten, Levelunterschied-Anpassung der Gegner-EXP, Entdeckungs-EXP, Quest-EXP, Rested-EXP (das Offline-Zeitstempel wird derzeit nicht gespeichert, nur Position + EXP + Gold + Inventar).
- Der aktuelle Implementierungsstand wird in `Projekt-Status.md` geführt.

---

## 1. Grundprinzip

Die normale Charakterprogression von Andora basiert auf Erfahrungspunkten (EXP) und Charakterleveln.

Im Grundspiel beträgt das maximale Charakterlevel:

**Level 40**

Mit Expansion 1 wird das maximale Charakterlevel auf:

**Level 50**

angehoben.

Das Level-Cap soll grundsätzlich erweiterbar bleiben, damit spätere Erweiterungen keine grundlegende Änderung des Progressionssystems benötigen.

Expansion 1 führt zusätzlich eine eigene Progression für die Unterwelt ein. Diese ist unabhängig von der normalen Charakterprogression und wird separat definiert.

---

## 2. Quellen für Charakter-EXP

Charakter-EXP kann im Grundspiel durch folgende Aktivitäten erhalten werden:

- Besiegen von Gegnern
- Abschließen von Quests
- Entdecken besonderer Orte

Weitere EXP-Quellen können später ergänzt werden.

Sammeln und Crafting gehören nicht zur normalen Charakter-EXP. Diese Systeme besitzen eigene Progressionen.

---

## 3. EXP-Kurve

Die benötigte EXP für den Aufstieg zum nächsten Level wird nicht über eine feste Tabelle definiert, sondern über eine Formel.

Grundprinzip:

`EXP bis zum nächsten Level = Basiswert + Faktor × Level²`

Dadurch steigt der EXP-Bedarf automatisch mit zunehmendem Charakterlevel.

Basiswert und Faktor sind Balancing-Werte und werden erst durch Tests endgültig festgelegt.

Das System unterscheidet zwischen:

- EXP-Fortschritt innerhalb des aktuellen Levels
- insgesamt verdienter Charakter-EXP

---

## 4. Verhalten am Level-Cap

Sobald ein Charakter das aktuell gültige Level-Cap erreicht hat, kann er keine weitere normale Charakter-EXP ansammeln.

Weitere EXP aus:

- Gegnern
- Quests
- Entdeckungen

werden verworfen.

Es existiert kein versteckter EXP-Speicher und keine Möglichkeit, EXP für eine spätere Erweiterung vorzusparen.

Wird das Level-Cap später erhöht, beginnt der Charakter vom bisherigen Maximallevel aus wieder normal mit der neuen Progression.

---

## 5. Attributpunkte durch Levelaufstieg

Ein neuer Charakter beginnt auf **Level 1** ausschließlich mit seinen normalen Startattributen.

Auf Level 1 erhält der Spieler keine zusätzlichen frei verteilbaren Attributpunkte.

Ab Level 2 erhält der Charakter bei jedem Levelaufstieg:

**5 frei verteilbare Attributpunkte**

Jedes zehnte erreichte Level erhält stattdessen:

**10 frei verteilbare Attributpunkte**

Beispiele:

- Level 2 → 5 Punkte
- Level 9 → 5 Punkte
- Level 10 → 10 Punkte
- Level 11 → 5 Punkte
- Level 20 → 10 Punkte
- Level 30 → 10 Punkte
- Level 40 → 10 Punkte
- Level 50 → 10 Punkte (Expansion 1)

Die 10 Punkte eines zehnten Levels ersetzen die normalen 5 Punkte. Sie werden nicht zusätzlich vergeben.

Die Punkte können frei auf die sieben Charakterattribute verteilt werden:

- Kraft
- Konstitution
- Geschicklichkeit
- Intelligenz
- Weisheit
- Glück
- Ausdauer

Die Klasse bestimmt nicht automatisch, wie diese Punkte verteilt werden.

Extreme oder ungewöhnliche Charakter-Builds sind ausdrücklich möglich.

---

## 6. Mehrere Level durch eine EXP-Belohnung

Eine große EXP-Belohnung kann einen Charakter mehrere Level gleichzeitig aufsteigen lassen.

Die Level werden dabei nacheinander verarbeitet.

Für jedes erreichte Level erhält der Charakter die dafür vorgesehenen Attributpunkte.

Beispielsweise muss beim Überspringen eines zehnten Levels trotzdem die dort vorgesehene Vergabe von 10 Attributpunkten stattfinden.

Das aktuelle Level-Cap kann dabei niemals überschritten werden.

---

## 7. Gegner-EXP und Levelunterschied

Die EXP eines besiegten Gegners wird abhängig vom Levelunterschied zwischen Charakter und Gegner angepasst.

Bei einem Gruppen-Kill gilt als „Charakter" für diese Berechnung das höchste Charakterlevel eines für den Kill EXP-berechtigten Gruppenmitglieds (siehe Abschnitt 8).

| Levelunterschied des Gegners | EXP |
|---|---:|
| 5 oder mehr Level höher | 125 % |
| 2–4 Level höher | 110 % |
| innerhalb ±1 Level | 100 % |
| 2–3 Level niedriger | 75 % |
| 4–5 Level niedriger | 50 % |
| 6–9 Level niedriger | 25 % |
| 10 oder mehr Level niedriger | 0 % |

Die Prozentwerte sind Balancing-Werte und können später angepasst werden.

Sehr niedrigstufige Gegner können dadurch nicht dauerhaft zum Farmen von Charakter-EXP verwendet werden.

---

## 8. Gruppen-EXP

Ein besiegter Gegner erzeugt unabhängig von der Gruppengröße insgesamt **100 % seiner berechneten normalen EXP**.

Es gibt keinen zusätzlichen Gruppenbonus.

Diese EXP werden gleichmäßig unter allen für den Kill berechtigten Gruppenmitgliedern aufgeteilt.

Beispiel bei 600 EXP:

- Solo → 600 EXP
- 2 Spieler → jeweils 300 EXP
- 3 Spieler → jeweils 200 EXP
- 4 Spieler → jeweils 150 EXP (maximale normale Gruppengröße im Grundspiel; mit Expansion 1: 5 Spieler à 120 EXP)

Das individuelle Charakterlevel eines Gruppenmitglieds verändert seinen Anteil an dieser Aufteilung nicht.

Die Gruppenaufteilung selbst bleibt einheitlich.

## 8.1 Gemischte Spielerlevel in Gruppen

Bei einem Gegner-Kill wird für die Berechnung des Levelunterschieds zwischen Gruppe und Gegner das **höchste Charakterlevel eines für den Kill EXP-berechtigten Gruppenmitglieds** verwendet.

Ablauf:

1. Ermittle alle für diesen Kill EXP-berechtigten Gruppenmitglieder.
2. Ermittle unter diesen Spielern das höchste Charakterlevel.
3. Berechne anhand dieses Levels und des Gegnerlevels den Leveldifferenz-Multiplikator (siehe Abschnitt 7).
4. Berechne damit die gesamte normale Gegner-EXP für diesen Kill.
5. Teile diese Gesamt-EXP gleichmäßig unter den EXP-berechtigten Gruppenmitgliedern auf.
6. Behandle einen nicht gleichmäßig teilbaren Integer-Rest deterministisch gemäß der bestehenden Aufteilungsregel: Der Rest geht an die ersten Empfänger in stabiler (Gruppen-)Reihenfolge, kein langfristiger Verlust.
7. Erst danach wird ein eventuell vorhandener Rested-EXP-Bonus individuell auf den persönlichen EXP-Anteil eines Spielers angewendet (siehe Abschnitt 12.7).

Das individuelle Level eines Gruppenmitglieds verändert nicht nachträglich seinen Anteil an der normalen Gruppen-EXP.

Beispiel:

Ein Level-20-Spieler und ein Level-30-Spieler töten gemeinsam einen Level-25-Gegner.

Für die Leveldifferenz wird Level 30 verwendet. Der Gegner (Level 25) liegt damit 5 Level unter dem Gruppen-Berechnungslevel (30). Nach der Tabelle in Abschnitt 7 entspricht das **50 %** der normalen Gegner-EXP.

Erst diese reduzierte Gesamt-EXP wird anschließend gleichmäßig zwischen den beiden EXP-berechtigten Spielern aufgeteilt.

Diese Regel verhindert insbesondere, dass ein hochstufiges Gruppenmitglied zusammen mit einem niedrigstufigen Charakter schwache Gegner farmt und dadurch für die Gruppe volle Gegner-EXP erzeugt.

---

## 9. Entdeckungs-EXP

Besondere Orte der Welt können eine kleine persönliche EXP-Belohnung vergeben.

Eine Entdeckung kann von jedem Charakter nur **ein einziges Mal** ausgelöst werden.

Der Entdeckungsstatus bleibt dauerhaft gespeichert und wird durch Logout oder Serverneustart nicht zurückgesetzt.

Entdeckungen sind persönlich:

Ein Gruppenmitglied, das einen Ort entdeckt, löst dadurch nicht automatisch die Entdeckung für andere Gruppenmitglieder aus.

Die konkrete EXP-Belohnung wird vom jeweiligen Entdeckungspunkt festgelegt.

Entdeckungsbelohnungen sollen bewusst klein bleiben und typischerweise im zwei- bis niedrigen dreistelligen EXP-Bereich liegen.

Sie sollen das Erkunden der Welt belohnen, aber keinen wesentlichen Ersatz für die normale Levelprogression darstellen.

---

## 10. Quest-EXP

Die EXP-Belohnung einer Quest wird von der jeweiligen Quest selbst festgelegt.

Es existiert keine globale Formel, welche automatisch die EXP-Belohnung aller Quests bestimmt.

Dadurch können Umfang, Schwierigkeit und Bedeutung einer Quest bei ihrer Belohnung individuell berücksichtigt werden.

Die genauen Regeln gehören zum separaten Quest-System.

---

## 11. Tod und Charakter-EXP

Der Tod eines Charakters verursacht keinerlei Malus auf Charakter-EXP.

Verbindliche Regeln:

- Bereits erworbene Charakter-EXP werden durch den Tod nicht reduziert.
- Es gibt keine EXP-Schuld.
- Es gehen keine Level oder Teile des aktuellen Level-Fortschritts verloren.
- Nach einem Tod wird der zukünftige Charakter-EXP-Verdienst nicht reduziert.
- Es gibt insbesondere keinen temporären EXP-Verdienstmalus nach einem Tod.

Andere Folgen des Todes werden unabhängig vom Progressionssystem definiert (siehe `Kampfsystem.md`, Abschnitt 15).

---

# 12. Rested EXP

## 12.1 Grundidee

Charaktere sammeln während längerer Offlinezeiten einen Rested-EXP-Pool.

Dieser soll Spielern nach einer Spielpause einen begrenzten Aufholbonus ermöglichen.

Rested EXP gilt ausschließlich für EXP aus besiegten Gegnern.

Rested EXP gilt nicht für:

- Quest-EXP
- Entdeckungs-EXP

---

## 12.2 Rested-Bonus

Solange sich EXP im Rested-Pool befindet, erhält der Charakter auf seine normale Kill-EXP einen Bonus von:

**+50 %**

Beispiel:

Ein Kill würde normalerweise 200 EXP ergeben.

Mit ausreichend Rested EXP erhält der Charakter:

- 200 normale EXP
- 100 Rested-Bonus-EXP
- insgesamt 300 EXP

Dabei werden 100 EXP aus dem Rested-Pool verbraucht.

Enthält der Pool weniger Bonus-EXP als für den vollständigen 50-%-Bonus benötigt werden, wird nur der tatsächlich noch vorhandene Rested-Wert als Bonus vergeben.

---

## 12.3 Maximale Größe des Rested-Pools

Der Rested-Pool kann maximal **50 % des EXP-Bedarfs des aktuellen Levels** enthalten.

Beispiel:

Benötigt ein Charakter für sein aktuelles Level 10.000 EXP bis zum nächsten Level, beträgt sein maximales Rested-Guthaben:

**5.000 EXP**

Der Maximalwert richtet sich immer nach dem aktuellen Charakterlevel.

---

## 12.4 Rested EXP beim Levelaufstieg

Ein Levelaufstieg verändert nicht die bereits vorhandene absolute Menge an Rested EXP.

Beispiel:

Vor dem Levelaufstieg:

`Rested-Pool = 2.300 EXP`

Nach dem Levelaufstieg:

`Rested-Pool = 2.300 EXP`

Lediglich die maximal mögliche Größe des Pools wird anhand des EXP-Bedarfs des neuen Levels angepasst.

Der vorhandene Pool wird:

- nicht prozentual umgerechnet
- nicht automatisch aufgefüllt
- nicht aufgrund des Levelaufstiegs erhöht

Das neue Level schafft lediglich zusätzlichen Platz im Pool, sofern dessen neues Maximum größer ist.

---

## 12.5 Aufbau des Rested-Pools

Rested EXP wird ausschließlich während Offlinezeit aufgebaut.

Pro 24 Stunden Offlinezeit wächst der Pool um:

**10 % des EXP-Bedarfs des aktuellen Levels**

Da der Pool maximal 50 % eines Levels enthalten kann, benötigt ein vollständig leerer Pool fünf Tage Offlinezeit, um vollständig gefüllt zu werden.

Teilweise Offlinezeiten können entsprechend anteilig berücksichtigt werden.

---

## 12.6 Berechnung der Offlinezeit

Rested EXP wird nicht fortlaufend für offline befindliche Charaktere berechnet.

Beim Verlassen des Spiels wird der relevante Zeitpunkt gespeichert.

Während der Charakter offline ist, findet keine regelmäßige Rested-EXP-Berechnung statt.

Erst beim nächsten Login wird einmalig ermittelt:

1. wie lange der Charakter offline war,
2. wie viel Rested EXP dadurch entstanden ist,
3. wie viel Rested EXP bereits vorhanden war,
4. wie hoch das aktuelle Maximum ist.

Der berechnete Zuwachs wird anschließend zum vorhandenen Pool addiert und am maximal zulässigen Wert gedeckelt.

Dadurch benötigt ein Charakter, der lange Zeit oder dauerhaft nicht mehr gespielt wird, keinerlei laufende Berechnung durch den Realm.

**VERBRAUCH DER OFFLINE-ZEIT**

VERBINDLICH:

* Die Offline-Zeit wird **nur bei einem erfolgreichen Realm-Login** einmalig konsumiert und dem Rested-Pool gutgeschrieben. Der Erfolgszeitpunkt ist in `Login_Realm_Architektur.md` (Abschnitt „Erfolgszeitpunkt des Realm-Logins") festgelegt: erst wenn der Server-Commit eine Owner-Zuordnung hergestellt hat.
* **Schlägt der Einstieg vor diesem Commit fehl** — insbesondere bei Quest- oder Datenladefehlern, bei `BLOCKED` oder nicht verfügbarer Elternkontrolle sowie bei einem Registry- oder Account-Konflikt —, wird die Offline-Zeit **nicht** konsumiert und dem Pool **nichts** gutgeschrieben. Der gespeicherte Logout-Zeitpunkt und der Offline-Zeitraum bleiben erhalten; die Berechnung erfolgt beim nächsten erfolgreichen Login.
* **Schlägt erst nach dem Commit `WELCOME` oder die Verbindung aus**, gilt der Login als zustande gekommen. Die Gutschrift bleibt; der Disconnect-Pfad schreibt anschließend den neuen Logout-Zeitpunkt.
* Ein fehlgeschlagener Einstieg erzeugt damit **weder** eine doppelte Gutschrift **noch** einen Verlust der Offline-Zeit: Solange der Logout-Zeitpunkt unverändert bleibt, wird derselbe Zeitraum beim nächsten erfolgreichen Login regulär — gegebenenfalls mit der dann längeren Offline-Dauer — verrechnet. Die Deckelung nach Abschnitt 12.3 gilt unverändert.

**At-most-once-Unterbrechungsausnahme** (Kurzname: Crash-Ausnahme)

* Die Offline-Zeit wird als **ein gemeinsamer, unteilbarer Schritt mit der Gutschrift** verbraucht, und zwar unmittelbar vor der Herstellung der Owner-Zuordnung, nach Abschluss aller für den Einstieg als blockierend definierten Vorprüfungen. Schlägt dieser Schritt fehl, wird der Login nicht committet und der Zeitraum bleibt vollständig erhalten.
* Bricht der Vorgang **abrupt** zwischen erfolgreichem DB-Commit dieses Schritts und der Herstellung der Owner-Zuordnung ab — insbesondere bei Prozessabsturz sowie, sofern der Handler an dieser Stelle abbrechbar ist, bei Task-Abbruch oder Panic —, wird der Zeitraum **ohne Sitzung** verbraucht und der Login gilt technisch nicht als zustande gekommen.
* **Spielwertneutralität:** Die Ausnahme verursacht gegenüber der regulär vorgesehenen Abrechnung **weder Wertverlust noch Mehrfachgutschrift**. Die vorgesehene Gutschrift ist bereits vollständig gebucht; lediglich ihre Zuordnung zu einem erfolgreich hergestellten Realm-Login entfällt. Der Bonus ist beim nächsten erfolgreichen Login verfügbar; die Zurechnung des Zeitraums erfolgt ohne Sitzung. Für die Spielwertbilanz ist die Ausnahme damit neutral.

---

## 12.7 Rested EXP in Gruppen

Die normale Gruppen-EXP wird zuerst vollständig berechnet und auf die berechtigten Gruppenmitglieder verteilt.

Erst danach wird für jeden Charakter individuell geprüft, ob Rested EXP vorhanden ist.

Beispiel:

Ein Gegner ergibt 600 normale EXP.

Drei berechtigte Gruppenmitglieder erhalten jeweils:

`200 normale EXP`

Hat nur Spieler A Rested EXP:

- Spieler A → 200 normale EXP + bis zu 100 Rested EXP
- Spieler B → 200 EXP
- Spieler C → 200 EXP

Die normale Gruppenbelohnung beträgt weiterhin insgesamt 600 EXP.

Rested EXP ist ein persönlicher Bonus aus dem individuellen Rested-Pool und kein Gruppenbonus.

---

# 13. Zukünftige Progressionssysteme

Die normale Charakterprogression ist nicht das einzige langfristig geplante Progressionssystem von Andora.

Sammelberufe und Handwerksberufe besitzen eigene Progressionen.

Mit Expansion 1 erhält die Unterwelt zusätzlich eine eigene Progression. Ein Charakter behält dabei seinen normalen Charakterfortschritt, muss aber zusätzlich innerhalb der Unterwelt voranschreiten.

Dadurch kann ein Charakter, der das Grundspiel-Level-Cap erreicht hat, nicht allein aufgrund seines normalen Charakterlevels sämtliche Inhalte der Unterwelt überspringen.

Die genaue Funktionsweise dieser Unterwelt-Progression wird im Rahmen von Expansion 1 separat festgelegt.

---

## Querverweise

| System | Dokument | Bezug |
|---|---|---|
| Gruppen-EXP | `Gruppensystem.md` | §7 100 % Monster-EXP, gleichmäßige Aufteilung unter berechtigten Mitgliedern; Leveldifferenz bei Gruppenkills nach höchstem EXP-berechtigtem Mitglieds-Level (dieses Dokument §7/§8). |
| Quest-EXP | `Quest-System.md` | §23 Questbelohnungen (Erfahrung); Höhe wird von der Quest selbst festgelegt (dieses Dokument §10). |
| Level-Cap | `Tier-Progression.md`, `project_overview.md`, `characters_world.md` | Maximallevel 40 (Grundspiel), 50 mit Exp1; erweiterbares Cap; Tiergrenzen nicht hart im Realm-Code. |
| Unterwelt-Progression | `exp1_Unterwelt.md` | §5 eigenständiges Unterwelt-EXP-Profil, unabhängig vom normalen Charakterlevel (dieses Dokument §13). |
| Handwerks-/Sammel-Progression | `Handwerks_und_Sammelsystem.md`, `Crafting.md`, `Sammelsystem.md` | eigene Progressionen, nicht Teil der normalen Charakter-EXP. |
| Attribute | `Attribute_und_Regeneration.md` | die sieben Grundattribute, auf die frei verteilbare Attributpunkte wirken (dieses Dokument §5). |
| Klassen-/Skill-Progression | `Klassensystem.md`, `Kampfsystem.md` | Levelaufstieg erhöht ausschließlich das Skillmaximum; kein automatischer Attribut- oder Skillwertzuwachs (Abgrenzung zu §5). |
| Todesfolgen | `Kampfsystem.md` | §15 keine EXP-Malus-Regel; Tod ohne Folgen für Charakter-EXP (dieses Dokument §11). |
