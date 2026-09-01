# Andora – Politik- und Herrschaftssystem

## 1. Status

**Status:** 🟡 Konzept / späteres Spielsystem

Das Politik- und Herrschaftssystem verbindet Fraktionen, Gilden und später das PvP-System miteinander.

Spielergilden können innerhalb ihrer Fraktion politischen Einfluss aufbauen und politische Ämter übernehmen.

Das höchste vorgesehene Amt ist derzeit das **Königsamt einer Fraktion**.

Politische Macht soll jedoch niemals dauerhaft durch früheren Fortschritt gesichert werden können.

> **Politische Macht wird nicht einmal erspielt – sie muss erhalten werden.**

---

# 2. Grundidee

Gilden sammeln innerhalb ihrer Fraktion einen politischen Einfluss- beziehungsweise Statuswert.

Dieser Wert zeigt, wie stark die Gilde aktuell innerhalb ihrer Fraktion engagiert und politisch etabliert ist.

Einfluss kann beispielsweise durch:

* Stadtquests
* Fraktionsquests
* wöchentliche Aufgaben
* Unterstützung der Fraktion
* besondere Fraktionsereignisse

erworben werden.

Die genauen Quellen werden später mit dem Quest- und Fraktionssystem definiert.

---

# 3. Fester Maximalwert

Der politische Einfluss besitzt einen festen Maximalwert.

Beispiel:

```text id="plx6cw"
Maximaler Einfluss: 100

Gilde A: 100
Gilde B:  87
Gilde C:  61
```

Keine Gilde kann unbegrenzt Einfluss ansammeln.

Dadurch kann eine sehr alte Gilde keinen uneinholbaren Vorsprung gegenüber später gegründeten Gilden aufbauen.

---

# 4. Alte Gilden besitzen keinen unendlichen Vorsprung

Eine Gilde, die seit dem ersten Tag eines Servers existiert, kann maximal denselben Einflusswert erreichen wie eine jüngere Gilde.

Beispiel:

```text id="5f1sux"
Gilde A
seit Serverstart aktiv
Einfluss: 100

Gilde B
viel später gegründet
Einfluss: 100
```

Beide haben denselben aktuellen Maximalwert erreicht.

Die jahrelange Existenz von Gilde A erzeugt keinen zusätzlichen politischen Wert oberhalb des Maximums.

---

# 5. Amtierende Gilde behält bei Gleichstand die Macht

Erreichen mehrere Gilden gleichzeitig den maximalen Einflusswert, findet nicht automatisch ein Machtwechsel statt.

Beispiel:

```text id="o3h8sw"
Gilde A: 100 ← Königsgilde
Gilde B: 100
Gilde C:  84
```

Gilde A bleibt an der Macht.

Gilde B hat zwar denselben Einfluss erreicht, aber die bestehende Regierung wird bei einem Gleichstand nicht automatisch abgelöst.

Grundregel:

> **Bei gleichem Einfluss behält die amtierende Gilde ihr Amt.**

---

# 6. Einfluss muss erhalten werden

Der politische Einfluss einer amtierenden Gilde sinkt regelmäßig.

Vorgesehen ist derzeit ein **wöchentlicher Einflussverlust**.

Die amtierende Gilde muss deshalb weiterhin Aufgaben für ihre Stadt beziehungsweise Fraktion erledigen, um diesen Verlust auszugleichen.

Beispiel:

```text id="7p8zrq"
Gilde A: 100

Wöchentlicher Verlust:
-10

Erarbeiteter Einfluss:
+10

Neuer Wert:
100
```

Die Gilde hat ihre politische Position erfolgreich erhalten.

---

# 7. Nachlassende Aktivität

Erledigt die amtierende Gilde nicht genügend Aufgaben, sinkt ihr Einfluss.

Beispiel:

```text id="ggw6v2"
Gilde A: 100

Wöchentlicher Verlust:
-10

Erarbeiteter Einfluss:
+6

Neuer Wert:
96
```

Die Gilde verliert dadurch nicht sofort sämtliche politische Bedeutung.

Sie wird lediglich verwundbarer gegenüber konkurrierenden Gilden.

---

# 8. Machtübernahme

Besitzt eine andere Gilde einen höheren aktuellen Einfluss als die amtierende Gilde, kann sie deren politisches Amt übernehmen.

Beispiel:

```text id="29lf5h"
Vorher:

Gilde A: 100 ← Königsgilde
Gilde B: 100


Nächste Woche:

Gilde A: 96
Gilde B: 100

        ↓

Gilde B übernimmt das Königsamt.
```

Der Gleichstand zuvor reichte nicht für eine Übernahme.

Erst nachdem Gilde B tatsächlich einen höheren aktuellen Einfluss besitzt, findet der Machtwechsel statt.

---

# 9. Kein Reset der abgelösten Gilde

Eine Gilde verliert bei einem Machtwechsel nur ihr Amt.

Ihr bisheriger Einfluss wird **nicht auf null zurückgesetzt**.

Beispiel:

```text id="90d39q"
Gilde A: 96  ← verliert das Amt
Gilde B: 100 ← übernimmt

NICHT:

Gilde A: 0
```

Gilde A bleibt damit weiterhin eine politisch bedeutende Kraft.

---

# 10. Rückkehr an die Macht

