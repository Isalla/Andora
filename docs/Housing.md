# Andora – Housing-System

## Status

**Geplant / späterer Entwicklungsabschnitt**

Das Housing-System wird erst konkret entwickelt, wenn Andora bereits über eine **spielbare und stabile Welt** verfügt.

Vorher haben grundlegende Systeme Priorität, insbesondere:

* Welt und Gebiete
* Spieler und Bewegung
* NPCs
* Items und Inventory
* Interaktionen
* Quests
* Combat
* Persistenz

Erst wenn diese Grundlagen praktisch funktionieren, wird das Housing-System umgesetzt.

> **Erst die Welt bauen – dann die Häuser darin.**

---

## 1. Grundidee

Andora besitzt zwei unterschiedliche Formen von Spieler-Housing:

1. Apartments in Städten
2. Eigene Grundstücke mit Häusern

Beide Systeme erfüllen unterschiedliche Zwecke und sollen sich auch
wirtschaftlich deutlich voneinander unterscheiden.

Apartments sind der günstige und leicht zugängliche Einstieg ins Housing.
Eigene Grundstücke und Häuser sind dagegen ein deutlich teureres,
langfristiges Ziel für Spieler.

---

## 2. Apartments

### 2.1 Zugang

Apartments befinden sich in Städten.

Der Spieler geht zu einem entsprechenden Gebäude bzw. einer Tür und kann
dort ein instanziertes Apartment mieten.

Es existiert dadurch kein persönliches Grundstück in der offenen Welt.

Mehrere Spieler können dasselbe Gebäude als Zugang verwenden, betreten
aber jeweils ihre eigene Housing-Instanz.

### 2.2 Miete

Ein Apartment wird nicht dauerhaft gekauft, sondern wöchentlich gemietet.

Die Miete kann auf zwei Arten bezahlt werden:

- automatische Zahlung
- manuelle Zahlung beim Betreten des Apartments

Ist die automatische Zahlung deaktiviert und die Miete fällig, kann beim
nächsten Betreten die ausstehende Miete bezahlt werden.

Ein Spieler soll nicht allein deshalb sofort sein Apartment verlieren,
weil er längere Zeit nicht eingeloggt war.

Die genaue Regelung für sehr lange unbezahlte Apartments wird später
festgelegt.

### 2.3 Nutzung

Apartments können später unter anderem verwendet werden für:

- Möbel
- Dekoration
- persönliche Gegenstände
- Trophäen
- Housing-Lager

Die genauen Funktionen werden in einer späteren Housing-Ausbaustufe
definiert.

---

## 3. Grundstücke und Häuser

Im Gegensatz zu Apartments ist dieses Housing deutlich kostspieliger und
als langfristiges Spielerziel gedacht.

### 3.1 Grundstücke in der offenen Welt

Grundstücke sind keine instanzierten Housing-Flächen.

Sie existieren als reale, begrenzte Bauflächen innerhalb der Spielwelt.
Ein dort errichtetes Haus ist damit für andere Spieler sichtbar und
Bestandteil des jeweiligen Gebietes.

Die Anzahl der verfügbaren Grundstücke ist bewusst begrenzt.

### 3.2 Grundstücke in der Unterwelt

Mit Expansion 1 wird die Unterwelt zunächst bis Ebene 100 ausgebaut.

Nach jeweils zehn gefährlichen Ebenen folgt eine sichere Ebene:

- Ebene 11
- Ebene 21
- Ebene 31
- Ebene 41
- Ebene 51
- Ebene 61
- Ebene 71
- Ebene 81
- Ebene 91

Diese sicheren Ebenen unterscheiden sich deutlich von den normalen
Unterwelt-Ebenen.

Jede sichere Ebene besitzt ein eigenes Biome- und Architekturthema.

Das Erscheinungsbild der sicheren Ebene gibt gleichzeitig einen
Vorgeschmack auf die folgenden acht regulären Ebenen.

Beispiel:

Ebene 21 ist eine sichere Zone mit einem bestimmten Biome- und
Architekturthema.

Die Ebenen 22 bis 29 greifen dieses Thema in Landschaft, Gegnerwelt,
Materialien und Atmosphäre wieder auf.

### 3.3 Housing-Flächen auf sicheren Ebenen

Auf jeder sicheren Ebene befindet sich eine begrenzte Fläche, auf der
Spieler Grundstücke erwerben können.

Diese Grundstücke sind physisch Bestandteil der sicheren Ebene.

Häuser müssen sich optisch an das Thema der jeweiligen Ebene anpassen.

Das bedeutet:

Ein Haus auf Ebene 11 kann einen anderen Baustil besitzen als ein Haus
auf Ebene 51 oder Ebene 91.

Der Baustil soll zur Landschaft, Architektur und Atmosphäre der jeweiligen
sicheren Ebene passen.

