# Ein Realm – mehrere offizielle Clients

**ZWECK:** Verbindliche Architekturentscheidung zur Client-Strategie von Andora: Andora darf langfristig mehrere offizielle Clients besitzen; alle Clients verbinden sich mit denselben normalen Realms; es gibt keine clientspezifischen Realms und keine clientexklusiven Gameplay-Inhalte; alle Clients bieten identische Inhalte; Unterschiede gelten ausschließlich für Darstellung und Präsentation; der Realm bleibt autoritativ; der Godot-Client ist der verbindliche Gameplay-Referenzclient (Godot-First); ein möglicher Unreal-Engine-PC-Client ist eine spätere Option und kein aktueller Implementierungsauftrag; das Client-/Realm-Protokoll bleibt engineunabhängig.

Diese Entscheidung gilt für alle offiziellen Andora-Clients und hat bei Widersprüchen Vorrang vor älteren Einzel-Aussagen. Die Darstellungs- und Performance-Grundlagen des Godot-Referenzclients stehen in `Clientdarstellung_und_Performance.md`; die Realm-Dimension in `Login_Realm_Architektur.md` und `Deployment_Betriebsarchitektur.md`.

---

## 1. Ein Realm – mehrere Clients

Andora darf langfristig mehrere offizielle Clients besitzen, beispielsweise:

* Godot-Client für Raspberry Pi
* Browser-Client
* möglicher Unreal-Engine-PC-Client
* weitere zukünftige Clients

Diese Clients stellen keine unterschiedlichen Versionen des Spiels dar.

Alle Clients verbinden sich mit denselben normalen Realms.

Das gilt für Realms jedes Rulesets: Das Ruleset (`normal`, später ggf. `hardcore`/`roleplay`) ist eine Eigenschaft des Realms, keine Client-Variante. Alle unterstützten offiziellen Clients eines Realms verwenden dasselbe Ruleset (siehe Realm-Rulesets in `Login_Realm_Architektur.md`).

Es gibt insbesondere keine getrennten:

* PC-Realms
* Pi-Realms
* Browser-Realms
* UE-Realms

Spieler unterschiedlicher Clients befinden sich gemeinsam in derselben Welt und können unmittelbar miteinander spielen.

> **Der Realm ist Andora. Die Clients sind verschiedene Fenster in dieselbe Welt.**

---

## 2. Identische Inhalte

Alle offiziellen Clients bieten dieselben Spielinhalte und Gameplay-Möglichkeiten.

Dazu gehören insbesondere:

* Welt und Gebiete
* Charaktere und Progression
* Klassen und Fähigkeiten
* Kampf
* NPCs
* Quests
* Dungeons und Raids
* Events
* Crafting
* Handel
* Loot
* Rätsel und Weltgeheimnisse
* soziale Systeme

Ein Client darf keine exklusiven Gameplay-Inhalte erhalten.

---

## 3. Unterschiede ausschließlich in der Darstellung

Clients dürfen sich technisch und kosmetisch deutlich unterscheiden.

Beispielsweise darf derselbe Inhalt dargestellt werden als:

* isometrische 2D-/Pre-Rendered-Welt im Godot-Client
* vereinfachte browsergeeignete Darstellung
* hochwertige vollständige 3D-Darstellung in einem zukünftigen Unreal-Client

Auch Beleuchtung, Modelle, Animationen, Partikel, Vegetation, Shader, Sound und andere Präsentationselemente dürfen sich unterscheiden.

Diese Unterschiede dürfen keinen Gameplay-Vorteil erzeugen.

Spielrelevante Informationen müssen auf allen Clients gleichwertig wahrnehmbar sein.

Godot-First bedeutet ausdrücklich **nicht**, dass ein zukünftiger PC-/UE-Client grafisch auf die Möglichkeiten des Raspberry Pi beschränkt werden muss. Ein leistungsfähiger Client darf dieselbe Spielwelt erheblich aufwendiger darstellen.

Beispiel: Ein Zauber kann im Godot-Client eine optimierte 2D-Animation verwenden und in einem Unreal-Client aus komplexen 3D-Partikeln, Beleuchtung und Animationen bestehen. Die gameplayrelevanten Eigenschaften des Zaubers bleiben jedoch identisch.

---

## 4. Realm bleibt autoritativ

Der Realm definiert die tatsächliche Spielwelt und das Gameplay.

Clientdarstellung darf keine unterschiedlichen:

* Trefferbereiche
* Reichweiten
* Positionen
* Cooldowns
* Ressourcen
* Schadenswerte
* Interaktionsregeln
* Sichtbarkeitszustände
* Questzustände
* Weltzustände

erzeugen.

Die Clients interpretieren denselben vom Realm bestimmten Spielzustand lediglich unterschiedlich.

---

## 5. Godot-First-Prinzip

Der bestehende Godot-/Raspberry-Pi-Client ist der verbindliche **Gameplay-Referenzclient**.

Neue Gameplay-Funktionen und neue Spielinhalte werden zuerst so entwickelt und validiert, dass sie mit diesem Client vollständig und spielerisch gleichwertig funktionieren.

Erst danach werden sie auf weitere Clients übertragen.

Entwicklungsrichtung:

**Realm → Godot-Referenzclient → weitere Clients**

Eine Gameplay-Funktion, die auf dem Godot-Referenzclient nicht vollständig und gleichwertig umgesetzt werden kann, darf nicht als exklusives Gameplay-Feature eines leistungsfähigeren Clients eingeführt werden.

Die bestehenden Darstellungs- und Performance-Grundlagen (`Clientdarstellung_und_Performance.md`: 2D/vorgerenderte Welt, 3D als Akzente, hybride Effekte mit Spielerwahl, 60/30 FPS auf Raspberry Pi 4) gelten für den Godot-Referenzclient.

---

## 6. Browser-Client

Auch ein zukünftiger Browserclient folgt denselben Regeln.

Er darf seine Darstellung an Browser-/Hardwaregrenzen anpassen, aber keine inhaltlich reduzierte oder spielmechanisch abweichende Andora-Version werden.

---

## 7. Unreal Engine

Ein möglicher Unreal-Engine-Client ist derzeit eine **spätere Option**, kein aktueller Implementierungsauftrag.

Die Möglichkeit, einen solchen Client zukünftig mit KI-/MCP-gestützter Entwicklung aufzubauen, darf als technische Perspektive erwähnt werden, wird aber nicht als bereits beschlossene Clientimplementierung dokumentiert.

Der aktuelle Entwicklungsfokus bleibt beim bestehenden Godot-Referenzclient.

---

## 8. Client-/Realm-Protokoll

Diese Entscheidung erhöht die Bedeutung eines engineunabhängigen und sauber dokumentierten Client-/Realm-Protokolls.

* Das Protokoll darf langfristig nicht unnötig an Godot-spezifische Darstellungsdetails gekoppelt werden.
* Der Realm liefert Spielzustand und relevante Gameplayinformationen; der jeweilige Client entscheidet über deren Darstellung.
* Das bestehende Protokoll in `shared/` (Client UND Server lesen dieselben Dateien) bleibt die Grundlage; engineunabhängige Erweiterungen sind dort zu pflegen.

---

## 9. Offene Punkte

* Konkrete weitere Clients (Browser, Unreal, andere) sind derzeit nicht umgesetzt und keine aktuellen Implementierungsaufträge.
* Technische Details eines Browser- bzw. Unreal-Clients werden erst bei konkreter Aufnahme in den Entwicklungsumfang festgelegt.