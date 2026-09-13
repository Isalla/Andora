# Andora – Lua-Scripting-System

## Status

**Konzept / Architekturspezifikation.**

Dieses Dokument ist die autoritative Spezifikation für die spätere Implementierung der serverseitigen Lua-Content-Scripting-Schicht von Andora.

Es wird **kein Code** beschrieben, der bereits existiert. Es wird **keine technische Festlegung** getroffen, die erst in der Implementierungsphase entschieden werden muss (siehe Abschnitt 10).

Lua ist in Andora die **Content- und Orchestrierungsschicht**. Lua ersetzt **keines** der bestehenden Systeme und wird **nicht** zur Autorität über Spielregeln.

---

## 1. Grundprinzip

> **Was eine Spielregel ist, gehört in die autoritativen Systeme. Was konkreter Content und dessen Ablauf ist, kann über Lua gesteuert werden.**

Der Realm bleibt für alle verbindlichen Spielregeln die Autorität, unter anderem für:

* Combat
* Charakterzustand
* Progression / EXP / Level
* Items
* Inventory
* Loot
* Queststatus und Persistenz
* NPC-/World-State
* Berechtigungen und Validierung

Lua beschreibt und orchestriert Content und fordert Aktionen über eine **kontrollierte API** an. Lua berechnet Spielergebnisse nicht eigenmächtig und schreibt nicht direkt den autoritativen Zustand.

**Beispiel Questbelohnung:**

```text
Lua fordert eine Questbelohnung an (z. B. EXP, Items, Gold).
        ↓
Rust validiert die Anforderung.
        ↓
Das bestehende Progression-/Item-/Inventory-System entscheidet
und führt die Änderung aus.
        ↓
Zustandsänderung / Event
```

Lua berechnet also nicht eigenmächtig Charakter-EXP und schreibt nicht direkt den Charakterzustand. Die bestehende Rust-Progression prüft und führt aus.

**Lua erhält keinen direkten Datenbankzugriff.**

---

## 2. Warum Lua

* Content kann ohne Änderungen am Realm-Kern erstellt werden.
* Quests, NPC-Verhalten, Weltinteraktionen und Cutscene-Abläufe können flexibel geändert werden.
* Kleine Content-Korrekturen erfordern nicht, dass Gameplay-Kernlogik neu geschrieben wird.
* Lua verbindet bestehende Systeme, ersetzt sie aber nicht.
* Die Architektur soll langfristig auch KI-gestützte Content-Erstellung ermöglichen, ohne dass eine Content-KI beliebigen Rust-Realm-Code verändern muss.

**Beispiel Cutscene:**

Timing, Reihenfolge, NPC-Bewegung, Dialog oder Kameraführung können später im Lua-Content angepasst werden, während die technische Cutscene-Engine unverändert bleibt.

Die technische Cutscene-Engine bleibt bei den zuständigen Engine-/Realm-/Client-Systemen. Lua beschreibt den konkreten Ablauf. Dies ermöglicht insbesondere, Cutscenes später leicht anzupassen, wenn Timing, Bewegung, Dialog, Kamera oder Reihenfolge beim Testen nicht gefallen.

Verwandt: `cutscene_system.md` und `ai_cutscene_system.md` definieren den Szenen- und Scene-Lock-Rahmen; dieses Dokument referenziert sie und verändert **keine** dort definierten Regeln.

---

## 3. Script-Domänen

Die folgenden Script-Arten sind vorgesehen.

### 3.1 Quest Scripts

Quest-Scripts steuern konkrete Questabläufe:

* Questannahme / Ablehnung
* sequenzielle Schritte
* parallele Schritte
* gemischte Abläufe
* dynamisches Aktivieren weiterer Schritte
* Completion-Callbacks
* Reaktion auf Questereignisse
* Abschluss / Belohnungsanforderung

**Queststatus, Validierung und Persistenz bleiben Rust-Aufgabe** (siehe `Quest-System.md` und Abschnitt 6).

### 3.2 NPC Scripts

NPC-Scripts steuern:

* Spawn
* Respawn
* Spielerinteraktion
* Hail / Ansprechen
* Dialogabläufe
* Reaktionen auf Queststatus
* NPC-spezifisches Verhalten

