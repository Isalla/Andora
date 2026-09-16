# Andora – Visual-Novel-Dialog- und Szenendarstellung

## Status

**Konzeptdokument – NICHT implementiert.**

Dieses Dokument hält die verbindlichen Regeln für die **Visual-Novel- (VN-) Darstellung**
von Dialogen und Szenen in Andora fest.

Es wird **kein Code** beschrieben, der bereits existiert. Es werden **keine endgültigen
Netzwerkpakete, APIs, Rust-Strukturen, Lua-APIs, Datenbankformate oder Dialogbaumformate**
festgelegt. Die späteren technischen Details (Abschnitt 17) bleiben bewusst offen.

Optionale KI-Dialogsegmente (geskriptete + KI-generierte Teile als Hybridmodell) sind in
Abschnitt 16 beschrieben.

Dieses Dokument erweitert die **bestehende Andora-Architektur**. Es entwirft **kein zweites
paralleles Dialog- oder Cutscene-System**.

Verbindliche Grundlage dieses Dokuments sind die bestehenden Architekturdokumente (kein Teil
dieses Dokuments darf ihnen widersprechen):

- `cutscene_system.md` – Dynamic Scene Engine, Szenenauslösung, Szenenzustände, Client-Aufgabe
- `ai_cutscene_system.md` – NPC Scene Lock und Szenensteuerung (§27)
- `Lua-Scripting-System.md` – Lua als Content-/Orchestrierungsschicht, Hail-/Dialogabläufe (§3.2),
  Zonen-/Regions-/Cutscene-Scripts, Autoritätsgrenzen (§6)
- `Quest-System.md` – Questdialog (Annahme §27.3, Abgabe §27.4), Talk-Objective
- `quests_stories.md` – Quest-/Story-Inhalte, Talk-Ziele, Dynamic-Scene-Integration
- `Andora-Studio.md` und `Content-Studio.md` – geplante Studio-/Editor-Struktur (Dialogue Editor)
- `Ki-NPC.md` – NPC-Verhalten, Beziehungen, Wissen (Rolle der Runtime-KI)

---

## 1. Zentrale VN-Leitregel

> **Die Visual-Novel-Darstellung ist keine vom Spiel getrennte Grafik.
> Sie ist die Nah- und Präsentationsansicht derselben Andora-Welt und
> derselben Charaktere.**

VN-Dialog und Cutscene sind dadurch **keine voneinander isolierten Systeme**.

Eine Szene darf Dialogdarstellung, In-World-Handlungen und optionale Story-/CG-Darstellung
**miteinander kombinieren**. Die alte Trennung „Cutscene nur im Client / Dialog nur als
Textbox" gilt nicht: Beides ist Präsentation desselben serverseitig autoritativen
Szenen-/Dialogzustands.

---

## 2. Normale Dialogdarstellung

Während eines Dialogs kann die **normale 2D-Spielwelt sichtbar** bleiben („2D trägt die Welt",
`Clientdarstellung_und_Performance.md`).

Die Präsentation **kann zusätzlich** enthalten:

- Charakterportrait links
- Charakterportrait rechts
- Sprecheridentität
- Dialogtext
- Expression (Gesichtsausdruck)
- Antwortmöglichkeiten

### Wichtige Regel: Portraits ersetzen die Spielfiguren nicht

Die Portraits **ersetzen** die Spielfiguren in der Welt **NICHT**.

Portrait und In-World-Figur repräsentieren **denselben Charakter**
(§7 – Expressions und visuelle Identität).

Die Nah-/Präsentationsansicht ist ein zusätzlicher Darstellungsmodus derselben Welt,
nicht eine getrennte Bühne.

---

## 3. In-World-Aktionen während des Dialogs

Ein Dialog muss die Welt **nicht einfrieren**.

Während Portraits/Text dargestellt werden, können Charaktere in der eigentlichen Spielwelt
passende Aktionen durchführen. Beispiel:

- gehen
- sich umdrehen
- zu einem Objekt laufen
- gestikulieren
- schmieden
- sich setzen
- etwas betrachten
- gegen eine Wand schlagen

Die konkrete Liste erlaubter Aktionen wird hier **NICHT finalisiert**.

### Gültige Grundsätze

- Bestehende Scene-/NPC-Lock-Konzepte (`ai_cutscene_system.md` §27, `cutscene_system.md`)
  bleiben maßgeblich.
- **Gameplayrelevante Weltaktionen bleiben serverautoritativ.** Der Client stellt nur die
  vom Server bestätigte Szene dar.
- Der Server kann die Bewegungen/Aktionen autoritativ freigeben (`SCENE_LOCKED`-Steuerung),
  damit sie konsistent und cheat-sicher sind; die visuelle Animation übernimmt der Client.

---

## 4. Beispiel Borin

Referenzablauf (nicht-technisches Beispiel, **keine** Netzwerkpakete, **keine** API):

```text
Spieler interagiert mit Borin
    ↓
Dialog beginnt
    ↓
Borin-Portrait links
Spieler-Portrait rechts
    ↓
Borin spricht
    ↓
Borins In-World-Figur läuft zum Amboss
    ↓
Borin beginnt zu schmieden
    ↓
Borins Portrait wechselt den Gesichtsausdruck
    ↓
Dialog geht weiter
    ↓
Spieler erhält ggf. Antwortmöglichkeiten
```

Das Beispiel demonstriert das Zusammenspiel von:

- serverautoritativem Dialogzustand
- In-World-Szene (Laufen, Schmieden)
- VN-Präsentation (Portraits links/rechts)
- Expressions
- Antwortmöglichkeiten

Es legt **keine** Datenformate fest.

---

## 5. Darstellungsstufen

Das System muss unterschiedliche Produktionsaufwände erlauben.

### Einfacher NPC

- kann nur ein **neutrales Portrait** besitzen
- normaler Dialog

### Wichtiger NPC

- mehrere Expressions
- ggf. Posen
- stärkere VN-Präsentation

### Story-/Schlüsselszene

- mehrere Charaktere
- In-World-Aktionen
- Expressions/Posen
- optional CG-/Storybild

> **Nicht jeder NPC benötigt vollständige VN-Artwork-Sets.**
> Die Darstellungsstufe ist eine Content-Entscheidung, keine technische Voraussetzung.

---

## 6. Mehrere Charaktere

Szenen dürfen **mehr als zwei Teilnehmer** besitzen.

Die „links/rechts"-Darstellung ist eine **Präsentationsmöglichkeit**, keine technische
Beschränkung auf zwei Charaktere (§2). Eine Szene darf den Wechsel der beteiligten
Sprecher/Charaktere abbilden.

**Pflicht:** Der jeweils sprechende Charakter muss visuell erkennbar gemacht werden können
(z. B. durch hervorgehobenes/aktives Portrait, Sprecherlabel oder ähnliche Kennzeichnung).

Die konkrete UI dafür bleibt Client-/UX-Design und wird hier **nicht finalisiert**.

---

## 7. Expressions und visuelle Identität

**Expressions sind Präsentationsdaten.** Beispiele (nicht verpflichtend, nicht vollständig):

- neutral
- freundlich
- ernst
- wütend
- überrascht

### Identität

> **Expressions dürfen die Identität des Charakters nicht verändern.**

Portrait, Expression, In-World-Sprite und ggf. CG müssen **erkennbar denselben Charakter**
darstellen (§10 – Character Visual ID). Eine Expression ist eine Variante desselben
Charakters, kein neuer Charakter.

---

## 8. Master-Charakter / Referenzbild

Für Andora Studio wird die bestehende NPC-/Dialogue-Editor-Planung (§14) erweitert.

Ein Charakter kann eine **definierte visuelle Identität** besitzen.

Für die Portrait-Erstellung soll später möglich sein:

- aus NPC-Eigenschaften erzeugen
- ein vorhandenes Referenzbild verwenden
- einen bereits vorhandenen Andora-Charakter verwenden (Ableitung von bestehender Identität)

Ein **freigegebenes Master-Portrait** bzw. Referenzmaterial kann als Grundlage für weitere
Expressions/Posen dienen.

Auch **mehrere Referenzbilder** dürfen später unterstützt werden. Beispiel:

- Gesicht
- Ganzkörper
- Seitenansicht

Die Referenzen dienen der **visuellen Konsistenz** (alle Varianten erkennen denselben
Charakter wieder).

Es wird **keine konkrete KI-/Bildgenerator-API** festgelegt.

---

## 9. KI-Generierung im Studio

Andora Studio soll später Portraits/Expressions erzeugen lassen können.

Die Architektur wird **nicht auf einen bestimmten Bildgenerator** festgelegt.

Konzeptioneller Ablauf:

```text
Andora Studio
    ↓
Character Definition / Referenzen
    ↓
Portrait Generator
    ↓
erzeugtes Asset
    ↓
Entwickler prüft / freigibt
    ↓
freigegebenes Master-Asset
    ↓
daraus Expressions / Posen
```

**Der Entwickler behält die Kontrolle über die Freigabe.**

Es wird **keine konkrete externe oder lokale KI** verbindlich festgelegt
(`Andora-Studio.md`, `Content-Studio.md` – lokale KI ohne Anbieterkopplung).

---

## 10. Character Visual ID

**Konzeptanforderung:**

Die verschiedenen visuellen Repräsentationen eines Charakters müssen später **eindeutig
miteinander verknüpft** werden können.

Beispielkonzept:

```text
Borin
 → In-World-Sprite
 → Master-Portrait
 → Expressions
 → optionale Posen
 → optionale CG-Darstellungen
 ```

Diese eine Identität sammelt die konsistenten Varianten von Portrait, Expression,
In-World-Sprite und ggf. CG für denselben Charakter (§7).

**KEINE endgültigen Feldnamen, DB-Schemata oder API-Namen** werden in diesem Dokument
festgelegt (`Character Visual ID` ist ein Konzeptname, keine technische Spezifikation).

---

## 11. CG-/Storybilder

Wichtige Szenen dürfen optional ein **CG-/Storybild** verwenden.

CG ist eine **zusätzliche Präsentationsform** und **kein separates Story-/Dialogsystem**.

Eine Szene kann beispielsweise wechseln zwischen:

- sichtbarer Spielwelt + Portraits
- stärker fokussierter VN-Darstellung
- CG-/Storybild

Der **serverseitige Szenen-/Dialogzustand bleibt davon getrennt**. Ein CG ist eine
Anzeigeform des bestehenden Szenen-/Dialogzustands, nicht dessen Ersatz.

---

## 12. Server / Lua / Client

### Rust Realm (autoritativ)

- autoritative Dialog-/Gameplayentscheidungen
- autoritative relevante Weltaktionen
- Auswahlvalidierung
- Quest-/Gameplay-Konsequenzen
- verwaltet den Dialog- und Szenenzustand (RAM/autoritativ)
- persistiert nur, was laut bestehender Persistenzregeln persistiert werden darf

### Lua (beschreibt/orchestriert)

- beschreibt/orchestriert innerhalb der bestehenden Lua-Architektur (`Lua-Scripting-System.md`)
- darf **keine Gameplay-Autorität** übernehmen (§6 des Lua-Dokuments)
- nutzt die bestehenden Domänen Npc / Interaction / Cutscene
- beschreibt Inhalt (z. B. Dialogstellen, Hail-/Dialogabläufe, Szenenphasen, i18n-Schlüssel),
  fordert Aktionen über kontrollierte APIs an

### Godot-Client (präsentiert)

- Portraitdarstellung
- Expressions
- Textboxen
- Präsentation
- Animation der **bestätigten** In-World-Aktionen
- CG-/Storybilder

### KI (optional)

