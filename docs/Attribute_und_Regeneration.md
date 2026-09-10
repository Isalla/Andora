# Andora – Attribute und Regeneration

## Status

**Konzept – verbindlicher Grundlagenentwurf.**

**Implementierungsstand (Rust-Realm):**

- **Grundattribute (Abschnitte 1–3) eingebaut:** `src/realm-rs/src/attributes.rs` mit
  `Attributes`-Struct (7 × i32), datengetriebene Konstanten (§11), abgeleitete Werte
  (f64), Hook `AttributeBonuses` für Ausrüstungsboni (§3, konkrete Items folgen separat),
  `recompute_max_resources` für Login-Initialisierung. DB-Spalten via Migration 011
  (`strength`, `agility`→dexterity, `intelligence`, `constitution` bereits in 001;
  `wisdom`, `luck`, `endurance` neu, neutral auf 10 – **keine beschlossene
  Start-/Rassenverteilung**, §2 folgt mit dem Rassen-/Charaktersystem). Aktive Wirkungen: Kraft→Nahkampf +0,5 %/Pkt, Konstitution→
  Max-HP +10/Pkt, Geschicklichkeit→Fernkampf +0,5 %/Pkt (reine Funktion, kein
  Fernkampfsystem), Intelligenz→Magischer Schaden +0,5 %/Pkt (in `ability.rs` wired),
  Weisheit→Max-Mana +20/Pkt, Glück→Crit +0,1 pp/Pkt (Aufschlag auf bestehenden
  Crit-Wurf, kein paralleles System), Ausdauer→Rüstungswert +0,2 %/Pkt (wirkt auf
  effektive Rüstung vor dem Cap) + Laufdauer +1 %/Pkt (vorbereitet, inaktiv).
  Rassenverteilung (§2) folgt mit dem Rassensystem.

- **HP-/Mana-Regeneration (Abschnitte 4–8) eingebaut:** `src/realm-rs/src/regen.rs`,
  eingebunden in `world.rs` / `main.rs`. Grundformel `(Basis + additive Boni) ×
  Zustandsmultiplikator` mit absoluten Werten pro Sekunde, Zustandsmultiplikatoren
  15 % / 100 % / 125 % (Sitzbonus greift nie im Kampf), Klassen-Basisregeneration
  und Levelwachstum +0,2/s pro Level, Delta-Tick-unabhängigkeit über interne
  f64-Bruch-Carrys, Deckelung bei Max-HP/Max-Mana, keine Wiederbelebung bei 0 HP.
  Klassentabelle/Level-Skalierung zentral in `regen.rs` (§11). Additive Boni technisch
  angeschlossen als `hp_regen_bonus`/`mana_regen_bonus`.

- **Offen:** Consumable-/Buff-Integration (§9, Umsetzung folgt separat), Sitz-UI/
  Client-Anbindung (Server-State `sitting` minimal vorhanden).

Dieses Dokument definiert die verbindliche Grundlage der sieben Grundattribute, der rassischen Attributausrichtung sowie der HP-/Mana-Regeneration von Andora. Konkrete Zahlenwerte sind ausdrücklich **erster Balancing-Stand** und werden durch tatsächliche Spieltests überprüft und angepasst. Die Architektur darf nicht davon abhängig sein, dass diese Zahlen dauerhaft unverändert bleiben.

Das Dokument ergänzt das Kampfsystem (`Kampfsystem.md`), das Klassensystem (`Klassensystem.md`), das Ability-System (`Ability-System.md`), die Item-/Ausrüstungs-Dokumente (`item_properties.md`, `inventory_system.md`) sowie die Rassen-Dokumente.

---

## 1. Grundattribute

Andora besitzt zunächst sieben Grundattribute:

* **Kraft**
* **Konstitution**
* **Geschicklichkeit**
* **Intelligenz**
* **Weisheit**
* **Glück**
* **Ausdauer**

