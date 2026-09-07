# Andora – Kampfsystem

## 1. Grundprinzip

Andora verwendet ein klassisches MMORPG-Kampfsystem.

Das Kampfsystem soll bewusst übersichtlich und technisch schlank bleiben. Komplexität entsteht später durch Klassen, Fähigkeiten, Ausrüstung, Gegner und das Zusammenspiel der Spieler und nicht durch ein aufwendiges Action-Kampfsystem.

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

## 4. Fähigkeiten

Fähigkeiten werden vom Spieler aktiv über seine Aktionsleiste ausgelöst.

Jede Fähigkeit besitzt ihren eigenen Cooldown.

Weitere Eigenschaften einer Fähigkeit werden nicht global durch das Kampfsystem festgelegt, sondern bei der jeweiligen Fähigkeit definiert.

Dadurch können spätere Klassen und Fähigkeiten unterschiedliche Mechaniken verwenden, ohne das Grundkampfsystem verändern zu müssen.

---

## 5. Bewegung im Kampf

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

## 6. Ressourcen

Alle Spielercharaktere verwenden grundsätzlich nur zwei zentrale Ressourcen:

**HP – Lebenspunkte**

HP bestimmen, wie viel Schaden ein Charakter überleben kann.

**Mana**

Mana wird für Fähigkeiten verwendet, die einen Manaverbrauch besitzen.

Auf zusätzliche klassenspezifische Grundressourcen wie Wut, Energie oder Fokus wird verzichtet.

---

## 7. Kampftempo

Andora verwendet ein klassisches MMORPG-Kampftempo.

Der Kampf soll nicht auf permanentes Ausweichen, Animation-Canceling oder extrem schnelle Eingaben ausgelegt sein.

Positionierung, Zielwahl, Fähigkeiten, Ausrüstung und das Zusammenspiel der Gruppe stehen stärker im Mittelpunkt.

Das vergleichsweise ruhige Kampfsystem unterstützt gleichzeitig das Ziel, den Client auch auf leistungsschwacher Hardware wie dem Raspberry Pi betreiben zu können.

Die visuelle Darstellung von Fähigkeiten, Zaubern und Kampfeffekten wird hier nicht vorgegeben, sondern folgt dem Darstellungsprinzip in `Clientdarstellung_und_Performance.md`: hybrider Ansatz (2D-Animationen, vorgerenderte 3D-Effekte als 2D-Animation, leichte echte 3D-Effekte im Performance-Budget); für geeignete Effekte kann der Client unterschiedliche Darstellungen derselben Fähigkeit unterstützen – die bevorzugte 2D-/3D-Darstellung wählt der Spieler in den Client-Grafikoptionen. Diese Auswahl ist rein clientseitig und hat keinerlei Auswirkung auf Schaden, Heilung, Reichweite, Wirkungsradius, Hitbox, Trefferberechnung, Dauer, Cooldown, Ressourcenverbrauch, Zielauswahl, Anzahl getroffener Ziele oder serverseitige Kampfregeln. Der Realm bleibt für die tatsächliche Spielmechanik autoritativ; Spieler mit 2D- und 3D-Darstellung erleben spielmechanisch exakt dasselbe Kampfgeschehen.

---

## 8. Aggro und Gruppenrollen

Gegner verwenden ein klassisches Aggro- bzw. Bedrohungssystem.

Tanks sollen Gegner an sich binden und deren Aufmerksamkeit kontrollieren können.

DDs konzentrieren sich auf das Verursachen von Schaden.

Heiler unterstützen die Gruppe und halten Gruppenmitglieder am Leben.

Die konkreten Auswirkungen von Fähigkeiten auf Aggro und Bedrohung werden bei den jeweiligen Fähigkeiten definiert.

Das Kampfsystem verhindert nicht, dass andere Spieler Aggro bekommen.

Greift beispielsweise ein DD einen anderen Gegner an und zieht dadurch dessen Aufmerksamkeit auf sich, ist dies eine normale Konsequenz seiner Zielwahl.

Der Tank ist nicht automatisch dafür verantwortlich, sämtliche Gegner zu kontrollieren, die andere Gruppenmitglieder eigenständig in den Kampf bringen.

---

## 9. Tod und Wiederbelebung

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

## 10. Respawnpunkte

Jedes größere Gebiet besitzt mehrere Respawnpunkte.

Beim Respawn wird der Spieler zum nächstgelegenen geeigneten Respawnpunkt gebracht.

Dadurch sollen unnötig lange Laufwege nach einem Tod vermieden werden.

Respawnpunkte werden entsprechend über die Gebiete verteilt und sind Teil der jeweiligen Gebietsgestaltung.

---

## 11. Todesmalus

Sterben soll eine Konsequenz besitzen, ohne den Spieler übermäßig zu bestrafen.

Jeder Tod erhöht deshalb einen XP-Malus.

Der kumulierte XP-Malus kann maximal **10 %** erreichen.

Die genaue Berechnung und der spätere Abbau des XP-Malus werden separat festgelegt.

Bereits erreichte Charakterstufen werden durch den Tod nicht direkt verändert.

---

## 12. Fraktionskämpfe innerhalb einer Gruppe

Für Kämpfe zwischen verfeindeten Fraktionen gelten die bereits definierten Gruppenregeln.

Befinden sich Spieler verschiedener Fraktionen gemeinsam in einer Gruppe und ein Gruppenmitglied beginnt einen Fraktionskampf, werden Gruppenmitglieder, die an diesem Konflikt nicht beteiligt sein dürfen, für die Dauer dieses Kampfes ausgegraut.

Der kämpfende Spieler wird dadurch für diese Gruppenmitglieder temporär von gruppenbasierten Unterstützungsmechaniken getrennt.

Gruppenheilungen oder vergleichbare Gruppeneffekte können dadurch nicht verwendet werden, um indirekt in einen Fraktionskampf einzugreifen.

Ein einem betroffenen Spieler zugeteilter Söldner wird innerhalb der Gruppe entsprechend behandelt.

Nach Ende des Fraktionskampfes wird die normale Gruppeninteraktion wiederhergestellt.

---

# Abgrenzung zu anderen Systemen

Das allgemeine Kampfsystem definiert ausschließlich die grundlegenden Regeln eines Kampfes.

Folgende Bereiche werden separat ausgearbeitet:

* konkrete Fähigkeiten und deren Eigenschaften
* Klassenmechaniken
* Waffen- und Ausrüstungswerte
* Attribute und Kampfwerte
* Gegner und deren Fähigkeiten
* normale Bosse
* Raids und Raidbosse
* detaillierte PvP-Mechaniken
* Loot und Belohnungen

Dadurch bleibt das Grundkampfsystem einfach und kann von allen späteren Spielsystemen gemeinsam verwendet werden.
