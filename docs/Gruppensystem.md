# Andora – Gruppensystem (V1)

## Status

**Status:** 🟡 Konzept – V1 (Grundsatzfestlegung, Implementierung folgt)

Die nachstehenden Regeln sind verbindlich. Die technische Umsetzung (Gruppenzustand, Einladungen/Vorschläge, räumliche Teilnahme, EXP-Verteilung, Gruppen-Claim, Reconnect) folgt im Realm-Server und ist noch nicht eingebaut (siehe `Projekt-Status.md`). V1 schafft zuerst die technische Grundlage: Gruppe bilden → verwalten → räumliche Teilnahme bestimmen → EXP verteilen → Claim an Gruppe binden → Disconnect/Reconnect behandeln.

---

# 1. Grundprinzip

Das Gruppensystem ermöglicht Spielern, sich zu einer gemeinsamen Gruppe zusammenzuschließen.

Die normale Gruppe besteht in V1 aus maximal **4 Spielern**.

Die maximale Gruppengröße muss technisch zentral konfigurierbar sein, damit sie später anhand von Spieltests ohne grundlegenden Umbau erhöht werden kann.

Ein Spieler kann gleichzeitig nur Mitglied einer normalen Gruppe sein.

Raid-/Schlachtzugsgruppen sind nicht Bestandteil von V1.

---

# 2. Gruppenleiter

Jede Gruppe besitzt genau einen Gruppenleiter.

Der Spieler, dessen Einladung zur Bildung der Gruppe angenommen wird, wird Gruppenleiter.

Der Gruppenleiter entscheidet über die Zusammenstellung der Gruppe.

## Rechte des Gruppenleiters

Der Gruppenleiter darf:

* Spieler einladen
* Vorschläge anderer Gruppenmitglieder annehmen oder ablehnen
* Gruppenmitglieder entfernen
* die Gruppenleitung freiwillig übertragen
* später den Lootmodus der Gruppe bestimmen

Nur der Gruppenleiter kann eine tatsächliche Gruppeneinladung verschicken.

Verlässt der Gruppenleiter die Gruppe ohne vorherige Übergabe, erhält das am längsten in der Gruppe befindliche verbleibende Mitglied die Gruppenleitung.

Bleibt nur noch ein Spieler übrig, wird die Gruppe als Gruppe aufgelöst. Bestehende Claims können entsprechend §8 auf den letzten Spieler übergehen.

---

# 3. Spielervorschläge

Normale Gruppenmitglieder dürfen andere Spieler für die Aufnahme in die Gruppe vorschlagen.

Ein Vorschlag ist keine Gruppeneinladung.

Der vorgeschlagene Spieler erhält zunächst keine Nachricht.

Der Gruppenleiter entscheidet, ob aus dem Vorschlag eine tatsächliche Einladung entsteht.

Erst wenn der Gruppenleiter zustimmt, erhält der vorgeschlagene Spieler die normale Gruppeneinladung und kann diese annehmen oder ablehnen.

Damit können Gruppenmitglieder bei der Zusammenstellung helfen, während die endgültige Entscheidung beim Gruppenleiter bleibt.

---

# 4. Gruppenanzeige

Jedes Gruppenmitglied erhält ein eigenes Feld im Gruppenfenster.

## Reihe 1

* Klassensymbol
* Level
* Name

## Reihe 2

* HP
* MP
* Buffs
* Debuffs

Ist ein Spieler außerhalb der Gruppenreichweite, wird sein gesamtes Gruppenfeld abgedunkelt.

Damit ist unmittelbar sichtbar, dass der Spieler weiterhin Mitglied der Gruppe ist, sich aber momentan nicht aktiv bei der Gruppe befindet.

---

# 5. Gruppenreichweite

Die Gruppenreichweite beträgt für V1:

**100 Meter**

Dieser Wert ist ein erster Testwert und muss zentral konfigurierbar sein.

Der **Gruppenleiter bildet den räumlichen Mittelpunkt der Gruppe**.

Es werden nicht die Entfernungen aller Gruppenmitglieder untereinander verglichen.

Für jedes Mitglied genügt:

`Distanz Spieler <-> Gruppenleiter <= Gruppenreichweite`

Dadurch kann keine Kette von Spielern entstehen, die die effektive Gruppenreichweite künstlich vergrößert.

Die Gruppenreichweite ersetzt nicht die individuellen Reichweiten von Fähigkeiten, Heilungen, Buffs, AoE-Effekten oder anderen Spielmechaniken.

---