- optionale **sprachliche Generierung** innerhalb erlaubter Grenzen (Abschnitt 16)
- kein eigenes Dialogsystem, kein Ersatz für geskriptete Inhalte
- Fallback auf geskriptete Fortsetzung bei Nichtverfügbarkeit (Abschnitt 16.6)

### Grenze

> **Der Server rendert KEINE Portraits oder Bilder.**

Der Server stellt höchstens Bild-/Asset-Referenzen (IDs) in seinem autoritativen Zustand zur
Verfügung; die Darstellung übernimmt der Client.

---

## 13. Quest-Integration

Das vorhandene Quest-System wird später an **denselben Dialogfluss** angebunden (`Quest-System.md`
§27, `quests_stories.md`).

Insbesondere zu berücksichtigen:

- Talk-Objective (V1-Zieltyp `talk`)
- Quest anbieten
- Quest annehmen / ablehnen (§27.3)
- Quest abschließen (§27.4)

### Autoritätsgrenzen

- Bestehende Quest-Autoritäts-/Persistenzregeln bleiben unverändert.
- „Ein Gespräch allein bedeutet nicht automatisch, dass ein Questziel abgeschlossen wird"
  (`quests_stories.md` §7).
- Der Server validiert Annahme, Fortschritt und Abschluss weiterhin vollständig selbst
  (§27.3/§27.4, Quest-Ignostic: Fortschritt niemals aus ungeprüftem Clientwert).

**Keine Quest-V1.2b-Implementierung** – dieser Auftrag definiert keine Quest-Logik.

---

## 14. Andora Studio

Es wird **kein neuer separater VN-Editor** erfunden.

Der bereits geplante **Dialogue Editor** sowie NPC-Editor / Cutscene-Bereich von Andora Studio
(`Andora-Studio.md`, `Content-Studio.md`) werden als gemeinsame Content-Werkzeuge weiterdenken.

Ziel:

> Ein Autor kann Dialog, Sprecher, Expressions und Szenenaktionen zusammenstellen,
> ohne dadurch mehrere konkurrierende Systeme pflegen zu müssen.

**Human und AI müssen später dieselben validierten Studio-Operationen verwenden**
(`Andora-Studio.md`, Content-KI – READ/COMPOSE/CREATE in `Content-Studio.md`).

---

## 15. Bestehende Reservierungen (Erweiterungspunkte)

Folgende vorhandene technische Vorbereitungen sind als Erweiterungspunkte zu verstehen:

| Baustein | Status | Ort |
|---|---|---|
| `NPC_TALK` (c2s, ID 6) | reserviert, `{npc_id, text}` (künftig) | `src/realm-rs/src/protocol.rs:16` |
| `NPC_TEXT` (s2c, ID 8) | reserviert (künftig) | `src/realm-rs/src/protocol.rs:44` |
| Talk-Objective | V1-Zieltyp `talk`, Datenmodell vorhanden | `quest.rs`, `Quest-System.md`, `quests_stories.md` |
| Lua-Domänen Npc / Interaction / Cutscene | vorhandene Script-Domänen | `Lua-Scripting-System.md` §3.2, §3.7, §3.8; `lua/domain.rs` |
| NPC Scene Lock | Konzept, Szenensteuerung | `ai_cutscene_system.md` §27, `cutscene_system.md` |
| Hail / Dialogabläufe | Lua-NPC-Scripts, Hail-Vorbereitung | `Lua-Scripting-System.md` §3.2 |

**ABER:**

- **Keine neuen Message-IDs vergeben.**
- `NPC_TEXT` wird noch **nicht final definiert**.
- **Keine endgültigen Payloads** festlegen.
- **Keine endgültige Dialogbaumstruktur** festlegen.
- **Keine endgültigen Lua-APIs** festlegen.

Diese Punkte bleiben bewusst offen (§17).

---

## 16. Optionale KI-Dialogsegmente (Hybridmodell)

**Status: Konzeptergänzung, NICHT implementiert.** Offene technische Details stehen in
Abschnitt 17.

