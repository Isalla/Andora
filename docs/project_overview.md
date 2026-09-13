# Andora – Projektübersicht

## Status

**Konzept / Entwicklung**

Andora ist ein serverautoritäres **2D Fantasy MMORPG** mit einer persistenten gemeinsamen Spielwelt.

Der Schwerpunkt liegt nicht nur auf klassischen MMORPG-Systemen wie Quests, Crafting, Gruppen und Raids, sondern auf einer Welt, deren NPCs als individuelle Personen existieren, Informationen weitergeben, Beziehungen entwickeln und auf Spieler sowie Ereignisse reagieren können.

---

## Grundsatz: Keine Monetarisierung

Andora ist vollständig kostenlos spielbar und wird nicht auf Geldverdienen durch das Spiel ausgerichtet.

Ausdrücklich ausgeschlossen sind:

* kaufbare Items, Ausrüstung oder Ingame-Währung gegen Echtgeld
* XP-, Loot- oder Progressions-Booster gegen Echtgeld
* bezahlbare Komfort-, Inventar- oder Charaktervorteile
* exklusive Inhalte, Charaktere oder Fähigkeiten gegen Echtgeld
* Pay-to-Win oder Pay-for-Progress

Freiwillige Spenden sind erlaubt. Sie dürfen ausschließlich nicht spielrelevante Anerkennung geben, zum Beispiel eine Donator-Kennzeichnung auf der Website oder ein optionaler, rein kosmetischer Ingame-Titel.

Die offizielle Andora-Website soll später eine freiwillige Möglichkeit zur finanziellen Unterstützung des Projekts anbieten. Diese ist ausdrücklich kein Shop und keine Gameplay-Monetarisierung; Zahlungsanbieter, Beträge, technische Umsetzung und organisatorische/rechtliche Ausgestaltung sind noch nicht festgelegt.

Ingame-Wirtschaft (Gold, Händler, Auktionshaus, Handel, Crafting) bleibt ein reguläres Spielsystem.

Die verbindliche Definition steht in `Monetarisierung_und_Donations.md`.

---

# 1. Projekt

**Name:** Andora
**Genre:** 2D Fantasy MMORPG
**Client:** Godot Engine 3.5 (verbindlicher Gameplay-Referenzclient / Godot-First); langfristig mehrere offizielle Clients möglich (Browser, möglicher Unreal-PC-Client, weitere) – gemeinsame Realms, identische Inhalte, Unterschiede nur in Darstellung (Details: `Mehrere_Offizielle_Clients.md`)
**Server:** fünf getrennte Andora-Serverdienste (API/Auth in Go, Login, Realm in Rust, Coordinator, Voice)
**Datenbank:** MariaDB (auth, realm_state_<realm>; keine zentrale world_data)
**Zielplattformen:** linux-amd64 und linux-arm64 (Debian/Linux)
**Content/Scripting:** Lua
**KI:** providerunabhängige Runtime-KI über den Coordinator; lokale Standard-/Basislösung: Ollama / lokale Sprachmodelle (externe KI-Provider später anbindbar, keine Cloud-Pflicht)

---

# 2. Plattformen

Die vorgesehenen Clientplattformen von Andora sind:

Raspberry Pi
Webbrowser

Der Raspberry Pi ist die primäre Referenz für die Leistungsanforderungen des Clients. Die Darstellung und Clientlogik werden deshalb bewusst ressourcenschonend entwickelt.

Die Webversion soll dieselbe Spielwelt und dieselben serverseitigen Systeme verwenden. Gameplay- und Weltlogik dürfen deshalb nicht von einer bestimmten Clientplattform abhängig sein.