Eine abgelöste Gilde kann ihren Einfluss erneut erhöhen.

Beispiel:

```text id="gwqjpu"
Gilde B: 100 ← Königsgilde
Gilde A:  96

        ↓

Gilde A erledigt Fraktionsaufgaben

        ↓

Gilde A: 100
Gilde B: 100

        ↓

Gilde B bleibt Königsgilde.
```

Gilde A muss nun darauf warten, dass Gilde B ihren Einfluss nicht vollständig halten kann.

Beispiel:

```text id="6dl8wr"
Gilde A: 100
Gilde B:  97 ← Königsgilde

        ↓

Gilde A besitzt höheren Einfluss

        ↓

Gilde A übernimmt wieder.
```

Dadurch können langfristige politische Rivalitäten zwischen Gilden entstehen.

---

# 11. Keine künstlichen politischen Resets

Das System benötigt keinen regelmäßigen vollständigen Reset aller politischen Werte.

Die Kombination aus:

* festem Maximalwert
* regelmäßigem Einflussverlust
* aktiver Wiedergewinnung
* Machtwechsel bei höherem Einfluss

sorgt dafür, dass politische Positionen erreichbar bleiben.

Vergangene Leistung bleibt relevant, garantiert aber keine ewige Herrschaft.

---

# 12. Macht bedeutet Aktivität

Eine Gilde kann sich nach Erreichen des Königsamtes nicht dauerhaft ausruhen.

Sie muss weiterhin etwas für ihre Fraktion leisten.

Grundprinzip:

> **Wer an der Macht bleiben möchte, muss für seine Fraktion aktiv bleiben.**

Das Königsamt ist damit kein dauerhafter Achievement-Titel.

Es ist eine aktuelle politische Position.

---

# 13. Stadt- und Fraktionsquests

Politischer Einfluss soll hauptsächlich durch Aktivitäten entstehen, die tatsächlich der jeweiligen Fraktion zugutekommen.

Dafür eignen sich insbesondere:

* Stadtquests
* wöchentliche Fraktionsquests
* Verteidigungsaufgaben
* Unterstützung von Fraktionsereignissen
* Versorgung
* regionale Aufgaben

Die konkreten Questtypen werden später entwickelt.

Das vorhandene Quest-System kann dafür verwendet werden.

Ein zweites paralleles politisches Quest-System ist nicht notwendig.

---

# 14. Politische Konkurrenz

Mehrere Gilden können gleichzeitig einen sehr hohen Einfluss besitzen.

Beispiel:

```text id="tawz8b"
Gilde A: 100 ← König
Gilde B: 100
Gilde C:  98
Gilde D:  91
```

Dadurch entsteht ein permanenter Wettbewerb.

Eine einzige schwächere Woche der Königsgilde kann bereits eine Machtübernahme ermöglichen.

---

# 15. Neue Fraktionen

Spätere Erweiterungen können neue Fraktionen nach Andora bringen.

Diese neuen Fraktionen besitzen zunächst noch keine jahrzehntelang etablierten politischen Machtstrukturen.

Dadurch entstehen neue Aufstiegsmöglichkeiten für:

* neue Spieler
* bestehende Spieler
* kleine Gilden
* neue Gilden
* etablierte Gilden aus anderen Fraktionen

Eine neue Fraktion ist damit nicht nur zusätzlicher Content.

Sie kann das politische Gleichgewicht der gesamten Welt verändern.

---

# 16. Wechsel etablierter Gilden

Eine mächtige Gilde einer bestehenden Fraktion kann versuchen, ihren Schwerpunkt auf eine neu hinzugefügte Fraktion zu verlagern.

Beispiel:

```text id="30q4mp"
Fraktion 1

Gilde A
→ Königsgilde
→ hoher Einfluss

        ↓

Erweiterung

        ↓

Fraktion 4 entsteht

        ↓

Gilde A versucht,
dort ebenfalls politische Macht aufzubauen.
```

Dafür müssen Mitglieder Zeit und Aktivität in die neue Fraktion investieren.

---

# 17. Machtvakuum durch Schwerpunktwechsel

Verlagert eine mächtige Gilde ihre Aktivität auf eine andere Fraktion, kann sie ihre bisherige politische Stellung vernachlässigen.

Beispiel:

```text id="pzt5dy"
Gilde A beherrscht Fraktion 1
        ↓
Gilde A konzentriert sich auf Fraktion 4
        ↓
weniger Aktivität in Fraktion 1
        ↓
Einflussverlust
        ↓
Gilde B bleibt in Fraktion 1 aktiv
        ↓
Gilde B überholt Gilde A
        ↓
Gilde B übernimmt das Königsamt
```

Dadurch kann eine neue Fraktion auch politische Veränderungen in älteren Gebieten auslösen.

---

# 18. Keine künstliche Besitzgarantie

Eine Gilde besitzt eine Fraktion nicht dauerhaft.

Auch eine ehemals dominante Gilde muss entscheiden, wo sie ihre Zeit und ihre Spieler einsetzt.

Das System muss deshalb nicht künstlich festlegen:

> Eine Gilde darf nur in einer einzigen Fraktion Macht besitzen.

Stattdessen können Aktivität, Fraktionszugehörigkeit und Einfluss dafür sorgen, dass politische Expansion automatisch Ressourcen und Aufmerksamkeit kostet.

