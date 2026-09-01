# Andora – Boss-System

## 1. Einordnung

Das Boss-System baut direkt auf dem allgemeinen Kampfsystem von Andora auf.

Grundlegende Regeln wie Anvisieren, Aggro, Gruppenrollen, Fähigkeiten und Tod bleiben auch bei Bosskämpfen bestehen.

Raidbosse werden separat behandelt, da ihre Kämpfe innerhalb eigener Raidinstanzen stattfinden.

---

# 2. Boss-Claim

Gebiets- und Dungeonbosse verwenden ein Claim-System.

Der Spieler bzw. die Gruppe, die einem freien Boss **als Erstes Schaden zufügt**, erhält den Claim auf diesen Boss.

Ab diesem Zeitpunkt gehört die Begegnung dieser Gruppe.

## Darstellung für andere Spieler

Für Spieler außerhalb der Claim-Gruppe wird der Boss grau dargestellt.

Andere Spieler können den Boss weiterhin:

* anvisieren
* angreifen
* ihm Schaden zufügen

Sie erhalten für diesen Boss jedoch **keine Belohnungen**.

Dazu gehören insbesondere:

* keine XP
* kein Loot
* keine sonstigen Bossbelohnungen

Entscheidend ist der ursprüngliche Claim und nicht, welche Gruppe später den meisten Schaden verursacht.

---

# 3. Verlust des Claims

Scheitert die ursprüngliche Gruppe vollständig oder bricht sie den Kampf ab, wird der Boss nicht von einer anderen Gruppe übernommen.

Stattdessen beginnt der **Boss-Reset**.

Der Boss:

1. beendet den aktuellen Kampf,
2. ignoriert weitere Angriffe,
3. kehrt zu seiner Ausgangsposition zurück,
4. wird vollständig zurückgesetzt,
5. verliert den bisherigen Claim.

Während des Rückwegs können andere Spieler den Boss weiterhin angreifen und ihm Schaden zufügen.

Der Boss reagiert während des Resets jedoch nicht darauf und beginnt keinen neuen Kampf.

Sollte der Boss während seines Resets trotzdem getötet werden, erhält **niemand Loot, XP oder andere Belohnungen**.

Erst nachdem der vollständige Reset abgeschlossen wurde, ist der Boss wieder frei und kann neu geclaimt werden.

Damit gelten die Zustände:

**Frei → Geclaimt/Kampf → Reset → Frei**

---

# 4. Dungeonbosse

Für Dungeonbosse gelten grundsätzlich dieselben Claim- und Resetregeln wie für Gebietsbosse.

Ein Dungeonboss bildet daher kein grundsätzlich anderes Kampfsystem.

Spezielle Eigenschaften einzelner Dungeons können später separat definiert werden.

---

# 5. Raidbosse

Raidbosse sind vom normalen Boss-Claim-System ausgenommen.

Raidbosskämpfe finden innerhalb einer eigenen Raidinstanz statt.

Durch die Instanz ist bereits eindeutig festgelegt, welcher Raid zu diesem Bosskampf gehört.

Die Regeln für Raidbosse und Raidinstanzen werden deshalb separat im Raid-System festgelegt.

---

# 6. Respawn von Bossen

Bosse können eine feste Respawnzeit besitzen.

Die Respawnzeit wird beim jeweiligen Boss hinterlegt und muss nicht für alle Bosse identisch sein.

Ein typischer Boss könnte beispielsweise nach:

**60 Minuten**

wieder erscheinen.

Der Respawn-Timer beginnt nach dem tatsächlichen Tod des Bosses.

Ein normaler Boss-Reset startet keinen Respawn-Timer, da der Boss dabei nicht gestorben ist.

---

# 7. Leichte Gebietsbosse

Leichte Gebietsbosse erscheinen direkt in der offenen Welt.

Nach ihrem Tod können sie nach ihrer festgelegten Respawnzeit erneut erscheinen.

Diese Bosse eignen sich insbesondere für **Questreihen**.

Dadurch kann ein Boss beispielsweise als Zwischengegner oder Abschluss einer längeren Questreihe dienen, ohne dass Spieler zunächst eine versteckte Spawnmechanik auslösen müssen.

„Leicht“ beschreibt dabei vor allem die Zugänglichkeit des Bosses und bedeutet nicht automatisch, dass jeder dieser Bosse alleine besiegt werden kann.

---

# 8. Starke versteckte Gebietsbosse

Starke Gebietsbosse können vollständig in der offenen Welt versteckt werden.

Ihre Existenz wird dem Spieler nicht durch das Interface verraten.

