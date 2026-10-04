# Andora – Kampfsystem

## 1. Grundprinzip

Andora verwendet ein klassisches MMORPG-Kampfsystem.

Das Kampfsystem soll bewusst übersichtlich und technisch schlank bleiben. Komplexität entsteht später durch Klassen, Fähigkeiten, Ausrüstung, Gegner und das Zusammenspiel der Spieler und nicht durch ein aufwendiges Action-Kampfsystem.

Für die erste Implementierung gilt der Grundsatz in Abschnitt 17: bewusst einfach, spiel- und testbar, ohne unnötige Übernahme komplexer Rating-, Expertise- oder Sondermechaniken anderer MMORPGs. Trefferchancen, Skillprogression, Rüstungsformeln, Caps und andere Zahlenwerte sollen anschließend anhand realer Praxistests angepasst werden können.

---

## 2. Anvisieren

Andora verwendet klassisches Targeting.

Jeder Spieler kann jeden für ihn gültigen Gegner frei anvisieren und angreifen.

Die freie Zielwahl wird nicht durch die Rolle innerhalb einer Gruppe eingeschränkt. Ein DD kann beispielsweise jederzeit einen anderen Gegner als der Tank angreifen.

Die daraus entstehenden Konsequenzen gehören zum Gruppenspiel. Zieht ein Spieler durch die Wahl eines anderen Gegners zusätzliche Aggro, muss die Gruppe damit umgehen.

### Ziel eines Gruppenmitglieds

Spieler können auch ein anderes Gruppenmitglied anvisieren.

Wird anschließend eine offensive Aktion ausgeführt, wird automatisch das aktuelle Gegnerziel des anvisierten Gruppenmitglieds verwendet.

Dadurch können Spieler beispielsweise den Tank anvisieren und automatisch dessen Ziel angreifen.

Wechselt der Tank sein Ziel, folgen die Spieler, die über ihn angreifen, seinem neuen Ziel.

Das System dient ausschließlich als Hilfestellung. Jeder Spieler kann weiterhin jederzeit selbst einen Gegner auswählen.

Dadurch können innerhalb einer Gruppe auch mehrere Ziele gleichzeitig bekämpft werden.

---

## 3. Grundangriff

Der Grundangriff wird vom Spieler manuell gestartet und kann auch wieder manuell beendet werden.

Nach dem Start läuft der Grundangriff automatisch weiter, solange ein gültiges Ziel vorhanden ist und die Voraussetzungen für einen Angriff erfüllt sind.

Jede Waffe besitzt eine eigene **Duration**.

Die Duration bestimmt, wie viel Zeit zwischen zwei automatischen Grundangriffen vergeht.

Dadurch können verschiedene Waffen unterschiedliche Angriffsgeschwindigkeiten besitzen, ohne dass dafür unterschiedliche Grundkampfsysteme benötigt werden.

---

## 4. Kampfskills und Waffenbeherrschung

Charaktere besitzen trainierbare Kampfskills, insbesondere für die verwendbaren Waffenarten und Schilde.

Ein grundsätzlich verfügbarer Skill startet mit dem Wert **1**.

Die Klasse bestimmt, welche Waffen-/Kampfskills **Hauptskills**, **Nebenskills** oder **nicht verwendbar** sind.

* **Hauptskills** können das vollständige aktuelle Skillmaximum erreichen.
* **Nebenskills** können vorläufig maximal **40 % des normalen Skillmaximums** erreichen.

Dadurch kann beispielsweise ein Krieger einen Bogen als Nebenwaffe zum Pullen benutzen, aber niemals dieselbe Beherrschung erreichen wie eine Klasse, für die der Bogen eine Hauptwaffe ist.

### Skillmaximum und Level

Das Charakterlevel erhöht ausschließlich das mögliche Skillmaximum. Der tatsächliche Skillwert steigt niemals automatisch durch einen Levelaufstieg.

Der Skill steigt nur durch tatsächliche, relevante Benutzung.

Je näher ein Skill seinem aktuellen Maximum kommt, desto schwieriger wird der nächste Skillanstieg. Wird durch einen Levelaufstieg neues Skillpotential freigeschaltet, beginnt für diesen neu verfügbaren Bereich wieder eine zunächst leichtere Progression, die zum neuen Maximum hin schwieriger wird.

