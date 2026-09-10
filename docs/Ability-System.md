# Andora – Ability-System

## Status

**Konzept – verbindlicher Grundlagenentwurf.**

Dieses Dokument definiert die verbindliche Grundlage des Ability-/Fähigkeitssystems von Andora. Konkrete Zahlenwerte, Balancingwerte, Effektdetails und exakte Kurven bleiben bewusst offen und sind Balancing.

Das Ability-System ist die verbindliche Systematik für alle aktiven Fähigkeiten von Spielern, NPCs, Monstern, Named und Bossen. Es ergänzt das allgemeine Kampfsystem in `Kampfsystem.md` und baut auf dessen Grundregeln auf.

---

## 1. Grundmodell der Fähigkeiten

Als aktuelle Basis existieren drei Ausführungsarten:

1. **Sofortfähigkeit**
2. **Fähigkeit mit Castzeit**
3. **Kanalisierung**

Diese drei Arten sind die derzeitige Grundlage. Sie dürfen später erweitert werden, falls eine neue Mechanik dies tatsächlich benötigt.

**Kein globaler Cooldown.** Andora besitzt keinen globalen Cooldown. Jede Fähigkeit besitzt ihren eigenen Cooldown.

NPC-/Monsterfähigkeiten verwenden ebenfalls eigene Cooldowns. Die normale Angriffs-Duration eines Gegners (Waffenduration für den automatischen Grundangriff, siehe `Kampfsystem.md` Abschnitt 3) ist davon getrennt.

Spieler und NPCs verwenden grundsätzlich dasselbe Ability-Grundsystem. Es wird kein separates zweites Ability-System für Gegner aufgebaut; Content/Lua definiert für jede Fähigkeit die jeweiligen Werte und Regeln, der Realm führt sie autoritativ aus (siehe auch `Kampfsystem.md` Abschnitte 18–21 und `Boss-System.md`).

---

## 2. Mana und Cooldown

Nicht jede Fähigkeit benötigt Mana. Jede Fähigkeit kombiniert Mana und Cooldown gemäß ihrer eigenen Definition. Mögliche Kombinationen sind unter anderem:

* Mana und eigener Cooldown
* kein Mana, dafür ein hoher Cooldown
* andere kombinative Varianten

Rassenfähigkeiten und Utility-Fähigkeiten können ohne Mana funktionieren und dafür lange Cooldowns besitzen.

Für Cast-Fähigkeiten gilt:

* Mana wird beim Start bzw. Aktivieren des Casts abgezogen.
* Wird der Cast unterbrochen, wird das abgezogene Mana **nicht** zurückerstattet.
* Der Ability-Cooldown beginnt erst nach erfolgreicher Ausführung der Fähigkeit.
* Ein versehentlich durch Bewegung unterbrochener Cast kann daher sofort erneut versucht werden, sofern die übrigen Voraussetzungen (Ziel, Reichweite, Sicht, Ressourcen, Status) erfüllt sind.

**Persistenz von Cooldowns:**

* Persistente bzw. lange Cooldowns bleiben serverseitig über Tod und Logout hinweg bestehen und laufen weiter.
* Normale kurze Kampffähigkeiten dürfen beim entsprechenden Tod bzw. Combat-Reset (Evade/Return, Boss-Reset) zurückgesetzt werden.

**Mana-Progression:**

* Stärkere bzw. spätere Fähigkeiten dürfen höhere Manakosten besitzen. Die konkreten Manakosten werden nicht jetzt festgelegt, sondern erst bei der Ausarbeitung der jeweiligen Fähigkeit.
* Spieler sollen später über Finetuning und Meisterbücher unter anderem die Möglichkeit erhalten können, Manakosten bestimmter Fähigkeiten zu reduzieren und dadurch in längeren Kämpfen effizienter zu werden (siehe Abschnitte 9, 11 und 12). Manakosten-Senkungen müssen mit den bestehenden Qualitäts- und Meisterschaftsregeln konsistent bleiben.
* Verhältnis und Balance von Manakosten, Manapool und Mana-Regeneration werden in `Attribute_und_Regeneration.md` (Abschnitte 4–10) behandelt; konkrete Kostensenkungen sind keine festen Architekturwerte, sondern Inhalts-/Balancewerte (siehe Abschnitt 14).

