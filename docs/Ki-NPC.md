# Aufgabe: Dynamisches NPC-, Informations- und Beziehungssystem für Andora

Analysiere zuerst die bestehende Serverarchitektur und vorhandenen NPC-, Spieler-, Raid-, Welt-, Event- und Datenbank-Komponenten.

Ändere noch nichts, bevor du verstanden hast, welche Systeme bereits vorhanden sind und wiederverwendet werden können.

## Ziel

Baue die Grundlage für ein dynamisches NPC-System, bei dem NPCs einmalige Personen in der gemeinsamen Spielwelt sind.

NPCs sollen:

* individuelle Beziehungen zu Spielern besitzen
* nur an einem Ort gleichzeitig existieren
* Informationen nur kennen, wenn sie diese tatsächlich erhalten haben
* Informationen untereinander weitergeben können
* auf Ereignisse reagieren können
* sichtbar durch die Welt reisen können
* unter bestimmten Bedingungen Spielern oder Gruppen helfen können
* nach einem Ereignis wieder zu ihrem normalen Aufenthaltsort zurückkehren

Die Runtime-KI (lokal aktuell: Ollama) soll später Persönlichkeit, Dialoge und begrenzte Entscheidungen ermöglichen.

Die Wahrheit über die Spielwelt bleibt jedoch immer beim Gameserver.

---

# Sehr wichtige Architekturregel

## Keine große NPC-Datei bauen

Dieses System MUSS modular aufgebaut werden.

Nicht alle Funktionen in:

* `npc.js`
* `npcManager.js`
* `gameServer.js`
* oder einer anderen einzelnen großen Datei

unterbringen.

Teile Verantwortlichkeiten in mehrere klar strukturierte Module auf.

Beispielsweise sinnvoll:

```text
server/
└── npc/
    ├── npcManager
    ├── npcState
    ├── npcMovement
    ├── npcRelationships
    ├── npcKnowledge
    ├── npcEvents
    ├── npcDecisionService
    ├── npcDialogueService
    └── npcRepository
```

Zusätzlich können je nach bestehender Architektur eigene Module entstehen für:

```text
events/
travel/
crime/
raids/
world/
```

Die konkreten Dateinamen dürfen an die bestehende Projektstruktur angepasst werden.

Entscheidend ist:

**Eine Datei = eine klar abgegrenzte Verantwortung.**

Wenn eine Datei beginnt, mehrere unabhängige Systeme zu enthalten oder unnötig groß zu werden, muss sie weiter aufgeteilt werden.

Keine künstliche Aufteilung in viele winzige Dateien, aber auch keine monolithischen Manager mit hunderten unterschiedlicher Verantwortlichkeiten.

---

# 1. NPCs sind einmalige Personen

Ein benannter NPC existiert serverweit nur einmal.

Beispiel:

```text
Borin
```

kann nicht gleichzeitig:

* in seiner Schmiede
* in Raid A
* in Raid B

existieren.

Jeder NPC benötigt deshalb einen eindeutigen Zustand.

Beispiel:

```text
IDLE
WORKING
TRAVELLING
RESERVED
AT_EVENT
AT_RAID
RETURNING
UNAVAILABLE
DEAD
```

Die Zustände sollen sinnvoll an die bestehende Architektur angepasst werden.

Der Server muss verhindern, dass zwei Systeme denselben NPC gleichzeitig reservieren.

Reservierungen müssen zuverlässig und möglichst atomar erfolgen.

---

# 2. Beziehungssystem

NPCs besitzen individuelle Beziehungen zu Spielern.

Beispiel:

```text
NPC: Borin
Player: 5821
Relationship: 78
```

Die Beziehung darf später durch verschiedene Ereignisse beeinflusst werden, beispielsweise:

* häufige Einkäufe
* Reparaturen
* erfüllte Aufträge
* Gespräche
* Hilfe für den NPC
* Angriffe
* Diebstahl
* Verrat
* Ereignisse in der Welt

Implementiere zunächst eine erweiterbare Grundlage.