Die genauen Regeln für Gilden mit Mitgliedern verschiedener Fraktionszugehörigkeiten werden später mit dem Gildensystem festgelegt.

---

# 19. Neue politische Chancen

Neue Fraktionen können besonders für Spieler interessant sein, die in einer etablierten Fraktion kaum noch Chancen auf ein hohes politisches Amt sehen.

Ein Spieler oder eine Gilde kann sich entscheiden:

```text id="8hr5zv"
Alte Fraktion

→ starke etablierte Konkurrenz
→ politische Ämter hart umkämpft

ODER

Neue Fraktion

→ neue politische Struktur
→ neue Gilden
→ neuer Wettbewerb
→ Chance auf politischen Aufstieg
```

Der Wechsel selbst unterliegt weiterhin den Regeln des Fraktionssystems.

Dazu gehören insbesondere Verrat, Beziehungsverlust und der erneute Aufbau von Vertrauen.

---

# 20. Das Königsamt

Das Königsamt ist derzeit als höchstes politisches Amt einer Fraktion vorgesehen.

Es soll nicht ausschließlich kosmetisch sein.

Der König beziehungsweise die herrschende Gilde erhält tatsächliche politische Möglichkeiten, die Auswirkungen auf die eigene Fraktion haben können.

Die konkreten Befugnisse werden später mit dem Politik- und PvP-System weiterentwickelt.

---

# 21. Fraktionskriege

Eine bereits vorgesehene königliche Befugnis ist die Möglichkeit, einer anderen Fraktion den Krieg zu erklären.

Grundsätzlich:

```text id="ex6r2z"
König Fraktion A
        ↓
Kriegserklärung
        ↓
Fraktion B
        ↓
Fraktionskrieg
        ↓
besondere PvP-/Kriegsregeln
```

Damit beeinflusst die politische Führung das tatsächliche Spielgeschehen ihrer Fraktion.

---

# 22. Fraktionskrieg ist Teil des späteren PvP-Systems

Die konkreten Regeln eines Fraktionskrieges werden noch nicht festgelegt.

Später zu entscheiden sind beispielsweise:

* Voraussetzungen einer Kriegserklärung
* mögliche Kosten
* Dauer
* Kriegsziele
* beteiligte Gebiete
* PvP-Regeln
* Siegbedingungen
* Niederlage
* Friedensschluss
* Belohnungen
* mögliche Konsequenzen

Diese Punkte gehören in das spätere PvP-/Fraktionskriegssystem.

---

# 23. Könige können die Spielwelt beeinflussen

Weil ein König politische Entscheidungen treffen kann, wird die Frage, welche Gilde an der Macht ist, auch für normale Mitglieder einer Fraktion relevant.

Unterschiedliche Gilden können unterschiedliche politische Schwerpunkte verfolgen.

Eine Gilde kann beispielsweise stärker auf PvP ausgerichtet sein.

Eine andere kann andere Interessen innerhalb ihrer Fraktion verfolgen.

Dadurch kann ein Machtwechsel spürbare Auswirkungen auf die gesamte Fraktion haben.

---

# 24. Politische Geschichte entsteht durch Spieler

Das System soll langfristig Geschichten ermöglichen, die nicht vollständig von den Entwicklern vorgeschrieben wurden.

Beispiel:

```text id="zcx76r"
Gilde A regiert Fraktion 1
        ↓
Fraktion 4 erscheint
        ↓
Gilde A versucht dort aufzusteigen
        ↓
Gilde B übernimmt Fraktion 1
        ↓
Gilde A scheitert möglicherweise in Fraktion 4
        ↓
neue politische Rivalitäten entstehen
```

Solche Entwicklungen entstehen aus den Entscheidungen und Aktivitäten der Spieler.

---

# 25. Verbindung zum Fraktionssystem

Das Politiksystem baut auf dem Fraktionssystem auf.

Dabei bleiben die dort festgelegten Grundregeln bestehen:

* Rasse und Fraktion sind unabhängig.
* Jede Rasse kann grundsätzlich jeder spielbaren Fraktion angehören.
* Fraktionswechsel sind möglich.
* Verrat besitzt Konsequenzen.
* alte Beziehungen werden beschädigt.
* Vertrauen einer neuen Fraktion muss aufgebaut werden.
* eine Rückkehr stellt alte Beziehungen nicht automatisch wieder her.

Politische Macht kann deshalb nicht einfach ohne Konsequenzen von einer Fraktion in eine andere übertragen werden.

---

# 26. Verbindung zum Quest-System

Stadt-, Wochen- und Fraktionsquests können politischen Einfluss erzeugen.

Dabei gilt weiterhin die bestehende Questarchitektur:

> **Lua beschreibt die Aufgabe. TypeScript prüft die Aufgabe. MariaDB merkt sich den Fortschritt. Godot zeigt ihn dem Spieler.**

Das Politiksystem verwendet das vorhandene Quest-System und erzeugt kein zweites Questframework.

---

# 27. Verbindung zum Gildensystem

Das Politiksystem benötigt später das Gildensystem für:

* Gildenmitgliedschaft
* Gildenaktivität
* politischen Einfluss
* Ämter
* Machtübernahmen
* politische Berechtigungen

Die genaue interne Organisation einer Gilde wird separat entwickelt.

---

# 28. Verbindung zum PvP-System