### 16.1 Hybrides Dialogmodell

Andora darf **geskriptete und KI-generierte Dialogsegmente miteinander kombinieren**.

Die KI ist dabei eine **optionale sprachliche Erweiterung des bestehenden Dialogsystems**
und **KEIN eigenes Dialogsystem**.

Prinzipbeispiel (kein Datenformat):

```text
KI-generierte persönliche Anrede
    ↓
geskripteter Hauptdialog
    ↓
geskriptete Spielerentscheidung
    ↓
serverautoritative Quest-/Storyfolge
    ↓
optional KI-generierter Abschied
```

Der geskriptete Teil trägt den Dialog; die KI ergänzt nur sprachliche Segmente an
freigegebenen Stellen.

### 16.2 Aktivierung pro NPC / Dialogsegment

KI-Unterstützung muss konzeptionell **gezielt aktivierbar** sein:

- für einzelne NPCs aktivierbar/deaktivierbar
- nur für bestimmte Dialogsegmente verwendbar (z. B. Anrede, Abschied)

Beispiel Borin:

| Segment | KI |
|---|---|
| Anrede | aktiviert |
| Hauptdialog | deaktiviert |
| Abschied | deaktiviert |

> Es werden keine endgültigen Config-Strukturen, Feldnamen oder Flags festgelegt
> (Abschnitt 17).

### 16.3 Testweise Einführung

KI-Dialogfunktionen sollen **schrittweise an einzelnen NPCs** getestet werden können
(z. B. Borin als Test-NPC).

Spätere reale Messungen und Spielerfahrung zeigen, ob und wie stark KI-Dialogfunktionen
eingesetzt werden.

> Es wird keine feste Anzahl von KI-NPCs festgelegt.

### 16.4 Sprachliche Personalisierung

Innerhalb des **freigegebenen Kontextes** darf die KI sprachlich personalisieren und dabei
Informationen verwenden, die der NPC gemäß dem bestehenden NPC-/Memory-System tatsächlich
kennen darf (`Ki-NPC.md` – persönliche Erinnerungen, Beziehungen, Wissen):

- Spielername
- bestehende NPC-Spieler-Beziehung
- zulässige Erinnerungen an frühere Begegnungen

> **Die KI darf daraus KEINE neuen autoritativen Gameplay-Fakten erzeugen.**

### 16.5 Autoritätsgrenze

KI-generierter Text darf insbesondere **NICHT selbstständig**:

- Questzustände verändern
- Questabschluss behaupten
- Items vergeben/entfernen
- Gold vergeben/entfernen
- Storyflags setzen
- neue autoritative Weltzustände erzeugen
- Gameplay-Konsequenzen bestimmen

> **Rust Realm und die definierten Dialog-/Gameplay-Scripts bleiben autoritativ**
> (§12 dieses Dokuments, `Lua-Scripting-System.md` §6, `Quest-System.md`).

### 16.6 Fallback

KI darf **keine Voraussetzung** dafür sein, dass ein Dialog funktioniert.

Falls die KI:

- nicht erreichbar ist,
- zu lange benötigt,
- fehlschlägt oder
- keine verwendbare Antwort liefert,

muss der Dialog mit einem **definierten geskripteten Fallback** fortgesetzt werden können.

Beispiel Borin:

```text
Spieler spricht Borin an
    ↓
KI-Anrede erfolgreich → persönliche Anrede
ODER
KI nicht verfügbar → Standardanrede (geskriptet)
    ↓
danach derselbe geskriptete Hauptdialog
```

> Keine konkrete Timeout-Zahl wird festgelegt (Abschnitt 17).

### 16.7 Leistung / Skalierung

Der Umfang der KI-Nutzung wird **nicht jetzt endgültig festgelegt**.

Später sollen reale Server-/KI-Messungen berücksichtigt werden, z. B.:

- KI-Anfragen pro Zeitraum
- Queue-Länge
- Antwortlatenz
- GPU-Auslastung
- VRAM-Auslastung
- Tokens pro Sekunde

Abhängig von Hardware, Spielerzahl und Messwerten muss die KI-Nutzung **reduziert oder
erweitert** werden können.

> Das grundlegende Dialog-/Quest-/VN-System muss auch bei **vollständig deaktivierter
> Dialog-KI** funktionieren.

### 16.8 Keine neue Architektur

**Keine neue KI-Dialogengine** wird entworfen.

Die bestehende Architektur bleibt:

- **Rust Realm** → Autorität
- **Lua** → Beschreibung/Orchestrierung
- **KI** → optionale sprachliche Generierung innerhalb erlaubter Grenzen
- **Godot** → Präsentation
- **Andora Studio** → Content-Erstellung/Konfiguration

Das vorhandene KI-NPC-/Memory-Konzept (`Ki-NPC.md`) wird berücksichtigt und **nicht
dupliziert**.

---

## 17. Bewusst offen / NICHT festgelegt

- konkrete Rust-Codestruktur (Dialogzustand, Dialogobjekt, Handler)
- endgültiges Netzwerkprotokoll / Paketpayloads (auch `NPC_TEXT`)
- endgültiges Dialogbaumformat / Dialoglogik
- endgültige Expression-/Portrait-Registry
- endgültige Character-Visual-ID-Struktur und Feldnamen
- endgültige Asset-Dateiformate (Portraits, Expressions, CG)
- konkrete Bild-/KI-Generator-Anbindung
- konkrete Lua-API-Namen oder Lua-Scriptaufbau
- Liste erlaubter In-World-Aktionen
- konkrete UI/UX der Antwortauswahl und Sprecherkennzeichnung
- Quest-V1.2b-Implementierung (ausdrücklich nicht Teil dieses Auftrags)
- konkrete KI-Dialoganbindung (Modell/Endpunkt), KI-Dialog-Timeout und
  Antwortqualitätsregeln (Abschnitt 16.6)
- Activierungskonfiguration pro NPC/Dialogsegment (Config-Strukturen, Feldnamen/Flags,
  Abschnitt 16.2)
- feste Anzahl/Liste von KI-Test-NPCs (Abschnitt 16.3)
- konkrete Messschwellen/Kapazitätsgrenzen für KI-Skalierung (Abschnitt 16.7)

---

## 18. Beziehung zu bestehender Dokumentation

| Dokument | Rolle |
|---|---|
| `cutscene_system.md` | Dynamic Scene Engine, Szenenzustände, Trigger, Client-Aufgabe; VN nutzt diesen Rahmen |
| `ai_cutscene_system.md` | NPC Scene Lock (§27) für In-World-Aktionen während Szenen |
| `Lua-Scripting-System.md` | Lua-Grenzen, Domänen Npc/Interaction/Cutscene, Hail/Dialogabläufe |
| `Quest-System.md` | Questdialog (Annahme/Abgabe), Talk-Objective, Questautorität |
| `quests_stories.md` | Story-/Questinhalt, Talk-Ziele, KI-/Scene-Integration |
| `Andora-Studio.md` | geplanter Dialogue Editor, modulares Studio, Studio-KI |
| `Content-Studio.md` | Arbeitsbereiche Dialoge/Cutscenes, Content-KI, READ/COMPOSE/CREATE |
| `Ki-NPC.md` | NPC-Verhalten, Runtime-KI, Beziehungen/Wissen |
| `Clientdarstellung_und_Performance.md` | „2D trägt die Welt", Darstellungs-/Performance-Grundgesetz |
| `Mehrere_Offizielle_Clients.md` | Darstellung darf zwischen Clients abweichen, Realm autoritativ |
| `Storytelling_und_Weltgeheimnisse.md` | fixe Story-/Questdialoge als feste redaktionelle Inhalte |

Dieses Dokument ersetzt keine der genannten Dokumente und führt keine widersprüchlichen
Parallelregeln ein.