Keine komplexe Balance erfinden, wenn sie noch nicht definiert wurde.

---

# 3. Gruppen profitieren unterschiedlich

Ein NPC kann wegen seiner Beziehung zu EINEM Spieler zu einem Raid oder Ereignis reisen.

Das bedeutet aber nicht, dass automatisch alle anderen Spieler dieselben Vorteile erhalten.

Beispiel Borin:

Ein Spieler besitzt eine sehr gute Beziehung zu Borin.

Dadurch entscheidet Borin möglicherweise, dem Raid zu helfen.

Im Raid:

* Borin ist für alle Spieler sichtbar.
* Spieler mit guter Beziehung erhalten möglicherweise bessere Dienste.
* Spieler mit neutraler Beziehung erhalten normale oder eingeschränkte Dienste.
* Spieler mit sehr schlechter Beziehung können abgelehnt werden.

Die genaue Balance soll später konfigurierbar sein.

NPC-Beziehungen sollen Möglichkeiten schaffen, aber nicht zwangsläufig zu einer Pflicht für jede Gilde werden.

---

# 4. NPC-Wissen

NPCs dürfen NICHT automatisch alles wissen, was auf dem Server passiert.

Jeder NPC besitzt eigenes Wissen.

Beispiel:

```text
Knowledge:
  subject
  information
  source
  timestamp
  confidence
```

Ein NPC darf nur auf Informationen reagieren, die:

* selbst erlebt wurden
* von einem anderen NPC erzählt wurden
* von einem Spieler erzählt wurden
* über ein anderes zulässiges Weltereignis erhalten wurden

Die Runtime-KI darf keine Fakten über die Welt erfinden.

Wenn Borin nicht weiß, dass ein Spieler in einem Raid steckt, darf sein KI-Dialog diese Information nicht kennen.

---

# 5. Nachrichten müssen physisch übertragen werden können

Beispielszenario:

Ein Spieler mit hoher Beziehung zu Borin befindet sich in einem schwierigen Raid.

Ein anderer NPC erfährt davon.

Dieser NPC kann als Bote zu Borin laufen.

Ablauf:

```text
Raid-Ereignis
↓
NPC erhält Information
↓
NPC entscheidet / erhält Aufgabe, Borin zu informieren
↓
NPC reist zu Borin
↓
NPC erreicht Borin
↓
Information wird übertragen
↓
Borin besitzt nun dieses Wissen
↓
Borin kann darauf reagieren
```

Die Information darf NICHT bereits bei Borin gespeichert werden, bevor der Bote ihn erreicht.

---

# 6. Nachrichten können verloren gehen

Das Informationssystem muss echte Konsequenzen erlauben.

Beispiel:

Ein Spieler tötet den Boten auf dem Weg zu Borin.

Dann:

```text
Bote stirbt
↓
Nachricht erreicht Borin nicht
↓
Borin weiß nichts vom Raid
↓
Borin bleibt in seiner Schmiede
```

Das System darf Borin nicht später automatisch das Wissen geben.

---

# 7. Zeugen und Straftaten

Wenn ein Spieler einen NPC angreift oder tötet, kann daraus ein separates Weltereignis entstehen.

Beispiel:

```text
Event:
type: murder
victim: messenger
perpetrator: player
location
timestamp
```

NPCs oder Spieler in Sichtweite können Zeugen sein.

NPC-Zeugen können Wissen über das Ereignis erhalten.

Beispiel:

```text
Witness:
NPC: merchant_12
knowledge:
  Täter
  Opfer
  Ort
  Zeitpunkt
confidence: 0.85
```

Eine Stadtwache darf einen Spieler nicht automatisch verfolgen, wenn niemand von der Tat weiß.

Mögliche Kette:

```text
Spieler tötet Boten
↓
Händler sieht Tat
↓
Händler erhält Wissen
↓
Händler meldet Tat einer Wache
↓
Wache erhält Wissen
↓
Stadt kann auf Täter reagieren
```

