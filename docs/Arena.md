# Andora – Arena-System

## Status

**Konzept / Planung**

Das Arena-System ist noch nicht implementiert. Dieses Dokument hält die bisher festgelegten Regeln und Architekturentscheidungen fest.

---

## 1. Grundprinzip

Arena-Kämpfe sind vom normalen Weltkampf getrennt.

> **Arena-Niederlage und Welt-Tod sind zwei vollständig getrennte Zustände.**

Ein Spieler, der in einer Arena besiegt wird, stirbt nicht in der Spielwelt.

Die normale Death-Mechanik darf deshalb während eines aktiven Arena-Kampfes nicht ausgelöst werden.

---

## 2. Arena-Match

Jeder Arena-Kampf besitzt einen eigenen serverseitigen Match-Kontext.

Beispiel:

```text
ArenaMatch
├── match_id
├── arena_id
├── player_ids[]
├── state
├── win_condition
├── started_at
└── result
```

Mögliche grundlegende Match-Zustände:

```text
WAITING
ACTIVE
FINISHED
```

Der aktive Match-Zustand liegt während des Kampfes im Arbeitsspeicher des Gameservers.

MariaDB dient für persistente Daten wie:

* Match-ID
* Teilnehmer
* Arena / Spielmodus
* Gewinner und Verlierer
* Start und Ende
* mögliche Wertungsänderungen
* Statistiken
* Match-Historie

Die Datenbank wird nicht bei jedem Treffer abgefragt.

---

## 3. Sieg- und Niederlagebedingungen

Der jeweilige Arena-Modus bestimmt seine eigenen Siegbedingungen.

Mögliche Bedingungen sind beispielsweise:

* Lebenspunkte erreichen 0
* Spieler steckt X Treffer ein
* Spieler erreicht X Punkte
* bestimmtes Ziel wird erfüllt
* Zeitlimit mit anschließendem Punktvergleich

Diese Bedingungen sollen nicht fest in die allgemeine Combat-Engine eingebaut werden.

Der Arena-Modus definiert die geltenden Regeln.

---

## 4. 0 HP in der Arena

Während:

```text
ArenaMatch.state = ACTIVE
```

bedeutet:

```text
HP <= 0
```

nicht den Tod des Spielers.

Stattdessen wird der Spieler als besiegt markiert.

Beispiel:

```text
arena_status = DEFEATED
hp = 0
combat_locked = true
```

Dabei wird insbesondere nicht ausgelöst:

* normaler Welt-Tod
* Leiche
* normaler Respawn
* Todesmalus
* Lootverlust

Der Spieler bleibt Bestandteil des Arena-Matches, kann aber nicht weiterkämpfen.

---

## 5. Ende des Kampfes

Der Server entscheidet verbindlich, wann der Kampf beendet ist.

Erst bei:

```text
ArenaMatch.state = FINISHED
```

werden die Arena-Kampfsperren aufgehoben.

Ein besiegter Spieler steht anschließend mit mindestens:

```text
HP = 1
```

wieder auf.

Danach greifen wieder die normalen Spielmechaniken.

---

## 6. Söldner und Begleiter

Söldner dürfen ihren Spieler grundsätzlich bis in die Arena begleiten.

Bei normalen Arena-Kämpfen nehmen sie jedoch nicht am Kampf teil.

Vor Kampfbeginn:

```text
Spieler betritt Arena
        ↓
Söldner folgt
        ↓
Match wird ACTIVE
        ↓
Söldner verlässt Kampffläche
        ↓
Söldner wartet am Arenarand
```

Während des aktiven Kampfes dürfen diese Söldner keine spielmechanische Unterstützung leisten.

Dazu gehören insbesondere:

* keine Heilung
* keine Buffs
* keine Angriffe
* keine Items
* keine sonstige Combat-Unterstützung

Sie bleiben jedoch als Figuren anwesend und können ihren Spieler beispielsweise anfeuern.

---

## 7. Verhalten nach dem Kampf

Sobald der Server das Match auf:

```text
FINISHED
```

setzt, dürfen die normalen Begleitermechaniken wieder aktiviert werden.

Die Söldner kommen vom Arenarand zu ihrem Spieler zurück.

Hat der Spieler beispielsweise einen Heiler als Begleiter und wurde im Kampf verletzt, darf der Heiler ihn **nach dem offiziellen Kampfende** wieder heilen.

Beispiel:

```text
Match = FINISHED
        ↓
Companion Actions freigegeben
        ↓
Spieler verletzt?
        ↓
Heiler kann HEAL_OWNER ausführen
```

Der Arena-Kampf selbst bleibt dadurch unbeeinflusst.

---

## 8. Arena-Modi mit Söldnern

Es kann besondere Arena-Modi geben, in denen Söldner ausdrücklich am Kampf teilnehmen dürfen.

