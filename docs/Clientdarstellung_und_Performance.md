# Clientdarstellung und Performance

**ZWECK:** Verbindliche Grundentscheidung für die Darstellung und Leistung des Andora-Clients (Godot-Referenzclient): wie die Welt gerendert wird, welche Rolle 3D spielt, welche 2D-/3D-Darstellungen Spieler bei Fähigkeiten, Zaubern und kurzlebigen Kampfeffekten selbst wählen können, woran die Performance-Messung ausgerichtet ist und welche technische Fachliteratur als (nicht verbindliche) Referenz herangezogen wird.

Dieses Dokument ist die zentrale, verbindliche Doku für das Thema „Client-Darstellung & Performance" des Godot-Referenzclients. Es ergänzt und präzisiert die allgemeineren Aussagen in `project_overview.md` und `architecture.md`; bei Widersprüchen gilt dieses Dokument. Die dahinterliegende Client-Strategie (mehrere offizielle Clients, gemeinsame Realms, Godot-First) steht in `Mehrere_Offizielle_Clients.md`.

---

## 1. Verbindliche Grundentscheidung (gilt für den Godot-Referenzclient)

> **2D trägt die Welt. 3D setzt die Akzente.**

Diese Grundentscheidung gilt für den verbindlichen Godot-/Raspberry-Pi-Referenzclient. Weitere offizielle Clients dürfen denselben Realm mit anderen Darstellungsprinzipien (bis hin zu vollständiger Echtzeit-3D) darstellen; verbindlich sind dabei die gemeinsamen Regeln aus `Mehrere_Offizielle_Clients.md` (identische Inhalte, kein Gameplay-Vorteil durch Darstellung, Realm autoritativ).

Die dauerhaft dargestellte Spielwelt ist überwiegend **2D bzw. aus vorgerenderten Assets**. Es gibt keine vollständige Low-Poly-3D-Welt als permanentes Rendermodell.

Stilrichtung: eine **detailreiche, atmosphärische 2D-Welt** in der Ästhetik klassischer, vorgerenderter bzw. isometrischer Fantasy-Welten (Richtung Diablo 2 / Ultima Online). Die gesparte Laufzeitleistung wird bewusst genutzt, um die Welt reichhaltig, dekoriert und atmosphärisch zu gestalten – nicht, um sie detailarm zu halten.

3D ist ausdrücklich auch ein **Produktionswerkzeug**: 3D-Modelle dürfen aus festgelegten Perspektiven zu 2D-Sprites bzw. zu 2D-Animationen vorgerendert werden. Es werden also 3D-Assets nicht in Echtzeit gerendert, sondern als Ausgangsmaterial für 2D-Inhalte verwendet.

Für geeignete Fähigkeiten, Zauber und kurzlebige Kampfeffekte kann der Client unterschiedliche visuelle Darstellungen desselben Effekts unterstützen. Für solche Effekte können vorhanden sein:

* eine performante **2D- bzw. vorgerenderte Darstellung**
* eine aufwendigere **Echtzeit-3D-Darstellung**

Der Spieler wählt die bevorzugte Darstellung **selbst in den Client-Grafikoptionen** aus (§4.2). 3D soll die Darstellung verbessern können, aber niemals Voraussetzung für Gameplay oder Client-Kompatibilität sein. Die Grundentscheidung wird damit um eine bewusste Spielerentscheidung erweitert.

---

## 2. Dauerhafte Welt = 2D / vorgerendert

Die dauerhafte Darstellung der Welt und ihrer Objekte stützt sich auf 2D-Assets:

* Boden-/Terrain-Sprites und -Tiles (auch isometrisch perspektiviert, sofern das als Präsentationsstil gewählt wird)
* Charakter-Sets (Rasse, Fraktion, Klasse, Auftreten) als 2D-Sprites/Animationen
* Gebäudesprites und deren Layer (z. B. transparente Dach-/Wand-Elemente beim Betreten von Innenräumen, siehe `project_overview.md` §19)
* Items, Effektplätze und dekorative Objekte als 2D-Assets
* Layer, Y-Sortierung, Transparenz, Tiefeneffekte zur räumlichen Lesbarkeit

Hinweis: „2D/vorgerendert" beschreibt die **Darstellungs- und Renderstrategie**, nicht zwingend einen Ausschluss eines isometrischen Präsentationsstils. Isometrische Darstellungen sind in dieser 2D-Welt durchaus möglich; verbindlich ist, dass die dauerhafte Welt aus 2D-/vorgerenderten Assets besteht.

---

## 3. 3D als Produktionswerkzeug

3D wird in Andora als **Werkzeug** genutzt, nicht als dauerhafte Render-Engine:

* 3D-Modelle (Charaktere, Objekte, Effekte) dienen zur Erstellung konsistenter 2D-Aufnahmen aus gewählten Perspektiven.
* Die Perspektive und Animation werden als 2D-Asset in den Client eingespielt.
* Damit lässt sich das Ziel einer detailreichen 2D-Welt mit kontrolliertem Renderaufwand verbinden.

---

## 4. Fähigkeiten, Zauber und kurzlebige Effekte: hybrider Ansatz mit spielerwählbarer Darstellung

### 4.1 Hybrider Ansatz

Für kurzlebige visuelle Effekte (Fähigkeiten, Zauber, Kampfeffekte, Partikel, kurze Animationen) gilt ein **hybrider Ansatz**:

* **2D-Animationen** als primäres, leistungsschonendes Mittel
* **Vorgerenderte 3D-Effekte**, als 2D-Animation bzw. -Sequenz in den Client eingespielt
* **Leichte echte Laufzeit-3D-Effekte**, sofern das Performance-Budget der Zielplattform es erlaubt

Die konkrete Variante eines einzelnen Effekts wird **anhand realer Performance-Tests auf der Zielplattform** entschieden. Einfache, echte 3D-Fähigkeiten sind ausdrücklich vorgesehen, sofern die Leistung das erlaubt. Es gibt keine verbindliche Vorab-Fixierung, dass jeder Effekt ausschließlich in einer bestimmten Technik umgesetzt wird.

### 4.2 Spielerwahl der Darstellung (2D / 3D)

Für geeignete Fähigkeiten, Zauber und kurzlebige Kampfeffekte unterstützt der Client nach Möglichkeit **unterschiedliche visuelle Darstellungen desselben Effekts**:

* eine performante **2D- bzw. vorgerenderte Darstellung** – auf Performance ausgelegt, insbesondere für schwächere Hardware
* eine aufwendigere **Echtzeit-3D-Darstellung** – sofern die Hardware dies ausreichend performant darstellen kann

Der Spieler wählt die bevorzugte Darstellung **selbst in den Client-Grafikoptionen** aus.

Beispiel – Menüpunkt **Grafik → Fähigkeitseffekte**:

* **2D** – auf Performance ausgelegte Darstellung, insbesondere für schwächere Hardware
* **3D** – aufwendigere visuelle Darstellung, sofern die Hardware dies ausreichend performant darstellen kann

Eine spätere zusätzliche **Auto-Einstellung** (automatische Wahl je nach Hardware oder Last) darf architektonisch möglich bleiben, wird mit dieser Entscheidung aber weder festgelegt noch implementiert. Sie ist zu unterscheiden vom bestehenden `auto perf_mode`, das unabhängig von der Darstellungswahl die Leistungsbudgets degradiert.

Nicht jeder Effekt muss zwingend zwei Varianten besitzen. Welche Fähigkeiten bzw. Effekte eine alternative Darstellung erhalten, wird während der späteren Content- und Effektentwicklung entschieden; erhält ein Effekt mehrere Darstellungen, wählt der Spieler zwischen ihnen (siehe diese §4.2).

### 4.3 Strikte Trennung von Darstellung und Gameplay

2D und 3D sind ausschließlich unterschiedliche **Clientdarstellungen derselben Fähigkeit**. Die Auswahl darf keinerlei Einfluss haben auf:

* Schaden
* Heilung
* Reichweite
* Wirkungsradius
* Hitbox
* Trefferberechnung
* Dauer
* Cooldown
* Ressourcenverbrauch
* Zielauswahl
* Anzahl getroffener Ziele
* serverseitige Kampfregeln

Der Realm bleibt für die tatsächliche Spielmechanik autoritativ. Der Client entscheidet lediglich, **wie ein vom Realm bestätigter Effekt dargestellt wird**. Spieler mit 2D- und 3D-Darstellung müssen deshalb spielmechanisch exakt dasselbe Kampfgeschehen erleben.

Eine 3D-Darstellung darf nicht Voraussetzung dafür sein, eine Fähigkeit korrekt wahrzunehmen oder spielerisch darauf reagieren zu können. Wesentliche Informationen eines Effekts müssen auch in der 2D-Darstellung ausreichend erkennbar bleiben.

Relevante Berührungspunkte: `Kampfsystem.md` (Fähigkeiten, Tempo, Effekte, Realm-Autorität über Kampf), `cutscene_system.md` (Szenen, Animationen, Effekte), `Charaktererstellung_und_Charakterdarstellung.md` (clientseitige Charakter-Sets).

---

## 5. Performance-Grundsatz

Die primäre Referenzplattform für die Client-Performance des Godot-Referenzclients ist der **Raspberry Pi 4**.

* **Ziel:** **60 FPS**
* **Untergrenze:** **30 FPS** unter definierter hoher Last