Für NPCs und Monster gilt zusätzlich: Beim Evade/Return (Kampfsystem.md Abschnitt 20) erfolgt ein vollständiger Cooldown-Reset, wie dort verbindlich festgelegt.

Die Persistenz von Cooldown-Zuständen folgt den allgemeinen Regeln für persistente Realm-Daten in `Datenbank_Architektur.md`.

---

## 3. Cast-Unterbrechung, Reichweite und Sicht

Casts können unterbrochen oder verhindert werden durch:

* Bewegung während des Casts
* Stun
* Silence
* gezielte Interrupts
* Tod
* verlorene Reichweite zum Ziel
* verlorene Sichtlinie zum Ziel

Silence verhindert die Verwendung entsprechender Cast-Fähigkeiten. Stun verhindert Handlungen entsprechend seinem Zustand.

Der Realm ist autoritativ. Beim Ausführen eines Casts prüft der Realm unter anderem:

* gültiges Ziel
* Reichweite zum Ziel
* einfache Sichtlinie zum Ziel

Bleiben diese Voraussetzungen während des Casts nicht aufrechterhalten, wird der Cast abgebrochen.

**Beispiel:** Ein Monster wechselt sein Ziel und läuft während des gegnerischen Casts hinter eine Wand. Verliert der Spieler dadurch Sichtlinie oder Reichweite, schlägt der Cast fehl.

Die Regel, dass Bewegung einen laufenden Zauber unterbricht, ist bereits in `Kampfsystem.md` (Abschnitt 9) festgehalten; dieses Dokument ergänzt die vollständigen Regeln über Zielvalidierung, Reichweite und Sicht.

---

## 4. AoE-Grundarten

Es gibt derzeit vier grundlegende AoE-Varianten:

### 1. Encounter-/Gruppen-AoE

Ein Gegner wird als Ziel gewählt. Die Wirkung betrifft dessen definierte Gegner-/Encountergruppe. Räumlich danebenstehende Gegner außerhalb dieser Gruppe werden dadurch nicht automatisch getroffen.

### 2. Target-Radius-AoE

Ein Gegner wird gewählt. Um dessen Position entsteht beim Einschlag ein räumlicher Wirkungsbereich. Auch bisher nicht am Kampf beteiligte gültige Gegner können getroffen und dadurch in den Kampf gezogen werden.

### 3. Caster-Radius-AoE

Der Wirkungsbereich entsteht um den ausführenden Spieler oder NPC. Gültige Ziele im Bereich werden getroffen.

### 4. Frei platzierter / Ground-Target-AoE

Der Spieler bestimmt innerhalb einer für die Fähigkeit definierten maximalen Reichweite eine Position. Um diese Position entsteht der Wirkungsbereich. Der Realm validiert die gewählte Position.

Räumliche AoEs besitzen grundsätzlich **kein künstliches maximales Ziellimit**. Alle gültigen Ziele im Wirkungsbereich werden berücksichtigt.

---

## 5. Freundliche Ziele und Heilung

* **Einzelheilungen und Einzelbuffs** dürfen auch auf freundliche Spieler **außerhalb der eigenen Gruppe** angewendet werden.
* **Gruppenfähigkeiten** bleiben auf die eigene Gruppe beschränkt.
* **Räumliche freundliche AoEs** richten sich nach ihrer Position: Ein platzierter Heilkreis kann jeden gültigen freundlichen Spieler innerhalb seines Wirkungsbereichs heilen, unabhängig von dessen Gruppenzugehörigkeit.

Gruppenzugehörigkeit und „freundliches Ziel" sind bewusst unterschiedliche Zielregeln.

Für die Definition, wer als „gültiges freundliches Ziel" gilt, bleiben die bestehenden Kampfregeln maßgeblich, insbesondere die Fraktionsregeln in `Kampfsystem.md` (Abschnitt 16).

---

## 6. Buffs, Debuffs und Crowd Control

* Effekte derselben Effektgruppe stapeln sich nicht. Der zuletzt erfolgreich angewendete Effekt **ersetzt** den vorherigen Effekt dieser Gruppe.
* Effekte unterschiedlicher Kategorien/Gruppen können gleichzeitig bestehen.

**Stun:** bleibt für seine vorgesehene Effektdauer bestehen; Schaden beendet ihn nicht automatisch.

**Root und Schlaf:** können durch erlittenen Schaden vorzeitig aufgelöst werden. Die konkrete Fähigkeit darf definieren, wie dieses Verhalten funktioniert.