Fraktionskriege bilden eine direkte Verbindung zwischen Politik und PvP.

Das Politiksystem entscheidet beispielsweise:

> Ein gültiger Fraktionskrieg wurde erklärt.

Das PvP-System entscheidet anschließend:

> Welche konkreten Kampfregeln gelten während dieses Krieges?

Dadurch bleiben politische Entscheidung und Kampfmechanik getrennte Systeme.

---

# 29. Erweiterbarkeit

Das System soll nicht auf exakt drei Fraktionen festgelegt werden.

Spätere Erweiterungen können weitere Fraktionen hinzufügen.

Ebenso sollen spätere politische Ämter unterhalb des Königs möglich bleiben, sofern sie für Andora sinnvoll sind.

Das grundlegende Prinzip bleibt:

```text id="j0ctbe"
Fraktion
   ↓
Gildenaktivität
   ↓
politischer Einfluss
   ↓
Ämter
   ↓
politische Entscheidungen
   ↓
Auswirkungen auf die Welt
```

---

# 30. Zentrale Grundsätze

> **Politische Macht wird nicht einmal erspielt – sie muss erhalten werden.**

> **Politischer Einfluss besitzt einen festen Maximalwert. Alte Gilden können dadurch keinen uneinholbaren Vorsprung ansammeln.**

> **Bei gleichem Einfluss behält die amtierende Gilde ihr Amt.**

> **Eine andere Gilde übernimmt das Amt erst, wenn ihr aktueller Einfluss höher ist als der der amtierenden Gilde.**

> **Eine abgelöste Gilde verliert ihr Amt, aber nicht ihren gesamten Einfluss. Sie kann sich wieder nach oben arbeiten.**

> **Wer an der Macht bleiben möchte, muss weiterhin für seine Fraktion aktiv sein.**

> **Neue Fraktionen schaffen neue politische Chancen und können gleichzeitig das Machtgleichgewicht bestehender Fraktionen verändern.**

> **Politische Ämter besitzen tatsächliche spielerische Bedeutung. Ein König kann Entscheidungen treffen, die seine gesamte Fraktion betreffen.**

> **Könige können Fraktionskriege ausrufen; die konkreten Kriegsregeln gehören in das spätere PvP-System.**

> **Die politische Geschichte Andoras soll nicht nur geschrieben werden – sie soll durch die Spieler entstehen.**

# Ergänzung – Fraktionsdiplomatie und gemischte Gruppen

## 31. Fraktionen befinden sich grundsätzlich im Frieden

Die Zugehörigkeit zu unterschiedlichen Fraktionen macht Spieler nicht automatisch zu Gegnern.

Solange zwischen zwei Fraktionen kein Krieg besteht, herrscht zwischen ihnen Frieden.

Spieler verschiedener friedlicher Fraktionen können grundsätzlich:

* gemeinsam questen
* Gruppen bilden
* Dungeons spielen
* World Events bestreiten
* miteinander handeln
* miteinander kommunizieren
* sich gegenseitig unterstützen

Fraktionen sind damit zunächst politische Zugehörigkeiten und keine permanenten PvP-Teams.

> **Unterschiedliche Fraktionen bedeuten nicht automatisch Feindschaft.**

---

## 32. Fraktionskrieg verändert die diplomatische Beziehung

Wird zwischen zwei Fraktionen ein gültiger Fraktionskrieg ausgerufen, ändert sich ihre diplomatische Beziehung.

Beispiel:

```text
Vorher:

Fraktion A ── Frieden ── Fraktion B

Nach Kriegserklärung:

Fraktion A ── Krieg ── Fraktion B
```

Die Spieler dieser beiden Fraktionen gelten während des Krieges nach den dafür vorgesehenen PvP-Regeln als feindlich.

Die konkreten Kampf-, Gebiets- und Schutzregeln werden später im PvP-/Fraktionskriegssystem definiert.

---

## 33. Andere Fraktionen bleiben unabhängig

Ein Krieg zwischen zwei Fraktionen macht nicht automatisch alle anderen Fraktionen zu Beteiligten.

Beispiel:

```text
Fraktion A ⚔ Fraktion B

Fraktion A ☮ Fraktion C
Fraktion B ☮ Fraktion C
```

Fraktion C bleibt gegenüber beiden Parteien friedlich.

Ein Spieler aus Fraktion C darf deshalb nicht allein aufgrund des Krieges zwischen A und B in deren Konflikt eingreifen.

> **Ein Fraktionskrieg betrifft nur die tatsächlich beteiligten Fraktionen.**

---

## 34. Gruppenzugehörigkeit verändert keine Diplomatie

Eine Gruppe überschreibt niemals den diplomatischen Zustand ihrer Mitglieder.

Beispiel:

```text
Gruppe:

Spieler A1 → Fraktion A
Spieler C1 → Fraktion C
Spieler C2 → Fraktion C

Außerhalb der Gruppe:

Spieler B1 → Fraktion B

Diplomatie:

A ⚔ B
A ☮ C
B ☮ C
```

Daraus folgt:

```text
A1 ↔ B1 = feindlich

C1 ↔ A1 = friedlich
C1 ↔ B1 = friedlich

C2 ↔ A1 = friedlich
C2 ↔ B1 = friedlich
```

C1 und C2 werden nicht zu Gegnern von B1, nur weil sie mit A1 in einer Gruppe sind.

