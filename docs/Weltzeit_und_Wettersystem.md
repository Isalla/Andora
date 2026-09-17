# Andora – Weltzeit- und Wettersystem

## 1. Status

**Dokumentation (Konzept).** Kein Rust-Code, keine Datenbankmigration, keine Netzwerk-/Message-IDs, keine Godot-Implementierung.

Diese Datei beschreibt das **gemeinsame serverseitige Weltumgebungssystem** von Andora für:

* Weltzeit
* Tag-/Nacht-Zyklus
* Tagesphasen
* regionales Wetter

Sie legt die verbindliche Grundarchitektur fest. Konkrete Zahlen, Dauer, Grenzen, Wahrscheinlichkeiten und die technische Repräsentation bleiben bewusst offen (Abschnitt 18). Diese Datei ersetzt die bisherige Ideennotiz „Wetter und Zeit" in `MMO-Systeme-Ideensammlung.md` als verbindliche Regelquelle; jene bleibt als offene Ideenquelle erhalten.

---

## 2. Architektur-Leitsatz

> **Der Realm bestimmt Weltzeit, Tagesphase und regionales Wetter autoritativ. Regeln und Parameter sind konfigurierbar; der aktuelle Weltzustand ist persistent. Der Client stellt diesen Zustand dar, entscheidet ihn aber nicht.**

---

## 3. Serverautorität

Der Rust Realm Server ist autoritativ für:

* aktuelle Weltzeit
* Spieltag
* Tagesphase
* Wetterzustand pro Wetterregion
* Wetterregion
* Wetterwechsel
* Wetterintensität
* zeitlichen Verlauf und Übergänge

Der spätere Godot-Client ist **nicht** autoritativ.

Der Client darf ausschließlich den **bestätigten Realm-Zustand** darstellen, beispielsweise:

* Helligkeit
* Himmel
* Sonnen-/Mondlicht
* Regen
* Schnee
* Nebel
* Winddarstellung
* Partikel
* Wettergeräusche
* audiovisuelle Übergänge

Es wird **keine konkrete Godot-Implementierung** festgelegt.

Einordnung in die bestehende Architektur:

> Der Server ist die Quelle der Wahrheit. (`project_overview.md`, Abschnitt 4 „Serverautorität"; dort sind „persistente Weltzustände" bereits der Serverautorität zugeordnet.)

Das Weltzeit-/Wettersystem folgt demselben Muster wie alle Andora-Systeme (`project_overview.md` Abschnitt 22, `architecture.md`):

```text
Realm-Server (Rust)
→ Regeln und Autorität

MariaDB (realm_state_<realm>)
→ persistenter Weltzustand

Godot (Referenzclient)
→ Darstellung
```

---

## 4. Gemeinsames Weltumgebungssystem

Weltzeit, Tag-/Nacht-Zyklus, Tagesphasen und regionales Wetter bilden **ein** gemeinsames serverseitiges System.

Es gibt bewusst **keine** parallelen, voneinander unabhängigen Teilmechaniken: Eine Tagesphase ist Teil der gemeinsamen Weltzeit; der Wetterzustand ist an Wetterregionen gebunden und wird vom selben Realm-Server simuliert.

---

## 5. Weltzeit

Der Realm besitzt eine gemeinsame, synchronisierte Weltzeit.

**Grundsätzlich erleben alle Spieler desselben Realms dieselbe Weltzeit.**

Das System muss konzeptionell mindestens unterscheiden können:

* **Spieltag**
* **Uhrzeit**
* **Tagesphase**

Mögliche Tagesphasen (Bezeichnungen, ohne bereits konkrete Uhrzeitgrenzen festzulegen):

* Morgendämmerung
* Tag
* Abenddämmerung
* Nacht

Beispielhafte Abfolge eines kompletten Tag-/Nacht-Zyklus:

```text
Morgendämmerung → Tag → Abenddämmerung → Nacht → Morgendämmerung → …
```

Die genauen Uhrzeitgrenzen der Tagesphasen bleiben offen (Abschnitt 18).

---

## 6. Zeitfaktor

Die Geschwindigkeit der Andora-Weltzeit ist **konfigurierbar**.

Ausdrücklich **nicht** jetzt festgelegt:

* wie viele reale Stunden ein Andora-Tag dauert
* der exakte Zeitfaktor
* die konkrete Länge einzelner Tagesphasen

Diese Werte sollen später durch Tests angepasst werden können (Server-first, Abschnitt 14).

Regel:

> Eine Änderung der Konfiguration darf den persistent gespeicherten Weltzustand nicht automatisch zurücksetzen.

Die Konfiguration bestimmt den **Verlauf** der Weltzeit, nicht die „Position" der Welt. Wird etwa der Zeitfaktor geändert, bleibt der aktuell geltende Spieltag samt Uhrzeit erhalten und läuft ab dann mit dem neuen Faktor weiter.