**Silence und Slow:** laufen normalerweise für ihre definierte Effektdauer; sie können durch passende Buff-/Cleanse-Effekte vorzeitig entfernt werden.

Der Cooldown einer Fähigkeit und die Effektdauer eines Buffs/Debuffs sind **strikt getrennte Konzepte**.

Beim Tod werden **alle** aktiven Buffs und Debuffs entfernt.

---

## 7. DoT/HoT und Waffen-Effekte

Normale Ability-Effekte folgen ihren Effektgruppen gemäß Abschnitt 6. Verwendet beispielsweise ein zweiter Barde einen Debuff derselben Gruppe, ersetzt der zuletzt erfolgreich angewendete Effekt den vorherigen.

**Waffenbasierte Effekte** werden anders behandelt:

* Mehrere **unterschiedliche Waffenquellen** dürfen ihren jeweiligen Waffen-Effekt gleichzeitig auf demselben Ziel besitzen.
* Beispiel: Ein Tank und ein Schurke besitzen jeweils eine vergiftete oder blutende Klinge. Beide Waffen dürfen ihren eigenen Effekt gleichzeitig auf demselben Gegner aktiv haben.
* Diese Waffen-Effekte werden entsprechend schwächer balanciert.

Für die **selbe Waffenquelle** gilt:

* Solange ihr Effekt aktiv ist, stapeln weitere Treffer den Effekt nicht, erhöhen ihn nicht und erneuern seine Dauer nicht.
* Erst nachdem der Effekt abgelaufen ist, kann ein späterer geeigneter Treffer derselben Quelle ihn erneut auslösen.

Ability-Effekte und Waffen-Effekte dürfen dieselben allgemeinen Realm-Effektmechanismen verwenden, müssen aber ihre **Effektquelle** unterscheiden können.

---

## 8. Fähigkeitsqualität

Reguläre aufwertbare Klassenfähigkeiten besitzen ein Qualitätssystem.

Eine neu durch den normalen Klassenfortschritt erhaltene Fähigkeit startet auf **Lehrling 1**.

### Qualitätsstufen und Quellen

**Lehrling:**

* Lehrling 1: Grundstufe beim Erlernen
* Lehrling 2: Schriftrolle beim klassenspezifischen NPC
* Lehrling 3: Schriftrollen beim klassenspezifischen NPC
* Lehrling 4: als Loot von normalen Monstern erhältlich

**Fortgeschritten:**

* Stufe 1: als Buch in Truhen auffindbar
* Stufe 2: durch Spieler mit seltenen Rohstoffen craftbar
  * Voraussetzung: Das entsprechende Rezept wurde zuvor in der Welt gefunden und gelernt, und die notwendigen Handwerksvoraussetzungen sind erfüllt.

**Meisterhaft:**

* Stufe 1: in besonders hochwertigen / großen Truhen auffindbar
* Stufe 2: mit raren Materialien craftbar
  * Voraussetzung: Das entsprechende seltene Rezept wurde zuvor gefunden und gelernt.

**Legendär:**

* extrem seltene Fähigkeitsbücher
* insbesondere Boss-/Raidboss-Loot
* nicht craftbar

Seltene Herstellungsrezepte können gehandelt werden und sollen dadurch wertvolle Wirtschaftsgüter bzw. Auktionshaus-Waren werden (siehe `Handwerks_und_Sammelsystem.md` und `[Auktionshaus und Marktplatz](./Auktionshaus%20und%20Marktplatz)`).

Fähigkeitsbücher, Schriftrollen und Rezepte sind grundsätzlich handelbare Gegenstände. Binding (Charakterbindung) erfolgt nur, wenn die jeweilige Gegenstandsdefinition dies explizit vorsieht (siehe `Lootsystem.md` und `item_properties.md`).

**Abgrenzung:** Die Fähigkeitsqualität ist ein eigenständiges Progressionssystem und nicht mit der Item-Quality (1–6) in `item_properties.md`, `inventory_system.md` oder `Crafting.md` identisch. Ein Fähigkeitsbuch ist ein Item mit eigener Quelle und eigener Qualitätsstufe, folgt aber nicht dem regulären Crafting-Overcap-System.

Der Begriff „Meisterhaft" wird im Ability-System als Fähigkeits-Qualitätsstufe verwendet und ist nicht mit der Crafting-Qualitätsstufe „meisterhaft" in `Crafting_Grundprinzip.md` identisch.