Der Performance-Grundsatz gilt für den Godot-Referenzclient; andere offizielle Clients sind nicht darauf beschränkt, dürfen aber auch keine gameplayrelevanten Unterschiede erzeugen (`Mehrere_Offizielle_Clients.md`). Die Darstellungswahl (2D/3D) in den Grafikoptionen (§4.2) dient insbesondere dem Zweck, dass schwächere Clients auf die günstigere 2D-Darstellung wechseln können, während leistungsfähigere Systeme optional aufwendigere 3D-Effekte verwenden. Die bereits verbindlichen Client-Performanceziele bleiben davon unberührt.

„Definierte hohe Last" ist kein frei erfundener Wert, sondern bezieht sich auf die bestehenden Client-Schutzmechanismen in `architecture.md`:

* `RENDER_CAP` (48/64 Entities)
* `auto perf_mode`
* Chunk-Texture-Batching
* aktive Effekte innerhalb des jeweils geltenden `perf_mode`-Budgets

Konkrete Degradationsparameter des `perf_mode`-Systems (welche Werte in welchem Modus greifen, ab wann welche Effekte reduziert werden) sind **noch offene Implementierungsdetails** und werden später durch Performance-Tests auf der Zielplattform festgelegt. Sie werden in diesem Dokument nicht vorgezeichnet.

Der Performance-Grundsatz (60 FPS / 30 FPS) ist eine **verbindliche Richtlinie**; die exakten Schwellen und Stufen des `perf_mode` bleiben als offener, testbasierter Punkt.

---

## 6. Technische Referenzen (ergänzend, nicht verbindlich)

Die Referenzliteratur in `references/` dient als **technische Unterstützung und Empfehlung** bzw. als Verfahrenshinweis. Sie begründet keine projektspezifische Designentscheidung, es sei denn Andora übernimmt sie ausdrücklich. Die Andora-Dokumentation in `docs/` bleibt für alle projektbezogenen Entscheidungen verbindlich und hat bei Widersprüchen Vorrang.

Eingangs-/Index: `references/README.md` (Kapitelstartseiten und Andora-Relevanz).

| Buch / Referenz | Kapitel / Abschnitt | Andora-Ableitung | Status / Einstufung |
|---|---|---|---|
| `godotenginegamedevelopmentprojects` | Ch. 7 „Additional Topics" (PDF p. 254) | Pixel-Snap-Empfehlung für 2D-Pixel-Art; geeignet für die 2D-Grafik des Andora-Client (Godot). | Direkt relevant (Godot-3.0-Referenz; API-Unterschiede zu Godot 3.5 prüfen). |
| `godotenginegamedevelopmentprojects` | Ch. 2 „Coin Dash" (PDF p. 36) | Sprite-/Animationsthemen und delta-basierte Bewegung; Muster für 2D-Sprite-Animation im Client. | Direkt relevant (Godot-3.0-Referenz; API prüfen). |
| `godotenginegamedevelopmentprojects` | Ch. 3 „Escape the Maze" (PDF p. 74) | TileSet/TileMap-Nutzung; Muster für 2D-Tiling der Welt. | Direkt relevant (Godot-3.0-Referenz; API prüfen). |
| `godotenginegamedevelopmentprojects` | Ch. 6 „3D Minigolf" (PDF p. 215) | 3D-Performance-Referenz, Normal-Mapping, 2D-fake-3D-Details; Kontext dafür, 3D sparsam einzusetzen bzw. 3D-Details in 2D zu faken. | Konzeptionelle Referenz (3D-Spar-Prinzip). |
| `masteringsfmlgamedevelopment` | Ch. 3 „Make It Rain! – Building a Particle System" (PDF p. 107) | Partikelsysteme und deren Performance-Kosten; Warnung vor unoptimierten Partikel-Pipelines. | Konzeptionell (verfahrens-/leistungsempfehlend für Effekte). |
| `masteringsfmlgamedevelopment` | Ch. 6 „Adding Some Finishing Touches – Using Shaders" (PDF p. 229) | Sprite-/Tile-Sheets statt vieler Texturwechsel, um Performance-Bottlenecks zu vermeiden. | Konzeptionell (Asset-/Rendering-Muster). |
| `masteringsfmlgamedevelopment` | Ch. 8 „Let There Be Light" (PDF p. 307) / Ch. 9 „The Speed of Dark – Lighting and Shadows" (PDF p. 361) | Lighting-/Shadow-Techniken, Normal Maps, shadow-mapping-ähnliche Verfahren als performanzfreundliche Mittel zur räumlichen Tiefe. | Konzeptionell (Lighting-/Shading-Hinweise). |
| `masteringsfmlgamedevelopment` | Ch. 10 „A Chapter You Shouldn't Skip – Final Optimizations" (PDF p. 410) | Profiling als Grundlage für Optimierungsentscheidungen, GPU-/CPU-Bottlenecks, Optimierung von Partikeln und Licht. | Konzeptionell (Verfahren: Profiling statt Vermutung). |
| `opengl4shadinglanguagecookbook` | Ch. 8 „Shadows" (PDF p. 335) / Ch. 10 „Particle Systems and Animation" (PDF p. 394) | Shader-Techniken für Licht, Schatten, Partikel unter GLSL (Godot GLES2); Referenz für 2D-/3D-Effektdarstellung im Client. | Konzeptionell (Shader-/Verfahrensreferenz für Effektvarianten). |
| `practicalgamedesign` | Ch. 12 „Accessibility" (PDF p. 316) | Barrierefreiheit/UX: Wesentliche Spielinformationen müssen auch unter reduzierter Grafik wahrnehmbar bleiben – Referenz für „3D ist keine Voraussetzung für Gameplay". | Konzeptionell (Design-/Barrierefreiheits-Referenz). |
| `gamedevelopmentpatternsandbestpractices` | Ch. 7 „Improving Performance with Object Pools" (PDF p. 214) | Object Pools: Objekte/Instanzen wiederverwenden – passt zu vielen gleichartigen Entities (RENDER_CAP). | Konzeptionell (Muster für Entity-Auftritt). |
| `gamedevelopmentpatternsandbestpractices` | Ch. 10 „Sharing Objects with the Flyweight Pattern" (PDF p. 290) | Flyweight: geteilte Assets/Strukturen für viele gleichartige Entities. | Konzeptionell (Muster für Asset-Wiederverwendung). |
| `gamedevelopmentpatternsandbestpractices` | Ch. 11 „Understanding Graphics and Animation" (PDF p. 312) | VSync/Refreshrate und zeitbasierte (statt framebasierte) Animation. | Konzeptionell (Grafik-/Animations-Empfehlung). |
| `buildinggreensoftware` | Ch. 3 „Code Efficiency" (PDF p. 53) | „Leverage Client Devices": Client-Ressourcen sparsam nutzen, effiziente Client-Nutzung, Ressourcen schonen. | Konzeptionell (Ressourcenschonung/Performance-Proxy). |