> **Eine Gruppe verändert keine diplomatischen Beziehungen.**

---

## 35. Neutralität während eines Fraktionskampfes

Beginnt A1 einen Fraktionskampf gegen B1, bleiben die Gruppenmitglieder aus Fraktion C neutral.

Sie dürfen nicht indirekt auf einer Seite des Krieges kämpfen.

Während dieses Konflikts können sie deshalb den beteiligten A1 nicht durch Gruppenmechaniken unterstützen.

Dies betrifft insbesondere:

* direkte Heilung
* Gruppenheilung
* Gruppen-AoE-Heilung
* Buffs
* Schutzfähigkeiten
* Cleanse-Effekte
* Crowd-Control-Unterstützung
* kampfrelevante Gruppenfähigkeiten
* Companion-Unterstützung

Damit kann eine neutrale Fraktion nicht als indirekte Unterstützung einer Kriegspartei verwendet werden.

---

## 36. Temporäre Trennung vom aktiven Gruppenverbund

Beginnt ein Gruppenmitglied einen Fraktionskampf, an dem andere Gruppenmitglieder aufgrund ihrer Fraktionszugehörigkeit nicht beteiligt sind, wird der kämpfende Spieler für diese neutralen Mitglieder **temporär aus dem aktiven Gruppenverbund genommen**.

Die eigentliche Gruppe wird dabei nicht aufgelöst.

Beispiel:

```text
Normale Gruppe:

[A1 – Fraktion A]
[C1 – Fraktion C]
[C2 – Fraktion C]

        ↓

A1 beginnt Fraktions-PvP gegen B1

        ↓

Aus Sicht von C1 und C2:

[A1 – ausgegraut]
[C1 – aktiv]
[C2 – aktiv]
```

A1 bleibt technisch Mitglied der persistenten Gruppe, zählt für C1 und C2 während dieses Kampfes jedoch nicht als aktives Gruppenmitglied.

---

## 37. Darstellung im Gruppeninterface

Ein temporär vom aktiven Gruppenverbund getrenntes Gruppenmitglied bleibt im Gruppenfenster sichtbar.

Sein Eintrag wird jedoch **ausgegraut**.

Damit erkennt der Spieler:

* das Gruppenmitglied gehört weiterhin zur Gruppe
* es befindet sich momentan in einem Fraktionskonflikt
* der eigene Charakter ist an diesem Konflikt nicht beteiligt
* das Gruppenmitglied kann momentan nicht unterstützt werden

Der Spieler wird nicht vollständig aus der Gruppenanzeige entfernt.

---

## 38. Nicht anvisierbar für neutrale Gruppenunterstützung

Während der temporären Trennung kann das kämpfende Gruppenmitglied von neutralen Gruppenmitgliedern nicht als Ziel kampfrelevanter Unterstützungsfähigkeiten ausgewählt werden.

Dadurch verhindert das System bereits bei der Zielauswahl ungültige Aktionen.

Der Client zeigt den Zustand an.

Die endgültige Entscheidung darüber, ob eine Aktion erlaubt ist, trifft jedoch immer der Server.

---

## 39. Gruppen-AoE berücksichtigt nur aktive Gruppenmitglieder

Gruppenfähigkeiten dürfen die Neutralitätsregel nicht umgehen.

Beispiel:

```text
C1 wirkt:

"Alle Gruppenmitglieder im Umkreis heilen"

A1 befindet sich im Fraktionskampf.

        ↓

C1 wird geheilt
C2 wird geheilt
A1 wird NICHT geheilt
```

A1 zählt für diese Fähigkeit temporär nicht zum aktiven Gruppenverbund von C1.

Dadurch müssen Gruppen-AoE-Fähigkeiten nicht mit einer Sonderregel pro Skill versehen werden.

Die grundlegende Gruppen- und Diplomatieregel entscheidet bereits, welche Charaktere gültige Ziele sind.

---

## 40. Kein tatsächlicher Gruppenaustritt

Die temporäre Trennung ist **kein echter Gruppenaustritt**.

Folgende Zustände bleiben grundsätzlich erhalten:

* Gruppenmitgliedschaft
* Gruppenleiter
* Gruppeneinladung
* bestehende Gruppenstruktur
* Dungeon-/Finder-Zusammenhang
* sonstige persistente Gruppendaten

Lediglich die kampfrelevante Verbindung zwischen Kriegspartei und neutralem Gruppenmitglied wird vorübergehend ausgesetzt.

---

## 41. Automatische Wiederherstellung

Sobald der Fraktionskampf für das betreffende Gruppenmitglied serverseitig beendet ist, wird die temporäre Trennung automatisch aufgehoben.

Beispiel:

```text
A1 beendet Fraktionskampf
        ↓
Kampfstatus wird vom Server aufgehoben
        ↓
A1 wird wieder aktives Gruppenmitglied
        ↓
Gruppenanzeige wieder normal
        ↓
Anvisieren wieder möglich
        ↓
Heilungen und Gruppenfähigkeiten funktionieren wieder
```

Eine erneute Gruppeneinladung ist nicht notwendig.

---

## 42. Serverautorität

Der Client entscheidet niemals selbst, ob zwei Charaktere aufgrund ihrer Fraktionen gegeneinander kämpfen oder sich unterstützen dürfen.

