# Lootsystem

## Grundprinzip

Das Lootsystem soll einfach, zufallsbasiert und für die offene Welt geeignet sein.

Nicht jeder mögliche Gegenstand eines Gegners wird bei jedem Kill fallen gelassen. Loot wird anhand von Loot-Tabellen erzeugt und bei jedem Gegner unabhängig ausgewürfelt.

Konkrete Dropchancen, Mengen, Timer und andere Balancingwerte werden erst bei der Implementierung und beim Testen festgelegt.

---

## Lootberechtigung und Claim

Der Claim eines Gegners bestimmt zunächst, welcher Spieler bzw. welche Gruppe auf dessen Loot zugreifen darf.

Bei Gruppen bestimmt anschließend das eingestellte Gruppen-Lootsystem, wie die Beute innerhalb der Gruppe verteilt wird.

Andere Spieler und Gruppen haben während des aktiven Claims keinen Zugriff auf den Loot.

### Ausnahme: Truhen

Truhen verlieren ihren exklusiven Claim nach Ablauf eines Timers und werden anschließend für alle Spieler frei zugänglich.

---

## Gruppen-Lootsystem

Innerhalb einer Gruppe kann eingestellt werden, wie Beute verteilt wird.

### FFA

Jedes lootberechtigte Gruppenmitglied kann die Beute aufnehmen.

Wer einen Gegenstand zuerst nimmt, erhält ihn.

### Gruppenleiter entscheidet

Der Gruppenleiter bestimmt, welches Gruppenmitglied einen Gegenstand erhält.

Dieser Modus eignet sich insbesondere für organisierte Gruppen und Raids.

### Würfeln

Lootberechtigte Gruppenmitglieder können um einen Gegenstand würfeln.

Der Gewinner des Würfelwurfs erhält den Gegenstand.

---

# Direktloot von Gegnern

Gegner können nach ihrem Tod direkt Beute hinterlassen.

Direkt von Gegnern erhaltene Ausrüstung bleibt grundsätzlich im einfachen Qualitätsbereich:

* schlechte Gegenstände
* normale Gegenstände

Hochwertige Beute soll hauptsächlich über Truhen und andere besondere Lootquellen ins Spiel gelangen.

Zusätzlich können Gegner Geld hinterlassen.

---

# Loot-Tabellen

Jeder Gegner bzw. Gegnertyp besitzt in der Datenbank eine Loot-Tabelle.

Diese Tabelle enthält die Gegenstände, die der Gegner grundsätzlich fallen lassen kann.

Es werden jedoch nicht automatisch alle Gegenstände der Tabelle erzeugt.

### Beispiel

Ein Gegner besitzt **7 mögliche Gegenstände** in seiner Loot-Tabelle.

Bei seinem Tod können beispielsweise **3 davon zufällig als tatsächlicher Loot ausgewählt** werden. Zusätzlich kann der Gegner etwas Geld hinterlassen.

Die übrigen Gegenstände erscheinen bei diesem Kill nicht.

Ein Gegenstand in einer Loot-Tabelle bedeutet daher lediglich:

> **Dieser Gegenstand kann von diesem Gegner erhalten werden.**

Es bedeutet nicht:

> **Dieser Gegenstand wird bei jedem Kill fallen gelassen.**

Benötigt ein Spieler einen bestimmten Gegenstand eines Gegners, kann es deshalb notwendig sein, diesen Gegner mehrfach zu besiegen.

---

# Truhen

Gegner können zusätzlich bzw. entsprechend ihrer Lootdefinition eine Truhe erzeugen.

Die Qualität des möglichen Inhalts steigt mit der Wertigkeit der Truhe.

## Normale Gegner

Normale Gegner können nur einfache Truhen droppen.

## Named-Gegner

Named-Gegner können auch höherwertige Truhen droppen.

Eine höherwertige Truhe ist jedoch **nicht garantiert**.

Auch ein Named kann besiegt werden, ohne dass eine besondere Truhe erscheint.

---

# Unabhängige Truhenwürfe