Hinweis zu den Einstufungen: „Direkt relevant" = direkt an den Godot-Client anwendbar (mit Prüfung der API-Version). „Konzeptionell" = allgemeine technische Empfehlung/Verfahren, keine Andora-fixierende Entscheidung.

---

## 7. Abgrenzung und offene Punkte

* **Kein Render-Fix vor Tests:** Konkrete `perf_mode`-Werte, und in welcher Variante konkrete Effekte umgesetzt werden, werden erst durch Performance-Tests auf dem Raspberry Pi 4 festgelegt. Die 2D-/3D-Darstellungswahl über die Grafikoptionen ist davon getrennt (§4.2, §4.3).
* **Auto-Einstellung nicht festgelegt:** Eine spätere automatische 2D-/3D-Wahl darf architektonisch möglich bleiben, wird mit dieser Entscheidung aber nicht festgelegt und nicht implementiert (§4.2).
* **Variantenzuordnung offen:** Ob ein konkreter Effekt eine 2D-, eine 3D- oder beide Varianten erhält, wird in der späteren Content- und Effektentwicklung entschieden; kein Effekt ist standardmäßig zu beiden Varianten verpflichtet (§4.2).
* **Plattform-Vorrang:** Raspberry Pi 4 ist die primäre Referenzplattform für den Godot-Referenzclient; alle offiziellen Clients nutzen dieselbe Spielwelt und dieselben serverseitigen Systeme (`project_overview.md` §2, `Mehrere_Offizielle_Clients.md`).
* **Keine detailarme Welt:** Das Ziel ist eine detailreiche, atmosphärische 2D-Welt; 3D dient als Akzent, Produktionswerkzeug und optional gewählter Darstellung, nicht als permanente Welt.

---

## 8. Verweise

* `architecture.md` – Client-Schutzmechanismen (`RENDER_CAP`, `auto perf_mode`, Chunk-Texture-Batching), Godot 3.5/GLES2, Raspberry Pi 4
* `project_overview.md` – §5 „Welt" und §23 „Technisches Ziel"
* `Kampfsystem.md` – Fähigkeiten, Tempo, Effekte
* `cutscene_system.md` – Szenen, Animationen, Effekte (Client für visuelle Präsentation)
* `Charaktererstellung_und_Charakterdarstellung.md` – clientseitige Charakter-Sets (2D-/vorgerendert)
* `Mehrere_Offizielle_Clients.md` – Client-Strategie: mehrere offizielle Clients, gemeinsame Realms, Godot-First
* `references/README.md` – Index der Referenzbibliothek und Kapitelseiten