Beispiel:

```text
Standard Arena
companions_allowed = false
```

oder:

```text
Companion Arena
companions_allowed = true
max_companions = 2
```

Der Arena-Modus entscheidet serverseitig, ob Begleiter teilnehmen dürfen.

Die Söldner-KI entscheidet dies nicht selbst.

---

## 9. Arena als Instanz

Für Arena-Kämpfe ist eine eigene serverseitige Instanz vorgesehen.

Beispiel:

```text
World
├── normale Region
├── Dungeon Instance
├── Arena Instance 1204
└── Arena Instance 1205
```

Nur die für das Match vorgesehenen Teilnehmer befinden sich aktiv in der Kampfinstanz.

Dadurch können:

* fremde Spieler nicht eingreifen
* Weltmonster nicht in den Kampf gelangen
* Teilnehmer eindeutig bestimmt werden
* Arena-Regeln sauber angewendet werden
* Kampfgrenzen kontrolliert werden
* Zuschauer von Teilnehmern getrennt werden

Die Arena muss dafür kein eigener Serverprozess sein.

Sie kann eine World-Instance innerhalb des Gameservers darstellen.

---

## 10. Zuschauer

Da eine instanzierte Arena von normalen Spielern nicht direkt betreten werden kann, erhält das Arena-System einen eigenen Zuschauermechanismus.

Zuschauer befinden sich nicht als aktive Teilnehmer in der Kampfinstanz.

Der Zuschauerzugriff ist:

```text
READ ONLY
```

Zuschauer können:

* Kämpfer sehen
* Bewegungen sehen
* Animationen sehen
* Treffer sehen
* Arena-Effekte sehen
* relevante Matchinformationen sehen

Zuschauer können nicht:

* kämpfen
* Skills benutzen
* Teilnehmer beeinflussen
* Gegenstände in den Kampf bringen
* sich als Teilnehmer ausgeben

---

## 11. Spectator Viewpoints

Eine Arena kann mehrere fest definierte Zuschauer-Sichtpunkte besitzen.

Beispiel:

```text
Arena
├── Viewpoint 1 – Gesamtansicht
├── Viewpoint 2 – Nordseite
├── Viewpoint 3 – Südseite
├── Viewpoint 4 – Seitenansicht
└── Viewpoint 5 – erhöhte Übersicht
```

Der Zuschauer darf zwischen diesen Sichtpunkten wechseln.

Die Kamera kann jedoch nicht frei durch die Arena bewegt werden.

> **Zuschauer dürfen ausschließlich zwischen den von der Arena vorgegebenen Spectator Viewpoints wechseln.**

Die Anzahl und Position der Viewpoints wird durch die jeweilige Arena definiert.

Eine kleine Arena kann beispielsweise nur wenige Sichtpunkte besitzen, während eine große Turnierarena mehr Perspektiven anbieten kann.

---

## 12. Arena-Monitor

Außerhalb einer Arena können Monitore bzw. Zuschauerflächen vorgesehen werden, auf denen laufende Kämpfe verfolgt werden können.

Diese verwenden ebenfalls die Spectator-Daten der Arena-Instanz.

Ein Monitor kann beispielsweise immer einen festgelegten:

```text
main_viewpoint
```

anzeigen.

Damit können Spieler einen laufenden Kampf beobachten, obwohl sich die eigentlichen Kämpfer in einer privaten Arena-Instanz befinden.

---

## 13. Technisches Grundprinzip

Die Arena erweitert bzw. überschreibt während eines Matches bestimmte normale Weltregeln.

```text
Combat Event
     ↓
Ist Spieler Teilnehmer eines ACTIVE ArenaMatch?
     ↓
JA                         NEIN
 ↓                           ↓
Arena-Regeln             Welt-Regeln
 ↓                           ↓
DEFEATED                 normaler Death
```

Die allgemeine Combat-Engine bleibt dadurch verwendbar.

Arena-spezifische Regeln werden über den Match-Kontext angewendet.

---

## 14. Wichtige Architekturregel

> **Arena-Kämpfe laufen in einer eigenen serverseitigen Instanz mit eigenem Match-Kontext. Der aktive Match-Zustand liegt im RAM und überschreibt für die Teilnehmer die normale Death-Mechanik. Persistente Ergebnisse werden in MariaDB gespeichert.**

Zusätzlich gilt:

> **Spielmodi bestimmen serverseitig, welche Fähigkeiten und Aktionen Begleitern während eines Matches erlaubt sind. Der NPC selbst bleibt dabei eine persistente Figur der Welt.**

Und für Zuschauer:

> **Arena-Zuschauer verwenden einen Read-only-Spectator-Modus mit festen, wechselbaren Kamerapositionen. Zuschauer greifen niemals in das aktive Match ein.**
