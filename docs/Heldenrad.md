# Andora – Heldenrad

## Status

**Konzept – Grundstruktur festgelegt.**

Die hier festgelegte Struktur ist verbindlich. Konkrete Zahlenwerte, Effekte und Darstellungsdetails werden später entschieden.

---

## 1. Grundidee

Das Heldenrad ist eine besondere, klassenübergreifende Kampffähigkeit.

Es ist kein reines Gruppenfeature. Es kann auch von einem einzelnen Spieler ausgelöst werden und funktioniert grundsätzlich auch solo. Der Solo-Effekt ist dabei bewusst vergleichsweise schwach.

Der Kern des Heldenrads lautet:

> **Je mehr unterschiedliche Klassen ihre Fähigkeiten wirksam einsetzen, desto mehr Anforderungen stellt das Rad – und desto stärker wird sein möglicher Erfolg.**

Das Heldenrad belohnt damit nicht bloß die Anzahl der Spieler, sondern die wirksame Beteiligung unterschiedlicher Klassen innerhalb ihrer jeweiligen Rollenidentität.

---

## 2. Auslösung und Kontext

Das Heldenrad wird von einem Spieler im Kampf ausgelöst.

In normalen Gruppen arbeitet das Heldenrad innerhalb der Gruppe des auslösenden Spielers.

In Raids existiert kein raidweites gemeinsames Heldenrad. Jede Gruppe innerhalb eines Raids besitzt ihren eigenen Heldenrad-Kontext.

Das Heldenrad muss von Solo bis zur jeweils zulässigen normalen Gruppengröße funktionieren. Die spätere Erhöhung der normalen Gruppengröße von 4 auf 5 mit Exp1 (siehe `exp1_Unterwelt.md`, Abschnitt 56, und `project_overview.md`, Abschnitt 14) muss mit diesem Prinzip vereinbar bleiben.

---

## 3. Ein Symbol pro beteiligter Klasse

Das Heldenrad fordert keine konkrete Ability-ID.

Es zeigt pro beteiligter Klasse genau ein Symbol, das eine Fähigkeitskategorie darstellt. Verschiedene Spieler derselben Klasse erzeugen dadurch kein zusätzliches Symbol; zählt also eine Klasse doppelt im Heldenrad mit, bleibt ihre Anforderung bei einem einzigen Symbol.

Eine Fähigkeit kann einer oder mehreren Fähigkeitskategorien zugeordnet sein.

Beispiel-Kategorien sind:

* Einziel-Schaden
* Flächenschaden (AoE)
* Schaden über Zeit
* Taunt
* Heilung
* Buff
* Debuff
* Kontrolle

Diese Kategorien sind Beispiele und Grundtypen. Sie sind keine endgültige vollständige Liste.

Verschiedene Klassen können unterschiedliche Symbol- und Kategorie-Optionen besitzen. Jede Klasse soll innerhalb ihrer eigenen Rollenidentität agieren können:

* Ein Tank kann beispielsweise einen Taunt oder einen passenden Schlag einsetzen.
* Ein Magier kann passende Zauber einsetzen.
* Ein Heiler oder Supporter kann Heilung, Buffs, Debuffs oder andere zu seiner Rolle passende Kategorien einsetzen.

Keine Klasse soll gezwungen sein, eine unpassende Aktion außerhalb ihrer Rolle auszuführen.

---

## 4. Gültige Fähigkeiten und freie Wahl

Hat ein Spieler genau eine Fähigkeit, die zu einem offenen Symbol passt, ist nur diese Fähigkeit für dieses Symbol gültig.

Hat ein Spieler mehrere passende Fähigkeiten, sind all diese Fähigkeiten gültige Möglichkeiten.

Das Heldenrad schreibt nicht vor, welche konkrete passende Fähigkeit verwendet werden muss. Die Wahl bleibt beim Spieler, und das Management der Cooldowns ist Teil der Herausforderung.

---

## 5. Erfolgreiche Wirkung wird vom Realm bestätigt

Ein Symbol wird nicht schon beim Drücken der Taste oder beim bloßen Auslösen einer Fähigkeit erfüllt.

Ein Symbol wird erst dann gelockt, wenn der Realm bestätigt, dass die passende Fähigkeit erfolgreich ihre relevante Wirkung erzielt hat.

Beispiele:

* Schadenssymbol: Es muss tatsächlicher Schaden verursacht worden sein.
* Flächenschaden: Mindestens ein gültiges Ziel muss tatsächlich Schaden erhalten haben.
* Schaden über Zeit: Der Effekt muss erfolgreich auf ein gültiges Ziel angewendet worden sein.
* Taunt: Der Taunt muss vom Realm erfolgreich angenommen worden sein.
* Heilung: Die Heilwirkung muss erfolgreich angewendet worden sein.
* Buff, Debuff oder Kontrolle: Der Effekt muss erfolgreich gesetzt worden sein.

