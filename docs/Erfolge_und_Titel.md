# Andora – Erfolge und Titel

## 1. Status

**Status:** Verbindliche Grundprinzipien (Konzept-Ebene)

Dieses Dokument definiert verbindlich, wie das Erfolgssystem, die Titel und besondere Realm-Titel in Andora funktionieren.

Es ergänzt die bestehenden Systeme (Quests, Raids, Dungeons, Unterwelt, Crafting, Sammeln, Boss-System, Realm-Ereignisse) und setzt keine technische Implementierung voraus.

> **Die konkrete technische Umsetzung wird mit dem Erfolgs-/Titel-/Eventsystem festgelegt.**
> Schwellenwerte, Effekte, Seltenheitsstufen und Datenmodelle werden nicht vorab definiert.

---

# 2. Grundprinzip

Andora wird ein Erfolgssystem erhalten.

Erfolge können an vielen Stellen der Welt vergeben werden:

* Quests
* Gegner-Kategorien
* Erkundung
* Rätsel
* Crafting
* Sammeln
* Dungeons
* Raids
* Unterwelt
* World-Ereignisse
* Realm-Ereignisse
* soziale oder besondere Aktivitäten

Erfolge sind grundsätzlich **Prestige, keine Pflicht**.

> **Erfolge belohnen Aufmerksamkeit und Leistung – sie zwingen keine Spielrichtung auf.**

Ein Spieler, der sich primär auf Raids, Dungeons und Endgame konzentriert, muss keine versteckten Erfolge jagen.
Ein Spieler, der Bücher liest, Ruinen untersucht und Rätsel löst, kann zusätzliche Erfolge und Titel sammeln, die andere nie erhalten.

---

# 3. Erfolge

Erfolge können gestaffelt sein.

Beispiel:

```text
Bestimmte Anzahl erledigter Aufgaben oder besiegter Gegner einer bestimmten Kategorie
      ↓
Erreichung der ersten Stufe
      ↓
Titel wird freigeschaltet
      ↓
weitere Stufen können zusätzliche Titel freischalten
```

Erfolge sollen:

* serverseitig validiert werden
* für alle Charaktere desselben Realms verfügbar sein
* als Sammlungs- und Prestige-Element dienen
* als Belohnung für besondere Leistungen sichtbar bleiben

> **Erfolge beschreiben Leistung. Sie ersetzen keine Spielmechaniken.**

---

# 4. Titel

Charaktere besitzen alle freigeschalteten Titel.

Im Character-Profil kann der Spieler wechseln, welcher Titel neben dem Charakternamen angezeigt wird.

Anzeigen erlaubt:

* genau ein Titel
* kein Titel

> **Die Anzeige von keinem Titel muss ebenfalls möglich sein.**

Titel sind grundsätzlich **kosmetisches Prestige**, keine Kampfvorteile.

> **Ein Titel verändert die Spielmechanik nicht.**

Solange keine explizite Ausnahme in der späteren Implementierung definiert wird, bleiben Titel rein repräsentativ.

Titel, die freiwillige Unterstützung des Projekts anerkennen, folgen derselben Regel: Freiwillige Spenden dürfen ausschließlich nicht spielrelevante Anerkennung geben. Dafür ist ein optionaler, rein kosmetischer Ingame-Unterstützer-Titel vorgesehen (siehe `Monetarisierung_und_Donations.md`). Er erzeugt keinen Bonus, keinen Vorteil und keinen besonderen Status.

---

# 5. Beispiel für gestufte Erfolge

Beispielkette:

```text
Bestimmte Anzahl Gnole besiegt
      ↓
Titel „Gnollenschlächter" freigeschaltet
      ↓
weitere Stufen können zusätzliche Titel freigeben
```

Diese Art von Beispiel illustriert die Grundregel:

* Gegner-Kategorie als Fortschrittsgrundlage
* versteckte oder sichtbare Belohnung
* Titel als sichtbares Prestige

---

# 6. Versteckte Titel

Nicht jede Entdeckung muss vorab im Erfolgsfenster sichtbar sein.

Ganze versteckte Rätsel oder verborgene Questketten dürfen Titel vergeben, die erst nach Entdeckung sichtbar werden.

Beispiel:

```text
Bücher aus drei Regionen gelesen
      ↓
Rätsel in einer Ruine gelöst
      ↓
eine geheime Questkette abgeschlossen
      ↓
ein verborgener Titel freigeschaltet
```

Der Spieler erkennt das Ziel nicht sofort aus dem Erfolgslog.
Er erkennt es durch Aufmerksamkeit in der Welt.

Wichtig:

> **Ein versteckter Titel ist erst sichtbar, wenn er tatsächlich freigeschaltet wurde.**
>
> Vorher darf er nicht als Eintrag erscheinen.

---

# 7. Seltenheits-Anzeige

Die raresten Titel dürfen optisch dezent hervorgehoben werden.

Erlaubte Grundformen:

* leichtes Leuchten
* dezentes Funkeln
* besondere Schriftfarbe
* minimale Animation

Grundprinzip:

> **Auffällige Effekte sollen selten bleiben.**
>
> Selten heißt, dass die meisten Spieler nicht ständig auffällige Titel sehen. Seltenheit soll spürbar, aber nicht zu allgegenwärtig werden.

---

# 8. Historische Realm-Titel

Große Realm-Ereignisse können einmalige historische Titel vergeben.

Ein solcher Titel kann einem Charakter gehören, der:

* auf diesem Realm eine bestimmte historische Entdeckung zuerst macht,
* ein einmaliges Weltereignis auslöst,
* einen entscheidenden Teil in einer großen Geschichte einbringt.

Dadurch werden diese Charaktere dauerhaft Teil der Geschichte ihres Realms.

Beispiel:

```text
Realm A
→ ein Spieler findet zuerst die verborgene Ruine X
      ↓
er erhält einen einmaligen historischen Titel
      ↓
im Chronik-System dieses Realms wird seine Entdeckung als Teil der lokalen Geschichte gespeichert
```

Relevante Grundprinzipien:

* historische Titel sind individuell
* sie können realmbezogen sein
* sie dürfen selten auftauchen
* sie können als Prestige und Erinnerung an reale Player-Aktivitäten funktionieren
* sie dürfen in die Realm-Chronik einfließen

> **Der Chronik-Entwurf wird in Storytelling_und_Weltgeheimnisse.md (Abschnitt 11: „Individuelle Realm-Chroniken") definiert.**

---

# 9. Abgrenzung zu bestehenden Systemen

## 9.1 Verhältnis zum Königsamt

Die Politik-/Herrschafts-Dokumentation (`Politik-Herrschaftssystem.md`) definiert das Königsamt als aktuelle politische Position und ausdrücklich nicht als dauerhaften Achievement-Titel.

Abgrenzung:

* Das Königsamt ist eine **politische Position innerhalb einer Fraktion**, die aktiv gehalten werden muss.
* Der historisch Realm-bezogene Titel ist eine **einmalige, prestige-ahnliche Erinnerung an reale Spielerleistung**.

Beides darf bestehen, aber sie müssen in der Implementierung getrennt gepflegt werden.

> **Das Königsamt ist kein Achievement-Titel. Realms-Titel sind aber keine politischen Ämter.**

## 9.2 Verhältnis zu Erfolgen und Titeln in der Ideensammlung

In `MMO-Systeme-Ideensammlung.md` werden Titel, Achievements und versteckte Erfolge als Ideenpunkt gelistet.

Mit diesem Dokument werden diese Punkte als **verbindliche Grundprinzipien** angelegt, während die konkreten Implementierungsdetails (Schwellen, Reihenfolge, Seltenheitsstufen und Effekte) weiterhin offen bleiben und mit dem Event-/Rätsel-/Erfolgs-System definiert werden.

---

# 10. Referenzbibliothek (references)

Die Fachliteratur *Practical Game Design* (`references/books/practicalgamedesign`) liefert die konzeptionelle Grundlage:

* **Chapter 13 „Balancing" (ab PDF p. 349)**
  * Kapitel über das Handling von optionalen Zielen, versteckten Herausforderungen, Erfolgen und Prestige.
  * Relevante Inhalte für Andora:
    * optionalen Belohnungen statt Pflicht
    * versteckte Geheimnisse und Erfolge als Belohnung für Aufmerksamkeit
    * Seltenheits-Anzeige als sichtbares Prestige

> Die andoranische Bindung an die oben genannten Prinzipien ist eine **Doku-/Design-Entscheidung**, keine technische Implementierung.

Die Referenzdokumente wurden geprüft und korrekt eingeordnet.

---

# 11. Zusammenfassung

Erfolge und Titel in Andora:

* sind Prestige-Elemente, keine Pflicht
* können versteckt, gestaffelt oder einmalig sein
* gehören zum Charakter und sind im Profil anzeigbar
* unterscheiden sich von politischen Positionen wie dem Königsamt
* können in die Realm-Chronik einfließen
* müssen mit dem Event-/Rätsel-/Erfolgs-System technisch angebunden werden, sobald die Implementierung startet

> **Erfolge und Titel sollen Aufmerksamkeit sichtbar machen – nicht Zwang.**
