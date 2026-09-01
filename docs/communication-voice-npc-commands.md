Andora – Kommunikation, Voice & NPC-Sprachbefehle

Chat-Kanäle

Say – sehr kurze Reichweite um den Spieler.
Nähe – Kommunikation mit Spielern in der näheren Umgebung.
Lokal – gesamtes aktuelles Gebiet; z. B. für Fragen, wenn niemand direkt in der Nähe ist.
Gruppe – ausschließlich Gruppenmitglieder.
Gilde – ausschließlich Gildenmitglieder.

Voice

Jeder Voice-fähige Chat-Reiter besitzt eigene Mikrofon-/Lautsprecher-Steuerung.
Voice ist standardmäßig stummgeschaltet.
Voice muss vom Spieler bewusst aktiviert werden.
Es darf immer nur einen aktiven Voice-Sendekanal geben.
Wird das Mikrofon eines anderen Reiters aktiviert, werden die anderen Sendekanäle automatisch deaktiviert.
Empfang und Senden sind voneinander unabhängig.
Empfangskanäle können einzeln stummgeschaltet werden.
Voice-Einstellungen werden pro Kanal gespeichert.

Private NPC-/Söldnerbefehle

Sprachbefehle an eigene Begleiter/Söldner sind kein öffentlicher Voice-Chat.
Andere Spieler hören weder den gesprochenen Befehl noch dessen Texttranskription.
Antworten/Bestätigungen des Begleiters sind standardmäßig ebenfalls nur für den Besitzer bestimmt.
Dadurch entsteht auch bei vielen Spielern mit mehreren Söldnern keine akustische Reizüberflutung.

Companion Push-to-Talk

Für Begleiterbefehle gibt es eine eigene PTT-Taste.
Beim Drücken wird der momentan aktive öffentliche Voice-Sender sofort clientseitig temporär stummgeschaltet.
Die Sprache wird ausschließlich an das Companion-Command-System geschickt.
Beim Loslassen wird exakt der vorherige Voice-Zustand wiederhergestellt.
War vorher kein Voice-Sender aktiv, bleibt anschließend alles stumm.
Dadurch kann beispielsweise während eines Gildengesprächs kurzfristig ein Kampfbefehl gegeben werden, ohne dass die Gilde ihn hört.

Natürliche Söldnerbefehle
Beispiele:

"Heile mich!"
"Greif mein Ziel an!"
"Beschütze mich!"
"Bleib hier!"
"Folgt mir!"
"Alle zurück!"
"Konzentriert euch auf meinen Gegner!"
"Flieht, der Gegner ist zu stark!"

Ein Befehl kann sich auf einen bestimmten Söldner oder die gesamte eigene Söldnergruppe beziehen.

Technische KI-Trennung

Sprache
  ↓
Speech-to-Text
  ↓
kleines ~4B-Modell
  ↓
Intent + Parameter
  ↓
Servervalidierung
  ↓
NPC-/Combat-System

Das kleine Modell interpretiert nur die Absicht. Es führt keine Spielaktion selbst aus.

Beispielsweise:

{
  "intent": "ATTACK_PLAYER_TARGET",
  "scope": "ALL_COMPANIONS",
  "confidence": 0.96
}

Der autoritative Server prüft anschließend Fähigkeiten, Ziel, Reichweite, Cooldowns, Zustand usw.

Freie NPC-Gespräche laufen getrennt über das normale größere Ollama-Dialogmodell.

NPC-KI und Lua

Auch die heute besprochene Lua-Idee gehört unbedingt dazu:

TypeScript = Engine + Autorität
Lua        = Gameplay + NPC-/KI-Definitionen
MariaDB    = persistenter Weltzustand
Ollama     = Sprache, Persönlichkeit und Interpretation

Lua kann pro NPC unter anderem Persönlichkeit, Prompt-Bausteine, erlaubte Verhaltensweisen, Dienste und Dialogregeln definieren.

Der Server ergänzt den Prompt mit dem tatsächlichen Weltzustand und dem tatsächlichen Wissen des NPCs.

Grundregel:

NPCs dürfen nur auf Informationen reagieren, die sie tatsächlich erhalten haben. Die KI darf keine Weltfakten erzeugen. Ein NPC darf jedoch bewusst lügen, täuschen, manipulieren oder Informationen verschweigen, wenn Persönlichkeit, Wissen und Situation dies erlauben.