Andora darf langfristig mehrere offizielle Clients besitzen (beispielsweise Godot-Client für Raspberry Pi, Browser-Client, möglicher Unreal-Engine-PC-Client, weitere zukünftige Clients). Diese sind keine unterschiedlichen Versionen des Spiels: Alle Clients verbinden sich mit denselben normalen Realms, es gibt keine getrennten PC-, Pi-, Browser- oder UE-Realms, und Spieler unterschiedlicher Clients spielen gemeinsam in derselben Welt. Alle offiziellen Clients bieten identische Inhalte; Unterschiede gelten ausschließlich für Darstellung und Präsentation. Der Godot-/Raspberry-Pi-Client ist der verbindliche Gameplay-Referenzclient (Godot-First: Realm → Godot-Referenzclient → weitere Clients). Die verbindliche Entscheidung steht in `Mehrere_Offizielle_Clients.md`.

Die Andora-Serverdienste laufen unabhängig von den Clientplattformen auf Debian-/Linux-Servern. Sie können eigenständig auf unterschiedlichen Servern betrieben werden. Die Zielarchitekturen sind mindestens `linux-amd64` und `linux-arm64`. Die Serverdienste, ihre Trennung und der Betrieb sind in `architecture.md`, `Auth_API_Architektur.md`, `Login_Realm_Architektur.md` und `Deployment_Betriebsarchitektur.md` beschrieben.

Pi, Web und weitere Clients sind unterschiedliche Zugänge zu derselben persistenten Andora-Welt.

Ebenso gilt: Alle unterstützten offiziellen Clients eines Realms verwenden dasselbe Ruleset. Es gibt keine Godot-, Browser- oder UE-spezifischen Rulesets (siehe Realm-Rulesets in `Login_Realm_Architektur.md`).

---

# 3. Grundidee

Andora verbindet klassische MMORPG-Systeme mit einer dynamischen persistenten Welt.

Spieler sollen nicht nur Aufgaben aus einer statischen Liste abarbeiten.

Sie sollen Teil einer Welt sein, in der:

* NPCs individuelle Personen sind
* NPCs Beziehungen zu Spielern entwickeln
* NPCs nur Informationen kennen, die sie tatsächlich erhalten haben
* Informationen zwischen NPCs weitergegeben werden können
* NPCs reisen und ihren Aufenthaltsort verändern
* World Events Auswirkungen auf die Welt besitzen
* Spielerhandlungen spätere Begegnungen beeinflussen können
* KI Dialoge und Reaktionen dynamischer gestaltet

---

# 4. Serverautorität

Andora verwendet eine serverautoritative Architektur.

Der Server bestimmt den tatsächlichen Zustand der Spielwelt.

Dazu gehören unter anderem:

* Spielerpositionen
* NPC-Positionen
* Combat
* Fähigkeiten / Ability-System (siehe `Ability-System.md`)
* Items
* Inventory
* Quests
* Beziehungen
* NPC-Wissen
* Reisen
* Crafting
* World Events
* Gruppen
* Raids
* Szenen
* persistente Weltzustände

Der Client stellt diese Informationen dar und sendet Spieleraktionen an den Server.

> **Der Server ist die Quelle der Wahrheit.**

Der Realm definiert damit die tatsächliche Spielwelt und das Gameplay; die Clients stellen denselben Realm-Zustand unterschiedlich dar.

> **Der Realm ist Andora. Die Clients sind verschiedene Fenster in dieselbe Welt.**

---

# 5. Welt

> **2D trägt die Welt. 3D setzt die Akzente.**

Andora ist grundsätzlich eine 2D-Spielwelt. Die dauerhaft dargestellte Welt ist überwiegend 2D bzw. aus vorgerenderten Assets; es gibt keine volle Low-Poly-3D-Welt als permanentes Rendermodell.

Stilrichtung ist eine detailreiche, atmosphärische 2D-Welt (Richtung klassischer, vorgerenderter bzw. isometrischer Fantasy-Welten), in der die gesparte Laufzeitleistung für Reichhaltigkeit und Atmosphäre genutzt wird.

3D dient ausdrücklich auch als **Produktionswerkzeug**: 3D-Modelle können aus festgelegten Perspektiven zu 2D-Sprites bzw. 2D-Animationen vorgerendert werden.

