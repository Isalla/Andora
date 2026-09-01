# Andora – Event-Matchmaking

## 1. Status

**Status:** 🟡 Konzept / späteres Event-System

Event-Matchmaking ist ein besonderes Matchmaking-System für zeitlich begrenzte oder spezielle Events.

Es ist ausdrücklich **nicht** das normale Gruppensystem und ersetzt nicht den Dungeon-Finder.

Während der normale Dungeon-Finder Spieler und Gruppen lediglich vermittelt und beide Seiten einer Zusammenführung zustimmen müssen, dürfen besondere Events bewusst andere Regeln verwenden.

---

# 2. Grundprinzip

> **Random-Matchmaking ist eine mögliche Eventregel und kein Grundsystem von Andora.**

Events dürfen Spieler beispielsweise zufällig auf Gruppen verteilen.

Die zufällige Gruppenzusammenstellung selbst kann dabei Bestandteil der Herausforderung sein.

---

# 3. Abgrenzung zum Dungeon-Finder

## Normaler Dungeon-Finder

Der Dungeon-Finder:

* sucht passende Spieler
* berücksichtigt Klasse, Rolle und Level
* zeigt dem gefundenen Spieler die vorhandene Gruppe
* zeigt dem Gruppenleiter den gefundenen Kandidaten
* benötigt die Zustimmung beider Seiten
* lässt den Spielern die Entscheidung über ihre Gruppenzusammenstellung

Grundregel:

> **Der Dungeon-Finder vermittelt – die Spieler entscheiden.**

---

## Event-Matchmaking

Ein Event kann dagegen ausdrücklich festlegen:

* zufällige Gruppenzusammenstellung
* zufällige Zuweisung von Spielern
* konkurrierende Gruppen
* Zeitrennen
* Geschicklichkeitsaufgaben
* besondere Gruppenregeln
* standardisierte Charakterbedingungen

Diese Regeln gelten ausschließlich innerhalb des jeweiligen Events.

---

# 4. Zufällige Gruppen

Ein Event kann angemeldete Spieler zufällig auf Gruppen verteilen.

Beispiel:

```text id="8k6a0p"
10 Spieler
     ↓
Event-Matchmaking
     ↓
zufällige Verteilung
     ↓
┌───────────────┬───────────────┐
│   Gruppe A    │   Gruppe B    │
│   5 Spieler   │   5 Spieler   │
└───────────────┴───────────────┘
```

Dabei muss keine klassische Zusammenstellung wie:

```text id="kz7m2n"
1 Tank
1 Heiler
3 DD
```

entstehen.

Die zufällige Zusammensetzung kann bewusst Teil des Events sein.

---

# 5. Ungewöhnliche Gruppenzusammenstellungen

Eine Event-Gruppe könnte beispielsweise bestehen aus:

```text id="wq4r8s"
Tank
Tank
Magier
Heiler
Heiler
```

oder:

```text id="f3v9da"
Tank
DD
DD
Magier
Magier
```

Die Spieler müssen anschließend herausfinden, wie sie die Stärken ihrer zufällig entstandenen Gruppe am besten einsetzen.

Damit können Events bewusst von den üblichen Gruppenkonventionen abweichen.

---

# 6. Gruppen gegeneinander

Event-Matchmaking kann mehrere Gruppen gleichzeitig erzeugen, die gegeneinander antreten.

Ein mögliches Modell:

```text id="n5ej7u"
10 zufällige Spieler
        ↓
   Event-Matchmaking
        ↓
┌──────────────┬──────────────┐
│   Gruppe A   │   Gruppe B   │
│  5 Spieler   │  5 Spieler   │
└──────────────┴──────────────┘
        ↓              ↓
 gleiche Aufgabe / gleiche Bedingungen
        ↓              ↓
      Event-Wettbewerb
        ↓
 schnellere / erfolgreichere Gruppe
        ↓
             gewinnt
```

---

# 7. Zeitrennen