### Vorläufige Balancingwerte

* Level 1: Skillmaximum 30
* normales Levelup: +5
* jedes 10. Level: +10 statt +5
* Nebenskillmaximum: 40 % des Hauptmaximums

Diese Zahlen sind ausdrücklich **konfigurierbare Balancingwerte** und keine unveränderlichen Architekturwerte.

### Abgrenzung zu Fähigkeiten und Sammelskills

Kampfskills sind nicht identisch mit den aktiven Fähigkeiten (siehe Abschnitt 8) und nicht mit den Sammel-/Handwerkskills (siehe `Sammelsystem.md` bzw. `Handwerks_und_Sammelsystem.md`). Kampfskills betreffen ausschließlich die Beherrschung von Waffenarten und Schilden.

---

## 5. Physische Trefferauflösung

Für die erste Version bleibt die physische Trefferauflösung bewusst einfach.

Grundsätzlich mögliche Ergebnisse:

* Verfehlen
* Ausweichen
* Parieren
* Blocken
* normaler Treffer
* kritischer Treffer

Der Waffenskill ist ein wesentlicher Bestandteil der Trefferwahrscheinlichkeit.

**Blocken** reduziert grundsätzlich Schaden und stellt nicht einfach dasselbe Ergebnis wie vollständiges Ausweichen oder Parieren dar.

Konkrete Wahrscheinlichkeiten und Formeln werden nicht endgültig festgelegt und sollen später anhand von Praxistests gebalanced werden.

---

## 6. Waffenschaden und Angriffsgeschwindigkeit

Der Grundschaden wird durch die verwendete Waffe vorgegeben.

Jede Waffe besitzt außerdem eine **Duration**, welche die Zeit zwischen automatischen Grundangriffen bestimmt (siehe Abschnitt 3).

Langsame Waffen wie Zweihandschwerter können höheren Grundschaden besitzen.

Schnelle Waffen wie Dolche verursachen geringeren Schaden pro Treffer, greifen dafür häufiger an.

Konkrete Schadenswerte und Durationswerte sind Balancingdaten.

---

## 7. Rüstung und physische Schadensreduktion

Ausrüstung liefert Rüstungswerte.

Aus dem gesamten relevanten Rüstungswert wird eine prozentuale physische Schadensreduktion berechnet.

Die genaue Umrechnungsformel wird noch nicht festgelegt und soll später durch Praxistests bestimmt und angepasst werden können.

Klassen besitzen unterschiedliche maximale physische Schadensreduktionen.

Vorläufige Beispiele:

* Tank: maximal 50 %
* Magier: maximal 20 %

Auch diese Werte sind Balancingwerte und später anpassbar.

Der Rüstungswert selbst darf über den für die Klasse notwendigen Wert hinausgehen; begrenzt wird die daraus resultierende effektive Schadensreduktion.

---

## 8. Fähigkeiten

Fähigkeiten werden vom Spieler aktiv über seine Aktionsleiste ausgelöst.

Jede Fähigkeit besitzt ihren eigenen Cooldown. Andora besitzt keinen globalen Cooldown. NPC- und Monsterfähigkeiten verwenden ebenfalls eigene Cooldowns; die normale Angriffs-Duration (Waffenduration für den automatischen Grundangriff) ist davon getrennt.

Die drei Ausführungsarten (Sofortfähigkeit, Fähigkeit mit Castzeit, Kanalisierung), Mana/Cooldown-Kombinationen, Cast-Unterbrechung, AoE-Grundarten, freundliche Ziele und Heilung, Buffs/Debuffs/CC, DoT/HoT und Waffen-Effekte, Fähigkeitsqualität und Meisterschaft sowie die Lua-/Realm-Trennung sind im **Ability-System** definiert (siehe `Ability-System.md`).

Spieler und NPCs verwenden grundsätzlich dasselbe Ability-Grundsystem; Content/Lua definiert die jeweiligen Werte und Regeln, der Realm führt sie autoritativ aus (siehe auch Abschnitte 18–21).

---

## 9. Bewegung im Kampf

### Nahkampf

Spieler können sich während des normalen Nahkampfes bewegen.

Solange sich das anvisierte Ziel in der erforderlichen Reichweite befindet, läuft der automatische Grundangriff weiter.

