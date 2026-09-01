# Andora – Dungeon-Finder

## 1. Status

**Status:** 🟡 Konzept

Der Dungeon-Finder unterstützt Spieler dabei, passende Mitspieler für normale Dungeons zu finden.

Er soll insbesondere Spielern ermöglichen, Gruppeninhalte zu spielen, ohne dafür dauerhaft einer Gilde oder festen Gruppe angehören zu müssen.

Der Dungeon-Finder übernimmt dabei die **Suche und Vermittlung**.

Die Entscheidung, ob Spieler tatsächlich gemeinsam eine Gruppe bilden, bleibt bei den Spielern.

---

# 2. Grundprinzip

> **Der Dungeon-Finder findet passende Spieler. Die Spieler entscheiden selbst, ob daraus eine Gruppe entsteht.**

Der Dungeon-Finder erstellt deshalb nicht ungefragt vollständige Gruppen und teleportiert Spieler nicht automatisch in einen Dungeon.

Vor einer Zusammenführung müssen beide Seiten zustimmen:

* der gefundene Spieler
* der Gruppenleiter

---

# 3. Anmeldung eines Spielers

Ein Spieler kann seinen Charakter für den Dungeon-Finder anmelden.

Dabei kennt das System mindestens:

* Charakter
* Klasse
* Rolle
* Level
* gewünschter Dungeon

Beispiel:

```text
Charakter: Boran
Klasse: Paladin
Rolle: Tank
Level: 20
Dungeon: Ruinen von Andora
```

Der Spieler signalisiert damit:

> Ich suche eine Gruppe für diesen Dungeon.

---

# 4. Suche einer bestehenden Gruppe

Eine bereits bestehende Gruppe kann ebenfalls den Dungeon-Finder verwenden.

Beispiel:

```text
Dungeon:
Ruinen von Andora

Vorhandene Gruppe:

Heiler   Level 21
DD       Level 19
DD       Level 20
Magier   Level 20

Gesucht:

Tank
Levelbereich passend zum Dungeon
```

Der Dungeon-Finder sucht anschließend nach angemeldeten Charakteren, die zu den Suchbedingungen passen.

---

# 5. Gefundener Kandidat

Findet der Dungeon-Finder beispielsweise einen passenden Tank, wird dieser **nicht sofort der Gruppe hinzugefügt**.

Stattdessen entsteht zunächst ein Match-Vorschlag.

Beide Seiten erhalten eine Anfrage.

---

# 6. Anfrage an den gefundenen Spieler

Der gefundene Spieler erhält Informationen über die bestehende Gruppe.

Beispiel:

```text
Dungeon-Gruppe gefunden

Dungeon:
Ruinen von Andora

Gruppe:

Heiler   Level 21
DD       Level 19
DD       Level 20
Magier   Level 20

Deine Rolle:

Tank – Level 20

[Beitreten]   [Ablehnen]
```

Der Spieler kann dadurch vor seiner Entscheidung sehen, mit welcher Gruppenzusammensetzung er den Dungeon betreten würde.

---

# 7. Freie Entscheidung des gefundenen Spielers

Der gefundene Spieler muss die vorgeschlagene Gruppe nicht akzeptieren.

Beispiel:

```text
DD
DD
Magier
Magier

Gesucht:
Tank
```

Ein Tank kann diese Gruppe ablehnen, wenn er beispielsweise lieber mit einem Heiler spielen möchte.

Der Dungeon-Finder schreibt keine bestimmte Entscheidung vor.

Nach einer Ablehnung kann der Spieler weiterhin im Dungeon-Finder bleiben und auf eine andere Anfrage warten.

---

# 8. Anfrage an den Gruppenleiter

Der Gruppenleiter erhält gleichzeitig Informationen über den gefundenen Kandidaten.

Beispiel:

```text
Passender Spieler gefunden

Klasse:
Paladin

Level:
20

Rolle:
Tank

[Annehmen]   [Ablehnen]
```

