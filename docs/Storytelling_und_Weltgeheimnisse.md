# Andora – Storytelling und Weltgeheimnisse

## 1. Status

**Status:** Verbindliche Grundprinzipien (Konzept-Ebene)

Dieses Dokument definiert verbindlich, wie Storytelling, Weltgeheimnisse, Bücher, verborgene Questketten, variable Rundrätsel, spielerausgelöste Realm-Ereignisse und Realm-Chroniken in Andora konzipiert werden.

Es ergänzt `quests_stories.md` (Inhalt/Quest-Definitionen) und `Quest-System.md` (Architektur/Ausführung), ohne deren Serverautorität zu verändern.

> **Dieses Dokument enthält keine technische Implementierung.**
> Zustandsmodelle, Trigger-Details und Datenmodelle werden später mit dem Event-/Rätselsystem spezifiziert.

---

# 2. Vier Ebenen des Storytellings

Andora wird erzählerisch auf vier Ebenen aufgebaut:

```text
1. Hauptstory
   → übergeordnete, langfristige Welthandlung

2. Nebenmissionen
   → lokale Geschichten über Personen, Orte, Kulturen und Konflikte

3. Rätsel und Weltgeheimnisse
   → entdeckbare Geheimnisse in der Welt (ohne Quest-Pflicht)

4. Spielerausgelöste Realm-Ereignisse
   → vorbereitete Welt-Ereignisse, die durch Spieler-Entdeckungen beginnen
```

Diese Ebenen ergänzen einander und schließen sich nicht aus.

Ein Spieler soll Andora auf unterschiedliche Weise erleben können:

* **Kampfsystemorientierte Spieler** (Raid, Endgame, Dungeons) können ihr Andora klar als Schlacht-/Endgame-Wahrnehmung spielen, die nicht auf versteckte Geschichten angewiesen ist.
* **Neugierige Spieler** (Bücher lesen, NPC zuhören, Orte untersuchen, Zusammenhänge erkennen) können Inhalte entdecken, die andere nie erleben müssen.

Die Welt soll beide Richtungen unterstützen, ohne dass eine Spielrichtung zur Pflicht für die andere wird.

---

# 3. Grundgedanke

> **Die Welt erklärt ihre Geheimnisse nicht. Sie hinterlässt Spuren.**

Andora soll keine klassische „Questmarker-Führung sein“.
Stattdessen soll die Welt selbst Hinweise, Widersprüche, Fragmente und Beobachtungen enthalten.

Neugier wird belohnt. Aufmerksames Lesen, Hören, Erkunden und Kombinieren führen zu besseren oder zusätzlichen Erfahrungen. Damit gilt:

* Weltgeheimnisse sollen optional entdeckbar sein.
* Keine Spielrichtung soll erzwungen werden.
* Versteckte Inhalte dürfen existieren, ohne dass alle Spieler sie finden müssen.
* Die Welt soll plausibel bleiben, auch wenn ein Spieler sie nur oberflächlich erlebt.

---

# 4. Hauptstory

Andora besitzt eine übergeordnete Hauptstory.

Diese Hauptstory kann sich über mehrere Veröffentlichungen und Erweiterungen hinweg entwickeln.

Grundprinzip:

* Große Zusammenhänge sollen für das Design früh bekannt sein.
* Die Spieler sollen diese Zusammenhänge aber nicht automatisch erfahren.
* Die Hauptstory darf Geheimnisse stellen, die zum Zeitpunkt der ersten Veröffentlichung noch nicht vollständig beantwortet werden.
* Eine spätere Erweiterung kann ein solches Geheimnis auflösen.
* Genauso kann sie ein größeres Geheimnis eröffnen, während sie ein älteres auflöst.

Beispielstruktur:

```text
Grundspiel
→ stellt ein oder mehrere große Fragen
→ gibt Hinweise, Fragmente und Widersprüche
→ löst nicht alle Fragen

Erweiterung 1
→ beantwortet einen Teil
→ verschärkt die Lage
→ öffnet ein größeres Geheimnis

Erweiterung 2
→ liefert neue Informationen
→ verbindet alte Regionen/NPCs neu
→ verschiebt die Perspektive der Hauptstory
```

Wichtig:

> **Früh konzeptionelle Kenntnis ≠ spätere Spieler-Aufklärung.**
>
> Das Design muss im Voraus wissen, was später wichtig wird.
> Die Spieler erfahren es aber erst, wenn die Welt es glaubwürdig offenlegt.

Dadurch können Hinweise in der Grundwelt platziert werden, ohne ihre Bedeutung sofort zu erklären.

---

# 5. Nebenmissionen

Nebenmissionen erzählen lokale Geschichten.

Sie können unabhängig von der Hauptstory existieren.

Mögliche Themen:

* NPCs und ihre privaten Biografien
* Regionen, Städte, Dörfer
* Kulturen, Gilden, Fraktionen
* historische Konflikte
* persönliche Ziele einzelner Personen
* wirtschaftliche oder lokale Probleme
* Verschwörungen
* Legenden

Eine Nebenmission muss nicht mit der Hauptstory verbunden sein.

Die Abgrenzung ist bewusst:

* **Nebenmission** ist eine lokal erzählte Geschichte, die oft durch NPC oder Ort getragen wird.
* **Weltgeheimnis** ist ein Fragment, das im World-Content liegt und durch Spieler-Entdeckung erkennbar werden kann.
* **Realm-Ereignis** ist ein vorbereitetes Weltstück, das durch Spieler-Entdeckung oder Spieler-Handlung aktiv wird.

Diese Formen können ineinander übergehen, müssen aber getrennt gedacht werden.

---

# 6. Bücher als Gameplay

Bücher dürfen in Andora mehr sein als Lore-Träger.

Ein Buch kann:

* Hintergrundgeschichte erzählen
* Orte beschreiben
* Geschichte in Textform darstellen
* Fragen offenlassen
* widersprüchliche Informationen enthalten
* indirekte Handlungsanweisungen liefern
* Hinweise auf andere Bücher, NPC oder Ruinen geben
* als Gegenstand in Quests, Rätseln oder Realm-Ereignissen verwendet werden

Bücher sollen in der Welt wirken wie natürliche Quellen.

Sie können gefunden, gekauft, gestohlen, gesammelt oder entdeckt werden.

Wichtiges Grundprinzip:

> **Bücher dürfen Hinweise geben, aber keine klassischen Questmarker ersetzen.**

Ein Buch darf z. B. folgendes liefern:

```text
Dieses Buch beschreibt den alten Tempel von Kelden.
Es erwähnt, dass im Winter bei Mondlicht ein verborgener Eingang sichtbar wird.
```

Das darf tatsächlich zu einem spielbaren Ablauf führen:

```text
Buch lesen
      ↓
Ort suchen
      ↓
zur richtigen Zeit dort sein
      ↓
verborgener Eingang erscheinen
      ↓
NPC fragt zum Buchinhalt
      ↓
richtige Antwort öffnet nächsten Abschnitt
```

Solche Kette ist ausdrücklich erlaubt.

Der Unterschied zur klassischen Questmarker-Führung:

* Die Welt enthält die Spur.
* Der Spieler muss sie lesen, beobachten oder kombinieren.
* Es gibt keinen zwingenden roten Faden im UI, es sei denn, die Quest wird tatsächlich als aktive Quest angenommen.

Bücher dürfen sowohl reine Lore sein als auch Spielmechanik sein.
Für den Spieler soll dies nicht immer sofort erkennbar sein.

---

# 7. Weltgeheimnisse

Weltgeheimnisse sind kleine, entdeckbare Geheimnisse in der Welt.

Sie können sich in verschiedenen Formen zeigen:

* Bücher
* NPC-Erzählungen
* Legenden
* Ruinen
* Symbole
* Inschriften
* versteckte Orte
* Gegenstände
* Umweltbeobachtungen
* historische Berichte
* widersprüchliche Überlieferungen
* ungewöhnliche Artefakte
* Raumereignisse
* saisonale Erscheinungen

Ein Geheimnis muss nicht zu einer Quest führen.

Es kann einfach eine Information sein.

Es kann zu einer Quest führen.

Es kann zu einer verborgenen Questkette gehören.

Es kann Teil eines größeren Realm-Ereignisses sein.

Es kann irrelevant sein für das Fortschreiten eines typischen Spielers.

Die Welt soll dem Spieler nicht automatisch verraten, was eine Entdeckung bedeutet.

Mögliche Deutungen einer Entdeckung:

* reine Lore
* Hinweis auf ein Rätsel
* Teil einer verborgenen Questkette
* Auslöser für ein Realm-Ereignis
* kosmetische oder Sammlungsbelohnung
* Hinweis, der später wichtig wird
* historisches Detail ohne aktuelle spielerische Folge

> **Die Spieler sollen nicht automatisch erkennen können, ob etwas nur Lore ist, Teil eines Rätsels, Teil einer Questkette, ein späterer wichtiger Hinweis oder ein Realm-Ereignis.**

Dadurch bleibt die Welt lebendig und überraschend.

---

# 8. Verborgene Questketten

Verborgene Questketten sind Questreihen, die nicht zwingend durch einen sichtbaren Questgeber initiiert werden.

Ihre Auslöser können sein:

* ein Buch
* ein Gegenstand
* ein verlassener Ort
* ein NPC-Hinweis
* eine Beobachtung
* eine widersprüchliche Geschichte
* ein Artefakt
* ein Raum-Event
* ein historischer Ort
* eine Inschrift
* eine seltene Begegnung

Diese Ketten können bewusst über mehrere Gebiete führen.

Ziel ist, dass Spieler:

* die Welt lernen
* die Welt erkunden
* Zusammenhänge entdecken
* Erkundung mit Storytelling verbinden
* sich nicht nach festen Questmarkern orientieren, sondern nach Hinweisen, Widersprüchen und Beobachtungen

Beispielkette:

```text
Bücher mit widersprüchlichen Aussagen
      ↓
eine Region wird genauer untersucht
      ↓
wenn ein Spieler eine bestimmte Beobachtung erkennt,
findet er einen verborgenen Eingang
      ↓
nach Lösung eines Rätsels spricht ein NPC über die Geschichte
      ↓
mit der richtigen Antwort öffnet sich eine weitere Ebene
      ↓
später kann die Kette ein Realm-Ereignis auslösen
```

Wichtig:

> **Eine verborgene Questkette muss nicht sichtbar sein, muss aber serverseitig gültig und prüfbar sein.**

Die Quest-Logik folgt weiterhin:

* Lua beschreibt die Quest
* Realm-Server (Rust) prüft die Bedingungen
* MariaDB speichert den Fortschritt
* Godot zeigt dem Spieler nur, was er aktuell erleben sollte

---

# 9. Variable persönliche Rätsel

Bestimmte Rätsel dürfen von jedem Spieler individuell gelöst werden.

Ein Rätsel kann von einer Gruppe gemeinsam erkannt werden, aber für jeden Charakter einen anderen genauen Fundpunkt ergeben.

Beispiel:

```text
Rätselaufgabe:
„Folge dem Bach bis zum Wald. Dort findest du einen Baum mit einer Notiz."
```

Für jeden Charakter kann der Realm-Server individuell entscheiden, welcher Baum tatsächlich die Notiz trägt.

Dadurch können fünf Spieler gleichzeitig denselben Hinweis besitzen und trotzdem an verschiedenen Orten suchen.

