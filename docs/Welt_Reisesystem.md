# Andora – Welt- und Reisesystem

## 1. Status

**Status:** 🟡 Konzept

Andora besitzt eine große zusammenhängende Spielwelt, die aus mehreren Großgebieten besteht.

Innerhalb eines Großgebietes soll sich der Spieler frei bewegen können, ohne durch Ladebildschirme aus der Welt gerissen zu werden.

Grundprinzip:

> **Die Welt wird durch Reisen erschlossen, nicht durch ein Menü freigeschaltet.**

---

# 2. Nahtlose Großgebiete

Die einzelnen Großgebiete Andoras sind in sich zusammenhängend und werden ohne sichtbare Ladebildschirme bereist.

Der Spieler kann beispielsweise:

* eine Hauptstadt verlassen
* durch umliegendes Grasland reisen
* Dörfer besuchen
* Wälder durchqueren
* in den Norden eines Gebietes reisen
* Bergregionen erreichen

ohne dabei einen Ladebildschirm zu sehen.

Technisch darf die Welt intern aus mehreren Zonen beziehungsweise Chunks bestehen.

Diese können vom Godot-Client im Hintergrund geladen und wieder entladen werden.

Für den Spieler bleibt das Großgebiet trotzdem eine zusammenhängende Welt.

---

# 3. Visuelle Identität der Fraktionsgebiete

Jedes Fraktionsgebiet besitzt eine deutlich erkennbare eigene visuelle Identität.

Diese entsteht unter anderem durch:

* Landschaft
* Vegetation
* Architektur
* Straßen
* Dörfer und Städte
* Landwirtschaft
* Tierhaltung
* Forstwirtschaft
* Rohstoffgewinnung
* regionale Infrastruktur

Dadurch soll der Spieler auch ohne Karte erkennen können, in welchem Fraktionsgebiet er sich befindet.

Beim Übergang zwischen zwei Gebieten verändert sich die Umgebung zunehmend.

Grenzregionen können dabei Elemente beider Gebiete miteinander verbinden.

> **Die Welt selbst zeigt dem Spieler, wessen Einflussgebiet er gerade bereist.**

---

# 4. Fraktionsquests zeigen die spätere Heimat

Bereits im neutralen Startgebiet geben die Fraktionsquestreihen einen Vorgeschmack auf die späteren Gebiete der jeweiligen Fraktion.

Die Questbereiche zeigen in kleiner Form:

* Landschaft
* Architektur
* Kultur
* Lebensweise
* typische Tätigkeiten
* Philosophie der Fraktion

Der Spieler lernt dadurch nicht nur die Ideologie einer Fraktion kennen.

Er bekommt gleichzeitig einen Eindruck davon, **in welcher Welt er später leben wird.**

Die Fraktionsentscheidung kann dadurch sowohl aus der Spielweise als auch aus dem persönlichen Gefühl für die jeweilige Umgebung entstehen.

---

# 5. Reisen innerhalb eines Großgebietes

Für größere Entfernungen innerhalb eines Großgebietes werden hauptsächlich Reittiere verwendet.

Beispiele:

```text
Hauptstadt
    ↓
Reittier
    ↓
nördliche Region
    ↓
Reittier
    ↓
Dorf
```

Reittiere sollen die Reise beschleunigen, ohne die Welt zu überspringen.

Der Spieler erlebt weiterhin:

* Landschaften
* Siedlungen
* andere Spieler
* NPCs
* Ereignisse
* mögliche Gefahren
* Veränderungen innerhalb des Gebietes

Weitere regionale Transportmittel wie Kutschen, Schiffe, Züge oder vergleichbare Systeme können später abhängig von Region und Kultur ergänzt werden.

---

# 6. Portalsteine

Portalsteine dienen hauptsächlich der schnellen Reise **zwischen weit voneinander entfernten Großgebieten**.

Sie ersetzen nicht die normale Fortbewegung innerhalb eines Gebietes.

Grundstruktur:

```text
Großgebiet A
     ↓
Portalstein
     ↓
Ladebildschirm
     ↓
Großgebiet B
```

Innerhalb von Großgebiet A beziehungsweise B wird anschließend wieder normal gereist.

---

# 7. Portalsteine müssen entdeckt werden

Portalsteine der offenen Welt stehen einem Charakter nicht automatisch zur Verfügung.

Der Spieler muss einen Portalstein zunächst selbst erreichen und vor Ort aktivieren.