# 6. Außerhalb der Gruppenreichweite

Ein Spieler außerhalb der Gruppenreichweite bleibt Mitglied der Gruppe.

Er gilt jedoch nicht als aktiv anwesendes Gruppenmitglied.

Folgen:

* Gruppenfeld wird abgedunkelt
* keine Gruppen-EXP
* kein gruppenbasierter Questfortschritt
* Richtungspfeil am Bildschirmrand zeigt zum Gruppenleiter

Der Richtungspfeil aktualisiert sich entsprechend der Position des Gruppenleiters.

Sobald der Spieler wieder innerhalb der Gruppenreichweite ist, verschwindet der Pfeil und die normale Gruppenteilnahme wird wieder aktiv.

Die Gruppenreichweite soll als gemeinsame technische Teilnahmebedingung wiederverwendbar sein, damit spätere Systeme nicht jeweils eigene Definitionen von „bei der Gruppe" entwickeln.

---

# 7. Gruppen-EXP

Ein Monster besitzt grundsätzlich **100 % seiner normalen EXP**.

Diese EXP wird gleichmäßig unter allen beim Tod des Monsters berechtigten Gruppenmitgliedern innerhalb der Gruppenreichweite verteilt.

## Verteilung

| Berechtigte Spieler | EXP pro Spieler | Gesamt |
| ------------------: | --------------: | -----: |
|                   1 |           100 % |  100 % |
|                   2 |            50 % |  100 % |
|                   3 |         33,33 % |  100 % |
|                   4 |            25 % |  100 % |

Entscheidend ist die aktive Gruppenmitgliedschaft und Gruppenreichweite zum Zeitpunkt des Monster-Todes.

Beispiel:

Eine Vierergruppe kämpft gegen ein Monster. Beim Tod befinden sich nur drei Mitglieder innerhalb der Gruppenreichweite.

Dann erhalten diese drei Spieler jeweils ein Drittel der Monster-EXP. Das entfernte Mitglied erhält keine EXP.

Die Gesamtmenge wird dadurch nicht reduziert.

Die konkrete EXP-Balance wird später im Spiel getestet. Falls Gruppen dadurch zu langsam oder zu schnell leveln, können die Werte angepasst werden, ohne die Grundarchitektur zu verändern. Ein Gruppenbonus auf Charakter-EXP ist dabei nicht vorgesehen (siehe `Erfahrung_und_Progressionssystem.md`, Abschnitt 8).

Die EXP-Berechnung selbst (EXP-Kurve, Levelunterschied-Anpassung, Level-Cap, Attributpunkte, Entdeckungs-/Quest-EXP und Rested EXP) ist in `Erfahrung_und_Progressionssystem.md` definiert. Bei Gruppenkills wird für die Levelunterschied-Anpassung das höchste Charakterlevel eines EXP-berechtigten Gruppenmitglieds verwendet (siehe `Erfahrung_und_Progressionssystem.md`, Abschnitt 8.1).

---

# 8. Gruppen-Claim

Der bestehende Grundsatz bleibt:

**Der erste gültige Schaden bestimmt den Claim.**

Verursacht ein Spieler den ersten gültigen Schaden und befindet er sich in einer Gruppe, gehört der Claim der **Gruppe als Einheit**.

Der Claim wird nicht an eine beim ersten Treffer eingefrorene Mitgliederliste gebunden.

## Beitritt während des Kampfes

Tritt ein Spieler während eines laufenden Kampfes der claimenden Gruppe bei, erhält er ebenfalls den Gruppen-Claim.

Tritt er erst nach Abschluss des Kampfes bei, entsteht kein rückwirkender Anspruch.

## Verlassen der Gruppe

Verlässt ein Spieler die Gruppe, verliert er unmittelbar seinen Anspruch aus dem Gruppen-Claim.

Der Claim verbleibt bei der bestehenden Gruppe.

Löst sich die Gruppe nach und nach auf, bleibt ein bestehender Claim schließlich beim letzten verbliebenen Spieler.

Damit geht ein laufender Claim nicht verloren, nur weil aus einer Gruppe wieder ein einzelner Spieler wird.

## Spawn- und Eventauslöser

Das Auslösen eines Spawns, einer Monsterwelle oder eines Events erzeugt keinen Claim.

Auch bei den „Wellen des Schmerzes" gilt weiterhin:

**Erster gültiger Schaden = Claim.**

Eine andere Gruppe darf deshalb einen ausgelösten Boss übernehmen, wenn sie zuerst gültigen Schaden verursacht.

---

