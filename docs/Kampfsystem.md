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

Jede Fähigkeit besitzt ihren eigenen Cooldown.

Weitere Eigenschaften einer Fähigkeit werden nicht global durch das Kampfsystem festgelegt, sondern bei der jeweiligen Fähigkeit definiert.

Dadurch können spätere Klassen und Fähigkeiten unterschiedliche Mechaniken verwenden, ohne das Grundkampfsystem verändern zu müssen.

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

## 15. Todesmalus

Sterben soll eine Konsequenz besitzen, ohne den Spieler übermäßig zu bestrafen.

Jeder Tod erhöht deshalb einen XP-Malus.

Der kumulierte XP-Malus kann maximal **10 %** erreichen.

Die genaue Berechnung und der spätere Abbau des XP-Malus werden separat festgelegt.

Bereits erreichte Charakterstufen werden durch den Tod nicht direkt verändert.

Tod, Wiederbelebung und Todesmalus in diesem Abschnitt beschreiben das reguläre (`normal`-)Ruleset. Andere Rulesets (z. B. ein späterer Hardcore-Realm) können abweichende Todesregeln definieren; diese sind noch nicht festgelegt (siehe Realm-Rulesets in `Login_Realm_Architektur.md`).

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

> **Noch keine Implementierung des Kampfsystems vornehmen.**

---

# Abgrenzung zu anderen Systemen

Das allgemeine Kampfsystem definiert die grundlegenden Regeln eines Kampfes.

Dazu gehören neben den bereits festgelegten Grundelementen (Anvisieren, Grundangriff/Duration, Fähigkeiten, Bewegung, Ressourcen, Tempo, Aggro, Tod/Respawn, Fraktionskämpfe) seit den heutigen Festlegungen auch:

* Kampfskills und Waffenbeherrschung (Haupt-/Nebenskills, Skillmaximum, Abschnitt 4)
* die grundsätzliche physische Trefferauflösung (Abschnitt 5)
* die Grundsätze für Waffenschaden und Angriffsgeschwindigkeit (Abschnitt 6)
* Rüstung und physische Schadensreduktion mit Klassen-Caps (Abschnitt 7)

Die konkreten Zahlenwerte dieser Bereiche sind bewusst Balancingdaten und werden bei der anschließenden Implementierung und über Praxistests festgelegt beziehungsweise angepasst.

Folgende Bereiche werden separat ausgearbeitet, ohne das Grundkampfsystem zu verändern:

* konkrete Fähigkeiten und deren Eigenschaften
* Klassenmechaniken und die konkrete Zuordnung von Haupt-/Nebenskills je Klasse
* konkrete Schadens-, Duration-, Rüstungs- und Skill-Balancingwerte
* Attribute und Kampfwerte
* Gegner und deren Fähigkeiten
* normale Bosse
* Raids und Raidbosse
* detaillierte PvP-Mechaniken
* Loot und Belohnungen

Dadurch bleibt das Grundkampfsystem einfach und kann von allen späteren Spielsystemen gemeinsam verwendet werden.