---

## 9. Auswirkungen der Fähigkeitsqualität

Die Verbesserungen bauen grundsätzlich aufeinander auf:

* **Lehrling:** verbessert primär die **Primärwirkung** der Fähigkeit, z. B. Schaden oder Heilung.
* **Fortgeschritten:** weitere Verbesserung der Primärwirkung; zusätzlich etwas geringere Manakosten, sofern die Fähigkeit Mana verwendet.
* **Meisterhaft:** bisherige Verbesserungen; zusätzlich etwas geringere Castzeit, sofern die Fähigkeit eine Castzeit besitzt.
* **Legendär:** bisherige Verbesserungen; zusätzlich etwas geringerer Cooldown, sofern die Fähigkeit einen Cooldown besitzt.

Besitzt eine Fähigkeit eine für die Qualitätsstufe vorgesehene Mechanik nicht, wird **keine künstliche Mechanik** hinzugefügt. Stattdessen wird die Primärwirkung entsprechend weiter verbessert.

* Eine Sofortfähigkeit bekommt nicht künstlich eine Castzeit.
* Eine Fähigkeit ohne Mana bekommt nicht künstlich Manakosten.

**Nicht jede Fähigkeit ist aufwertbar.** Insbesondere Rassen-/Utility-Fähigkeiten wie Nachtsicht oder längeres Laufen können vollständig außerhalb dieses Qualitätssystems stehen. Aufwertbarkeit ist eine Eigenschaft der konkreten Fähigkeit.

---

## 10. Erlernen regulärer Klassenfähigkeiten

Reguläre Klassenfähigkeiten werden beim vorgesehenen Level **automatisch gelernt** und stehen sofort auf Lehrling 1 zur Verfügung. Der Spieler muss dafür nicht zu einem Klassentrainer zurückkehren.

Der Klassentrainer bleibt dennoch relevant, insbesondere durch Lehrling-2- und Lehrling-3-Schriftrollen.

Besondere Fähigkeiten dürfen später weiterhin durch andere Quellen wie Quests, Bücher, versteckte Inhalte usw. freigeschaltet werden.

Fähigkeitsbücher dürfen bereits **vor Erreichen** des benötigten Levels gefunden, besessen, gehandelt, gekauft oder gelagert werden. Die Voraussetzungen werden beim Lesen bzw. Anwenden geprüft. Dadurch kann ein Spieler beispielsweise bereits vor Level 20 ein Buch für eine Level-20-Fähigkeit besitzen und es unmittelbar nach Erreichen von Level 20 verwenden.

---

## 11. Meisterschaft durch Benutzung

Fähigkeitsqualität und Fähigkeitsmeisterschaft sind **zwei voneinander getrennte Progressionssysteme**.

Meisterschaft entsteht durch tatsächliche, erfolgreiche Verwendung einer Fähigkeit. Nicht der Tastendruck oder Cast zählt, sondern ein **vom Realm bestätigter sinnvoller Erfolg**.

Beispiele für Meisterschaftsfortschritt:

* Heilung: tatsächlich HP wiederhergestellt
* Schaden: tatsächlich Schaden verursacht
* Debuff: erfolgreich auf Ziel angewendet
* Taunt: erfolgreich akzeptiert / angewendet
* Wiederbelebung: Spieler tatsächlich wiederbelebt

Wirkungsloses Spammen erzeugt keinen Fortschritt.

Der Meisterschaftsfortschritt soll ähnlich der bestehenden Waffenprogression (Kampfskills, siehe `Kampfsystem.md` Abschnitt 4) mit zunehmendem Fortschritt langsamer werden. Eine nominelle Größenordnung wie „100 erfolgreiche Anwendungen" bedeutet daher nicht zwangsläufig exakt 100 Casts; gegen Ende kann der Fortschritt deutlich langsamer steigen. Exakte Kurven und Werte bleiben Balancing.

Nicht jede Fähigkeit muss meisterbar sein. Meisterbarkeit und Aufwertbarkeit (Abschnitt 9) sind unabhängige Eigenschaften einer Fähigkeit.

---

## 12. Meisterschaftsauswahl

Beim Erreichen einer Meisterschaftsstufe kann eine Fähigkeit eine Auswahl funktionaler Weiterentwicklungen anbieten. Diese müssen nicht lediglich Zahlenwerte erhöhen.

**Beispiele:**