Glaubwürdige Grundidee hinter solcher Individualisierung:

* Spielerverhalten ist individuell
* jede Gruppe sieht die Welt anders
* Rätsel sollen nicht wie eine globale Koordinaten-Aufgabe wirken

Regeln:

* Hinweise dürfen zwischen Spielern geteilt werden.
* Eine feste Komplettlösung für alle Charaktere ist nicht erforderlich.
* Die individuelle Lösung muss serverseitig validierbar sein.
* Die Welt soll plausibel bleiben, auch wenn ein anderer Spieler an einem anderen Ort fündig wird.

Technische Umsetzung:

> **Die genaue serverseitige Umsetzung wird beim Event-/Rätselsystem festgelegt.**
> Es dürfen keine festen Datenmodelle oder Trigger-Details vorab definiert werden.

---

# 10. Spielerausgelöste Realm-Ereignisse

Realm-Ereignisse sind vorbereitete, zunächst inaktive Welt-Ereignisse.

Sie können unabhängig auf jedem Realm durch Spieler-Entdeckungen oder Spieler-Handlungen ausgelöst werden.

Beispiel:

```text
Ein Spieler entdeckt eine Ruine
      ↓
NPCs in der Region erfahren davon
      ↓
Forscher-Gilden bekommen Hinweise
      ↓
Recherche und Expedition starten
      ↓
Übergreifende Forschung läuft über längere Zeit
      ↓
später gibt es neuen Content, zusätzliche Erkenntnisse oder ein nächstes Ereignis
```

Ein solches Ereignis kann die Welt dauerhaft auf diesem Realm verändern.

Grundprinzip:

* Ereignisse sind vorbereitet, nicht improvisiert.
* Sie werden serverseitig als gültige Zustände definiert.
* Sie können zeitabhängig sein.
* Sie können auf verschiedenen Realm-Instanzen unabhängig reagieren.
* Sie können mehrere Phasen besitzen.
* Sie können Content über die reale Entwicklungszeit des Projekts hinweg nutzen.
* Sie können später als historisches Ereignis in die Realm-Chronik einfließen.

Wichtig:

> **Die reale Entwicklungszeit darf Teil der Spielwelt sein.**
>
> Wenn ein Event-Mechanismus in der Entwicklung noch nicht verfügbar ist, kann ein Realm-Ereignis trotzdem als vorbereitetes Story-Fenster definiert werden und später technisch angebunden werden.

Bestehendes Beispiel in der Welt:

`exp2_Region_Mandalonien_Ruf_und Woechentliches_Event.md` definiert das wöchentliche Mandalonier-Event, bei dem das Ruf-Verhalten von Spielern und Gilden bestimmt, wer zum Ziel wird. Dieses Dokument bestätigt damit den Grundgedanken, dass ein vorbereitetes Weltereignis durch Spielerhandlungen beeinflusst oder ausgelöst werden kann.

---

# 11. Individuelle Realm-Chroniken

Jeder normale Realm besitzt grundsätzlich dieselbe Andora-Welt und denselben verfügbaren Content.

Die Unterschiede entstehen dadurch, welche vorbereiteten Ereignisse auf welchem Realm entdeckt oder ausgelöst wurden und wie weit sie fortgeschritten sind.

Daher entwickelt jeder Realm im Laufe der Zeit eine eigene Chronik derselben Welt.

Ausnahmen:

* **Classic** wird bewusst als abweichender Realm-Realm definiert und ist von dieser Grundregel ausgenommen.

Die Chronik entsteht aus:

* gefundenen Realm-Ereignissen
* ausgelösten Ereignisketten
* abgeschlossenen Welt-Quests
* einmaligen Entdeckungen
* einzigartigen historischen Momenten
* individuellen Titel-Zuweisungen

Dadurch entsteht:

```text
Realm A
→ Geschichte aus Entdeckungen, Events und Entdeckungsketten

Realm B
→ andere Geschichte aus ihren eigenen Entdeckungen und Events

Realm C
→ dritte Variante derselben Welt
```

Diese Individualität soll Spielern eine eigene Weltgeschichte geben, ohne dass jeder Realm technisch vollständig unterschiedliche Inhalte haben muss.

Wichtig:

> **Realm-Unterschiede sollen aus der Geschichte derselben Welt entstehen, nicht aus komplett verschiedenen Content-Listen.**

Realmübergreifender Spieler-Austausch wird ausdrücklich gewünscht.

Forum-Austausch und Community-Diskussion sind ausdrücklich erwünscht.

Sie sind kein Kernmechanismus, sondern eine gewünschte Community-Kultur.

---

# 12. Abgrenzung zu bestehenden Systemen

Diese Dokumentation verweist auf bestehende Andora-Systeme und ändert deren Grundprinzipien nicht.

Relevante Systeme:

* **Quest-System:** definiert Zustände, Ziele, serverseitige Prüfung und Client-Präsentation
* **quests_stories.md:** definiert Hauptgeschichte, Questdefinitionen, Belohnungen, KI-/Scene-Kopplungen
* **Ki-NPC.md / ai_system.md:** definiert NPC-Wissen, Informationsweitergabe, KI-Dialoge
* **Event-Matchmaking.md / Boss-System.md:** definiert instanzbasierte Gruppen- und Event-Abläufe
* **Expansions-Dokumente:** definieren spezifische Regionen, Rätsel-Event-Beispiele und wöchentliche Events

Diese Doku ergänzt diese Systeme, setzt ihnen nicht entgegen und verändert keine technische Autorität.

---

# 13. Referenzbibliothek (references)

Die folgende Fachliteratur unterstützt die konzeptionelle Grundlage nicht als technische Norm, sondern als Design-Referenz:

* `references/books/practicalgamedesign` – *Practical Game Design*
  * **Chapter 8 „Games and Stories" (ab PDF p. 199):** Storytelling, narrativer Fortschritt, Environmental Storytelling, Story als Welt-Auslöser. Relevante Inhalte für:
    * die vier Erzählebenen
    * Bücher als Lore-/Informationsquelle
    * Environment-Rätsel
    * Hinweise, die nicht als klassische Questmarker ausgedrückt werden
  * **Chapter 13 „Balancing" (ab PDF p. 349):** Kapitel über das Handling von optionalen Herausforderungen, versteckten Zielen und Erfolgen. Relevante Inhalte für:
    * versteckte Entdeckungen
    * optionale Belohnungen
    * Erfolge und Titel als Prestige, nicht als Pflicht
    * versteckte Erfolge als Belohnung für hohe Aufmerksamkeit

> Die andoranische Bindung an die oben genannten Prinzipien ist eine **Doku-/Design-Entscheidung**, keine technische Implementierung.

Die Referenzdokumente wurden geprüft und korrekt eingeordnet. Es handelt sich um konzeptionelle Fachliteratur, nicht um eine technische Schnittstelle.

---

# 14. Zusammenfassung

Andora soll so gestaltet werden, dass:

* Geschichte, Welt, Erkundung und RPG-Systeme ineinandergreifen
* Bücher, NPC-Dialoge, Ruinen und Landschaften als Quellen dienen
* kleine Geheimnisse existieren, ohne alle eine Pflicht zu sein
* Realm-Entdeckungen die Weltgeschichte selbst verändern
* Spieler individuell ihre Andora-Erfahrung formen
* keine technische Pflicht zu „allem finden" besteht

Die Welt soll nicht nur ein RPG-Spielplatz sein.
Sie soll eine Welt sein, die Geschichten erzählt, Spuren hinterlässt und sich durch die Handlungen der Spieler formt.

> **Zeigt die Welt mehr, als sie erklärt. Und belohnt Aufmerksamkeit.**