Housing wird dadurch gleichzeitig zu einem sichtbaren Teil der
Gebietsgestaltung.

### 3.4 Grundstück und Haus bleiben getrennt

Grundstück und Haus sind weiterhin zwei getrennte Objekte.

Das Grundstück definiert unter anderem:

- Position
- Baufläche
- Umgebung
- erlaubten Baustil
- Außendekoration

Das Haus ist das darauf errichtete Gebäude.

Für beide können getrennte Kosten beziehungsweise Zahlungen existieren.

Sie dürfen technisch und wirtschaftlich nicht zu einem einzigen
Besitzobjekt verschmolzen werden.

### 3.5 Grundstücksgrößen

Die Grundstücke besitzen unterschiedliche Größen.

Es gibt kleine, günstigere Grundstücke sowie größere und entsprechend
teurere Grundstücke.

Die konkreten Größen und Preise werden später beim Balancing festgelegt.

### 3.6 Anzahl der Grundstücke

Jede sichere Unterwelt-Ebene besitzt mindestens 10 reale Grundstücke.

Die tatsächliche Anzahl ist nicht fest vorgegeben und richtet sich nach
der Größe, dem Aufbau und der verfügbaren Baufläche der jeweiligen Ebene.

Eine sichere Ebene kann beispielsweise 10, 15 oder 19 Grundstücke
besitzen, wenn sich diese sinnvoll in die Gestaltung der Ebene integrieren
lassen.

Es gibt keine feste Obergrenze allein aufgrund des Housing-Systems.
Die Gebietsgestaltung bestimmt, wie viele Grundstücke sinnvoll Platz
finden.

Die Grundstücke sind reale, begrenzte Flächen innerhalb der Welt.
Sind alle Grundstücke einer Ebene vergeben, werden keine zusätzlichen
Housing-Instanzen erzeugt.

### 3.7 Grundstückszahlung

Für ein Grundstück fällt alle 7 Tage eine Zahlung an.

Die genaue Höhe kann unter anderem von Größe und Standort des
Grundstücks abhängen.

Grundstück und Haus bleiben getrennte Objekte und können getrennte
Zahlungen besitzen.

---

## 4. Getrennte Zahlungen

Für Grundstück und Haus existieren getrennte Zahlungen.

Beispiel:

- Zahlung für das Grundstück (alle 7 Tage, siehe 3.7)
- separate Zahlung für das darauf errichtete Haus

Die genaue Höhe der Grundstückszahlung richtet sich beispielsweise nach
Größe und Standort (3.5, 3.7). Zahlungsperiode und Höhe der Hauszahlung
sowie die Konsequenzen bei ausbleibender Zahlung werden erst beim
späteren Wirtschaftssystem festgelegt.

Insbesondere darf derzeit noch nicht festgelegt werden:

- wann ein Grundstück verloren geht
- wann ein Haus verloren geht
- ob Gegenstände automatisch eingelagert werden
- ob eine Schonfrist existiert
- wie lange eine solche Schonfrist dauert

Diese Entscheidungen gehören in die spätere Housing- und
Wirtschaftsplanung.

---

## 5. Wirtschaftliche Rolle

Housing besitzt bewusst unterschiedliche Einstiegshürden.

### Apartment

- günstig
- früh erreichbar
- wöchentliche Miete
- kein eigenes Grundstück
- instanziert

### Grundstück + Haus

- deutlich teurer
- langfristiges Spielerziel
- eigenes bebaubares Grundstück
- Grundstückszahlung alle 7 Tage
- Außengestaltung möglich
- Grundstück und Haus wirtschaftlich getrennt

Damit kann ein Spieler bereits relativ früh ein persönliches Zuhause
besitzen, während ein eigenes Grundstück mit Haus etwas Besonderes
bleibt.

---

## 6. Noch offene Entscheidungen

Folgende Punkte sind noch nicht Teil von Housing V1 und werden später
festgelegt:

- Hausgrößen und Ausbaustufen
- Bau- und Crafting-System für Häuser
- Möbelplatzierung
- Housing-Lager
- Besucher- und Zugriffsrechte
- gemeinsames Housing
- Gilden-Housing
- Handel oder Verkauf von Grundstücken
- Zahlungsfristen
- Folgen unbezahlter Grundstücks- oder Hauskosten
- Schutz von Housing-Gegenständen bei Verlust eines Hauses

---

## 7. Querverweise

- `docs/exp1_Unterwelt.md` (§18 Unterirdisches Housing): Grundstücke können auch in großen sicheren Kavernen der Unterwelt liegen; Housing bleibt Bestandteil der Landschaft.
- `docs/inventory_system.md`: Housing-Lager ist eine spätere Housing-Ausbaustufe, kein Bestandteil des Inventar-V1.
- `docs/MMO-Systeme-Ideensammlung.md` (Housing): Backlog-Themen, die nicht Teil von Housing V1 sind.
