# Voice-System

Diese Doku fasst alle bereits festgelegten Regeln des Andora-Voice-Systems zusammen. Sie ersetzt keine bestehende Doku, sondern bündelt die Voice-Regeln an einem Ort und verweist für die Details auf die jeweiligen Fachdokumente.

Quellen dieser Doku:

- [communication-voice-npc-commands.md](./communication-voice-npc-commands.md) – Voice-Kanalmechanik, Companion-PTT, NPC-/Söldner-Sprachbefehle, KI-Verarbeitung
- [chat_system.md](./chat_system.md) – serverseitige Kommunikationsrechte, Voice-Rechte, Elternkontrolle, Chat-/Voice-Nebenregeln
- [parental_control.md](./parental_control.md) – Rahmen der serverseitig erzwungenen Elternkontrolle
- [project_overview.md](./project_overview.md) – Übersicht: Begleiter/Söldner, Kommunikation

**Trägerservice:** Das Voice-System wird vom eigenen **Voice-Server** getragen, einem der fünf getrennten Andora-Serverdienste. Er übernimmt die Voice-Kanäle und die serverseitig gespeicherte Voice-Konfiguration. Die Voice-Rechte eines Spielers werden serverseitig verwaltet. Zur Fünf-Dienste-Architektur und zum Betrieb siehe `Deployment_Betriebsarchitektur.md` und `architecture.md`.

---

## 1. Serverseitige Voice-Rechte

Der Server definiert die maximal erlaubten Voice-Rechte für einen Account.

Die Voice-Einstellungen des Spielers werden serverseitig gespeichert. Dadurch bleiben sie bei einem Gerätewechsel oder einem Wechsel des Andora-Clients erhalten.

Der Spieler kann erlaubte Voice-Funktionen in seinen Einstellungen zusätzlich deaktivieren.

> **Der Client darf aus einem serverseitigen „erlaubt“ ein lokales „deaktiviert“ machen, aber niemals eine serverseitig verbotene Voice-Funktion aktivieren.**

Der Server kennt dadurch:

- die maximal erlaubten Voice-Rechte des Accounts,
- welche erlaubten Voice-Funktionen der Spieler aktiviert bzw. deaktiviert hat,
- die vom Spieler tatsächlich aktivierten Voice-Kanäle.

---

## 2. Trennung: Spieler-Voicechat und sprachbasierte Spiel-/KI-Steuerung

Voice in Pimmo besteht aus zwei getrennten Funktionen:

1. **Voicechat zwischen Spielern**
   - Sprachkommunikation zwischen Spielern über die öffentlichen Voice-Kanäle.
   - Die Aufnahme erfolgt clientseitig, die Übertragung serverseitig.
   - Diese Funktion kann unter Elternkontrolle durch den Eltern-Account gesperrt werden.

2. **Sprachbasierte Spiel- und KI-Steuerung**
   - Sprachinteraktion mit NPCs und KI-Entitäten.
   - Private Sprachbefehle für Begleiter und Söldner.
   - Diese Funktionen richten sich an die Spielwelt und nicht an andere Spieler.

Die beiden Funktionen sind unabhängig voneinander und besitzen separate Einstellungen.

- Separate Einstellungen für:
  - Sprachinteraktion mit NPCs/KI
  - Sprachbefehle für Begleiter/Söldner
- Die elterliche Voice-Sperre betrifft ausschließlich die Sprachkommunikation zwischen Spielern.
- Sprachinteraktion mit NPCs/KI sowie Sprachbefehle für Begleiter/Söldner bleiben trotz elterlicher Voice-Sperre grundsätzlich verfügbar.
- Kann der Spieler eine dieser Funktionen in seinen eigenen Voice-/Sprach-Einstellungen deaktivieren, darf der Client dafür keine unnötige Audioaufnahme, keine Übertragung und keine Speech-to-Text- bzw. KI-Verarbeitung auslösen.
- Ein freiwilliges Deaktivieren ändert die serverseitigen maximalen Rechte nicht.

## 3. Voice-Kanäle und Steuerung

Jeder Voice-fähige Chat-Reiter besitzt eine eigene Mikrofon-/Lautsprecher-Steuerung.

- Voice ist standardmäßig stummgeschaltet.
- Voice muss vom Spieler bewusst aktiviert werden.
- Es darf immer nur einen aktivierten Voice-Sendekanal geben.
- Wird das Mikrofon eines anderen Reiters aktiviert, werden die anderen Sendekanäle automatisch deaktiviert.
- Empfang und Senden sind voneinander unabhängig.
- Empfangskanäle können einzeln stummgeschaltet werden.
- Voice-Einstellungen werden pro Kanal gespeichert.

Die Chat-Kanäle selbst (Say, Nähe, Lokal, Gruppe, Gilde) sind in [communication-voice-npc-commands.md](./communication-voice-npc-commands.md) definiert.

---

## 4. Voice-Übertragung