Es gibt:

* keinen Questmarker
* keinen Bossmarker
* keinen Fortschrittsbalken
* keine Eventanzeige
* keine Ankündigung
* keine Erklärung der Spawnbedingungen

Ein Spieler, der in einem Gebiet lediglich Gegner für XP bekämpft, soll nicht wissen, dass dort überhaupt ein Boss erscheinen kann.

Diese Bosse sind Teil der Erkundung von Andora.

---

# 9. „Wellen des Schmerzes“

**„Wellen des Schmerzes“** ist die interne Entwicklerbezeichnung für eine mögliche Spawnmechanik starker versteckter Gebietsbosse.

Der Begriff wird dem Spieler nicht angezeigt.

In einem begrenzten Bereich befinden sich zunächst normale Gegnergruppen.

Werden die notwendigen Gegner besiegt, kann sich die Zusammensetzung der nachfolgenden Spawns verändern.

Über mehrere solcher Stufen entwickelt sich das Gebiet weiter, bis schließlich die Bedingungen für den versteckten Boss erfüllt wurden.

Eine mögliche Entwicklung könnte beispielsweise sein:

**Normale Gegnergruppen
→ veränderte Gegnergruppen
→ stärkere Gegner
→ Named-Hauptmänner
→ versteckter Gebietsboss**

Die genaue Anzahl und Zusammensetzung dieser Stufen ist nicht global festgelegt.

---

# 10. Unterschiedliche Gegner pro Welle

Eine neue Welle muss nicht lediglich stärkere Versionen derselben Gegner enthalten.

Jede Welle besitzt ihre eigene Gegnerzusammensetzung.

Dabei können sich verändern:

* Gegnerart
* Anzahl der Gegner
* Stärke
* Rollen
* Gruppenzusammensetzung
* Named-Gegner

Beispielsweise könnte eine Spawnkette verwenden:

**Welle 1:** Eber

**Welle 2:** Dachse

**Welle 3:** Wölfe

**Welle 4:** stärkere oder seltenere Kreaturen

**Welle 5:** Named-Kreaturen

**Abschluss:** versteckter Gebietsboss

Eine andere Spawnkette könnte dagegen dieselbe Gegnerfamilie verwenden und lediglich deren Rollen verändern.

---

# 11. Unterschiedliche Taktiken

Die Wellen können so gestaltet werden, dass Spieler ihre Kampftaktik verändern müssen.

Beispiel:

**Welle 1:** ausschließlich Nahkämpfer

**Welle 2:** Nahkämpfer und Bogenschützen

**Welle 3:** Nahkämpfer, Bogenschützen und Heiler

**Welle 4:** stärkere Kombinationen und ein Named-Hauptmann

Weitere Wellen können diese Mechaniken neu kombinieren.

Der Schwierigkeitsgrad steigt dadurch nicht ausschließlich über höhere HP- oder Schadenswerte.

Neue Gegnerarten und Gruppenzusammenstellungen können neue Zielprioritäten und andere Vorgehensweisen erfordern.

---

# 12. Keine erkennbare globale Wellenstruktur

Die „Wellen des Schmerzes“ besitzen bewusst **kein global einheitliches Muster**.

Spieler sollen nach der Entdeckung eines versteckten Bosses nicht automatisch wissen, wie alle anderen versteckten Bosse gefunden werden.

Anzahl, Gegnerarten, Stärke, Rollen und Zusammensetzung können bei jeder Spawnkette unterschiedlich sein.

Auch innerhalb einer einzelnen Kette müssen Veränderungen nicht immer nach demselben Schema erfolgen.

Dadurch ist für Spieler nicht eindeutig erkennbar, ob ein Respawn tatsächlich Teil einer versteckten Bosskette ist.

Ein Wechsel der Gegner bedeutet nicht automatisch:

**„Das war eine Welle.“**

Stärkere Gegner bedeuten ebenfalls nicht automatisch:

**„Wir lösen gerade einen Boss aus.“**

Und ein Named-Gegner muss nicht zwangsläufig bedeuten:

**„Als Nächstes kommt der Boss.“**

Ein vollkommen normaler Respawn kann aus Sicht des Spielers genauso aussehen wie eine tatsächliche Stufe einer versteckten Bosskette.

---

# 13. Entdeckung durch normales Spielen

Versteckte Gebietsbosse sollen auch vollkommen zufällig entdeckt werden können.

Eine Gruppe könnte beispielsweise lediglich einen geeigneten Ort zum Leveln suchen.

Sie findet ein Gebiet mit vielen Gegnergruppen und beginnt dort zu grinden.