Eine mögliche Eventform ist ein Rennen gegen die Zeit.

Beide Gruppen erhalten dieselbe Aufgabe unter vergleichbaren Bedingungen.

Beispiele:

* denselben Dungeon abschließen
* denselben Boss besiegen
* mehrere Ziele erreichen
* NPCs retten
* bestimmte Gegenstände sammeln
* Mechanismen aktivieren
* einen Parcours bewältigen

Die Gruppe, die das Ziel zuerst erreicht, gewinnt den Run und die dafür vorgesehene Belohnung.

---

# 8. Gleiche Aufgabe

Bei direkten Wettbewerben sollen die Gruppen grundsätzlich dieselbe Aufgabe erhalten.

Dadurch bleibt verständlich, warum eine Gruppe gewonnen hat.

Beispiel:

```text id="cq3t1m"
Gruppe A
↓
3 Türme zerstören
10 Gefangene retten
Endziel erreichen


Gruppe B
↓
3 Türme zerstören
10 Gefangene retten
Endziel erreichen
```

Die schnellere Gruppe gewinnt.

---

# 9. Charakterbasierte Herausforderungen

Ein Event kann weiterhin die normalen Fähigkeiten der Charaktere verwenden.

Dann spielen beispielsweise eine Rolle:

* Klasse
* Fähigkeiten
* Gruppenzusammensetzung
* Ausrüstung
* Charakterlevel
* Buffs
* taktische Zusammenarbeit

Eine ungewöhnliche Random-Gruppe muss dann versuchen, aus ihrer jeweiligen Zusammensetzung das Beste herauszuholen.

---

# 10. Spielerbasierte Herausforderungen

Events können jedoch auch bewusst so gestaltet werden, dass nicht die Stärke des Charakters, sondern das **Können des Spielers** entscheidet.

Mögliche Herausforderungen:

* Geschicklichkeitsparcours
* Sprungpassagen
* bewegliche Hindernisse
* Reaktionsaufgaben
* Ausweichmechaniken
* Schalterrätsel
* Beobachtungsaufgaben
* Koordinationsaufgaben
* Teamrätsel

Hier kann Charakterprogression bewusst eine kleinere oder gar keine Rolle spielen.

---

# 11. Geschicklichkeit statt Ausrüstung

Bei entsprechenden Events können Charakterwerte standardisiert oder für die relevante Mechanik bedeutungslos gemacht werden.

Dadurch bekommt ein Spieler mit besonders hochwertiger Ausrüstung nicht automatisch einen Vorteil.

Entscheidend sind stattdessen beispielsweise:

* Reaktionsfähigkeit
* Bewegung
* Timing
* Beobachtung
* Verständnis der Mechanik
* Kommunikation
* Zusammenarbeit

Grundprinzip:

> **Ein Event darf prüfen, wie gut der Spieler ist – nicht nur, wie stark sein Charakter geworden ist.**

---

# 12. Kooperative Geschicklichkeit

Geschicklichkeitsevents sollen nicht ausschließlich aus individuellen Parcours bestehen.

Auch die Zusammenarbeit einer Gruppe kann geprüft werden.

Beispiele:

Ein Spieler hält einen Schalter gedrückt, während andere eine Passage überwinden.

Mehrere Spieler müssen Mechanismen gleichzeitig aktivieren.

Ein Spieler öffnet einen Weg, den zunächst nur seine Gruppenmitglieder benutzen können.

Die Gruppe muss sich bei beweglichen Hindernissen gegenseitig helfen.

Mehrere Aufgaben müssen koordiniert in der richtigen Reihenfolge ausgeführt werden.

Dadurch entscheidet nicht nur individuelles Können, sondern auch Teamarbeit.

---

# 13. Mischung aus Charakter- und Spielerfähigkeit

Ein Event kann beide Systeme kombinieren.

Beispielsweise:

```text id="p2df7x"
Phase 1
Kampf
→ Charakterfähigkeiten wichtig

Phase 2
Parcours
→ Spieler-Geschicklichkeit wichtig

Phase 3
Gruppenmechanik
→ Kommunikation wichtig

Phase 4
Boss
→ Klassen + Können + Zusammenarbeit
```

Dadurch können sehr unterschiedliche Eventtypen entstehen.

---

# 14. Eventregeln sind lokal

Besondere Matchmaking- oder Charakterregeln gelten ausschließlich innerhalb des jeweiligen Events.

Ein Random-Matchmaking-Event verändert nicht:

* den normalen Dungeon-Finder
* normale Gruppen
* Gildengruppen
* normale Dungeon-Regeln
* die allgemeine Charakterprogression

Nach Ende des Events gelten wieder die normalen Andora-Systeme.

---

# 15. Experimentierfeld für neue Spielideen

Event-Matchmaking kann später auch genutzt werden, um ungewöhnliche Spielmechaniken auszuprobieren.

Beispielsweise:

* neue Gruppengrößen
* ungewöhnliche Rollenverteilungen
* Random-Gruppen
* Zeitrennen
* besondere Dungeon-Regeln
* standardisierte Ausrüstung
* Geschicklichkeitsspiele
* kooperative Rätsel

Funktioniert eine Idee gut, kann sie erneut verwendet oder weiterentwickelt werden.

Funktioniert sie nicht, muss daraus kein dauerhaftes Grundsystem entstehen.

---

# 16. Temporärer Charakter

Event-Gruppen bestehen nur für das jeweilige Event.

Nach Abschluss:

```text id="gx9v4c"
Event endet
    ↓
Ergebnis wird ausgewertet
    ↓
Belohnung vergeben
    ↓
Event-Gruppe wird aufgelöst
    ↓
Spieler kehren zum normalen Spiel zurück
```

Dauerhafte Gruppen oder Freundschaften können Spieler anschließend selbstverständlich selbst bilden.

---

# 17. Belohnungen

Die genaue Belohnungsstruktur wird später zusammen mit den jeweiligen Events festgelegt.

Bei Wettbewerbsevents kann die Gewinnergruppe eine besondere Belohnung erhalten.

Dabei muss nicht jedes Event dieselbe Belohnungslogik verwenden.

Die Belohnung gehört zur Definition des jeweiligen Events und nicht fest zum Matchmaking-System.

---

# 18. Fairness

Bei direkten Wettbewerben müssen die für den Wettbewerb relevanten Bedingungen für beide Gruppen vergleichbar sein.

Insbesondere bei Zeitrennen gilt:

> **Beide Gruppen erhalten dieselbe Aufgabe unter vergleichbaren Bedingungen.**

Zufällige Gruppenzusammenstellungen dürfen dagegen bewusst unterschiedlich sein, wenn genau dies Bestandteil des Eventdesigns ist.

---

# 19. Serverautorität

Der Server verwaltet:

* Event-Anmeldung
* Teilnehmer
* Randomisierung
* Gruppenzuweisung
* Event-Instanzen
* Startzeit
* Eventzustand
* Ziele
* Fortschritt
* Abschlusszeit
* Gewinner
* Belohnungen

Der Client stellt das Event lediglich dar und sendet Spieleraktionen an den Server.

---

# 20. Ziel des Systems

Event-Matchmaking soll Andora die Möglichkeit geben, zeitweise bewusst mit den normalen Regeln zu spielen.

Random-Gruppen, Wettrennen und Geschicklichkeitsaufgaben werden dadurch zu besonderen Erlebnissen, ohne den normalen Dungeon-Finder oder das reguläre Gruppenspiel zu verändern.

> **Der normale Dungeon-Finder hilft Spielern, ihre Gruppe zu finden.**

> **Event-Matchmaking darf die Gruppe selbst zur Herausforderung machen.**

Und:

> **Nicht jedes Event muss fragen, wie stark dein Charakter ist. Manche dürfen fragen, wie gut du spielst.**