Die Grundattribute wirken als Kampf- und Charakterwerte in das gesamte Spiel hinein. Neben Rasse, Klasse, Level und Ausrüstung bilden sie die Basis der tatsächlichen Charakterentwicklung.

### Aktueller erster Balancing-Stand

#### Kraft

* erhöht den Nahkampfschaden
* erhöht die Tragkraft
* derzeitiger Richtwert für Schaden: etwa **+0,5 % Nahkampfschaden pro Punkt**
* konkrete Balance kann nach Spieltests geändert werden

#### Konstitution

* erhöht das maximale Leben
* derzeitiger Richtwert: **+10 Max-HP pro Punkt**

#### Geschicklichkeit

* erhöht den Fernkampfschaden
* derzeitiger Richtwert: etwa **+0,5 % Fernkampfschaden pro Punkt**
* konkrete Balance kann nach Spieltests geändert werden

#### Intelligenz

* erhöht magischen Schaden
* derzeitiger Richtwert: etwa **+0,5 % magischer Schaden pro Punkt**
* konkrete Balance kann nach Spieltests geändert werden

#### Weisheit

* erhöht das maximale Mana
* derzeitiger Richtwert: **+20 Max-Mana pro Punkt**

#### Glück

* erhöht die Chance auf kritische Treffer
* derzeitiger Richtwert: **+0,1 Prozentpunkte Crit-Chance pro Punkt**
* Glück ist damit ausdrücklich ein **Kampfwert** und nicht primär ein Loot-Fundwert

#### Ausdauer

* erhöht die mögliche Laufdauer
* erhöht zusätzlich leicht den Rüstungswert
* derzeitiger Richtwert: **+1 % Laufdauer** und etwa **+0,2 % auf den vorhandenen Rüstungswert pro Punkt**
* der Rüstungsbonus verändert den Rüstungswert und ist nicht direkt gleichbedeutend mit Schadensreduktion (die Umrechnung von Rüstungswert in Schadensreduktion bleibt in `Kampfsystem.md` Abschnitt 7 festgelegt)

> **Alle genannten Zahlen sind erster Balancing-Stand und müssen später durch tatsächliche Spieltests überprüft werden.**

---

## 2. Rassen und Attribute

### Grundprinzip

* **Jede Rasse soll jede Klasse sinnvoll spielen können.**
* Rassen geben eine erkennbare natürliche Prägung, sind aber **keine Klassensperre** und sollen nicht die spätere Charakterentwicklung dominieren.
* Rassische Attributunterschiede auf Level 1 sollen typischerweise ungefähr **5 Punkte** und maximal etwa **10 Punkte** betragen.
* Die großen Attributunterschiede entstehen später hauptsächlich durch **Klasse, Level und insbesondere Ausrüstung**.

### Aktuelle Rassenschwerpunkte

**Menschen**

* Allround-Rasse
* vergleichsweise gleichmäßige Attribute
* keine extreme Spezialisierung

**Elfen**