Nach einiger Zeit verändern sich möglicherweise die Gegner.

Die Gruppe macht weiter.

Später erscheinen vielleicht stärkere Gegner oder ein Named-Hauptmann.

Die Spieler wissen zu diesem Zeitpunkt noch immer nicht zwingend, dass sie gerade eine versteckte Spawnkette ausgelöst haben.

Irgendwann erscheint plötzlich der Boss.

Die Gruppe wollte ursprünglich lediglich XP sammeln und hat durch ihr normales Spielen ein Geheimnis der Welt entdeckt.

---

# 14. Entdecken bedeutet nicht Besiegen

Eine Gruppe, die einen versteckten Boss auslöst, muss nicht stark genug sein, ihn auch zu besiegen.

Das Spiel passt den Boss nicht automatisch an die Gruppe an, die ihn entdeckt hat.

Eine Levelgruppe kann deshalb versehentlich einen Gegner hervorlocken, dem sie noch nicht gewachsen ist.

Die Entdeckung selbst ist bereits Teil des Erlebnisses.

Die Spieler können anschließend beispielsweise ihrer Gilde davon erzählen und gemeinsam zurückkehren, um die Begegnung genauer zu untersuchen.

---

# 15. Spielerwissen

Das Wissen über versteckte Gebietsbosse gehört den Spielern.

Das Spiel erklärt weder:

* wo sich diese Bosse befinden,
* welche Bosse überhaupt existieren,
* noch wie ihre Spawnbedingungen funktionieren.

Spieler müssen diese Zusammenhänge selbst entdecken.

Eine Gruppe kann ihre Entdeckung innerhalb ihrer Gilde weitergeben.

Andere Spieler können anschließend versuchen, die beobachteten Ereignisse zu reproduzieren und die tatsächlichen Bedingungen herauszufinden.

Eine Gilde kann dieses Wissen öffentlich machen oder versuchen, es für sich zu behalten.

---

# 16. Bosskämpfe bleiben öffentlich

Versteckte Gebietsbosse befinden sich weiterhin in der normalen offenen Welt.

Sie werden nicht in eine private Instanz verschoben.

Andere Abenteurer können deshalb zufällig an einem laufenden Bosskampf vorbeikommen.

Beispielsweise kann eine Gilde einen bisher unbekannten Boss bekämpfen, während eine andere Gruppe wegen einer vollkommen normalen Quest durch dasselbe Gebiet reist.

Diese Spieler sehen den Boss und erfahren dadurch erstmals von seiner Existenz.

Sie kennen dadurch allerdings noch nicht automatisch seine Spawnbedingungen.

Sie können ihre Beobachtung anschließend anderen Spielern oder ihrer eigenen Gilde weitererzählen.

Das Spiel verhindert diese Informationsverbreitung nicht künstlich.

---

# 17. Erkundung und Bossjagd

Sobald die ersten versteckten Gebietsbosse entdeckt wurden, können Spieler daraus schließen, dass möglicherweise weitere Geheimnisse dieser Art existieren.

Dadurch kann sich aus dem normalen Erkunden ein freiwilliges Bossjäger-Spiel entwickeln.

Spieler ziehen durch andere Gebiete und untersuchen:

* ungewöhnliche Gegnergruppen,
* Grindgebiete,
* veränderte Respawns,
* Named-Gegner,
* auffällige Gegnerzusammensetzungen.

Da normale Respawns und tatsächliche Bosswellen bewusst nicht eindeutig voneinander unterscheidbar sind, bleibt diese Suche unsicher.

Ein gefundenes Muster garantiert nicht, dass dasselbe Muster an einem anderen Ort funktioniert.

---

# 18. Besonderer Loot

Starke versteckte Gebietsbosse können besonderen Loot besitzen.

Damit erhalten Spieler einen zusätzlichen Anreiz, nach diesen Begegnungen zu suchen und ihre Spawnbedingungen zu entschlüsseln.

Die konkreten Gegenstände und Lootregeln werden später im entsprechenden Loot- bzw. Gegenstandssystem definiert.

---

# Designgrundsatz

**Versteckte Gebietsbosse werden entdeckt, nicht angekündigt.**

Die Welt zeigt den Spielern lediglich, was geschieht.

Ob sie darin ein Muster erkennen, die Spawnbedingungen entschlüsseln, dieses Wissen für sich behalten oder mit anderen Spielern teilen, bleibt ihnen überlassen.

Ein Spieler soll nach einem normalen Respawn niemals mit Sicherheit wissen können:

**„Das war gerade eine Bosswelle.“**