- Voice-Daten werden nur für tatsächlich aktivierte und erlaubte Kanäle übertragen.
- Hat ein Spieler einen Voice-Kanal deaktiviert, sendet der Client dafür keine Voice-Daten.
- Für einen deaktivierten Kanal sendet der Server dem Spieler ebenfalls keine Voice-Streams.
- Ziel: unnötiger Netzwerkverkehr und unnötige Serverlast werden vermieden.

---

## 5. Private Begleiter- und Söldner-Sprachbefehle

Sprachbefehle an eigene Begleiter/Söldner sind kein öffentlicher Voice-Chat.

- Andere Spieler hören weder den gesprochenen Befehl noch dessen Texttranskription.
- Antworten/Bestätigungen des Begleiters sind standardmäßig ebenfalls nur für den Besitzer bestimmt.
- Dadurch entsteht auch bei vielen Spielern mit mehreren Söldnern keine akustische Reizüberflutung.

Beispielbefehle: „Heile mich!“, „Greif mein Ziel an!“, „Beschütze mich!“, „Bleib hier!“, „Folgt mir!“, „Alle zurück!“, „Konzentriert euch auf meinen Gegner!“, „Flieht, der Gegner ist zu stark!“

Ein Befehl kann sich auf einen bestimmten Söldner oder die gesamte eigene Söldnergruppe beziehen.

### Companion Push-to-Talk

Für Begleiterbefehle gibt es eine eigene PTT-Taste:

- Beim Drücken wird der momentan aktive öffentliche Voice-Sender sofort clientseitig temporär stummgeschaltet.
- Die Sprache wird ausschließlich an das Companion-Command-System geschickt.
- Beim Loslassen wird exakt der vorherige Voice-Zustand wiederhergestellt.
- War vorher kein Voice-Sender aktiv, bleibt anschließend alles stumm.
- So kann beispielsweise während eines Gildengesprächs kurzfristig ein Kampfbefehl gegeben werden, ohne dass die Gilde ihn hört.

### Technische Verarbeitung

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

Das kleine Modell interpretiert nur die Absicht. Es führt keine Spielaktion selbst aus. Der autoritative Server prüft anschließend Fähigkeiten, Ziel, Reichweite, Cooldowns, Zustand usw.

Freie NPC-Gespräche laufen getrennt über das normale größere Dialogmodell der Runtime-KI (lokal: Ollama).

---

## 6. Voice unter Elternkontrolle

Solange die Elternkontrolle für einen Account aktiv ist, gelten zusätzlich die dort definierten Voice-Beschränkungen.

Die elterliche Voice-Sperre betrifft ausschließlich die Sprachkommunikation zwischen Spielern.

- Diese Voice-Sperre wird separat vom Textchat geregelt, unabhängig vom Chat-Status.
- Ohne elterliche Freigabe kann der Spieler weder den Voicechat anderer Spieler hören noch über die öffentlichen Kanäle mit ihnen sprechen.
- In diesem Fall kann die Voice-Funktion auch in den Einstellungen nicht aktiviert werden.
- Die sprachbasierte Spiel- und KI-Steuerung (Sprachinteraktion mit NPCs/KI, private Begleiter-/Söldnerbefehle) ist von dieser Sperre nicht betroffen und bleibt grundsätzlich verfügbar.

> **Hinweis:** Das Voice-System selbst gehört nicht zum aktuellen Releaseumfang und wird erst später implementiert. Die Voice-Berechtigung und die elterliche Voice-Sperre sind jedoch bereits heute Bestandteil der Elternkontrolle; details zu den Voice-Regeln und der elterlichen Sperre stehen in [parental_control.md](./parental_control.md).

---

## 7. Noch nicht definiert

Folgende Voice-Themen haben aktuell keine festgelegten Regeln:

- Ob und wie Voice-Kommunikation geloggt wird (bisher sind nur Text-Chat und Spieler-KI-Chats definiert; in `chat_system.md` als „noch nicht definiert“ gelistet)
- Welche der Voice-Rechte einzeln elternkontroll-geschützt sind (oder Voice pauschal) – Granularität der elterlichen Freigabe nicht festgelegt
- Audioqualität, Bandbreite, Codec und Netzwerkparameter für Voice
- Lautstärkestufen, Reichweiten-/Abstandsabhängigkeit des Voice-Empfangs
- Verhalten bei Reconnect/Sitzungswechsel: welcher lokale/deaktivierte Zustand trägt über (serverseitig gespeicherter Zustand ist Basis, Details offen)
- Moderation/Spreacherkennung bei Voice (z. B. Störungsmelde-System, automatische Stummschaltung)
- Voice-Historie auf dem Client (Nachhören, Nachlesen)

---

## 8. Verweise

- Voice-Kanal- und Companion-Details: [communication-voice-npc-commands.md](./communication-voice-npc-commands.md)
- Serverseitige Kommunikationsrechte, Elternkontrolle, Chat-Logging: [chat_system.md](./chat_system.md)
- Elternkontrolle-Rahmen: [parental_control.md](./parental_control.md)
- Übersicht (Kommunikation, Begleiter/Söldner): [project_overview.md](./project_overview.md)