**Wiederbelebung:**

* Variante mit Manakosten und kürzerem Cooldown
* Variante ohne Manakosten und dafür deutlich längerem Cooldown

**Eisangriff:**

* Variante mit zusätzlichem Schaden über Zeit (DoT)
* alternative Variante mit zunehmendem Slow, der sich über etwa 3 Sekunden verstärkt und anschließend zu einer kurzen Erstarrung von etwa 1–2 Sekunden führen kann

Die konkreten Zahlen sind keine endgültigen Balancewerte.

Meisterschaften können:

* Effekte hinzufügen
* Effekte verändern
* zeitliche Effektketten erzeugen
* Kosten verändern
* Cooldowns verändern
* andere für die Fähigkeit geeignete funktionale Veränderungen vornehmen

Effekte dürfen zeitliche Phasen und Folgeeffekte besitzen.

Meisterschaftsdefinitionen sollen content- bzw. Lua-getrieben sein, damit sie später ohne grundlegende Änderung des Realm-Kampfsystems angepasst und gebalanced werden können (siehe Abschnitt 14).

Die Anzahl und Art der Meisterschaftsoptionen ist nicht global fest vorgeschrieben. Eine Fähigkeit kann mehrere, wenige oder keine Meisterschaftsoptionen besitzen.

---

## 13. Dauerhaftigkeit der Meisterschaftsauswahl

Zum Start ist eine einmal gewählte Meisterschaft **nicht vom Spieler zurücksetzbar**.

Die Spieler sollen das System zunächst kennenlernen und die Auswahl soll Gewicht besitzen.

Bereits von Anfang an soll es in geeigneten Städten einen thematisch passenden NPC geben, der später eine Meisterschafts-Reset-Funktion erhalten kann. Zu Beginn besitzt dieser NPC diese Funktion noch nicht.

Falls später aufgrund von Spielerwünschen oder Systementwicklung ein Reset eingeführt wird, kann dieser NPC die Funktion erhalten. Ein solcher Reset soll grundsätzlich **sehr teuer** sein und damit eine bedeutende Entscheidung bzw. einen Gold-Sink bleiben.

Exakte Preise und Bedingungen sind noch nicht festgelegt.

---

## 14. Lua-/Realm-Trennung

Konkrete Fähigkeiten, Qualitätswerte, Meisterschaftsvarianten und ihre Contentregeln sollen später Lua- bzw. datengetrieben definiert werden.

Der Realm bleibt autoritativ.

* **Lua** beschreibt Regeln, Werte und gewünschte Effekte.
* **Der Realm** validiert und führt die tatsächlichen Spielwirkungen aus.

Lua darf **nicht** selbst zur Autorität über Schaden, Heilung, Zielvalidierung, Cooldowns, Positionen oder persistenten Zustand werden.

Die Ability-Architektur muss außerdem **semantische Kategorien** und **vom Realm bestätigte Ability-Ergebnisse** unterstützen, damit:

* Meisterschaftsfortschritt (Abschnitt 11) darauf aufbauen kann
* das Heldenrad (`Heldenrad.md`) darauf aufbauen kann

Semantische Fähigkeitskategorien (z. B. Einziel-Schaden, Flächenschaden, Heilung, Taunt, Buff, Debuff, Kontrolle) werden vom Realm bei erfolgreicher Anwendung bestätigt und stehen für übergeordnete Mechaniken wie das Heldenrad zur Verfügung.

---

## 15. NPCs, Named und Bosse

Normale NPCs verwenden dasselbe Ability-Grundgerüst wie Spieler, benötigen aber keine normale Spieler-Meisterschaft.

Named/Champions und Bosse dürfen **meisterschaftsähnliche Varianten** oder Erweiterungen ihrer Fähigkeiten besitzen. Solche Varianten können beispielsweise abhängig von einer Bossphase freigeschaltet werden (siehe `Boss-System.md`, `Kampfsystem.md` Abschnitte 18–21).

Dafür soll kein separates zweites Ability-System entstehen. Content/Lua definiert die jeweiligen Boss- und Phase-spezifischen Ability-Regeln; der Realm führt sie autoritativ aus.

---

## Querverweise zu bestehenden Systemen