Der Gruppenleiter entscheidet ebenfalls selbst, ob dieser Spieler aufgenommen werden soll.

---

# 9. Beidseitige Zustimmung

Eine Gruppe wird erst zusammengeführt, wenn beide Seiten zugestimmt haben.

```text
Gefundener Spieler
       │
       └── bestätigt
                │
                ▼
            MATCH
                ▲
                │
       ┌── bestätigt
       │
Gruppenleiter
```

Nur wenn beide bestätigen:

```text
Spieler bestätigt
+
Leader bestätigt
        ↓
Match akzeptiert
        ↓
Spieler wird Gruppe hinzugefügt
```

Lehnt eine Seite ab, kommt das Match nicht zustande.

---

# 10. Keine vorgeschriebene Standardgruppe

Der normale Dungeon-Finder erzwingt keine feste Gruppenzusammensetzung wie:

```text
1 Tank
1 Heiler
3 DD
```

Eine Gruppe darf selbst entscheiden, welche Zusammensetzung sie ausprobieren möchte.

Beispielsweise könnte eine Gruppe bewusst aus:

```text
Tank
DD
DD
Magier
Magier
```

bestehen.

Der Dungeon-Finder kann dabei helfen, fehlende Spieler zu finden.

Er entscheidet jedoch nicht, ob diese Zusammenstellung spielerisch sinnvoll ist.

---

# 11. Unterschied zwischen Vermittlung und Matchmaking

Der normale Dungeon-Finder ist ein **Vermittlungssystem**.

Er ist kein vollständig automatisches Matchmaking-System.

Grundregel:

> **Der Finder übernimmt die Suche – nicht die Entscheidung.**

Dadurch bleibt die Kontrolle bei den Spielern.

---

# 12. Dungeon-Start

Ist die gewünschte Gruppe vollständig und haben alle notwendigen Spieler ihre Teilnahme bestätigt, kann der Dungeon-Run beginnen.

Die Gruppe wird anschließend in den entsprechenden Dungeon gebracht.

Der Dungeon selbst kann als eigene Instanz laufen.

---

# 13. Temporäre Dungeon-Gruppe

Eine über den Dungeon-Finder entstandene Gruppe ist grundsätzlich eine temporäre Zweckgemeinschaft.

```text
Dungeon-Finder
      ↓
Gruppe gefunden
      ↓
Bestätigung
      ↓
Dungeon-Run
      ↓
Dungeon abgeschlossen
      ↓
Finder-Gruppe endet
```

Nach Abschluss des Runs können die Spieler wieder getrennte Wege gehen.

---

# 14. Keine dauerhafte soziale Verpflichtung

Der Dungeon-Finder soll insbesondere Spielern helfen, die:

* überwiegend solo spielen
* keiner Gilde angehören
* keine feste Gruppe besitzen
* spontan einen Dungeon spielen möchten
* nur begrenzt Zeit haben

Ein Spieler muss deshalb keine dauerhafte soziale Bindung eingehen, nur um einen normalen Dungeon spielen zu können.

---

# 15. Freiwillige weitere Zusammenarbeit

Verstehen sich Spieler während eines Dungeon-Runs gut, können sie anschließend freiwillig:

* Freunde werden
* eine normale Gruppe bilden
* weitere Dungeons gemeinsam spielen
* später einer gemeinsamen Gilde angehören

Dies ist jedoch unabhängig vom Dungeon-Finder.

Der Finder selbst erzwingt keine dauerhafte Verbindung.

---

# 16. Verbindung zur Charakterprogression

Der Dungeon-Finder stellt einen möglichen Weg zu Gruppencontent und damit auch zu besserer Ausrüstung dar.

Dadurch kann beispielsweise ein überwiegend solo spielender Charakter:

```text
Solo spielen
      ↓
Tier-Basic-Ausrüstung
      ↓
Dungeon-Finder benutzen
      ↓
temporäre Gruppe
      ↓
Dungeon spielen
      ↓
Chance auf bessere Ausrüstung
      ↓
wieder solo weiterspielen
```