### Fernkampf

Auch normale Fernkampfangriffe können während der Bewegung ausgeführt werden.

Solange das Ziel innerhalb der erforderlichen Reichweite bleibt, wird der automatische Angriff fortgesetzt.

### Zauber

Zauber unterscheiden sich davon bewusst.

Beginnt ein Spieler einen Zauber und bewegt sich währenddessen, wird der laufende Zauber unterbrochen.

Weitere Eigenschaften eines Zaubers werden über die jeweilige Fähigkeit definiert.

---

## 10. Ressourcen

Alle Spielercharaktere verwenden grundsätzlich nur zwei zentrale Ressourcen:

**HP – Lebenspunkte**

HP bestimmen, wie viel Schaden ein Charakter überleben kann.

**Mana**

Mana wird für Fähigkeiten verwendet, die einen Manaverbrauch besitzen.

Auf zusätzliche klassenspezifische Grundressourcen wie Wut, Energie oder Fokus wird verzichtet.

Die **Regeneration** dieser beiden Ressourcen folgt einem gemeinsamen technischen Grundprinzip: Effektive Regeneration = (Basisregeneration + additive Regenerationsboni) × Zustandsmultiplikator. Es gelten dieselben Zustandsmultiplikatoren (im Kampf immer 15 %, außerhalb des Kampfes stehend 100 %, sitzend 125 %), die Klasse bestimmt die Basisregeneration, und die Grundattribute beeinflussen die maximalen Pools über Konstitution (Max-HP) und Weisheit (Max-Mana). Die vollständigen Regeln und ersten Balancingwerte stehen in `Attribute_und_Regeneration.md`.

---

## 11. Kampftempo

Andora verwendet ein klassisches MMORPG-Kampftempo.

Der Kampf soll nicht auf permanentes Ausweichen, Animation-Canceling oder extrem schnelle Eingaben ausgelegt sein.

Positionierung, Zielwahl, Fähigkeiten, Ausrüstung und das Zusammenspiel der Gruppe stehen stärker im Mittelpunkt.

Das vergleichsweise ruhige Kampfsystem unterstützt gleichzeitig das Ziel, den Client auch auf leistungsschwacher Hardware wie dem Raspberry Pi betreiben zu können.

Die visuelle Darstellung von Fähigkeiten, Zaubern und Kampfeffekten wird hier nicht vorgegeben, sondern folgt dem Darstellungsprinzip in `Clientdarstellung_und_Performance.md`: hybrider Ansatz (2D-Animationen, vorgerenderte 3D-Effekte als 2D-Animation, leichte echte 3D-Effekte im Performance-Budget); für geeignete Effekte kann der Client unterschiedliche Darstellungen derselben Fähigkeit unterstützen – die bevorzugte 2D-/3D-Darstellung wählt der Spieler in den Client-Grafikoptionen. Diese Auswahl ist rein clientseitig und hat keinerlei Auswirkung auf Schaden, Heilung, Reichweite, Wirkungsradius, Hitbox, Trefferberechnung, Dauer, Cooldown, Ressourcenverbrauch, Zielauswahl, Anzahl getroffener Ziele oder serverseitige Kampfregeln. Der Realm bleibt für die tatsächliche Spielmechanik autoritativ; Spieler mit 2D- und 3D-Darstellung erleben spielmechanisch exakt dasselbe Kampfgeschehen.

---

## 12. Aggro und Gruppenrollen

Gegner verwenden ein klassisches Aggro- bzw. Bedrohungssystem.

Tanks sollen Gegner an sich binden und deren Aufmerksamkeit kontrollieren können.

DDs konzentrieren sich auf das Verursachen von Schaden.

Heiler unterstützen die Gruppe und halten Gruppenmitglieder am Leben.

Die konkreten Auswirkungen von Fähigkeiten auf Aggro und Bedrohung werden bei den jeweiligen Fähigkeiten definiert.

Das Kampfsystem verhindert nicht, dass andere Spieler Aggro bekommen.

Greift beispielsweise ein DD einen anderen Gegner an und zieht dadurch dessen Aufmerksamkeit auf sich, ist dies eine normale Konsequenz seiner Zielwahl.