NPC-Zustand, Position, Wissen und Weltzugehörigkeit bleiben autoritativ in Rust bzw. den dafür zuständigen Systemen (siehe `Ki-NPC.md`).

### 3.3 Item Scripts

Item-Scripts steuern:

* Examine / Untersuchen
* Benutzen
* kontextabhängige Interaktionen
* Questbezug
* Reaktionen auf Weltzustand oder Nähe zu Weltobjekten

**Item- und Inventory-Zustand bleiben autoritativ in Rust** (siehe `item_properties.md`, `inventory_system.md`).

### 3.4 Ability Scripts

Ability-Scripts beschreiben konkrete Fähigkeiten und Effekte.

**Wichtige Klassenstruktur:**

Gemeinsame Fähigkeiten einer Grundklasse liegen auf Ebene der Grundklasse.

```text
abilities/
  fighter/
    Taunt.lua
    Kick.lua
    warrior/
      ...
    paladin/
      ...
```

Alle Kämpfer-Unterklassen erhalten die gemeinsamen Kämpferfähigkeiten wie Taunt und Kick. Krieger und Paladin erhalten zusätzlich ihre jeweiligen spezialisierten Fähigkeiten.

Dasselbe Prinzip soll später für die anderen Klassenfamilien möglich sein (Magier, Priester, Kundschafter; siehe `Klassensystem.md`).

> **Wichtig:** Die Ordnerstruktur dient der Organisation und darf NICHT alleinige autoritative Quelle für Klassenberechtigungen sein. Die tatsächliche Freigabe einer Fähigkeit muss durch autoritative Ability-/Klassendaten bzw. den Realm validiert werden.

**Combat-Regeln selbst bleiben in Rust.** Lua darf kontrollierte Combat-Aktionen anfordern, aber das bestehende Combat-System nicht umgehen (siehe `Kampfsystem.md`, `Ability-System.md`).

### 3.5 Zone Scripts

Zone-Scripts dienen zonenweiten Content-Ereignissen und Reaktionen auf Welt-/Gebietsereignisse.

### 3.6 Region Scripts

Region-Scripts steuern räumlich begrenzte Bereiche:

* Enter
* Leave
* Tick
* Umwelteffekte

Beispiele:

* Lava
* Giftnebel
* Kälte
* heilende Bereiche
* besondere Dungeonflächen

**Trennung von Geometrie und Verhalten:**

```text
Daten definieren WO die Region liegt.
Lua definiert WAS als Content dort geschieht.
```

Autoritative Auswirkungen wie Schaden werden über Rust-Systeme ausgeführt (`Kampfsystem.md`).

### 3.7 Interaction Scripts

Interaction-Scripts steuern spezielle Weltaktionen, zum Beispiel:

* Objekt untersuchen
* Gegenstand aufnehmen
* Zelt anzünden
* Totem zerstören
* spezielle Weltobjekte benutzen
* kontextabhängige Aktionen

### 3.8 Cutscene Scripts

Cutscene-Scripts orchestrieren konkrete Cutscene-Abläufe:

* Cutscene starten
* Kameraabläufe anfordern
* NPC-Bewegungen
* Dialoge
* Animationen
* zeitliche Abläufe
* Spawn-/Encounter-Anforderungen
* Übergänge zurück zum Spieler
* Questfortschritt nach oder während einer Cutscene

Die technische Cutscene-Engine bleibt bei den zuständigen Engine-/Realm-/Client-Systemen (siehe `cutscene_system.md`, `ai_cutscene_system.md`). Lua beschreibt den konkreten Ablauf.

---

## 4. Eventmodell

Das Lua-System ist **ereignisbasiert** aufgebaut. Lua-Scripte reagieren auf kontrollierte Events.

Die folgenden Namen sind **ausschließlich konzeptionelle Beispiele** und keine endgültigen API-Namen:

```text
player_enter_area
player_leave_area
npc_spawn
npc_respawn
npc_hail
npc_killed
item_examined
item_used
quest_accepted
quest_declined
quest_step_completed
quest_completed
ability_cast
player_login
player_logout
region_enter
region_leave
region_tick
cutscene_started
cutscene_finished
```