| System | Dokument | Bezug zum Ability-System |
|---|---|---|
| Kampf-Grundsystem | `Kampfsystem.md` | Grundregeln (Anvisieren, Grundangriff, Aggro, Tod/Respawn, Combat V2). Ability-System ergänzt diese um Fähigkeitsmodell, Cast-Regeln, AoE und Effektlogik. |
| Heldenrad | `Heldenrad.md` | Baut auf semantischen Kategorien und Realm-bestätigten Ability-Ergebnissen auf (siehe Abschnitt 14). Heldenrad selbst ist eine nicht aufwertbare Fähigkeit. |
| Klassensystem | `Klassensystem.md` | Grundklassen und Rollenidentität; Klassenfähigkeiten werden gemäß Abschnitt 10 beim vorgesehenen Level automatisch erlernt. |
| Boss-System | `Boss-System.md` | Boss-Reset (Evade/Return) mit vollständigem Cooldown-Reset; Boss-Phasen können meisterschaftsähnliche Ability-Varianten freischalten (Abschnitt 15). |
| Loot | `Lootsystem.md` | Fähigkeitsbücher als Loot-Quellen: Truhen (Fortgeschritten 1, Meisterhaft 1), normale Monster (Lehrling 4), Boss-/Raidboss-Loot (Legendär). |
| Crafting | `Crafting_Grundprinzip.md`, `Crafting.md`, `Handwerks_und_Sammelsystem.md` | Seltene Rezepte für Fortgeschritten 2 und Meisterhaft 2; seltenes Rezept + seltene Rohstoffe als Wirtschaftskreislauf. |
| Auktionshaus | `Auktionshaus und Marktplatz` | Seltene Herstellungsrezepte als handelbare Wirtschaftsgüter. |
| Item-System | `item_properties.md`, `inventory_system.md` | Fähigkeitsbücher sind Items; Ability-Qualität ist nicht identisch mit Item-Quality (1–6). |
| Datenarchitektur | `Datenbank_Architektur.md` | Persistente Cooldown-Zustände über Tod/Logout; Meisterschaftsstände als Realm-Daten. |
| Rassen-Fraktionen | `Rassen-Fraktionen.md`, Rasse-Dokumente | Rassenfähigkeiten können außerhalb des Ability-Qualitätssystems stehen. |
| Storytelling | `Storytelling_und_Weltgeheimnisse.md` | Bücher als Gameplay; Fähigkeitsbücher als Fundgegenstände in der Welt. |
| Combat V2 | `Kampfsystem.md` Abschnitte 18–21 | NPCs/Monster verwenden dasselbe Ability-Grundsystem; Content/Lua als Grundzustand; Home-Zone/Evade-Return. |

---

## Bewusst offene Punkte

Folgende Themen werden durch dieses Dokument **nicht** abschließend festgelegt und bleiben bewusst offen:

* vollständige Regeln für passive Fähigkeiten
* vollständige Kanalisierungsregeln
* Wiederbelebungsregeln im Kampf und außerhalb des Kampfes
* genaue Qualitätswerte und Balancezahlen
* konkrete Meisterschaftskurven
* endgültige Meisterschaftsoptionen einzelner Fähigkeiten
* vollständige Liste semantischer Ability-Kategorien
* vollständiges Cleanse-/Dispel-System
* weitere Ability-/Combo-Systeme
* konkrete Benennung / Anzahl der semantischen Kategorien

---

## Abgrenzung zu anderen Systemen

Das Ability-System ergänzt und erweitert das allgemeine Kampfsystem in `Kampfsystem.md`, ohne dessen Grundregeln zu verändern.

Folgende Bereiche bleiben weiterhin im allgemeinen Kampfsystem definiert und werden hier nicht wiederholt:

* Anvisieren und Zielwahl (Kampfsystem Abschnitt 2)
* Grundangriff und Duration (Abschnitt 3)
* Kampfskills und Waffenbeherrschung (Abschnitt 4)
* Physische Trefferauflösung (Abschnitt 5)
* Waffenschaden und Angriffsgeschwindigkeit (Abschnitt 6)
* Rüstung und Schadensreduktion (Abschnitt 7)
* Bewegung im Kampf (Abschnitt 9)
* Aggro und Gruppenrollen (Abschnitt 12)

Das Heldenrad (`Heldenrad.md`) ist eine eigenständige, klassenübergreifende Kampffähigkeit. Es ist als nicht aufwertbare Fähigkeit Teil des Ability-Systems, besitzt jedoch keine Fähigkeitsqualität und kann nicht durch Schriftrollen oder Bücher verbessert werden.