Der Server berücksichtigt unter anderem:

```text
Fraktion Spieler A
        +
Fraktion Spieler B
        +
aktueller diplomatischer Zustand
        +
aktiver Fraktionskampf
        +
Gruppenzugehörigkeit
        +
Neutralitätsstatus
        ↓
gültige / ungültige Aktion
```

Godot stellt das Ergebnis lediglich dar, beispielsweise durch:

* ausgegraute Gruppenmitglieder
* nicht anwählbare Ziele
* feindliche Darstellung
* neutrale Darstellung

> **Der Server entscheidet über Feindschaft und Neutralität – der Client zeigt sie nur an.**

---

## 43. Zentrale Regeln

> **Fraktionen sind im normalen Zustand friedlich und können miteinander PvE spielen.**

> **Erst ein aktiver Fraktionskrieg macht die beteiligten Fraktionen nach den vorgesehenen PvP-Regeln zu Gegnern.**

> **Unbeteiligte Fraktionen bleiben neutral.**

> **Eine Gruppe verändert keine diplomatischen Beziehungen.**

> **Neutrale Gruppenmitglieder dürfen nicht indirekt in einen Fraktionskampf eingreifen.**

> **Ein an einem Fraktionskampf beteiligtes Gruppenmitglied wird für neutrale Gruppenmitglieder temporär aus dem aktiven Gruppenverbund genommen, bleibt aber Mitglied der eigentlichen Gruppe.**

> **Währenddessen wird das Gruppenmitglied ausgegraut und ist für kampfrelevante Unterstützung nicht anvisierbar.**

> **Gruppen-AoE, Heilungen, Buffs und andere Gruppenfähigkeiten berücksichtigen nur den aktuell aktiven Gruppenverbund.**

> **Nach Ende des Fraktionskampfes wird die normale Gruppenverbindung automatisch wiederhergestellt.**

## 44. Söldner-NPCs im Fraktionskampf

Söldner-NPCs und andere einem Spieler fest zugeordnete Kampfgefährten werden im Fraktionskampf anders behandelt als neutrale Spieler.

Ein Söldner besitzt innerhalb eines solchen Kampfes nicht unabhängig vom Auftraggeber einen neutralen Gruppenstatus.

Stattdessen folgt er dem aktuellen Kampfstatus des Spielers, dem er zugewiesen ist.

Beispiel:

```text
Gruppe:

A1 – Fraktion A
└── Söldner Borin

C1 – Fraktion C
└── Söldner Mira

Diplomatie:

Fraktion A ⚔ Fraktion B
Fraktion C ☮ Fraktion A
Fraktion C ☮ Fraktion B
```

Beginnt A1 einen Fraktionskampf gegen einen Spieler aus Fraktion B, darf sein Söldner Borin A1 weiterhin im Kampf unterstützen.

Borin kann entsprechend seiner Rolle beispielsweise:

* Gegner angreifen
* A1 verteidigen
* A1 heilen
* A1 buffen
* vorgesehene Kampffähigkeiten einsetzen

Der Söldner übernimmt damit für diesen Kampf die Kampfseite seines zugewiesenen Spielers.

> **Ein zugewiesener Söldner-NPC folgt im Fraktionskampf der Kampfseite seines Auftraggebers.**

---

### Darstellung für neutrale Gruppenmitglieder

Da C1 gegenüber Fraktion A und Fraktion B neutral ist, wird C1 nicht in den Fraktionskampf hineingezogen.

Aus Sicht von C1 wird deshalb nicht nur A1, sondern auch dessen Söldner Borin temporär aus dem aktiven Gruppenverbund genommen.

```text
Aus Sicht von C1:

[A1]          ← ausgegraut
[  Borin]     ← ausgegraut

[C1]          ← aktiv
[  Mira]      ← aktiv
```

A1 und Borin bleiben weiterhin Mitglieder beziehungsweise zugeordnete Einheiten der Gruppe.

Sie sind für C1 während des Fraktionskampfes jedoch keine aktiven Gruppen- oder Unterstützungsziele.

---

### Söldner bleibt trotzdem im Kampf aktiv

Das Ausgrauen bedeutet nicht, dass Borin seinen Kampf beendet.

Borin kann weiterhin:

```text
Borin
  ↓
folgt A1
  ↓
A1 befindet sich im Fraktionskampf
  ↓
Borin unterstützt A1
  ↓
Borin kann gegen die Kriegsgegner von A1 kämpfen
```

Die temporäre Gruppentrennung gilt lediglich gegenüber den neutralen Gruppenmitgliedern.

---

### Keine Unterstützung durch neutrale Gruppenmitglieder

C1 kann während dieses Kampfes weder A1 noch Borin durch kampfrelevante Gruppenmechaniken unterstützen.

Dies betrifft beispielsweise:

* direkte Heilungen
* Gruppenheilungen
* AoE-Gruppenheilungen
* Buffs
* Schutzfähigkeiten
* Cleanse
* sonstige kampfrelevante Gruppenfähigkeiten

Da Borin gemeinsam mit A1 temporär aus dem aktiven Gruppenverbund von C1 genommen wurde, wird er von entsprechenden Gruppenfähigkeiten ebenfalls nicht erfasst.

---

### Söldner neutraler Spieler