> Die endgültige technische API wird erst in einer späteren Implementierungsphase festgelegt. Diese Liste beschreibt lediglich, an welche Art von Ereignissen Lua angebunden werden kann.

Die bestehenden Systemdokumente verwenden für dieselben konzeptionellen Ereignisse teils andere konventionelle Schreibweisen (z. B. `AreaEntered`, `QuestCompleted`, `QuestProgressChanged` in `Quest-System.md` und `cutscene_system.md`). Beide Schreibweisen sind konzeptionell und stellen keine endgültige API dar.

---

## 5. Quest-Integration und Ablaufmodelle

Lua orchestriert konkrete Questabläufe. Der Realm besitzt und persistiert den autoritativen Questzustand.

### Sequenziell

```text
Step 1 -> Step 2 -> Step 3 -> Abschluss
```

### Parallel

```text
          -> Ziel A ->
Start     -> Ziel B -> Abschluss
          -> Ziel C ->
```

### Gemischt

```text
             -> Ziel B ->
Step A       -> Ziel C -> Step E -> Abschluss
             -> Ziel D ->
```

Lua definiert bzw. orchestriert solche Abläufe, einschließlich des dynamischen Aktivierens weiterer Schritte.

**Quest V1 selbst wird separat spezifiziert und implementiert** (siehe `Quest-System.md`, `quests_stories.md`). Dieses Dokument hält die benötigte Flexibilität für die Lua-Seite fest, definiert jedoch keine Quest-V1-Mechanik.

---

## 6. Sicherheits- und Autoritätsgrenzen

Dieser Abschnitt ist zentral für das Gesamtsystem.

### 6.1 Lua darf NICHT

* direkt auf MariaDB zugreifen
* beliebiges SQL ausführen
* beliebige Dateien lesen/schreiben
* Betriebssystembefehle ausführen
* den Realm-Prozess kontrollieren
* Charakterwerte direkt außerhalb kontrollierter APIs verändern
* EXP-Regeln umgehen
* Inventory-Regeln umgehen
* Loot-Regeln umgehen
* Combat-Regeln umgehen
* Klassenberechtigungen umgehen
* beliebigen Netzwerkverkehr öffnen
* Rust-Speicher direkt manipulieren

Lua bekommt ausschließlich **explizit freigegebene Realm-APIs**.

### 6.2 Prinzip

```text
Lua fordert Aktion an
        ↓
Rust validiert
        ↓
bestehendes autoritatives System
        ↓
Zustandsänderung / Event
```

### 6.3 Beispiel Quest-Step

Lua fordert „Quest Step abschließen" an.

Rust prüft mindestens:

* Quest existiert
* Charakter besitzt Quest
* Quest ist aktiv
* Step existiert
* Step darf in diesem Zustand abgeschlossen werden
* Fortschritt wird autoritativ persistiert
* notwendige Folgeevents werden erzeugt

---

## 7. Fehlerisolation (Architekturziel)

**Ein fehlerhaftes Lua-Script darf den Realm nicht zum Absturz bringen.**

Später vorzusehen:

* Scriptfehler werden geloggt
* kontrollierter Abbruch des betroffenen Script-Aufrufs
* keine unkontrollierte Weitergabe von Lua-Fehlern in den Realm
* Schutz vor Endlosschleifen / übermäßig langer Ausführung
* begrenzte Ressourcen pro Script-Aufruf
* nachvollziehbare Script-/Datei-/Event-Angabe in Fehlermeldungen

> Die konkreten technischen Limits (z. B. Instruction Limits, Memory Limits) werden erst bei der Implementierung festgelegt und dürfen in diesem Dokument nicht erfunden werden.

---

## 8. Script-Organisation

Konzeptionelle Zielstruktur (noch keine unveränderliche technische API):

```text
scripts/
  quests/
  npcs/
  items/
  abilities/
    fighter/
      warrior/
      paladin/
    mage/
    priest/
    scout/
  zones/
  regions/
  interactions/
  cutscenes/
  shared/
```

Die Struktur beschreibt die geplante Content-Organisation. Gemeinsam nutzbare Lua-Helfer können unter `shared/` liegen, sofern dies später sicher umgesetzt wird.

---

## 9. Content-Änderungen und Reload (Ziel)

Ziel ist, dass Lua-Content leichter austauschbar und testbar ist als fest kompilierte Realm-Kernlogik.

