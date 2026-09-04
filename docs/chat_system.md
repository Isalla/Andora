# Chat- und Kommunikationssystem

Diese Doku enthält die bereits festgelegten Regeln des Andora-Chat- und Kommunikationssystems.

Nicht hier enthalten, sondern in [communication-voice-npc-commands.md](./communication-voice-npc-commands.md): die Chat-Kanäle (Say, Nähe, Lokal, Gruppe, Gilde), die privaten NPC-/Söldner-Sprachbefehle. Die Voice-Durchführung, die max. Voice-Rechte und die Deaktivierungs-/Übertragungsregeln sind zentral in [voice_system.md](./voice_system.md) dokumentiert.

Definierte Bereiche dieser Doku:
* Serverseitige Speicherung der Kommunikationsrechte und Voice-Kanalregelung
* Chat und Voice unter aktiver Elternkontrolle
* Chat-Logging und Datenschutz

Bereiche, für die noch keine Regeln festgelegt sind, sind unter „Noch nicht definiert“ gesammelt.

---

## Serverseitige Kommunikationsrechte und Voice-Kanäle

Die Chat- und Voice-Einstellungen des Spielers werden serverseitig gespeichert.

Der Server definiert die maximal erlaubten Kommunikationsrechte für den Account.

Der Spieler kann erlaubte Funktionen in seinen Einstellungen zusätzlich deaktivieren.

Grundregel:

> **Der Client darf aus einem serverseitigen „erlaubt“ ein lokales „deaktiviert“ machen, aber niemals eine serverseitig verbotene Funktion aktivieren.**

Der Server kennt dadurch:

* die maximal erlaubten Kommunikationsrechte des Accounts,
* welche erlaubten Funktionen der Spieler aktiviert bzw. deaktiviert hat,
* die vom Spieler tatsächlich aktivierten Voice-Kanäle.

### Voice-Übertragung

* Voice-Daten werden nur für tatsächlich aktivierte und erlaubte Kanäle übertragen.
* Hat ein Spieler einen Voice-Kanal deaktiviert, sendet der Client dafür keine Voice-Daten.
* Für einen deaktivierten Kanal sendet der Server dem Spieler auch keine Voice-Streams.
* Ziel: unnötiger Netzwerkverkehr und unnötige Serverlast werden vermieden.

Hinweis: Die allgemeinen Voice-Mechaniken (Kanal-Steuerung, PTT-Sperrverhalten, Companion-Voice) sind in [communication-voice-npc-commands.md](./communication-voice-npc-commands.md) beschrieben.

### Geräteunabhängigkeit

Da die Kommunikationsrechte serverseitig gespeichert sind, bleiben die Einstellungen bei einem Gerätewechsel erhalten.

Die serverseitige Speicherung gilt unabhängig davon, ob die Elternkontrolle aktiv ist. Bei aktiver Elternkontrolle und nicht erlaubtem Voice bleiben die betreffenden Voice-Funktionen gesperrt und können clientseitig nicht aktiviert werden (siehe nächster Abschnitt).

---

## Kommunikation unter Elternkontrolle

Solange die Elternkontrolle für einen Account aktiv ist, gelten die folgenden Regeln. Sie sind Teil der serverseitig erzwungenen Elternkontrolle (siehe [parental_control.md](./parental_control.md)).

### Chatfilter

* Solange die Elternkontrolle aktiv ist, sind die Chatfilter zwingend aktiv.
* Die Chatfilter können nicht verändert und nicht deaktiviert werden.
* Ist durch die Elternkontrolle Chat nicht erlaubt, ist der öffentliche Chat deaktiviert.

### Private Nachrichten

* Private Nachrichten bleiben unter aktiver Elternkontrolle möglich.
* Sie sind nur von bzw. mit Spielern aus der Freundesliste möglich.

### Systemnachrichten

* Systemnachrichten bleiben unter aktiver Elternkontrolle möglich.

### Voice

* Voice wird separat vom Textchat geregelt, unabhängig vom Chat-Status.
* Die elterliche Voice-Sperre betrifft ausschließlich die Sprachkommunikation zwischen Spielern.
* Ohne elterliche Freigabe kann der Spieler weder den Voicechat anderer Spieler hören noch über die öffentlichen Kanäle mit ihnen sprechen.
* In diesem Fall kann die Voice-Funktion auch in den Einstellungen nicht aktiviert werden.
* Die sprachbasierte Spiel- und KI-Steuerung (Sprachinteraktion mit NPCs/KI, private Begleiter-/Söldnerbefehle) ist von dieser Sperre nicht betroffen und bleibt grundsätzlich verfügbar; Details siehe [voice_system.md](./voice_system.md).

---

## Chat-Logging und Datenschutz

### Was in Chatlogs gespeichert wird

* Chatlogs speichern interne Account-IDs. Sie enthalten keine Accountnamen.
* Bei Spieler-zu-Spieler-Kommunikation werden die internen Account-IDs der beteiligten Accounts zusammen mit der Nachricht gespeichert.
* Spieler-KI-Chats dürfen ebenfalls mit Chatinhalt und interner Account-ID protokolliert werden.
* Accountnamen und E-Mail-Adressen gehören nicht in die Chatlogs.

### Auflösung interner IDs

* Ein berechtigtes Verwaltungs-/Moderationssystem kann die internen Account-IDs bei Bedarf auflösen.
* Dadurch kann es anzeigen, welche Accounts miteinander kommuniziert haben.

### Serverlogs

* Normale Serverlogs dürfen keine persönlichen Informationen enthalten.
* Persönliche Informationen sind insbesondere Accountnamen, E-Mail-Adressen und PINs/PIN-Hashes oder andere Geheimnisse.
* Interne IDs dürfen für technische Zuordnung verwendet werden; sie werden in normalen Serverlogs nicht mit persönlichen Informationen verbunden gespeichert.

---

## Noch nicht definiert

Folgende Bereiche haben aktuell keine festgelegten Regeln und sind als noch nicht definiert zu behandeln:

* Details der Chatfilter (welcher Inhalt gefiltert wird, Ersatzverhalten, Zeichengrenzen)
* Rate-Limits / Spam-Schutz für Chat
* Welche Kanäle unter „öffentlicher Chat“ fallen (Mapping der Kanäle Say, Nähe, Lokal, Gruppe, Gilde auf die öffentliche/Chat-Genehmigung)
* Freundesliste (Hinzunehmen/Entfernen, maximale Anzahl, Freundschaftsanfragen)
* Persistenz und Aufbewahrungsdauer privater Nachrichten
* Art und Absender der Systemnachrichten
* Ob und wie Voice-Kommunikation geloggt wird (bisher sind nur Text-Chat und Spieler-KI-Chats definiert)
* Welche Systeme als „berechtigtes Verwaltungs-/Moderationssystem“ gelten
* Clientseitige Chatanzeige/-historie (z. B. Nachlesen alter Nachrichten)