Der Tank ist nicht automatisch dafür verantwortlich, sämtliche Gegner zu kontrollieren, die andere Gruppenmitglieder eigenständig in den Kampf bringen.

---

## 13. Tod und Wiederbelebung

Sinken die HP eines Spielers auf null, stirbt der Charakter.

Dem Spieler erscheint das **weiße Licht**.

Anschließend hat er grundsätzlich zwei Möglichkeiten:

**Auf Wiederbelebung warten**

Der Spieler bleibt tot am Ort seines Todes und kann darauf warten, von einem Heiler wiederbelebt zu werden.

Er muss sich nicht sofort entscheiden.

Stirbt der Heiler ebenfalls oder möchte der Spieler nicht länger warten, kann er weiterhin den Respawn wählen.

**Respawn**

Der Spieler kann sich für einen Respawn entscheiden.

Der Charakter wird anschließend zum nächstgelegenen Respawnpunkt versetzt.

---

## 14. Respawnpunkte

Jedes größere Gebiet besitzt mehrere Respawnpunkte.

Beim Respawn wird der Spieler zum nächstgelegenen geeigneten Respawnpunkt gebracht.

Dadurch sollen unnötig lange Laufwege nach einem Tod vermieden werden.

Respawnpunkte werden entsprechend über die Gebiete verteilt und sind Teil der jeweiligen Gebietsgestaltung.

---

## 15. Tod und Charakter-EXP

Sterben soll eine Konsequenz besitzen, ohne die Charakterprogression übermäßig zu bestrafen.

Der Tod verursacht keinerlei Malus auf Charakter-EXP:

- Bereits erworbene Charakter-EXP werden durch den Tod nicht reduziert.
- Es gibt keine EXP-Schuld.
- Es gehen keine Level oder Teile des aktuellen Level-Fortschritts verloren.
- Nach einem Tod wird der zukünftige Charakter-EXP-Verdienst nicht reduziert.
- Es gibt insbesondere keinen temporären EXP-Verdienstmalus nach einem Tod.

Die verbindlichen Regeln zu Tod und Charakter-EXP stehen in `Erfahrung_und_Progressionssystem.md` (Abschnitt 11).

Tod, Wiederbelebung und Todesregeln in diesem Abschnitt beschreiben das reguläre (`normal`-)Ruleset. Andere Rulesets (z. B. ein späterer Hardcore-Realm) können abweichende Todesregeln definieren; diese sind noch nicht festgelegt (siehe Realm-Rulesets in `Login_Realm_Architektur.md`). Für das Encounter-Design gilt dabei das dort festgelegte Hardcore-Fairnessprinzip: gleiche lesbare Grundregeln auf allen Rulesets, Schwierigkeit über Konsequenz.

---

## 16. Fraktionskämpfe innerhalb einer Gruppe

Für Kämpfe zwischen verfeindeten Fraktionen gelten die bereits definierten Gruppenregeln.

Befinden sich Spieler verschiedener Fraktionen gemeinsam in einer Gruppe und ein Gruppenmitglied beginnt einen Fraktionskampf, werden Gruppenmitglieder, die an diesem Konflikt nicht beteiligt sein dürfen, für die Dauer dieses Kampfes ausgegraut.

Der kämpfende Spieler wird dadurch für diese Gruppenmitglieder temporär von gruppenbasierten Unterstützungsmechaniken getrennt.

Gruppenheilungen oder vergleichbare Gruppeneffekte können dadurch nicht verwendet werden, um indirekt in einen Fraktionskampf einzugreifen.

Ein einem betroffenen Spieler zugeteilter Söldner wird innerhalb der Gruppe entsprechend behandelt.

Nach Ende des Fraktionskampfes wird die normale Gruppeninteraktion wiederhergestellt.

---

## 17. Grundsatz für die erste Implementierung

Das Kampfsystem soll zunächst bewusst einfach implementiert werden.

Es findet keine unnötige Übernahme komplexer Rating-, Expertise- oder Sondermechaniken anderer MMORPGs statt.

Die erste Version soll spielbar und testbar sein. Trefferchancen, Skillprogression, Rüstungsformeln, Caps und andere Zahlenwerte werden anschließend anhand realer Praxistests angepasst (siehe auch Abschnitte 4 bis 7).