Der Söldner eines neutralen Spielers übernimmt nicht automatisch die Kampfseite eines anderen Gruppenmitglieds.

Im Beispiel bleibt Mira an C1 gebunden:

```text
C1 bleibt neutral
        ↓
Mira folgt C1
        ↓
Mira bleibt ebenfalls neutral
```

Mira darf deshalb nicht stellvertretend für C1 in den Fraktionskampf zwischen A und B eingreifen.

Dadurch kann die Neutralitätsregel nicht über einen Söldner umgangen werden.

---

### Ende des Fraktionskampfes

Sobald der Fraktionskampf für A1 serverseitig beendet ist, wird die temporäre Trennung auch für seinen Söldner aufgehoben.

```text
Fraktionskampf endet
        ↓
A1 wieder aktives Gruppenmitglied
        ↓
Borin ebenfalls wieder aktiv
        ↓
beide nicht mehr ausgegraut
        ↓
normale Gruppenmechaniken funktionieren wieder
```

Eine erneute Gruppeneinladung oder Söldnerzuweisung ist nicht erforderlich.

---

### Serverautorität

Die Zuordnung zwischen Spieler und Söldner wird vom Server verwaltet.

Der Server bestimmt:

```text
Söldner
   ↓
zugewiesener Spieler
   ↓
dessen aktueller Kampfstatus
   ↓
Kampfseite des Söldners
   ↓
gültige Gegner und Unterstützungsziele
```

Der Söldner beziehungsweise seine KI darf diese Kampfzugehörigkeit nicht selbstständig verändern.

Insbesondere darf die KI keine eigene Fraktionsentscheidung treffen oder aufgrund eines Dialogs die Kampfseite wechseln.

---

### Grundregeln

> **Ein Söldner-NPC folgt im Fraktionskampf dem Kampfstatus des Spielers, dem er zugewiesen ist.**

> **Wird der Spieler für neutrale Gruppenmitglieder ausgegraut, wird auch sein zugewiesener Söldner ausgegraut.**

> **Spieler und Söldner bleiben Bestandteil der Gruppe, zählen für neutrale Gruppenmitglieder während des Konflikts jedoch nicht zum aktiven Gruppenverbund.**

> **Der Söldner darf seinen zugewiesenen Spieler weiterhin aktiv im Fraktionskampf unterstützen.**

> **Der Söldner eines neutralen Spielers bleibt gemeinsam mit seinem Spieler neutral und darf nicht stellvertretend in den Fraktionskrieg eingreifen.**

> **Nach Ende des Fraktionskampfes werden Spieler und Söldner automatisch wieder vollständig in den aktiven Gruppenverbund aufgenommen.**


## 45. Gruppenfähigkeiten und Zielermittlung während paralleler Kämpfe

Die temporäre Trennung eines Gruppenmitglieds aus dem aktiven Gruppenverbund gilt auch für die Zielermittlung von Heilungen, Buffs und anderen Gruppenfähigkeiten.

Gruppenfähigkeiten wirken ausschließlich auf Charaktere, die aus Sicht des Auslösers aktuell gültige aktive Gruppenmitglieder sind.

Beispiel:

```text
Gruppe:

A1 – Fraktion A → befindet sich im Fraktions-PvP
C1 – Fraktion C → befindet sich im PvE
C2 – Fraktion C → Heiler, befindet sich im PvE

Aus Sicht von C2:

A1 → ausgegraut / inaktiv
C1 → aktives Gruppenmitglied
C2 → aktives Gruppenmitglied
```

C1 besitzt nur noch wenig Lebenspunkte und C2 verwendet eine Gruppenheilung.

```text
Gruppenheilung von C2
        ↓
Server ermittelt aktive Gruppenmitglieder
        ↓
C1 ✓ → wird geheilt
C2 ✓ → wird geheilt
A1 ✗ → wird vollständig ignoriert
```

Dabei spielt es keine Rolle, wie nahe A1 am Heiler steht.

Auch wenn A1:

* unmittelbar neben C2 steht
* nur noch sehr wenig Leben besitzt
* geometrisch innerhalb des AoE-Bereichs steht

erhält er keine Heilung.

Dasselbe gilt für einen A1 zugewiesenen Söldner-NPC, solange dieser gemeinsam mit A1 für C1 und C2 aus dem aktiven Gruppenverbund ausgegraut ist.

> **Gruppenfähigkeiten wirken ausschließlich auf Mitglieder des aktuell aktiven Gruppenverbunds. Temporär ausgegraute Mitglieder und deren zugewiesene Söldner werden bei der Zielermittlung vollständig ignoriert.**

---

## 46. Offensive AoE-Fähigkeiten

Eine offensive AoE-Fähigkeit trifft nicht automatisch alle Charaktere innerhalb ihres Wirkungsbereichs.

Der Wirkungsbereich bestimmt zunächst lediglich, **welche möglichen Ziele räumlich erreicht werden**.

Anschließend entscheidet der Server für jedes mögliche Ziel, ob dieses aus Sicht des Auslösers tatsächlich ein gültiges feindliches Ziel ist.

Grundprinzip:

```text
AoE wird ausgelöst
       ↓
räumliche Ziele bestimmen
       ↓
für jedes Ziel:
Ist es für den Auslöser feindlich?
       ↓
JA  → gültiges Schadensziel
NEIN → ignorieren
```