Die Darstellung kann durch Techniken wie:

* Layer
* Y-Sortierung
* Transparenz
* Shader
* Licht
* Schatten
* Tiefeneffekte

räumlicher wirken.

Fähigkeiten, Zauber und kurzlebige Effekte werden nach einem **hybriden Ansatz** dargestellt: 2D-Animationen, vorgerenderte 3D-Effekte als 2D-Animation oder leichte echte Laufzeit-3D-Effekte. Für geeignete Effekte kann der Spieler die bevorzugte Darstellung (z. B. 2D oder 3D) desselben Effekts selbst in den Client-Grafikoptionen wählen. Diese Wahl ist rein clientseitig und hat keinerlei Auswirkung auf Schaden, Heilung, Reichweite, Wirkungsradius, Trefferberechnung, Cooldown, Ressourcenverbrauch, Zielauswahl oder serverseitige Kampfregeln – der Realm bleibt für die tatsächliche Spielmechanik autoritativ. Eine 3D-Darstellung ist keine Voraussetzung, um einen Effekt korrekt wahrzunehmen oder darauf reagieren zu können. Details in `Clientdarstellung_und_Performance.md` (§4).

Die eigentliche Welt- und Gameplaylogik bleibt unabhängig davon, ob ein Gebiet isometrisch oder klassisch von oben dargestellt wird.

Die verbindliche Darstellungsvorgabe, die Stil- und Performance-Grundsätze sowie die technische Referenzierung stehen in `Clientdarstellung_und_Performance.md`.

---

# 6. Charaktere

Spieler erstellen ihren eigenen Charakter.

Geplante Völker umfassen:

* Menschen
* Elfen
* Andorer
* Luzilla
* Mandalonier

Die Völker besitzen unterschiedliche visuelle und erzählerische Eigenschaften.

Das aktuelle maximale Charakterlevel ist:

**Level 40**

Spätere Erweiterungen können die Levelgrenze erhöhen.

---

# 7. Quests und Geschichten

Andora besitzt ein serverautoritäres Quest-System.

Mögliche Questziele umfassen:

* Kämpfen
* Sammeln
* Gespräche
* Reisen
* Entdeckungen
* Lieferungen
* Eskorten
* Crafting
* World Events

Questdefinitionen können über Lua bereitgestellt werden.

Der tatsächliche Questfortschritt wird serverseitig kontrolliert und persistent gespeichert.

Die Storytelling-Ebenen (Hauptstory, Nebenmissionen, Rätsel und Weltgeheimnisse, spielerausgelöste Realm-Ereignisse), Bücher als Gameplay, verborgene Questketten und variable persönliche Rätsel sind in `Storytelling_und_Weltgeheimnisse.md` verbindlich definiert.

---

# 8. NPC-System

NPCs sollen ein zentraler Bestandteil von Andora werden.

Ein wichtiger Grundsatz lautet:

> **Jeder NPC ist eine einmalige Person in einer gemeinsamen Welt – kein NPC existiert gleichzeitig an zwei Orten.**

NPCs können unter anderem besitzen:

* Persönlichkeit
* Beruf
* Beziehungen
* Wissen
* Aufenthaltsort
* Tagesablauf
* Dienstleistungen
* soziale Kontakte
* Reisen
* Erinnerungen an relevante Ereignisse

NPC-Erinnerungen werden beim Coordinator persistent gespeichert und gehen bei einem Neustart nicht verloren. Jeder Charakter besitzt zu jedem NPC seinen eigenen unabhängigen Beziehungsstatus. Zusätzlich existiert Shared Knowledge, das allgemein erzählbares Wissen über einen Charakter enthält, getrennt von persönlichen Erinnerungen. Details: `Ki-NPC.md`.

NPCs können auf Spieler und andere NPCs reagieren.

---

# 9. NPC-Wissen

NPCs besitzen keine automatische globale Allwissenheit.