Wenn eine Stadtwache die Tat selbst sieht, kann sie direkt reagieren.

Das Kriminalitätssystem zunächst nur als erweiterbare Grundlage implementieren.

Keine vollständige Justizsimulation bauen, wenn sie nicht notwendig ist.

---

# 8. NPC-Reisen durch die Welt

NPCs sollen nicht einfach am Ziel erscheinen.

Wenn Borin zu einem Raid reist:

```text
Schmiede
↓
verlässt Stadt
↓
reist über Weltkarte
↓
erreicht Raid-Eingang
↓
tritt dem Raid-Ereignis bei
```

Andere Spieler sollen Borin unterwegs sehen können.

Sie sollen ihn theoretisch auch ansprechen können.

---

# 9. Effiziente Reise-Simulation

NPCs müssen nicht dauerhaft vollständig simuliert werden, wenn kein Spieler in ihrer Umgebung ist.

Beispiel:

```text
NPC: Borin
state: TRAVELLING
route: city_to_raid_04
startedAt
expectedArrival
progress
```

Wenn kein Spieler in der Umgebung ist:

* Reise abstrakt berechnen
* keine permanente Pathfinding-Simulation notwendig

Betritt ein Spieler das Gebiet:

* aktuelle Position anhand der Route und Reisezeit bestimmen
* NPC dort materialisieren
* sichtbare Bewegung übernehmen

Dadurch soll das System auch mit vielen NPCs skalierbar bleiben.

---

# 10. Übergang in einen Raid

Erreicht Borin den Raid-Eingang:

```text
World NPC
↓
Raid Transition
↓
Raid NPC
```

Es handelt sich weiterhin um denselben NPC.

Kein Klon erzeugen.

Nach dem Raid:

```text
Raid
↓
verlässt Raid
↓
RETURNING
↓
reist zurück
↓
Schmiede
↓
WORKING / IDLE
```

Während Borin weg ist, ist seine Schmiede entsprechend nicht durch ihn besetzt.

---

# 11. Andere NPCs reagieren auf seine Abwesenheit

Andere NPCs sollen seinen Zustand kennen können, wenn es logisch ist.

Beispiel:

Ein Spieler kommt zur Schmiede.

Borin ist unterwegs.

Ein Lehrling könnte über die Runtime-KI sagen:

> Borin ist vor einer Weile Richtung Nordstraße aufgebrochen.

Wichtig:

Der Lehrling darf dies nur sagen, wenn er entsprechende Informationen besitzt.

---

# 12. Runtime-KI-Aufgabe (lokal aktuell: Ollama; providerunabhängig, siehe `Coordinator.md` §3.1)

Die Runtime-KI ist NICHT die Datenbank und NICHT die Wahrheit der Welt.

Die Runtime-KI darf verwendet werden für:

* natürlich formulierte Dialoge
* Persönlichkeit
* Reaktionen auf bekannte Informationen
* Auswahl zwischen serverseitig erlaubten Aktionen
* begrenzte soziale Entscheidungen

Der Gameserver entscheidet:

* NPC-Position
* NPC-Zustand
* Wissen
* Reputation
* Inventar
* Reisezeit
* Verfügbarkeit
* Raid-Zugehörigkeit
* Tod
* Straftaten
* erlaubte Aktionen

Die Runtime-KI bekommt ausschließlich relevante strukturierte Informationen.

Beispiel:

```text
NPC: Borin
Personality: direct, loyal, gruff
CurrentState: TRAVELLING
Destination: Ancient Ruins
KnownFacts:
- Stammkunde Arin befindet sich dort in einem Raid
- Bote Mira hat diese Information überbracht
RelationshipWithArin: 82

Player asks:
"Wo willst du hin?"
```

Die Runtime-KI darf daraus natürliche Sprache erzeugen.

Es darf aber keine neuen Weltfakten erzeugen.

---

# 13. Events statt harter Kopplung

Die Systeme möglichst über Events bzw. klar definierte Schnittstellen verbinden.

Beispiel:

```text
RaidNeedsHelp
MessengerDispatched
MessageDelivered
NPCStartedTravel
NPCArrived
NPCEnteredRaid
NPCLeftRaid
NPCWitnessedCrime
CrimeReported
NPCDied
```

Nicht:

```text
raidManager ruft direkt 14 Funktionen aus npcManager auf
```

Ziel ist eine wartbare Architektur, in der neue NPC-Verhaltensweisen später ergänzt werden können.

---

# 14. Persistenz

Prüfe, welche Zustände gespeichert werden müssen, damit ein Serverneustart keine inkonsistente Welt erzeugt.

Beispielsweise:

* NPC-Position bzw. Aufenthaltsort
* NPC-State
* Beziehungen
* langfristiges Wissen
* aktive Reisen
* aktive Reservierungen
* wichtige Ereignisse

Temporäre Werte müssen nicht zwangsläufig dauerhaft gespeichert werden.

Nutze das bestehende Datenbanksystem, wenn möglich.

Keine zweite Datenbankarchitektur parallel aufbauen.

---

# 15. Performance

Das MMO soll später viele Spieler und NPCs unterstützen.

Deshalb:

* keine permanenten KI-Provider-Aufrufe für jeden NPC
* keine KI-Berechnung pro Server-Tick
* keine vollständige Pathfinding-Simulation für unbeobachtete NPCs
* Ereignisse bevorzugen
* KI nur dann aufrufen, wenn eine Entscheidung oder ein Dialog wirklich benötigt wird
* Ergebnisse sinnvoll zwischenspeichern, wenn möglich

Die grundlegende Weltlogik muss auch funktionieren, wenn der KI-Provider zeitweise nicht erreichbar ist.

---

# 16. Fehlerfälle

Berücksichtige mindestens:

* Bote stirbt
* Borin stirbt oder ist nicht verfügbar
* Raid endet, bevor Borin ankommt
* Borin ist bereits für ein anderes Ereignis reserviert
* Server startet während einer NPC-Reise neu
* der KI-Provider antwortet nicht
* Zielgebiet ist nicht verfügbar
* Spieler verlässt den Raid
* mehrere Spieler versuchen gleichzeitig denselben NPC auszulösen

In diesen Fällen darf kein NPC dupliziert oder in einem ungültigen Zustand zurückgelassen werden.

---

# Vorgehen

Arbeite in dieser Reihenfolge:

1. Bestehende Architektur vollständig analysieren.
2. Vorhandene NPC-, Event-, Raid-, Welt- und Persistenzsysteme identifizieren.
3. Prüfen, was davon wiederverwendet werden kann.
4. Einen modularen Architekturplan erstellen.
5. Geplante Dateien und deren Verantwortlichkeiten auflisten.
6. Prüfen, ob einzelne geplante Dateien zu viele Verantwortlichkeiten enthalten.
7. Erst danach implementieren.
8. Kleine, nachvollziehbare Schritte verwenden.
9. Nach jedem größeren Schritt prüfen, ob bestehende Funktionen weiterhin funktionieren.
10. Tests für zentrale Zustandsübergänge erstellen.
11. Keine unnötigen Systeme implementieren, die für die aktuelle Grundlage noch nicht benötigt werden.
12. Am Ende dokumentieren:

* neue Dateien
* geänderte Dateien
* Datenmodell
* Events
* Zustandsmaschine
* offene Punkte
* mögliche spätere Erweiterungen

## Besonders wichtig

**Keine große Datei bauen, die das komplette NPC-System enthält.**

Vor der Implementierung ausdrücklich zeigen:

```text
Datei/Modul → Verantwortung
```

Wenn du während der Implementierung bemerkst, dass ein Modul mehrere unabhängige Verantwortlichkeiten übernimmt, refaktoriere es in getrennte Module.

Die Architektur soll langfristig erweiterbar sein, weil später weitere NPC-Berufe, Beziehungen, Reisen, Ereignisse, Informationsketten und KI-gesteuerte Verhaltensweisen hinzukommen werden.