---

## 7. Regionales Wetter

Wetter ist **nicht zwingend realmweit identisch**.

Der Realm soll **regionale Wetterzustände** unterstützen, die an **Wetterregionen** gebunden sind.

Beispiele (ausdrücklich keine vollständige Wetterdefinition):

| Beispielregion | mögliches Wetter |
|---|---|
| Menschengebiet | Regen |
| Luzilla-Bergregion | Schnee |
| Andorer-Dünenregion | trockener Wind / Sandwetter |

Regeln:

* Wetterregionen müssen **nicht zwingend** exakt den politischen oder fraktionalen Gebietsgrenzen entsprechen.
* Die endgültige Aufteilung der Wetterregionen bleibt offen (Abschnitt 18).
* Wetterregionen können sich von den Großgebieten/Zonen des Reisesystems (`Welt_Reisesystem.md`) unterscheiden; die konkrete Zuordnung wird hier nicht festgelegt.

---

## 8. Wetterzustand

Ein regionaler Wetterzustand soll konzeptionell mindestens enthalten können:

* **Wetterart**
* **Intensität**
* **Beginn** bzw. eine zeitliche Referenz
* **Dauer** bzw. den geplanten Wechsel
* **Übergang** zum nächsten Zustand

Mögliche Wetterarten (nur Beispiele, keine endgültige Enum-/API-Festlegung):

```text
CLEAR
CLOUDY
RAIN
STORM
FOG
SNOW
```

---

## 9. Wetterübergänge

Wetter soll nicht ausschließlich aus abrupten Zustandswechseln bestehen.

Das System muss **Übergänge** ermöglichen.

Beispiel:

```text
CLEAR
→ CLOUDY
→ RAIN
```

Regel:

> Der Realm bestimmt den autoritativen Wetterübergang (inklusive Zeitverlauf). Der Client darf daraus fließende audiovisuelle Übergänge erzeugen.

Die konkrete Interpolations- beziehungsweise Renderingtechnik bleibt ausschließlich Sache des Clients und hat keine Gameplay-Bedeutung (§3, §15).

---

## 10. Persistenz und Abgrenzung zur Player-Persistenz

Weltzeit und Wetter sind **persistenter Realm-/Weltzustand**.

Sie gehören ausdrücklich **nicht** zur Player-Persistenz: `Player_Persistenz.md` behandelt ausschließlich laufende, spielergebundene Zustände (Position, Progression, Gold, Inventar, Questzustand). Weltzeit und Wetter sind realmgebunden und werden davon getrennt behandelt.

**Vorgesehene Trennung:**

| Ebene | Inhalt |
|---|---|
| **Konfiguration** | bestimmt Regeln und Parameter des Systems (§6, §11) |
| **MariaDB / realm_state_<realm>** | speichert den aktuellen autoritativen Weltzustand |
| **Rust Realm** | laufende autoritative Simulation |
| **Godot** | Darstellung (§3) |

Der persistente Weltzustand soll konzeptionell mindestens ermöglichen:

* aktuelle Weltzeit bzw. einen ausreichenden Zeitreferenzzustand
* aktuellen Spieltag
* aktuellen Wetterzustand je Wetterregion
* die für laufende Wetterübergänge erforderlichen Informationen

Es wird **kein konkretes DB-Schema** festgelegt und **keine Migration** erstellt.

Einordnung in die Datenhaltung:

> Weltzeit und Wetter sind Realm-Daten im Sinne von `Datenbank_Architektur.md` (Abschnitt 5 „Enthält Realm-Daten": Weltfortschritt, Weltveränderungen, regionale Zustände, Eventzustände). Jeder Realm besitzt dadurch seinen eigenen persistenten Weltzeit-/Wetterzustand (Realm-Isolation, `Datenbank_Architektur.md` Abschnitt 10). Verwendet wird die bestehende Realm-Datenbank `realm_state_<realm>`, es kommt keine neue Datenbank hinzu.

---

## 11. Konfiguration vs. Zustand

Ausdrücklich zu unterscheiden:

* **Konfiguration** sagt: „Wie arbeitet das System?"
* **Persistenter Realm-State** sagt: „Wo befindet sich die Welt gerade?"

Beispiele für später konfigurierbare Werte:

* Weltzeit-Geschwindigkeit (§6)
* Wetterparameter
* mögliche Wechselregeln
* weitere Timing-Parameter

Es werden **keine finalen Config-Key-Namen erfunden**, sofern nicht bereits vorhanden. Das entspricht der bestehenden Konfigurationskonvention (`config.rs`, Umgebungsvariablen-Pattern der Realm-Doku).

---

## 12. Realm-Neustart

Nach einem Realm-Neustart darf Weltzeit/Wetter **nicht automatisch** auf einen festen Standardzustand zurückgesetzt werden, also insbesondere nicht auf:

```text
Tag 1
12:00 Uhr
CLEAR
```

Regel:

> Der persistierte Weltzustand muss grundsätzlich wieder aufgenommen werden.

Eine Gameplay-Frage bleibt ausdrücklich **offen**:

* **Variante A:** Die Andora-Weltzeit läuft während einer Realm-Downtime anhand real vergangener Zeit weiter.
* **Variante B:** Die Weltzeit friert während der Downtime ein und läuft nach dem Start vom letzten Zustand weiter.

**Keine dieser Varianten wird jetzt ausgewählt** (Abschnitt 18).

---

## 13. Spieler-Login und Gebietswechsel

Ein Spieler, der:

* einloggt
* eine Wetterregion betritt
* die Wetterregion wechselt

muss den **aktuellen autoritativen Weltzeit-/Wetterzustand** erhalten können.

Grundregel:

> Es wird **kein** pro Spieler eigener Wetterzustand erzeugt.

Spieler in derselben Wetterregion erleben denselben autoritativen Wetterzustand.

Es werden **keine finalen Netzwerkpakete oder Message-IDs** definiert.

---

## 14. Server-first

Das System muss **vollständig serverseitig modellierbar und testbar** sein, bevor ein Godot-Client existiert.

Das entspricht der bestehenden Andora-Reihenfolge:

```text
Serverfunktion
→ autoritativer Zustand
→ spätere Clientdarstellung
```

In diesem Auftrag wird **keine Clientimplementierung** festgelegt.

---

## 15. Gameplay-Auswirkungen (bewusst offen)

Es wird **nicht** festgelegt, ob Wetter oder Tageszeit später Auswirkungen haben auf:

* Kampf
* Sichtweite
* Bewegung
* Spawnraten
* Monster
* Ressourcen
* Quests
* Events
* Händler
* NPC-Tagesabläufe
* Öffnungszeiten
* Fähigkeiten
* Buffs/Debuffs

Solche Verknüpfungen sind später möglich, sind derzeit aber **keine beschlossene Gameplay-Regel**.

Das Grundsystem muss auch **rein atmosphärisch** funktionieren können.

---

## 16. NPC-/KI-Grenze

Später können NPC-Systeme Weltzeit oder Wetter als **autoritativen Realm-Kontext lesen**.

Beispiele:

* NPC erkennt Nacht
* NPC erkennt Regen
* Dialog kann das aktuelle Wetter berücksichtigen

Grenze:

> NPC-KI darf Weltzeit oder Wetter nicht eigenmächtig verändern.

Es werden **keine NPC-Zeitpläne oder Öffnungszeiten** definiert.

---

## 17. Keine zweite Wahrheitsquelle

Es wird **keine separate Datei** als parallelen persistenten Laufzeit-State eingeführt.

Die vorgesehene Trennung lautet ausnahmslos:

```text
Konfiguration:        Regeln / Parameter
MariaDB:              persistenter aktueller Realm-/Weltzustand
Rust Realm:           laufende autoritative Simulation
Godot:                Darstellung
```

---

## 18. Bewusst offene Punkte

Folgende Punkte werden in diesem Auftrag ausdrücklich **nicht** entschieden:

* Verhältnis Realzeit zu Andora-Zeit
* Dauer eines Andora-Tages
* genaue Tagesphasengrenzen
* Verhalten während Realm-Downtime (weiterlaufen vs. einfrieren, Abschnitt 12)
* konkrete Wetterarten
* Wetterwahrscheinlichkeiten
* Wetterdauer
* genaue Wetterregionen
* regionale Klima-/Wetterregeln
* mögliche Jahreszeiten
* Gameplay-Auswirkungen
* NPC-Reaktionen / Tagesabläufe
* konkrete Persistenzstruktur / DB-Schema
* konkrete Netzwerkrepräsentation
* konkrete Clientdarstellung

---

## 19. Querverweise

* `project_overview.md` – Abschnitt 4 „Serverautorität" (persistente Weltzustände) und Abschnitt 22 „Content-Architektur"
* `architecture.md` – Dienstetrennung, realm_state_<realm>
* `Datenbank_Architektur.md` – Abschnitt 5 (Realm-Daten), Abschnitt 10 (Realm-Isolation)
* `Player_Persistenz.md` – Abgrenzung: Player-Persistenz behandelt spielergebundene Zustände, keine Weltzeit/Wetter
* `Welt_Reisesystem.md` – Großgebiete/Zonen (Bezugspunkt für mögliche Wetterregionen)
* `MMO-Systeme-Ideensammlung.md` – Abschnitt „Wetter und Zeit" (Ideenquelle, keine verbindliche Quelle)