Grundregel:

> **NPCs dürfen nur auf Informationen reagieren, die sie tatsächlich erhalten haben.**

Informationen können beispielsweise entstehen durch:

* eigene Beobachtung
* Gespräche
* andere NPCs
* Spieler
* Boten
* World Events
* berufliche Informationsquellen

Dadurch können unterschiedliche NPCs unterschiedliche Kenntnisse über dieselbe Welt besitzen.

NPCs unterscheiden zwischen persönlich erlangtem Wissen und allgemein gehörtem Wissen (Shared Knowledge). Ein NPC, der einen Spieler nie persönlich getroffen hat, kann trotzdem von ihm gehört haben, zum Beispiel über Fraktions-, Regional- oder Bekanntheitsgrenzen hinweg. Details: `Ki-NPC.md`.

---

# 10. NPC-Beziehungen

Spieler können Beziehungen zu NPCs entwickeln.

Beziehungen können beeinflussen:

* Preise
* Dienstleistungen
* Dialoge
* Vertrauen
* Hilfsbereitschaft
* Crafting-Aufträge
* besondere Möglichkeiten
* Informationsweitergabe
* Ablehnung

Ein Schmied, bei dem ein Spieler regelmäßig arbeitet und einkauft, kann diesen Spieler beispielsweise anders behandeln als einen Fremden.

---

# 11. KI-System

KI wird serverseitig zentral über den Coordinator an KI-Provider angebunden; im lokalen Betrieb ist Ollama die Standard-/Basislösung.

Die Runtime-KI-Architektur ist providerunabhängig dokumentiert (Details: `ai_system.md`, `Coordinator.md`): Später können auch externe KI-Provider über deren API angebunden werden, ohne Realm oder Gameplay-Systeme grundlegend umbauen zu müssen und ohne eine zwingende Cloud-Abhängigkeit zu erzeugen.

Sie dient unter anderem für:

* NPC-Dialoge
* natürliche Reaktionen
* Interpretation von Spielerbefehlen
* narrative Dialoge
* personalisierte Szenen
* Interpretation natürlicher Crafting-Wünsche

Die KI besitzt keine direkte Kontrolle über die Spielwelt.

> **Die KI erzählt mit den Fakten der Welt – sie bestimmt die Fakten der Welt nicht.**

Gameplayentscheidungen werden vom Realm-Server (Rust) validiert.

---

# 12. Crafting

Crafting soll ein bedeutender Bestandteil der Welt werden.

Geplant sind unter anderem:

* verschiedene Materialien
* unterschiedliche Qualitätsstufen
* Rezepte
* Handwerker-NPCs
* individuelle Aufträge
* Beziehungen zu Handwerkern
* besondere Gegenstände
* seltene Masterwork-Ergebnisse

Spieler können einem Handwerker später auch natürlich beschreiben, was sie herstellen lassen möchten.

Die KI interpretiert den Wunsch.

Der Server entscheidet anschließend, ob der Gegenstand möglich ist und welche:

* Materialien
* Kosten
* Eigenschaften
* Qualität
* Herstellungszeit

gelten.

---

# 13. Items und Inventory

Gegenstände besitzen ein serverseitiges Item-System.

Geplant sind unter anderem:

* Equipment
* Verbrauchsgegenstände
* Materialien
* Questgegenstände
* besondere Gegenstände
* verschiedene Qualitätsstufen
* Gewicht

Rucksäcke können unterschiedliche Kapazitäten besitzen.

`weight` beschreibt das tatsächliche Gewicht eines Gegenstands.

1 Item / 1 Stack = 1 Inventarslot (Item System V1; siehe `item_properties.md`).

---

# 14. Gruppen und Raids

Spieler können gemeinsam Inhalte bestreiten.

Im Grundspiel beträgt die maximale Größe einer normalen Spielergruppe **4 Spieler**. Mit Exp1 wird diese maximale Gruppengröße auf **5 Spieler** erhöht (siehe `exp1_Unterwelt.md`, Abschnitt 56). Raidgrößen und Raidgruppen werden dadurch nicht neu definiert.