Gelingt die relevante Wirkung nicht, bleibt das Symbol offen. Weitere passende Fähigkeiten des Spielers können anschließend noch versucht werden. Wenn keine weitere passende Fähigkeit rechtzeitig zur Verfügung steht, kann das Heldenrad auslaufen.

---

## 6. Zeitfenster und Scheitern

Das Heldenrad steht nur für ein begrenztes Zeitfenster zur Verfügung.

Läuft dieses Zeitfenster ab, bevor alle erforderlichen Symbole erfolgreich gelockt wurden, ist das gesamte Heldenrad gescheitert.

Es gibt keinen Teil- oder Trostpreis für ein unvollständig gelocktes Heldenrad.

---

## 7. Skalierung und Stärke

Weniger unterschiedliche Klassen bedeuten weniger Anforderungen. Der Erfolg wird leichter, dafür schwächer.

Mehr unterschiedliche Klassen bedeuten mehr Anforderungen. Der Erfolg wird schwieriger, dafür stärker.

Dieses Skalierungsprinzip gilt von Solo bis zur jeweiligen zulässigen normalen Gruppengröße.

---

## 8. Freischaltung

Das Heldenrad wird bereits in frühen Levels freigeschaltet.

Die Freischaltung ist an den jeweiligen Klassen-Grundstock der beteiligten relevanten Fähigkeiten gebunden.

Die konkrete Freischaltstufe ist damit noch nicht endgültig festgelegt, da sie in den bestehenden verbindlichen Dokumenten keine feste Levelzahl definiert.

---

## 9. Architekturprinzip

Das Heldenrad baut auf serverbestätigten Kampf- und Fähigkeitsergebnissen auf.

Es darf nicht allein durch Client-Eingaben erfüllt werden. Ein Symbol wird ausschließlich gelockt, wenn der Realm die tatsächliche, relevante Wirkung der verwendeten Fähigkeit bestätigt.

Das Ability-System ist in `Ability-System.md` definiert und stellt semantische Fähigkeitskategorien sowie Realm-bestätigte Ability-Ergebnisse bereit, auf denen das Heldenrad aufbaut (siehe `Ability-System.md` Abschnitt 14).

Das Heldenrad selbst ist eine **nicht aufwertbare Fähigkeit**. Es besitzt keine Fähigkeitsqualität (Lehrling/Fortgeschritten/Meisterhaft/Legendär) und kann nicht durch Schriftrollen oder Bücher verbessert werden.

---

## 10. Bewusst offen

Folgende Punkte bleiben bewusst offen und werden unabhängig von dieser Grundstruktur festgelegt:

* der endgültige Name, falls „Heldenrad“ später durch einen anderen Namen ersetzt wird
* die genaue Freischaltstufe
* das genaue Zeitfenster
* der genaue Effekt des Heldenrads
* die genaue Stärke des Effekts
* die genaue Wahrscheinlichkeit beziehungsweise Erfolgsbedingung
* die genauen Symbolgrafiken und die UI-Darstellung
* die vollständige Liste der Fähigkeitskategorien
* die genauen Auswahlregeln für die Symbole
* der Cooldown des Heldenrads
* zusätzliche normale Skill-Kombos außerhalb des Heldenrads

Zusätzliche normale Skill-Kombos außerhalb des Heldenrads sind ausdrücklich eine spätere Designentscheidung.

Bis zu dieser Entscheidung ist das Heldenrad die einzige konkret definierte klassenübergreifende Combo- und Synergie-Mechanik in Andora.

---

## 11. Abgrenzung zu anderen Systemen

Das Heldenrad ist eine eigenständige, klassenübergreifende Kampffähigkeit auf Konzept-Ebene.

Es verändert weder das Grundkampfsystem in `Kampfsystem.md` noch die Rollen- und Klassenstruktur in `Klassensystem.md`. Seine Grundlage sind die rollenidentitätsnahen Fähigkeiten der Klassen und die Ability-Architektur mit semantischen Fähigkeitskategorien (siehe `Ability-System.md`).

Es ist unabhängig von Raidszenarien definiert: Es besitzt kein raidweites gemeinsames Heldenrad, sondern arbeitet in normalen Gruppen innerhalb der jeweiligen Gruppe. Die Gruppengrößengrenzen und ihr Exp1-Wachstum sind in `project_overview.md` (Abschnitt 14) und `exp1_Unterwelt.md` (Abschnitt 56) festgelegt.

Weitere normale Skill-Kombos außerhalb des Heldenrads bleiben eine spätere Designentscheidung und sind hier nicht definiert.