Eine Gilde ist deshalb hilfreich, aber keine Voraussetzung für normalen Dungeon-Content.

---

# 17. Klasse, Rolle und Level

Für das Matching sind insbesondere relevant:

```text
Klasse
Rolle
Level
Dungeon
```

Beispielsweise:

```text
Paladin
Tank
Level 20
```

Die Rolle muss nicht zwingend ausschließlich aus der Klasse abgeleitet werden, falls das spätere Klassensystem unterschiedliche Rollen einer Klasse ermöglicht.

Der Dungeon-Finder arbeitet deshalb mit der **tatsächlich angemeldeten Rolle** des Charakters.

---

# 18. Keine Gearscore-Bewerbungsplattform

Der Dungeon-Finder soll zunächst bewusst einfach bleiben.

Für die Vermittlung stehen vor allem Klasse, Rolle und Level im Mittelpunkt.

Das System soll nicht unnötig zu einer Bewerbungsplattform werden, bei der Spieler allein anhand immer umfangreicherer Ausrüstungsstatistiken vorsortiert werden.

Ob später für bestimmte besonders schwierige Inhalte zusätzliche Voraussetzungen notwendig werden, kann bei der Entwicklung dieser Inhalte entschieden werden.

---

# 19. Ablehnung

Eine Ablehnung ist ein normaler Bestandteil des Systems.

Sie bedeutet lediglich:

> Dieses vorgeschlagene Match kommt nicht zustande.

Der Dungeon-Finder kann anschließend nach einer anderen passenden Kombination suchen.

Spieler sollen nicht gezwungen werden, eine Gruppenzusammenstellung anzunehmen, mit der sie nicht spielen möchten.

---

# 20. Anfrage läuft aus

Eine Match-Anfrage kann eine begrenzte Bestätigungszeit besitzen.

Reagiert eine Seite nicht innerhalb dieser Zeit, gilt das Match als nicht zustande gekommen.

Die genaue Dauer wird später beim UI- und Gameplay-Design festgelegt.

---

# 21. Abgrenzung zu Event-Matchmaking

Der normale Dungeon-Finder darf nicht mit späteren Event-Modi verwechselt werden.

Bei besonderen Events kann Andora bewusst Regeln verwenden wie:

* vollständig zufällige Gruppen
* zwei zufällige Gruppen gegeneinander
* Zeitrennen
* ungewöhnliche Klassenzusammenstellungen
* standardisierte Charakterwerte
* Geschicklichkeitswettbewerbe

Diese Mechaniken gehören zum **Event-Matchmaking** und sind keine Grundregel des normalen Dungeon-Finders.

---

# 22. Serverautorität

Der Server verwaltet:

* Finder-Anmeldungen
* Gruppensuchen
* Kandidaten
* Match-Vorschläge
* Bestätigungen
* Gruppenmitgliedschaften
* Dungeon-Zuordnung
* Dungeon-Instanzen
* Abschluss des Runs

Der Client zeigt lediglich die entsprechenden Informationen und Eingabemöglichkeiten an.

Ein Client kann sich nicht selbst einer Gruppe hinzufügen oder einen Match-Vorschlag als angenommen markieren, ohne dass der Server dies validiert.

---

# 23. Ziel des Systems

Der Dungeon-Finder soll den Zugang zu normalen Gruppeninhalten erleichtern, ohne die soziale Freiheit der Spieler einzuschränken.

Ein Solospieler soll spontan einen Dungeon spielen können.

Eine bestehende Gruppe soll unkompliziert ein fehlendes Mitglied finden können.

Gleichzeitig behalten beide Seiten die Kontrolle darüber, mit wem sie spielen.

> **Der Dungeon-Finder vermittelt Gruppen – er schreibt keine Gruppen vor.**

> **Der Finder übernimmt die Suche. Die Spieler treffen die Entscheidung.**