Erst danach wird dieser Portalstein als mögliches Reiseziel freigeschaltet.

```text
neues Gebiet erreichen
        ↓
Portalstein finden
        ↓
Portalstein aktivieren
        ↓
Reiseziel dauerhaft freigeschaltet
```

Dadurch kann ein neuer Charakter nicht einfach über das Portalnetz beliebig durch ganz Andora reisen.

---

# 8. Keine automatische Freischaltung durch Level

Das Charakterlevel allein schaltet keine Portalsteine frei.

Ein Level-15-Charakter kann deshalb nicht automatisch sämtliche Gebiete Andoras über das Portalnetz erreichen.

Schafft es ein Spieler jedoch tatsächlich auf anderem Weg in ein gefährliches oder weit entferntes Gebiet und erreicht dort den Portalstein, darf seine Erkundung grundsätzlich belohnt werden.

> **Nicht das Level entdeckt die Welt – der Spieler entdeckt sie.**

---

# 9. Sonderportale

Bestimmte Sondergebiete sind von der normalen Aktivierungsregel der Portalsteine ausgenommen.

Dazu können beispielsweise gehören:

* Gildeninseln
* Dungeons
* Raids
* besondere Instanzen
* Eventgebiete
* persönliche oder gruppengebundene Gebiete

Bei diesen Zielen entscheidet nicht die vorherige Welterkundung, sondern die jeweilige Zugangsberechtigung.

Beispiel Gildeninsel:

```text
Spieler tritt Gilde bei
        ↓
Gildeninsel wird zugänglich
        ↓
Portal kann verwendet werden

Spieler verlässt Gilde
        ↓
Zugangsberechtigung entfällt
```

Damit bleiben zwei unterschiedliche Systeme erhalten:

> **Portalnetz der offenen Welt = Erkundung**

> **Sonderportale = Zugangsberechtigung**

---

# 10. Ladebildschirme

Innerhalb eines normalen Großgebietes gibt es keine sichtbaren Ladebildschirme.

Ladebildschirme werden hauptsächlich verwendet bei:

* Portalreisen zwischen Großgebieten
* Reisen zu Gildeninseln
* Betreten technisch getrennter Dungeons
* Betreten von Raids
* Wechsel in besondere Instanzen
* anderen vollständig getrennten Sondergebieten

Beispiel:

```text
Dorf
 ↓
Landschaft
 ↓
Wald
 ↓
nächstes Dorf
 ↓
Bergregion

KEIN Ladebildschirm
```

Dagegen:

```text
Großgebiet A
     ↓
Portal
     ↓
Ladebildschirm
     ↓
Großgebiet B
```

---

# 11. Technische Grundidee

Die nahtlose Welt bedeutet nicht, dass ein gesamtes Großgebiet permanent vollständig im Speicher liegen muss.

Godot kann die Umgebung intern in kleinere Bereiche beziehungsweise Chunks aufteilen.

Vereinfacht:

```text
Großgebiet
    ↓
mehrere interne Chunks
    ↓
benötigte Chunks laden
    ↓
entfernte Chunks entladen
```

Dieser Vorgang geschieht im Hintergrund und soll für den Spieler nicht als Gebietswechsel wahrnehmbar sein.

Bei einer Portalreise darf dagegen das bisherige Großgebiet vollständig verlassen und das neue Großgebiet geladen werden.

---

# 12. Zentrale Grundsätze

> **Andoras Großgebiete werden innerhalb ihrer Grenzen nahtlos und ohne sichtbare Ladebildschirme bereist.**

> **Die visuelle Gestaltung der Welt zeigt dem Spieler, in welchem Fraktionsgebiet er sich befindet.**

> **Bereits die Fraktionsquests geben einen Vorgeschmack auf die spätere Heimat einer Fraktion.**

> **Reittiere dienen hauptsächlich der Fortbewegung innerhalb eines Großgebietes.**

> **Portalsteine verbinden weit voneinander entfernte Großgebiete.**

> **Portalsteine der offenen Welt müssen persönlich entdeckt und aktiviert werden.**

> **Das Charakterlevel allein schaltet keine Portalziele frei.**

> **Sondergebiete wie Gildeninseln können stattdessen über eine Zugangsberechtigung freigeschaltet werden.**

> **Ladebildschirme erscheinen hauptsächlich bei Portalreisen und beim Wechsel in technisch getrennte Sondergebiete.**

> **Die erste Reise erschließt die Welt – spätere Reisen dürfen komfortabler werden.**