Jeder Gegner würfelt seinen Loot und seine mögliche Truhe **unabhängig von allen anderen Gegnern** aus.

Es gibt keinen künstlichen Glücks- oder Pechausgleich.

Das System verhindert insbesondere nicht mehrere hochwertige Truhen unmittelbar hintereinander.

Werden beispielsweise drei geeignete Named-Gegner nacheinander besiegt und haben alle drei beim unabhängigen Lootwurf Glück, können tatsächlich **drei hochwertige bzw. Master-Truhen** erscheinen.

Genauso kann eine längere Reihe von Gegnern keine hochwertige Truhe erzeugen.

**Glücks- und Pechsträhnen sind ausdrücklich erlaubt.**

---

# Truhen-Claim

Eine erzeugte Truhe gehört zunächst dem Spieler bzw. der Gruppe mit gültiger Lootberechtigung.

Für Gruppen gilt während dieser Zeit das eingestellte Gruppen-Lootsystem.

Jede Truhe besitzt anschließend zwei relevante Zeitmechaniken:

### Claim-Timer

Solange der Claim-Timer läuft, bleibt die Truhe für den ursprünglichen Spieler bzw. die ursprüngliche Gruppe reserviert.

Nach Ablauf des Claim-Timers wird die Truhe **für alle Spieler frei**.

Verbliebener Inhalt kann dann von anderen Spielern genommen werden.

### Despawn-Timer

Nach einer weiteren Zeitspanne verschwindet die Truhe vollständig aus der Welt.

Ist die Truhe bereits leer, despawnt sie **sofort**.

Ist beim Ablauf des Despawn-Timers noch Inhalt vorhanden, verschwindet die Truhe mitsamt dem verbliebenen Inhalt.

---

# Wertigkeit und Lebensdauer von Truhen

Die Dauer der Truhen-Timer richtet sich nach der Wertigkeit der Truhe.

> **Je wertvoller die Truhe, desto länger bleiben ihre Timer aktiv.**

Dadurch erhalten Spieler bei besonders wertvoller Beute mehr Zeit, die Truhe zu öffnen und ihren Inhalt innerhalb einer Gruppe zu verteilen.

Einfache Truhen verschwinden entsprechend schneller.

Die konkreten Zeiten werden erst bei der Implementierung und beim Balancing festgelegt.

---

# Noch nicht festgelegte Werte

Folgende Werte werden bewusst noch nicht im Konzept festgeschrieben:

* konkrete Dropchancen
* Anzahl möglicher Lootgegenstände pro Gegner
* Anzahl tatsächlich ausgewählter Gegenstände
* Geldmengen
* Wahrscheinlichkeit für Truhen
* Wahrscheinlichkeit einzelner Truhenqualitäten
* konkrete Truhen-Wertigkeitsstufen
* Dauer des Claim-Timers
* Dauer des Despawn-Timers
* weitere Loot-Balancingwerte

Diese Werte werden bei der Implementierung definiert und anschließend durch Tests angepasst.

Neue Ideen können dadurch später integriert werden, ohne das grundlegende Lootkonzept neu entwickeln zu müssen.

## Gebundene Gegenstände

Bestimmte besonders wertvolle Gegenstände können beim Erhalt an den Charakter gebunden werden.

Gebundene Gegenstände können nicht an andere Spieler weitergegeben, verkauft oder über das Auktionshaus gehandelt werden.

Dies betrifft insbesondere sehr hochwertigen Raidloot und andere Gegenstände, deren Wert aus einer besonderen spielerischen Leistung entstehen soll.

Dadurch soll verhindert werden, dass solche Gegenstände nur wegen ihres Handelswertes beansprucht werden, obwohl ein anderer Spieler sie tatsächlich benötigt.

Welche Gegenstände gebunden werden, wird über die jeweilige Gegenstandsdefinition festgelegt.

Für die Spielerwirtschaft vorgesehene Gegenstände, insbesondere Rohstoffe und hergestellte Gegenstände, bleiben grundsätzlich handelbar.