> Dieses Dokument behauptet **nicht**, dass echtes Live-Hot-Reload bereits technisch beschlossen oder implementiert ist.

Festgehalten als Ziel:

* möglichst geringe Hürde für Content-Änderungen
* Lua-Scripte sollen unabhängig von Änderungen am Rust-Quellcode bearbeitet werden können
* Reload-/Cache-/Versionsstrategie wird in der späteren technischen Spezifikation entschieden
* laufende Quest-/Cutscene-Zustände müssen bei einem späteren Reload-Konzept berücksichtigt werden

---

## 10. Abgrenzung: Jetzt festgelegt / Erst Implementierung

Dieses Dokument unterscheidet ausdrücklich zwischen Architekturziel und späterer Implementierung.

### 10.1 Jetzt festgelegt

* Lua ist serverseitige Content-Scripting-Schicht
* Rust bleibt autoritativ
* Script-Domänen (Abschnitt 3)
* Sicherheitsprinzip (Abschnitt 6)
* Eventprinzip (Abschnitt 4)
* Klassen-/Ability-Hierarchie (Abschnitt 3.4)
* Cutscene-Orchestrierung (Abschnitt 3.8)
* Quest-Integration (Abschnitte 3.1, 5)
* Regions-/World-Integration (Abschnitte 3.5, 3.6)
* Fehlerisolation als Architekturziel (Abschnitt 7)
* Script-Organisation als konzeptionelle Zielstruktur (Abschnitt 8)

### 10.2 Noch NICHT festlegen bzw. erfinden

Folgende Entscheidungen erfolgen in der späteren Implementierungsphase und werden in diesem Dokument bewusst **nicht** getroffen:

* konkrete Lua-Runtime / Library für Rust
* endgültige API-Funktionsnamen
* Instruction Limits
* Memory Limits
* Threading-Modell
* Hot-Reload-Implementierung
* genaue Cache-Strategie
* konkrete Performance-Limits
* endgültiges Dateiformat für Script-Metadaten
* konkrete Netzwerkprotokolländerungen

> Hinweis: Im Repository liegt unter `deps/lua/` eine referenz-vendordierte Lua-5.5.1-Quelle (unverändert, **nicht** eingebaut; siehe `deps/README.md` und `docs/README.md`). Das ist eine reine Referenzquelle und **keine** Festlegung auf eine konkrete Rust-Anbindungsbibliothek oder Integrationsart. Die endgültige Runtime-Entscheidung bleibt der Implementierungsphase vorbehalten.

---

## 11. Beziehung zu bestehenden Systemen

| System | Dokument | Bezug zum Lua-Scripting-System |
|---|---|---|
| Quest-System | `Quest-System.md`, `quests_stories.md` | Lua beschreibt Questdefinitionen und orchestriert Abläufe; Realm prüft und persistiert Questzustand |
| Cutscene / Scene | `cutscene_system.md`, `ai_cutscene_system.md` | Lua orchestriert konkrete Cutscene-Abläufe; Scene Engine / Realm bleiben technisch zuständig |
| Kampf | `Kampfsystem.md` | Content-/Lua-definierter Gegner-Grundzustand; Combat-Regeln bleiben in Rust |
| Ability | `Ability-System.md` | Content-/Lua-definierte Fähigkeiten, Qualitätswerte, Meisterschaftsvarianten; Realm führt aus |
| Klassen | `Klassensystem.md` | Fähigkeitshierarchie je Grund-/Unterklasse; Klassenberechtigungen werden durch autoritative Daten validiert |
| Inventory / Items | `inventory_system.md`, `item_properties.md` | Lua steuert Item-/Interaction-Content; Item- und Inventory-Zustand bleibt autoritativ in Rust |
| NPC | `Ki-NPC.md` | NPC-Scripts steuern Content-Verhalten; NPC-Zustand, Wissen, Beziehungen bleiben autoritativ |
| Architektur | `architecture.md`, `project_overview.md` | Lua als Content-/Scripting-Schicht innerhalb der serverautoritativen Architektur |
| Status | `Projekt-Status.md` | Lua-Scripting ist Konzept/Architekturziel; es existiert kein implementierter Lua-Content-Layer |