# 9. Disconnect und Reconnect

Ein Verbindungsabbruch entfernt einen Spieler nicht sofort aus der Gruppe.

Die Reconnect-Frist beträgt maximal:

**5 Minuten**

Der genaue Wert soll zentral konfigurierbar sein.

## Normales Gruppenmitglied

Während der Reconnect-Frist:

* bleibt der Spieler Mitglied der Gruppe
* wird er als offline behandelt
* erhält er keine Gruppen-EXP
* erhält er keinen gruppenbasierten Questfortschritt

Reconnectet der Spieler innerhalb der Frist, kehrt er in seine bestehende Gruppenmitgliedschaft zurück.

Nach Ablauf der Frist wird er aus der Gruppe entfernt.

## Disconnect des Gruppenleiters

Ein Disconnect des Gruppenleiters darf die Gruppe nicht handlungsunfähig machen.

Die Gruppenleitung wird deshalb unmittelbar **vorübergehend an das am längsten in der Gruppe befindliche Online-Mitglied** übertragen.

Dieses Mitglied wird währenddessen auch zum Mittelpunkt der 100-Meter-Gruppenreichweite.

Dadurch kann die Gruppe weiter:

* kämpfen
* Gruppen-EXP erhalten
* Questfortschritt erhalten
* ihre normale Gruppenreichweite verwenden

Reconnectet der ursprüngliche Gruppenleiter innerhalb der 5-Minuten-Frist, erhält er die Gruppenleitung automatisch zurück.

Läuft die Reconnect-Frist ab, wird der ursprüngliche Leiter aus der Gruppe entfernt. Die vorübergehend übertragene Gruppenleitung wird anschließend dauerhaft.

---

# 10. Noch nicht Bestandteil von V1

Bewusst nicht Bestandteil dieses ersten Gruppensystems sind unter anderem:

* Raid-/Schlachtzugsgruppen
* Dungeon-Finder
* Event-Matchmaking
* Gildensystem
* vollständige Lootverteilung
* FFA-/Leiter-/Würfel-Lootlogik
* Rollenwarteschlangen
* automatische Gruppenzusammenstellung
* konkrete Questsystem-Implementierung
* vollständige Gruppen-UI

V1 soll zuerst die technische Grundlage schaffen:

**Gruppe bilden -> verwalten -> räumliche Teilnahme bestimmen -> EXP verteilen -> Claim an Gruppe binden -> Disconnect/Reconnect behandeln.**

---

# Cross-Referenzen

- **Kampfsystem.md** – Der Gruppen-Claim (§8) ist die Gruppenebene des allgemeinen Claim-Grundsatzes (erster gültiger Schaden). Die Gruppenreichweite ersetzt nicht individuelle Reichweiten von Fähigkeiten, Heilungen oder AoE.
- **Boss-System.md** – Boss-Claim (§2): Spieler oder Gruppe, die einem Boss als Erstes Schaden zufügt, erhält den Claim. Gruppen-Claim ist die Gruppenebene davon; „Wellen des Schmerzes" (§9) bleiben ebenfalls Claim-first.
- **Lootsystem.md** – Gruppen-Loot wird von der Claim-Berechtigung angestoßen; konkrete Verteilung (FFA/Leiter/Würfel) ist nicht Teil von V1.
- **Quest-System.md** – Gruppenbasierter Questfortschritt (§12–13) nutzt die räumliche Teilnahmebedingung (§5/6) als aktive Anwesenheit.
- **Dungeon-Finder.md** – Vermittelt Spieler für Gruppen; das Gruppensystem (V1) bildet die Grundlage, auf die der Dungeon-Finder aufbaut.
- **Event-Matchmaking.md** – Erzeugt bewusst andere Regeln als das normale Gruppensystem; Gruppenzusammenstellung ist hier Teil der Eventherausforderung.
- **exp1_Unterwelt.md** (Abschnitt 56) – Normale Gruppengröße steigt von 4 auf 5 mit Exp1. Die zentrale Konfigurierbarkeit (§1) ermöglicht dies ohne Architekturumbau.
- **Ability-System.md** (§7) – Gruppenfähigkeiten bleiben auf die eigene Gruppe beschränkt; die Gruppenzugehörigkeit muss vom Ability-System abgefragt werden können.
- **Politik-Herrschaftssystem.md** (§§36–39) – Temporäre Trennung vom aktiven Gruppenverbund bei Fraktionskämpfen; baut auf der Gruppenzugehörigkeit (V1) auf, überschreibt sie aber nicht.