> **Stand: Combat V1 ist implementiert** (Rust-Realm `src/realm-rs/src/combat.rs`):
> manuell gestarteter/beendeter Auto-Grundangriff, Ziel-/Reichweitenvalidierung,
> Waffen-Duration, Trefferauflösung (Miss/Dodge/Parry/Block/Normal/Krit),
> Waffenschaden, Rüstungsreduktion mit Klassen-Caps, Tod/KILL.
> Die konkreten Zahlenwerte (Trefferchancen, Schadens-, Duration-, Rüstungs-
> und Skill-Balancing) sind **vorläufig** und liegen als `COMBAT_*`-Werte in
> `config.env` (siehe `Projekt-Status.md`). Sie werden anhand realer
> Praxistests angepasst (§§4–7). Die frühere Anweisung, noch keine
> Implementierung vorzunehmen, ist damit für Combat V1 überholt.

---

## 18. Gegnergrundregeln (NPCs und Monster)

NPCs und Monster besitzen einen Content-/Lua-definierten Grundzustand.

Normale Regeln und Eigenschaften eines NPCs/Monsters werden in Content/Lua (beziehungsweise in den zugehörigen Spawn-/DB-Daten) definiert und nicht als feste Gameplaywerte in den Rust-Combat-Kern hartverdrahtet. Der Realm wertet diese Regeln aus und bleibt die endgültige Autorität.

Dabei sind `attackable` (angreifbar) und `aggressive` (aggressiv) getrennte Eigenschaften:

* **attackable** bestimmt, ob ein Spieler den Gegner überhaupt angreifen kann.
* **aggressive** bestimmt, ob der Gegner von sich aus einen Kampf beginnen kann.

Aggressive Gegner können selbstständig Kämpfe beginnen.

Friedliche Stadt-NPCs können standardmäßig weder angreifbar noch aggressiv sein; der konkrete Zustand ist Contententscheidung.

---

## 19. Kontextabhängige Überschreibungen

Quest-, Dialog-, Spieler- oder Gruppenkontext kann den Standard eines NPCs/Monsters gezielt überschreiben, beispielsweise Angriffserlaubnis, Aggressivität oder Verhalten in einer bestimmten Phase.

Solche Überschreibungen sind an ihren Kontext gebunden und laufen mit dessen Ende (zum Beispiel Phasenende) automatisch aus. Danach gilt wieder der Content-/Lua-definierte Grundzustand.

---

## 20. Home-Zone, Verfolgung und Evade/Return

Gegner besitzen eine Home-Zone statt zwingend starrer Spawnpunkte. Die Verfolgung von Spielern ist nur innerhalb definierter Grenzen möglich.

Wird ein Gegner zu weit von seiner Home-Zone entfernt, zu lange ohne gültigen Kampfbezug verfolgt oder anderweitig aus seinem definierten Bereich gezogen, geht er in **Evade/Return** über.

Evade/Return ist ein Combat-Reset, weder Tod noch Respawn:

* die Entity ist während Evade/Return **nicht angreifbar**,
* sie besitzt kein Aggro und ist kein gültiges Kampfziel,
* sie führt keine Angriffe aus,
* sie kehrt in ihre Home-Zone zurück,
* bei Rückkehr erfolgen vollständige HP-/Mana-Regeneration und vollständiger Cooldown-Reset,
* Evade/Return ist nicht durch erneutes Angreifen unterbrechbar.

Für Bosse gilt dieselbe einheitliche Regel: Auch ein Boss ist während seiner Rückkehr (Boss-Reset) nicht angreifbar; Details stehen in `Boss-System.md`.

---

## 21. Respawn, Aggroformen, Schwierigkeit und Architekturgrenze

### Respawn

Respawnzeiten sind Contentwerte und werden nicht als feste Werte in den Rust-Combat-Kern gelegt. Der tatsächlich gültige Wert kann in Lua beziehungsweise in speziellen Spawn-/DB-Daten definiert oder überschrieben werden.

Standardwerte, sofern Content nichts anderes festlegt:

* Questmonster: **3 Minuten**
* normale Monster: **5 Minuten**
* Named: **10 Minuten**
* besondere seltene Named/Bosse: individuelle Werte, zum Beispiel **24 Stunden oder länger**