* **Geschicklichkeit**
* **Glück**
* (bestehende ältere Angabe „Geschicklichkeit + Ausdauer" wird hiermit korrigiert)

**Andorer**

* **Intelligenz**
* **Weisheit**
* (bestehende ältere Angabe „Glück + Weisheit" wird hiermit korrigiert)
* Glück beeinflusst nicht primär das Finden seltener Gegenstände – Glück ist nun ein Kampfwert und erhöht die Crit-Chance

**Luzilla**

* **Geschicklichkeit**
* **Ausdauer**
* vorgesehen für Expansion 1

**Mandalonier**

* **Kraft**
* **Konstitution**
* aufgrund ihres großen/kräftigen Körperbaus natürliche **Tank-Tendenz**
* vorgesehen für Expansion 2
* auch hier **keine Tank-Pflicht oder Klassensperre**

Die konkrete Werteverteilung je Rasse wird beim eigentlichen Rassen-/Charaktersystem umgesetzt und über Spieltests abgeglichen.

---

## 3. Ausrüstung und Attribute

* **Waffen, Rüstungen und Accessoires können Grundattribute erhöhen.**
* Ausrüstung soll im späteren Spiel einen **großen Teil der tatsächlichen Attributentwicklung und des Finetunings** ausmachen.
* Dadurch bleibt die Rasse eine Grundprägung, während der Spieler seinen Charakter über Ausrüstung stark beeinflussen kann.
* Gecraftete Gegenstände können abhängig vom bestehenden Crafting-/Qualitätssystem unterschiedliche bzw. zusätzliche Attributwerte erhalten.
* Ob Attributboni langfristig ausschließlich ganzzahlig bleiben, wird erst durch Spieltests entschieden.

Die Item-strukturellen Eigenschaften (Waffen, Rüstung, Accessoires) bleiben in `item_properties.md` definiert; dieses Dokument ergänzt die Zuordnung von Attributboni zu Ausrüstung.

---

## 4. HP- und Mana-Regeneration

HP und Mana verwenden **dasselbe technische Grundprinzip** der Regeneration.

### Grundformel

```text
Effektive Regeneration =
(Basisregeneration + additive Regenerationsboni)
× Zustandsmultiplikator
```

* Regeneration erfolgt als **absoluter Wert pro Sekunde** und nicht als Prozentsatz des maximalen Ressourcenpools.
* Dadurch können Max-HP und Max-Mana im Highlevel stärker wachsen als die Regeneration: Ein großer Ressourcenpool benötigt relativ länger, um vollständig regeneriert zu werden.

---

## 5. Zustandsmultiplikatoren

Für HP und Mana gelten **dieselben Grundmultiplikatoren**:

| Zustand | Multiplikator |
|---|---|
| im Kampf | **15 %** |
| außerhalb des Kampfes, stehend/normal | **100 %** |
| außerhalb des Kampfes, sitzend | **125 %** |

**Wichtig:** Im Kampf gelten **immer 15 %**, unabhängig davon, ob der Charakter steht oder sitzt. Der Sitzbonus darf während eines Kampfes **nicht** greifen.

Damit kann Sitzen im Kampf nicht Heil-/Manatränke oder andere Ressourcenmechaniken ersetzen.

---

## 6. Klassen-Basisregeneration

Als erster Balancing-Ausgangspunkt für **Level 1** gelten:

| Klasse | HP/s | Mana/s |
|---|---|---|
| Kämpfer | 6 | 3 |
| Magier | 3 | 7 |
| Priester | 4 | 6 |
| Kundschafter | 5 | 4 |

Als erster einfacher Richtwert für das **Levelwachstum** ist vorgesehen:

* **+0,2 HP-Regeneration pro Level**
* **+0,2 Mana-Regeneration pro Level**

Auch diese Werte sind ausdrücklich **erste Balancingwerte** und später durch Spieltests veränderbar.

Klasse bestimmt damit den **Regenerationsschwerpunkt**, während der maximale Ressourcenpool separat über Charakter-/Klassenbasis, Attribute, Level, Ausrüstung und Buffs wächst.

---

## 7. Regenerationsboni

Regenerationsboni werden grundsätzlich als **absolute additive Werte** behandelt.

**Beispiel Mana:**

```text
Basis-Mana-Regeneration:      12 Mana/s
Getränk/Buff:                 +4 Mana/s
Summe:                        16 Mana/s
```

Danach wird der Zustandsmultiplikator angewendet:

```text
normal außerhalb des Kampfes:     16 Mana/s
sitzend außerhalb des Kampfes:    20 Mana/s
im Kampf:                          2,4 Mana/s
```

Dadurch können Nahrung, Getränke, Ausrüstung und Buffs auch im Highlevel relevante Regenerationsboni liefern.

---

## 8. Genauigkeit

* Regenerationswerte und entsprechende Boni dürfen **Dezimalwerte** besitzen.
* Für sichtbare Werte reicht **eine Nachkommastelle**, z. B.:

```text
+8,9 HP/s
+14,2 Mana/s
```

* Intern soll **sauber gerechnet** werden; unnötige Rundungsfehler müssen vermieden werden.

---

## 9. Essen, Getränke und Tränke (konzeptionelle Abgrenzung)

Die eigentliche Implementierung des Consumable-Systems erfolgt **später**. Dieses Dokument legt bereits die konzeptionelle Abgrenzung fest.

Es gibt zwei grundlegend unterschiedliche Wirkprinzipien:

### Speisen/Getränke mit Dauerwirkung

* wirken über eine bestimmte **Dauer**
* **Essen** erhöht primär die **HP-Regeneration**
* **Getränke** wie Tee oder Kaffee können primär die **Mana-Regeneration** erhöhen
* hochwertige Varianten können zusätzliche positive Effekte besitzen
* schlecht gecraftete Varianten können auch **Mali** verursachen
* **Crafting-Qualität** kann konkrete Regenerationswerte beeinflussen
* gecraftete Regenerationswerte dürfen **eine Nachkommastelle** besitzen (siehe Abschnitt 8)

### Direkte Heil-/Manatränke

* stellen beim Benutzen **sofort** einen Teil von HP bzw. Mana wieder her
* dies ist **keine Regeneration**, sondern direkte Ressourcenwiederherstellung

### Inventarplätze

Essen und Tränke/Getränke sollen im Inventarsystem **eigene dafür vorgesehene Item-/Verbrauchsplätze** erhalten und später manuell oder automatisch benutzt werden können.

Die genaue Consumable-Logik wird separat entwickelt und soll jetzt noch nicht implementiert werden.

---

## 10. Mana-Progression

* Stärkere bzw. spätere Fähigkeiten dürfen **höhere Manakosten** besitzen.
* Die konkreten Manakosten werden **nicht jetzt festgelegt**, sondern erst bei der Ausarbeitung der jeweiligen Fähigkeiten.
* Spieler sollen später über **Finetuning und Meisterbücher** unter anderem die Möglichkeit erhalten können, **Manakosten bestimmter Fähigkeiten zu reduzieren** und dadurch in längeren Kämpfen effizienter zu werden.
* Dies muss mit dem bestehenden Ability-/Mastery-Konzept konsistent bleiben (siehe `Ability-System.md` Abschnitte 2, 9, 11 und 12).

---

## 11. Architektur-/Balancing-Grundsatz

* Konkrete Balancezahlen nach Möglichkeit **daten-/contentgetrieben** halten und nicht unnötig fest in die Realm-Kampflogik einbauen.
* Wir wollen später Werte verändern können, **ohne die grundlegende Architektur neu entwickeln** zu müssen.
* Die in Abschnitt 1 genannten Richtwerte sind erste Balancingwerte; ihre spätere Anpassung darf keinen Architekturumbau erfordern.

---

## Querverweise

| System | Dokument | Bezug |
|---|---|---|
| Kampf-Grundsystem | `Kampfsystem.md` | Ressourcen (HP/Mana), Rüstung und Schadensreduktion, Trefferauflösung inkl. kritischer Treffer, Aggro. |
| Klassensystem | `Klassensystem.md` | Grundklassen und Rollen; Klassen-Basisregeneration (Abschnitt 6 dieses Dokuments). |
| Ability-System | `Ability-System.md` | Mana-Kosten, Cooldowns, Fähigkeitsqualität, Meisterschaft (inkl. Meisterbücher), datengetriebene Definitionen. |
| Items | `item_properties.md`, `inventory_system.md` | Attributboni auf Ausrüstung, Consumable-Slots. |
| Crafting | `Crafting_Grundprinzip.md`, `Handwerksystem.md`, `Handwerks_und_Sammelsystem.md` | Qualitätsabhängige Regenerations-/Attributwerte, Essen/Getränke/Tränke. |
| Rassen | `Rassen-Fraktionen.md`, Rasse-Dokumente (`Rasse_*.md`, `exp*_Rasse_*.md`) | Rassische Attributschwerpunkte (Abschnitt 2 dieses Dokuments). |