Die Gruppen- und Raidstruktur ist auch Bezugspunkt für das Heldenrad (siehe `Heldenrad.md`): Das Heldenrad besitzt kein raidweites gemeinsames Heldenrad, sondern arbeitet in normalen Gruppen innerhalb der jeweiligen Gruppe.

Geplant sind:

* Gruppen
* Gilden
* Gruppenaufgaben
* World Events
* Raids
* Bossbegegnungen

Raids bleiben bewusst Spielerinhalt.

NPC-Söldner oder KI-Begleiter dürfen keine echten Spieler in einem Raid ersetzen.

NPCs können einen Raid jedoch außerhalb des eigentlichen Kampfes unterstützen.

---

# 15. Begleiter und Söldner

Spieler können von NPC-Begleitern oder Söldnern unterstützt werden.

Diese können beispielsweise:

* folgen
* kämpfen
* schützen
* heilen
* auf Befehle reagieren

Natürliche Sprachbefehle können durch ein kleines KI-Modell in strukturierte Spielbefehle übersetzt werden.

Die tatsächliche Aktion wird anschließend vom Server validiert.

---

# 16. Kommunikation

Andora besitzt unterschiedliche Kommunikationsbereiche.

Geplant sind:

* Say
* Nähe
* Lokal
* Gruppe
* Gilde

Sprachkommunikation kann ebenfalls integriert werden.

Begleiterbefehle können über einen privaten Push-to-Talk-Kanal gegeben werden, sodass andere Spieler weder Sprachbefehl noch Transkription hören.

NPCs in entsprechender Wahrnehmungsreichweite können öffentliche `Say`-Kommunikation wahrnehmen und darauf reagieren.

---

# 17. World Events

Die Welt kann durch dynamische Ereignisse verändert werden.

Beispiele:

* Angriffe auf Städte
* besondere Gegner
* regionale Ereignisse
* öffentliche Feste
* Belagerungen
* Veränderungen von Gebieten
* gemeinschaftliche Aufgaben

World Events können:

* NPCs beeinflussen
* Reisen auslösen
* Informationen verbreiten
* Quests verändern
* Szenen auslösen
* Spieler zusammenführen

Vorbereitete, durch Spieler-Entdeckungen oder -Handlungen ausgelöste Realm-Ereignisse und die individuellen Realm-Chroniken (jeder Realm erzählt dieselbe Welt, aber durch seine eigene Geschichte) sind in `Storytelling_und_Weltgeheimnisse.md` (§ 10–11) verbindlich definiert.

---

# 18. Dynamic Scene System

Andora besitzt ein serverautoritäres Szenensystem.

Es kann beispielsweise verwendet werden für:

* Quest-Szenen
* Bossbegegnungen
* World Events
* Hochzeiten
* besondere NPC-Ereignisse
* Story-Momente

Der Realm-Server (Rust) kontrolliert die Mechanik.

Lua definiert das Drehbuch.

Godot stellt die Szene dar.

Die KI kann ausdrücklich freigegebene Dialogteile improvisieren.

> **Eine Szene soll zuverlässig gescriptet sein, sich aber nicht gescriptet anfühlen.**

---

# 19. Häuser und Bauen

Spieler sollen eigene Häuser besitzen bzw. bauen und verwenden können.

Das System ist Bestandteil der langfristigen Weltplanung.

Bei Gebäuden kann die 2D-Darstellung beispielsweise Dächer und Wände beim Betreten transparent ausblenden, damit Innenräume sichtbar werden.

Die genaue Hausbau- und Besitzarchitektur wird separat definiert.

---

# 20. Gilden

Gilden bilden einen sozialen Bestandteil der persistenten Welt.

Geplant sind unter anderem:

* gemeinsame Aktivitäten
* Gildenaufgaben
* Gruppenorganisation
* Raids
* soziale Systeme

Weitere Gildenmechaniken werden separat spezifiziert.

---

# 21. Exploration

Erkundung ist ein wichtiger Bestandteil von Andora.

Spieler sollen unterschiedliche:

* Regionen
* Städte
* Dörfer
* Landschaften
* Dungeons
* besondere Orte

entdecken können.

Gebietsbetritt kann serverseitige Ereignisse, Quests oder Dynamic Scenes auslösen.

---

# 22. Content-Architektur

Andora trennt Engine, Inhalte, Persistenz und KI.

```text id="q7s9k0"
Realm-Server (Rust)
→ Engine, Regeln und Autorität

Lua
→ Gameplay-Inhalte und Definitionen

MariaDB
→ persistenter Zustand

KI-Provider (lokal: Ollama)
→ Sprache, Interpretation und kontrollierte Improvisation

Godot (Referenzclient)
→ Client und Darstellung
```

Diese Trennung soll ermöglichen, Inhalte später zu erweitern, ohne zentrale Servermechaniken ständig verändern zu müssen.

Die serverseitige Lua-Content-Scripting-Schicht (Script-Domänen, Sicherheitsgrenzen, Eventmodell) ist in `Lua-Scripting-System.md` spezifiziert.

Fähigkeiten, Qualitätswerte und Meisterschaftsvarianten werden Lua- bzw. datengetrieben definiert; der Realm bleibt autoritativ (siehe `Ability-System.md` Abschnitt 14).

Weitere offizielle Clients (Browser, möglicher Unreal-PC-Client, weitere) folgen derselben Trennung; Details in `Mehrere_Offizielle_Clients.md`.

---

# 23. Technisches Ziel

Andora soll trotz dynamischer Welt und KI-Systemen ressourcenschonend bleiben.

Dafür gelten unter anderem folgende Prinzipien:

* 10-Hz-World-Tick
* Area-of-Interest-System
* begrenzte Clientdarstellung
* KI niemals pro Tick
* KI eventbasiert
* asynchrone KI-Provider-Anfragen
* serverseitige AI-Budgets
* persistente Daten nur dort speichern, wo sie benötigt werden
* Weltlogik funktioniert auch ohne verfügbaren KI-Provider
* **Performance-Grundsatz:** 60 FPS als Ziel, Untergrenze 30 FPS unter definierter hoher Last (Raspberry Pi 4 als primäre Referenzplattform für den Godot-Referenzclient; „definierte hohe Last" bezieht sich auf die Client-Schutzmechanismen in `architecture.md`)

Die verbindlichen Darstellungs- und Performance-Grundlagen (2D-Welt, 3D-Akzente, hybride Effekte, spielerwählbare 2D-/3D-Darstellung gleicher Effekte inklusive strikter Trennung von Darstellung und Gameplay, Referenzliteratur) stehen in `Clientdarstellung_und_Performance.md`. Diese gelten für den Godot-Referenzclient; weitere offizielle Clients sind nicht auf die Raspberry-Pi-Leistung beschränkt, dürfen aber keine gameplayrelevanten Unterschiede erzeugen (`Mehrere_Offizielle_Clients.md`).

---

# 24. Ziel

Andora soll sich nicht nur wie eine Sammlung klassischer MMORPG-Systeme anfühlen.

Das langfristige Ziel ist eine Welt, in der Spieler das Gefühl bekommen:

> **Die Welt wartet nicht nur darauf, dass der Spieler eine Quest anklickt – sie existiert auch ohne ihn.**

NPCs haben Orte, Beziehungen und Wissen.

Ereignisse können Auswirkungen haben.

Informationen können sich verbreiten.

Spieler können Beziehungen und Geschichten aufbauen.

KI unterstützt diese Welt dabei, natürlicher auf ihre Bewohner und Spieler zu reagieren, ohne selbst die Kontrolle über die Spielregeln zu übernehmen.