Der Respawn-Timer beginnt erst beim tatsächlichen Tod der Entity. Langfristige Respawnzustände müssen Realm-Neustarts überstehen.

### Aggroformen

Es gibt drei Aggroformen, die auch kombiniert auftreten können:

* **Solo-Aggro:** der Gegner reagiert einzeln.
* **soziale Aggro:** umstehende Gegner derselben Gruppe oder Fraktion steigen in den Kampf ein.
* **feste Gruppe/Rudel:** der Verband agiert als feste Einheit.

Die Anwendung dieser Formen im Encounter- und Pull-Design (Geometrie, Positionierung, Patrouillen, dynamische Gefahr) ist in `exp1_Unterwelt.md` (Abschnitte 51–55) verbindlich festgelegt; dort stehen auch die Designziele für Beobachtung, Kommunikation und Spielerwissen.

### Schwierigkeit

Die Schwierigkeit ist contentabhängig. Es gibt keine separate Combat-Engine pro Contenttyp; unterschiedliche Gegner, Gebiete, Dungeons und Bosse nutzen dasselbe Grundkampfsystem mit unterschiedlichen Contentwerten.

### Architekturgrenze

* **Lua/Content** definiert Regeln und Standardwerte.
* **DB/persistenter Weltzustand** hält konkrete persistente Spawn- und Zustandsinformationen.
* **Kontext** (Quest/Dialog/Spieler/Gruppe) überschreibt gezielt und zeitlich begrenzt.
* **Realm** wertet aus und hat die endgültige Autorität.

> **Stand:** Die Abschnitte 18–21 sind als Combat V2 **umgesetzt** (Realm-Binär); ein gemeinsamer, realm-autoritativer Kampfkern für Spieler UND NPC/Monster, Content-/DB-Schicht (Migration 009) mit Probe-Spawnzone 0 für den vertikalen Schnitt. Siehe auch `Projekt-Status.md` und zum Boss-System `Boss-System.md`.

## 22. Fähigkeiten/Abilities als Combat V3

Das **Ability-System** (Konzept: `Ability-System.md` §§1–16) ist als Combat V3 **umgesetzt** und folgt derselben Architekturgrenze: Der Rust-Kern ist rein mechanisch und contentunabhängig, die Fähigkeiten selbst sind Daten (DB-Tabelle `ability_definitions`, Migration `010_combat_v3.sql`).

Umsetzungsstand in Kürze:

* **Ausführungsarten:** `instant` und `cast` (mit `cast_time_ms`); `channel` verhält sich vorläufig wie `cast`.
* **Ressourcen/Cooldown:** Mana wird beim Cast-Start abgezogen, Cooldown startet erst nach erfolgreicher Ausführung, keine Manarückgabe bei Unterbrechung, kein globaler Cooldown. Spieler-Ability-Cooldowns werden als absolute Ablaufzeitpunkte persistiert und bleiben über Logout/Reconnect erhalten (`P-18`, umgesetzt; `docs/Player_Persistenz.md` Abschnitt 23, `Ability-System.md` Abschnitt 2). Das persistente Set beim Tod wird aus der tatsächlichen Ability-Registry ausgewertet; das zuvor offene `on_death`-TODO ist damit erledigt. NPC-Cooldowns bleiben RAM-Zustand mit vollständigem Reset bei Evade/Return.
* **Cast-Unterbrechung:** durch Bewegung, Stun, Silence, Tod, Reichweiten- oder Sichtlinien-Verlust; Stun/Silence/Root blockieren Cast-Start bzw. Bewegung über das Effektmodell.
* **Effekte:** Buffs/Debuffs/Stun/Silence/Root/Slow mit Gruppenlogik (gleiche Gruppe ersetzt, unterschiedliche parallel), DoT/HoT mit Tick-Intervall (`tick_ms`, `next_tick_at`), Entfernung aller Effekte beim Tod; Waffen-Effekte folgen der Quelle-Sichtweise.
* **AoE:** `single`, `target_radius`, `caster_radius`, `ground`; Zielauswahl über die Welt, kein künstliches Ziellimit.
* **Events:** Realm-autoritative S2C-Frames für Cast- und Effektzustände (ABLITY/EFFECT), Instants senden ihre Wirkung sofort.
* **Programmstruktur:** modulares Kampf-Modul unter `src/realm-rs/src/combat/` (`ability.rs`, `effects.rs`, `cooldowns.rs`, `aoe.rs`, `events.rs`, `targeting.rs`), wiederverwendbar für Spieler, NPCs, Named und Bosse (Details: `Kampfsystem_V3_Wiederverwendung.md`).