> **AoE bestimmt den Bereich – die Zielregeln bestimmen, wer darin tatsächlich getroffen wird.**

---

## 47. Neutraler Spieler verursacht keinen Schaden an Kriegsparteien

Beispiel:

```text
Diplomatie:

Fraktion A ⚔ Fraktion B

Fraktion C ☮ Fraktion A
Fraktion C ☮ Fraktion B
```

Am selben Ort befinden sich:

```text
A1 → Fraktion A
B1 → Fraktion B
C1 → Fraktion C
C2 → Fraktion C / Magier

sowie mehrere feindliche PvE-Mobs
```

A1 und B1 befinden sich miteinander im Fraktions-PvP.

C1 und C2 kämpfen gleichzeitig gegen die PvE-Mobs.

C2 verwendet einen offensiven Flächenzauber:

```text
AoE von C2

Mob 1 → feindlich gegenüber C2 → Schaden ✓
Mob 2 → feindlich gegenüber C2 → Schaden ✓

A1    → friedlich gegenüber C2 → kein Schaden
B1    → friedlich gegenüber C2 → kein Schaden
C1    → friedlich gegenüber C2 → kein Schaden
```

A1 und B1 sind zwar **untereinander Feinde**, aber keiner von beiden ist ein Feind von C2.

Ihre gegenseitige Feindschaft wird nicht auf C2 übertragen.

> **Die Feindschaft zweier Charaktere macht diese nicht automatisch zu Feinden eines dritten Charakters.**

---

## 48. Parallele PvE- und PvP-Kämpfe

Durch diese Zielregeln können am selben Ort mehrere unterschiedliche Kämpfe stattfinden, ohne dass sie sich automatisch miteinander vermischen.

Beispiel:

```text
A1 ⚔ B1
Fraktions-PvP

direkt daneben:

C1 + C2 ⚔ PvE-Mobs
PvE
```

C1 und C2 können:

* den Fraktionskampf beobachten
* ihre PvE-Quest fortsetzen
* Mobs bekämpfen
* sich gegenseitig heilen
* ihre normalen Gruppenfähigkeiten verwenden

Sie werden durch den Fraktionskampf von A1 nicht selbst in PvP versetzt.

---

## 49. Neutrale Spieler bleiben vollständig PvE-fähig

Die temporäre Trennung eines Gruppenmitglieds bedeutet keine Einschränkung der normalen Spielaktivität neutraler Gruppenmitglieder.

Wenn A1 Fraktions-PvP betreibt, können C1 und C2 unabhängig davon:

* weiter questen
* Mobs bekämpfen
* Ressourcen sammeln
* miteinander kämpfen und unterstützen
* den PvP-Kampf beobachten
* weiterziehen

Lediglich die direkte oder indirekte Beteiligung am Fraktionskampf ist ausgeschlossen.

> **Ein Fraktionskampf eines Gruppenmitglieds zwingt neutrale Gruppenmitglieder nicht dazu, ihre eigenen Aktivitäten zu unterbrechen.**

---

## 50. Einheitliche Zielprüfung

Die Regeln sollen nicht für jede einzelne Fähigkeit erneut als Sonderfall implementiert werden.

Stattdessen verwendet das Kampfsystem eine gemeinsame serverseitige Zielprüfung.

Vereinfacht:

```text
Fähigkeit
   ↓
mögliche Ziele
   ↓
Zieltyp prüfen
   ↓
┌─────────────────────────────┐
│ offensive Fähigkeit         │
│ → gültiges feindliches Ziel │
│                             │
│ Gruppenfähigkeit            │
│ → aktives Gruppenmitglied   │
│                             │
│ unterstützende Fähigkeit    │
│ → gültiges Supportziel      │
└─────────────────────────────┘
   ↓
gültige Ziele erhalten Effekt
```

Damit muss eine einzelne Fähigkeit nicht selbst das komplette Fraktions-, Gruppen- und Kriegssystem verstehen.

Das Fraktions- und Gruppensystem liefert dem Kampfsystem den gültigen Beziehungs- beziehungsweise Aktivstatus.

Das Kampfsystem verwendet diesen Status anschließend für seine Zielermittlung.

---

## 51. Serverautorität

Die Zielprüfung erfolgt immer serverseitig.

Der Client darf beispielsweise einen großen Feuer-AoE darstellen, entscheidet aber nicht selbst, welche Charaktere dadurch Schaden erhalten.

Der Server berücksichtigt:

```text
Auslöser
   +
mögliches Ziel
   +
Fraktionsbeziehung
   +
diplomatischer Zustand
   +
aktueller Kampfstatus
   +
aktiver Gruppenstatus
   +
Fähigkeitstyp
   ↓
gültiges / ungültiges Ziel
```

Godot stellt lediglich das Ergebnis dar.

---

## 52. Zentrale Kampfregel

> **Eine Fähigkeit wirkt nicht auf einen Charakter, nur weil dieser sich räumlich innerhalb ihres Wirkungsbereichs befindet. Der Server entscheidet anhand der aktuellen Beziehung zwischen Auslöser und Ziel, ob der Charakter ein gültiges Ziel der Fähigkeit ist.**

Dadurch können PvE, Fraktions-PvP und neutrale Spieler gleichzeitig am selben Ort existieren, ohne dass ihre Kämpfe automatisch miteinander vermischt werden.