**Nicht Bestandteil von Combat V3** (bleiben offen): Claim/Ownership-Ability-Logik, Gruppensystem (V1-Design siehe `Gruppensystem.md`, Code-Implementierung im Realm-Server steht aus), Cleanse/Dispel, vollständige Channel-Regeln, Passive Fähigkeiten, Meisterschaft, Klassenfähigkeiten, endgültiges Balancing.

> **Stand:** Combat V3 **umgesetzt** (Realm-Binär, 82 Unit-Tests grün); Probe-Fähigkeiten als Seed in Migration 010 (`fire_bolt`, `healing_light`, `soul_rend`, `frost_nova`, `choke`, `battle_shout`). NPC-Ausführung ihrer Lernfähigkeiten ist noch nicht verdrahtet und folgt mit den Boss-Fähigkeiten.

---

# Abgrenzung zu anderen Systemen

Das allgemeine Kampfsystem definiert die grundlegenden Regeln eines Kampfes.

Dazu gehören neben den bereits festgelegten Grundelementen (Anvisieren, Grundangriff/Duration, Fähigkeiten, Bewegung, Ressourcen, Tempo, Aggro, Tod/Respawn, Fraktionskämpfe) seit den heutigen Festlegungen auch:

* Kampfskills und Waffenbeherrschung (Haupt-/Nebenskills, Skillmaximum, Abschnitt 4)
* die grundsätzliche physische Trefferauflösung (Abschnitt 5)
* die Grundsätze für Waffenschaden und Angriffsgeschwindigkeit (Abschnitt 6)
* Rüstung und physische Schadensreduktion mit Klassen-Caps (Abschnitt 7)

Die konkreten Zahlenwerte dieser Bereiche sind bewusst Balancingdaten und werden bei der anschließenden Implementierung und über Praxistests festgelegt beziehungsweise angepasst.

Seit den Combat-V2-Festlegungen gehören außerdem dazu:

* Gegnergrundregeln mit Content-/Lua-definiertem Grundzustand und getrennten Eigenschaften `attackable`/`aggressive` (Abschnitt 18)
* kontextabhängige Überschreibungen mit automatischem Auslaufen am Kontextende (Abschnitt 19)
* Home-Zone, Verfolgungsgrenzen und Evade/Return als Combat-Reset (Abschnitt 20)
* Respawn als Contentwerte mit Standardzeiten, Aggroformen (Solo/sozial/feste Gruppe), contentabhängige Schwierigkeit und Architekturgrenze Lua–DB–Kontext–Realm (Abschnitt 21)

Die Abschnitte 18–21 sind als Combat V2 umgesetzt; Details zur NPC-/Monster-Ebene stehen in `Projekt-Status.md`.

Folgende Bereiche sind mit Combat V3 umgesetzt oder werden separat ausgearbeitet, ohne das Grundkampfsystem zu verändern:

* das Ability-System mit konkreten Fähigkeiten, Ausführungsarten, Mana/Cooldown, Cast-Unterbrechung, AoE, Buffs/Debuffs, Waffen-Effekten (umgesetzt als Combat V3, Abschnitt 22 und `Ability-System.md`)
* Klassenmechaniken und die konkrete Zuordnung von Haupt-/Nebenskills je Klasse
* konkrete Schadens-, Duration-, Rüstungs- und Skill-Balancingwerte
* Attribute und Kampfwerte (verbindliche Grundlagen: `Attribute_und_Regeneration.md`)
* Gegner und deren Fähigkeiten (NPC-Ausführung der Fähigkeiten folgt mit den Boss-Fähigkeiten)
* normale Bosse
* Raids und Raidbosse
* detaillierte PvP-Mechaniken
* Loot und Belohnungen
* das Heldenrad als besondere klassenübergreifende Kampffähigkeit (siehe `Heldenrad.md`)

Dadurch bleibt das Grundkampfsystem einfach und kann von allen späteren Spielsystemen gemeinsam verwendet werden.
