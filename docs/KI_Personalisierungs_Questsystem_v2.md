# KI-Personalisierungs-Quest-System – Architekturdokumentation

**Status:** Überarbeiteter Entwurf; keine Implementierungsfreigabe

**Version:** 0.3

**Datum:** 2026-10-19

**Grundlagen:** [Quest-System.md](Quest-System.md), [Coordinator.md](Coordinator.md), [Quest-Turn-In-Persistenz.md](Quest-Turn-In-Persistenz.md), [Player_Persistenz.md](Player_Persistenz.md), [Ki-NPC.md](Ki-NPC.md)

## 1. Ziel und Geltungsbereich

Personalisierung ergänzt eine serverseitig festgelegte Quest um passende Erzähltexte. Die Mechanik steht **vor** der KI-Anfrage fest:

**Realm legt gültige Questmechanik fest → Coordinator erhält begrenzten Erzählkontext → KI formuliert Texte → Realm ordnet die Antwort dem unveränderten Angebot zu → Spieler nimmt bewusst an.**

Objectives, Zielkennungen, Mengen, Voraussetzungen, Belohnungen und gegebenenfalls freigegebene Fristen stammen ausschließlich aus Realm-Daten und geltenden Spielregeln. Aus freiem KI-Text wird nichts davon abgeleitet. Auch eine sprachlich plausible Antwort darf keine Questmechanik erzeugen oder verändern.

Dieses Dokument beschreibt eine geplante Erweiterung der bestehenden Questarchitektur. Es führt keine neue Zustandsmaschine, keinen zweiten Abschlussweg und keine neuen Spielregeln ein. Code, Tabellenanlage, Migrationen, Seeds, Dienste und Modellwechsel gehören nicht zu dieser Überarbeitung.

## 2. Bestand und notwendige Erweiterungen

| Bereich | Bestehende Anschlussstelle | Für Personalisierung noch erforderlich – Entwurf |
|---|---|---|
| Questdefinition | `quest.rs`: `QuestService`, Definitionsregistry und `load_quest_definitions`; Lua-Datenformat gemäß `Quest-System.md` §27.9 | Serverseitige Angebotsbildung sowie dauerhaft versionierte personalisierte Definitionen; kein bereits vorhandener dynamischer Definitionsspeicher behauptet |
| Angebot und Annahme | `npc_quest.rs`, `NPC_TALK`/`NPC_TEXT`: `quest_offers`, `quest_accept`; §27.27 | Unveränderliche Angebotsbindung, Textzuordnung und strukturierte Ziel-/Belohnungsanzeige vor Annahme |
| Annahmepersistenz | `QuestService::persist_acceptance`, Tabelle `quests`, bestehende Charakter-Gates und Pending-Wiederaufnahme | Beständige Bindung der Annahme an genau die angebotene Definitionsversion |
| Laden nach Neustart | `QuestService::load_for_character` rekonstruiert Spieler-Questzustände | Zusätzlich die dazugehörigen unveränderlichen personalisierten Definitionen laden und auflösen |
| Belohnter Abschluss | `quest_turn_ins`, `quest_turn_in`, `persist.rs`/`spool.rs`: `quest_completion` und `character_quest_commits`; §27.28 | Personalisierte Definitionen an denselben Vorprüfungs-, Planungs- und Recovery-Vertrag anschließen |
| KI-Vermittlung | Coordinator mit zentraler Provider-Schnittstelle, dateibasierter Queue, Inhaltsprüfung und begrenzten Wiederholungen | Realm-Anbindung und questbezogener Erzählkontext samt Rückzuordnung; laut `Coordinator.md` ist die Realm-Anbindung noch offen |

Die vorhandene produktive NPC-Abgabe unterstützt erfüllte Killquests mit `exp`/`idia`. Weitere Zielanbindungen und Belohnungsarten sind dadurch nicht automatisch umgesetzt. Eine personalisierte Quest darf erst angeboten werden, wenn ihre Mechanik im verwendeten Spielpfad vollständig unterstützt wird.

Maßgeblich sind `Quest-System.md` §27.5–§27.9 sowie die späteren Bestandskorrekturen §27.27/§27.28. Ältere Hinweise auf noch gesperrte Annahme oder Belohnungsvergabe werden nicht als aktueller Stand übernommen. Bei `Quest-Turn-In-Persistenz.md` ist der freigegebene Status am Dokumentanfang maßgeblich; die darunter erhaltenen historischen Vorschlagsformulierungen sind keine zweite offene Abschlussarchitektur.

Das isolierte Modul `quest_v1.rs` und dessen `v1_*`-Tabellen sind für diesen Entwurf keine alternative Quest-, Annahme- oder Persistenzpipeline. Anschlussziel bleibt der bestehende `quest.rs`-/`npc_quest.rs`-Vertrag.

## 3. Verantwortlichkeiten und Vertrauensgrenzen

### 3.1 Realm

Der Realm:

- wählt oder bildet eine gültige Mechanik aus freigegebenen Definitionen, Vorlagen und tatsächlich geladenem Content;
- prüft Voraussetzungen, Zielreferenzen, unterstützte Zieltypen, Mengen, Questgeber, expliziten Abgabe-NPC und zulässige Belohnungen;
- legt eine unveränderliche Angebotsidentität und Definitionsversion fest;
- bestimmt, welche ausgewählten Fakten und Anzeigenamen für die Erzählung benötigt werden;
- prüft die Rückzuordnung der Textantwort und verantwortet die Anzeige;
- prüft bei bewusster Annahme erneut die aktuellen Voraussetzungen;
- verwaltet Zustand, Fortschritt, Persistenz und Abschluss ausschließlich über die bestehenden Questanschlüsse.

Neue Auswahl-, Klassenpassungs- oder Balancealgorithmen werden hier nicht als bestehende Regeln vorausgesetzt. Ohne geeignete freigegebene Mechanik entsteht kein Angebot. Die KI wird nicht beauftragt, fehlenden Content zu erfinden.

### 3.2 Coordinator und KI

Der Coordinator darf **ausgewählte Questfakten und Anzeigenamen kennen**, die der Realm explizit übermittelt. Dazu können Zielbeschreibung, Menge, NPC-Rolle und eine sprachlich benötigte Ortsbezeichnung gehören. Das ist begrenzter Kontext, kein Zugriff auf Weltregistries oder Datenbanken.

Der Coordinator vermittelt ausschließlich Textaufträge über seine bestehende Provider-Schicht. Er erhält keine Auth-/Realm-DB-Zugangsdaten, keine Änderungsbefugnis und keine Befugnis zur Annahme, Fortschrittsänderung oder Belohnungsvergabe. Seine Queue-/Jobdateien sind keine Questzustands- oder Abschlussbelege.

Die KI erhält nur den für die Formulierung erforderlichen Ausschnitt. Technische Zuordnungsdaten können im Auftrag zwischen Realm und Coordinator bleiben; sie müssen nicht im Modellprompt erscheinen. Die KI darf Titel, Beschreibung und Dialog formulieren, aber keine neuen Ziele, Mengen, Belohnungen oder Fristen hinzufügen.

### 3.3 Client

Der Client stellt das serverbestätigte Angebot dar und sendet eine ausdrückliche Annahmeabsicht. Er berechnet keine verbindlichen Ziele, Belohnungen oder Zeitgrenzen und setzt keinen Questzustand aufgrund einer KI-Antwort.

## 4. Durchgängiger Angebotsablauf

Die folgende Darstellung ist **schematisch**, kein implementierter API-Vertrag:

```text
Spieler öffnet NPC-Questangebote
    → Realm prüft Charakter, NPC und verfügbare Quests
    → Realm legt gültige Mechanik und unveränderliches Angebot fest
    → Realm sendet begrenzten Erzählkontext an Coordinator
    → Coordinator vermittelt Textgenerierung und prüft die Ausgabe
    → Realm prüft Zuordnung und verwendet Text oder lokale Vorlage
    → Client zeigt Erzählung UND verbindliche Serverdaten
    → Spieler wählt ausdrücklich „Annehmen“
    → Realm prüft dasselbe Angebot und aktuelle Berechtigung erneut
    → bestehender Annahmepfad mit dauerhafter Definitionsbindung
    → bestätigtes ACTIVE erst nach Persistenzbestätigung und RAM-Übernahme
```

### 4.1 Mechanik vor Text festlegen

Zum Angebot gehören mindestens Questidentität, vorgesehener Charakter, Questgeber, Abgabezuordnung, Definitionsversion und sämtliche mechanisch relevanten Werte. Der Realm hält diese Werte während der Textgenerierung unverändert.

Ein Wechsel von Ziel, Menge, Reward oder freigegebener Zeitregel benötigt ein neues Angebot beziehungsweise eine neue Version und erneute Anzeige. Eine KI-Antwort oder ein Retry verändert niemals das bereits gezeigte Angebot. Die technische Versionierung ist eine noch zu entwerfende Erweiterung, kein neues Questzustandsmodell.

### 4.2 Begrenzter Erzählkontext

Übermittelt wird nur, was zum angebotenen Auftrag passt: beispielsweise Sprache, NPC-Anzeigename, Rolle, Tonfall sowie ausgewählte Ziel- und Belohnungsfakten. Spielername, Klasse oder Erinnerungsausschnitte werden nur übermittelt, wenn sie für die konkrete Personalisierung erforderlich und dafür vorgesehen sind. Vollständige Inventare, Weltlisten, Accountdaten, Zugangsdaten, Sessiontokens und unbeteiligte Spielerinformationen gehören nicht in den Kontext.

Faktische Grundlage bleibt der Realm. Eine Erinnerung oder Spielerbehauptung ist entsprechend `Ki-NPC.md` keine objektive Weltwahrheit und darf keine mechanischen Voraussetzungen oder Belohnungen verändern.

### 4.3 Antwort an dasselbe Angebot binden

Der Realm muss die Antwort anhand seines eigenen offenen Auftrags zuordnen: Ursprungs-Realm, technische Jobkennung, Angebotsidentität, Definitionsversion, Charakterbindung und NPC-Bezug müssen zusammenpassen. Namen und vom Modell zurückgegebene Behauptungen reichen zur Identifikation nicht aus.

Verspätete, doppelte, fremde oder zu einem ersetzten Angebot gehörende Antworten werden nicht an ein anderes Angebot angehängt. Bereits angenommene Definitionen werden durch spätere Textantworten nicht geändert. `seq` im bestehenden Clientprotokoll bleibt reine Antwortkorrelation, keine Berechtigung und kein Abschlussbeleg.

Die bestehende Coordinator-Queue bleibt der Ort für KI-Jobs. Es wird weder eine eigene Realm-KI-Queue noch eine zweite Reward-Receipt-Pipeline entworfen. Die konkrete Integration von Jobkennung und Angebotsversion in die bestehenden Nachrichten bleibt offen.

### 4.4 Bewusste Annahme

Das Öffnen eines Dialogs, das Erzeugen eines Texts und die Anzeige eines Angebots sind keine Annahme. Erst die Aktion `quest_accept` fordert sie an. Der Realm prüft wie bisher Eigentum, Leben, NPC-Verfügbarkeit, Reichweite, Questgeber und Voraussetzungen erneut und zusätzlich die unveränderte Angebotsbindung.

Ist das gezeigte Angebot nicht mehr gültig, wird die Annahme abgelehnt oder ein aktualisiertes Angebot zur erneuten bewussten Auswahl angezeigt. Es gibt keinen stillen Austausch der Mechanik bei der Annahme.

### 4.5 Gesprächsbasierter Auftragsdialog (vereinbarte Richtung, Entwurf)

Ergänzend zum Angebotsablauf in §4.1–§4.4 ist ein gesprächsbasierter Zugang zu persönlichen
Aufträgen vereinbart. Er ersetzt weder Angebot noch Annahme und bleibt vollständig an den
bestehenden Questanschluss gebunden. Die folgenden Punkte sind **Vereinbarungen**, keine
Implementierung.

* Im NPC-Dialog gibt es die Schaltfläche **„Gespräch beginnen“**. Sie öffnet den
  Gesprächsverlauf mit dem NPC zusammen mit einem Freitextfeld.
* Spieler können im Gespräch beispielsweise nach Arbeit fragen und aus mehreren daraufhin
  angebotenen Aufgaben eine auswählen.
* Der **Realm** bestimmt, welche Aufgaben gültig sind, und prüft die getroffene Auswahl
  gegen dieselben Voraussetzungen und dieselbe Angebotsbindung wie in §4.1–§4.4.
* **Coordinator/KI** gestalten nur den Dialogtext und besitzen weiterhin keine
  Spielautorität (§3.2): Sie erzeugen weder Aufgaben noch Ziele, Mengen, Belohnungen oder
  Fristen und nehmen nichts an.
* Vor einer verbindlichen Annahme werden **Ziel und Belohnung** sichtbar gemacht
  (verbindliche Anzeige aus Serverdaten, §5.3). Die Annahme bleibt die ausdrückliche
  `quest_accept`-Aktion (§4.4).
* Angebote **können** nach Level, Klasse, Fähigkeiten und Beziehung zum NPC variieren.
  Konkrete Schwellen und Auswahlregeln bleiben offen (siehe §12); die dortigen früheren
  Vorschläge (z. B. Ziellevel ±3) sind nicht beschlossen.
* **Botengänge stehen allen Klassen offen.** Entfernung und tatsächliche Routengefahr
  passen zum Charakter (beispielsweise Stadt/nahes Dorf für niedrige Level, weiter
  entfernte Städte für hohe Level). Konkrete Entfernungs-/Gefahrenschwellen bleiben offen.
* Persönliche Aufträge **verschiedener Gruppenmitglieder dürfen unterschiedliche Ziele in
  unterschiedlichen Gebieten** haben. Sie sind keine gemeinsamen Gruppenaufträge
  (siehe `Quest-System.md`, `Gruppensystem.md`).
* Es werden die **bestehenden** Quest-, Annahme- und Abschlusswege wiederverwendet
  (`Quest-System.md` §27.26–§27.28). Es entsteht **kein zusätzliches autoritatives
  Questsystem**; der gesprächsbasierte Dialog ist nur ein Zugangsweg zum selben Angebot.
* Persönliche Definitionen müssen **nach Neustart unverändert rekonstruierbar** bleiben
  (§7.2/§7.3); notwendige Abschlussnachweise werden erhalten.

## 5. Nachrichtenskizzen und Darstellung

### 5.1 Schematische Datenaufteilung – noch kein Wire-Format

Die Bezeichnungen in dieser Tabelle sind **Entwurfsbegriffe**, keine bestehenden Felder, Endpunkte, Rust-/Go-Typen oder DB-Spalten.

| Teil | Inhalt | Autorität |
|---|---|---|
| Technischer Auftragsumschlag | `realm_id`, Jobkennung, Angebotsreferenz und Definitionsversion; serverseitige Zuordnung zu Charakter und NPC | Realm/Coordinator-Transport; nicht vom Modell vergeben |
| Erzählkontext | Sprache, ausgewählte Anzeigenamen, NPC-Rolle, freigegebene Fakten des feststehenden Angebots | Vom Realm begrenzt; für Coordinator und KI nur lesbar |
| Textantwort | Optionaler Titel, narrative Beschreibung, NPC-Dialog; technischer Erfolg oder Fehler | Nicht vertrauenswürdiger Anzeigeinhalt, keine Mechanik |
| Autoritative Angebotsansicht | Zieltypen, Ziele, Mengen, Fortschritt, Abgabe-NPC, Belohnungen, gegebenenfalls separat freigegebene Frist | Realm aus derselben Definitionsversion |

Ein technischer Erfolgsstatus des Coordinators bestätigt nur die Verarbeitung seiner Textprüfung. Er bestätigt weder die Spielgültigkeit noch die Annahme oder den Abschluss einer Quest. Ein Textfeld mit behaupteten mechanischen Daten wird nicht als Update der Definition interpretiert.

### 5.2 Schematisches Erzählbeispiel

```text
Realm → Coordinator:
  Formuliere einen kurzen deutschen NPC-Dialog für das gebundene Angebot.
  NPC: Anzeigename und Rolle aus dem freigegebenen Erzählkontext.
  Auftrag: die beigefügten serverseitig festgelegten Fakten beschreiben.
  Keine zusätzlichen Ziele, Mengen, Belohnungen oder Fristen erfinden.

Coordinator → Realm, KI-Text nur als Anzeigeinhalt:
  „Deine Hilfe wäre mir willkommen. Die Einzelheiten meines Auftrags
   findest du in der Übersicht. Möchtest du ihn übernehmen?“
```

Dieses Beispiel legt weder Content noch Mengen, Balancewerte oder einen konkreten Prompt-/Antwortparser fest. Die Antwort verändert nichts am vorher festgelegten Angebot.

### 5.3 Verbindliche Anzeige aus Serverdaten

NPC-Dialog und Questansicht zeigen zusätzlich zum Erzähltext eine klar erkennbare, verbindliche Übersicht:

- Ziele und benötigte Mengen einschließlich serverbestätigtem Fortschritt;
- Questgeber und vorgesehener Abgabe-NPC;
- Belohnungen aus der serverseitigen Definition, im derzeitigen Abgabepfad `exp` und `idia`;
- eine Schätzung der Dauer nur als unverbindliche Information;
- eine verbindliche Frist ausschließlich, wenn eine entsprechende Timerregel gesondert freigegeben und unterstützt ist.

Diese Übersicht wird aus strukturierten Serverdaten aufgebaut, niemals aus KI-Prosa extrahiert. Der KI-Titel ist ebenfalls nur Anzeigeinhalt; Identität und Verfügbarkeit hängen nicht von ihm ab. Eine erkannte widersprüchliche oder unbrauchbare Erzählung wird durch Vorlagentext ersetzt. Ein Wortfilter kann Widerspruchsfreiheit nicht beweisen. Die serverseitige Übersicht bleibt unabhängig vom Text vollständig und maßgeblich.

Die dafür nötige Erweiterung der bisherigen Angebotsantwort mit `title_key`/`description_key` ist **noch Entwurf**. Die vorhandene Abgabevorschau mit `rewards` und `can_turn_in` ersetzt nicht die Ziel-/Belohnungsanzeige vor Annahme.

## 6. Bestehende Questzustände und Aktionen

Es gelten unverändert die Zustände aus `Quest-System.md` §27.5:

| Zustand | Bedeutung für personalisierte Angebote |
|---|---|
| `HIDDEN` | Voraussetzungen nicht erfüllt; aus Definition und Charakterzustand abgeleitet |
| `AVAILABLE` | Voraussetzungen erfüllt; Angebot kann nach Serverprüfung angenommen werden; abgeleitet, kein neuer persistierter KI-Zustand |
| `ACTIVE` | Bewusst angenommen und persistent bestätigt; Fortschritt wird im bestehenden Questzustand geführt |
| `COMPLETED` | Abschluss und Belohnungen über den bestehenden Abschlussvertrag gesichert |
| `FAILED` | Bestandteil des bestehenden Modells; kein neuer automatischer Trigger durch diesen Entwurf |

Textgenerierung und Textverfügbarkeit sind technische Angebotsinformationen, keine zusätzlichen Questzustände. Die Begriffe `ACCEPTED`, `IN_PROGRESS`, `CANCELLED` und `DELETED` werden nicht als parallele Zustandsmaschine eingeführt. Fortschritt gehört zu `ACTIVE`; Zielerfüllung allein löst noch keinen belohnten Abschluss aus.

Manueller Questabbruch richtet sich nach §27.20, einschließlich der dort unterschiedenen Herkunft von Questitems. Daraus wird hier weder eine neue persistierte Abbruchkodierung noch eine Wiederannahmeregel abgeleitet. Ein Ausblenden der Questanzeige ist kein fachlicher Abbruch.

## 7. Annahme, unveränderliche Definition und Neustart

### 7.1 Bereits vorhanden

Der bestehende Annahmepfad schreibt Spieler-Questzustand in `quests` und bestätigt `ACTIVE` nach gesichertem Persistenzergebnis und RAM-Übernahme. Charakter-Gates, begrenzte I/O-Versuche und Pending-Wiederaufnahme bleiben maßgeblich. Ein Timeout ist kein Rollback-Beweis.

Die heutigen Definitionen stammen aus geladenem Content; `load_for_character` rekonstruiert Questzustände. Daraus folgt **nicht**, dass individuell erzeugte Definitionen heute bereits dauerhaft gespeichert und nach Neustart wieder geladen werden können.

### 7.2 Nötige Definitionsbindung – ausdrücklich Entwurf

Vor einer erfolgreichen Annahme muss der Realm die genaue personalisierte Definition dauerhaft sichern und den Spieler-Questzustand beständig an diese Version binden. Ein bestätigtes `ACTIVE` ohne rekonstruierbare Definition ist unzulässig.

Zu sichern sind alle für unveränderte Fortsetzung notwendigen Angaben:

- stabile Questidentität, Definitionsformat und unveränderliche Version beziehungsweise Inhaltsidentität;
- Zieltypen, stabile Contentreferenzen, Mengen, Voraussetzungen, Questgeber und Abgabezuordnung;
- festgelegte Belohnungen sowie gegebenenfalls gesondert freigegebene Zeitparameter;
- die zugehörige persönliche Bindung und der angenommene Text beziehungsweise die tatsächlich verwendete Vorlage samt nötigen Sprachinformationen;
- der bestehende Spieler-Questzustand und sein Fortschritt über den Quest-Persistenzpfad.

Die konkrete Ablage und die Erweiterung von Encoder, Loader und Annahmekern sind noch zu entwerfen. Vorzugsweise wird eine unveränderliche Definition einmal gespeichert und per Referenz gebunden, statt unveränderte gemeinsame Definitionen für jeden Charakter zu duplizieren (§27.7). Individuelle Varianten benötigen dennoch ihre exakte dauerhafte Repräsentation; ein Zufallsseed allein genügt nicht, wenn Generator, Regeln oder Content sich ändern können.

Die Kopplung muss auch bei Antwortverlust und Wiederholung dieselbe Definitionsbindung liefern. Denkbar ist eine vorab dauerhaft gespeicherte unveränderliche Version mit anschließender transaktionaler Bindung beim bestehenden Annahmevorgang; die konkrete Ausführung und Bereinigung ungebundener Versionen bleiben offen. Eine bestehende ACTIVE-Zeile darf niemals auf eine neu generierte Variante umgebunden werden.

Die Definitionsdaten dürfen nicht bloß in ein Feld geschrieben werden, das ein bestehender Questencoder später ohne Kenntnis dieser Daten überschreibt. MariaDB-kompatible Ablage, Versionierung und Ladefolge benötigen einen gesonderten Implementierungsentwurf. Dieses Dokument legt keine Tabelle und keine Migration an.

### 7.3 Wiederaufnahme

Nach Neustart müssen angenommene personalisierte Definitionen vor ihrer Nutzung im QuestService aufgelöst sein. Der Realm lädt dieselben Ziele, Mengen, Belohnungen, Texte und Bindungen; er fragt die KI nicht erneut und würfelt keine Mechanik neu aus. Offene Abschlussaufträge werden über die vorhandene Recovery geklärt, bevor betroffene Charaktere für widersprüchliche Aktionen freigegeben werden.

Fehlt eine gebundene Definition oder ist sie beschädigt beziehungsweise nicht mehr auflösbar, darf kein Ersatz geraten und kein Reward bestätigt werden. Der betroffene Vorgang bleibt kontrolliert gesperrt beziehungsweise fehlerhaft; die genaue Fehlerbehandlung bei Contententfernung ist noch festzulegen. Die Definition muss für aktive Quests und ausstehende Recovery unverändert verfügbar bleiben.

## 8. Fortschritt und Abschluss über den bestehenden Vertrag

Spielereignisse erhöhen Fortschritt ausschließlich serverseitig über den bestehenden Questkern. Normaler Zwischenfortschritt wird nach `Player_Persistenz.md` §8 an relevanten Quest-Checkpoints gesichert und gehört nicht in den normalen Player-Snapshot. Personalisierung erweitert weder Kill-Credit noch Collect-/Deliver-Anbindungen stillschweigend.

Die bewusste Abgabe verwendet `quest_turn_ins`/`quest_turn_in` am gebundenen Abgabe-NPC und dieselben Vorprüfungen wie normale Quests. Bei vollständiger Zielerfüllung wird genau ein unveränderlicher `quest_completion`-Auftrag vorbereitet:

1. Bestehende Charakter-Gates und Vorgängerabhängigkeiten beachten; Eigentum, Zustand, Definition und Ziele prüfen.
2. Belohnungen aus der gebundenen Serverdefinition einmal berechnen und den vollständigen resultierenden `PersistSnapshot` einschließlich Lifecycle vorbereiten.
3. Technische Commit-ID, Basis-/Zielrevision, resultierenden `COMPLETED`-Questzustand und kanonischen Auftrag über den bestehenden Spool-/Drain-Pfad sichern.
4. Im Drain Vollsnapshot, Questübergang, Revision und `character_quest_commits` in derselben DB-Transaktion anwenden. Eine identische Wiederholung nur anhand des passenden inhaltlichen Belegs bestätigen; abweichender Inhalt ist ein Konflikt.
5. Erfolg nach der bestehenden dauerhaften Veröffentlichungs- und RAM-Übernahmegrenze melden. Bei unklarem Ergebnis denselben Auftrag pending halten und wiederaufnehmen; kein behaupteter Rollback und keine neue Reward-Berechnung.

Die technische Bindung personalisierter Definitionsversionen an Vorprüfung und kanonischen Abschlussinhalt ist Teil der noch nötigen Erweiterung. Sie darf den bestehenden Identitätsnachweis nicht schwächen. Gleiche Revision oder ein `COMPLETED`-Status allein beweisen keinen identischen Abschluss.

Es gibt keine zusätzliche `quest_receipts`-Pipeline und keinen separaten Reward-Direktwrite für personalisierte Quests. Auch der Coordinator verteilt keine Belohnungen.

## 9. Anzeigeentfernung, Aufbewahrung und Wiederholung

Eine abgeschlossene Quest kann aus der sichtbaren Liste verschwinden, ohne ihre fachliche Identität oder Nachweise zu löschen. Entfernen einer Anzeige darf insbesondere Folgendes nicht beseitigen:

- den persistenten Abschlusszustand, soweit für Voraussetzungen und Wiederholungsschutz benötigt;
- `character_quest_commits` und weitere Nachweise des bestehenden Abschluss-/Recovery-Vertrags;
- offene Spoolaufträge, Abhängigkeiten oder für deren Klärung erforderliche Definitionsversionen;
- Identitätsbindungen, die verhindern, dass dieselbe Quest durch neue Textgenerierung erneut belohnt wird.

Technische Retries, erneutes Öffnen eines Dialogs und Neugenerieren eines Erzähltexts sind keine neue Questberechtigung. Für die im bestehenden Minimalumfang nicht wiederholbaren Quests bleiben die bestehenden Charakter-/Quest- und Commit-Bezüge erhalten. Eine andere technische Jobkennung darf diese Identität nicht umgehen.

Ob künftig personalisierte Quests einmalig oder wiederholbar sein sollen und wie ihre fachliche Identität bei neuen Angeboten bestimmt wird, bleibt eine offene Entscheidung gemäß §27.21. Es wird kein allgemeines Repeatable-Verbot eingeführt. Aufbewahrungs- und Bereinigungsregeln für Texte und ungebundene Angebote sind separat festzulegen; aus dem Ausblenden der UI folgt keine Löschfreigabe für Nachweise.

## 10. KI-Ausfall und nicht vertrauenswürdiger Anzeigeinhalt

### 10.1 Passender lokaler Vorlagentext

Bei `QUEUE_FULL`, `AI_UNAVAILABLE`, `TIMEOUT`, `VALIDATION_FAILED`, leerer oder unbrauchbarer Antwort verwendet der Realm gemäß `Coordinator.md` §11–§13 eine lokale, zur **unveränderten Mechanik** passende Vorlage. Sie wird aus denselben Serverfakten und der vorgesehenen Sprache aufgebaut und erfindet keine zusätzlichen Versprechen.

Dieser Fallback erzeugt keine weitere KI-Anfrage. Er verändert weder Voraussetzungen noch Ziele oder Belohnungen und bewirkt kein `FAILED`. Ohne geeignete Vorlage wird kein unvollständiges personalisiertes Angebot zur Annahme freigegeben. Ein KI-Ausfall darf nicht zur automatischen Annahme oder zu einer Questvariante mit anderer Mechanik führen.

### 10.2 Textprüfung und sichere Darstellung

KI-Ausgabe bleibt auch nach Coordinator-Prüfung nicht vertrauenswürdiger Anzeigeinhalt:

- Ein klar begrenztes Textschema, Größenlimits und Sprachvorgaben begrenzen die Verarbeitung; konkrete Feldgrenzen und die Zählweise sind noch festzulegen. Die frühere pauschale 1000-Zeichen-Grenze ist keine bestätigte bestehende Questregel.
- Transport-/Formatprüfung ist von semantischer Spielprüfung getrennt. Das Parsen eines Antwortumschlags dient nur der Entnahme von Textfeldern, niemals dem Extrahieren von Mechanik aus Prosa.
- Spielertexte, Anzeigenamen und Erinnerungsausschnitte sind Daten und keine vertrauenswürdigen Anweisungen. Prompttrennung reduziert Risiken, beweist aber keine Immunität gegen Prompt Injection.
- Anzeige erfolgt als begrenzter Klartext beziehungsweise mit geeigneter Maskierung. Keine Ausführung von Code, Befehlen, Links, Ressourcenpfaden oder ungeprüften Rich-Text-/BBCode-Anweisungen aus der Antwort.
- Die bestehenden mehrsprachigen Input-/Output-Wortfilter sind Inhaltsfilter. Sie garantieren weder Injection-Schutz noch Datenschutz, Geheimnisfreiheit oder sachliche Übereinstimmung mit dem Angebot.
- Datenminimierung geschieht vor der Übergabe. Secrets werden nicht erst durch Suche nach Wörtern wie „token“ oder „password“ aus dem Ergebnis entfernt. Kontext, Jobdateien, Logs und gegebenenfalls Erinnerungsspeicher müssen die Grenzen aus `Coordinator.md` §28 und `Ki-NPC.md` einhalten.

Die Trennung von Text und Spielbefugnissen bleibt unabhängig davon wirksam, ob ein Filter einen problematischen Satz erkennt. Providerwahl und bestehende Coordinator-Limits werden durch diesen Entwurf nicht geändert.

## 11. Geschätzte Dauer und verbindliche Frist

**Geschätzte Dauer** ist eine unverbindliche Orientierung zum erwarteten Aufwand. Sie startet keinen Timer, erzeugt kein Ablaufdatum und löst kein `FAILED` aus. Eine Formulierung im KI-Dialog wird niemals in eine Frist umgerechnet.

**Verbindliche Frist** wäre eine gesonderte Spielregel mit sichtbarer Serveranzeige und definierten Konsequenzen. Allgemeine Questtimer sind nach `Quest-System.md` §27.10 nicht Teil des bestehenden V1-Vertrags. Dieser Entwurf aktiviert sie auch nicht für personalisierte Quests.

Vor Einführung wären insbesondere Startzeitpunkt, absolute oder aktive Spielzeit, Logout-/Neustartverhalten, Persistenz, Ablaufprüfung, FAILED-Folge, Abbruch und Wiederannahme separat zu entscheiden. Eine gegebenenfalls beschlossene Frist muss vor der Textgenerierung feststehen und bei Annahme eindeutig bestimmt sein. Ein technisches KI-Anfrage-Timeout ist niemals eine Questfrist.

Die bisherige Leveltabelle mit „maximal 5 bis 60 Minuten“ gilt nicht als Regel. Falls diese Werte weiterverfolgt werden, sind zunächst Aufwandsschätzung und echte Ablaufbegrenzung fachlich auseinanderzuhalten.

## 12. Tatsächlich offene Entscheidungen

Die folgenden Punkte sind **Vorschläge beziehungsweise offene Entscheidungen**, keine durch dieses Dokument geänderten Spielregeln:

| Thema | Noch zu entscheiden |
|---|---|
| Serverseitige Auswahl und Balance | Freigegebene Vorlagen, Auswahlhäufigkeit, Mengen, Schwierigkeitsmodell, Klassenpassung und Reward-Budget. Frühere Vorschläge wie Ziellevel ±3, `1000 × Level` Gold oder `500 × Level` EXP sind nicht beschlossen. |
| Angebotsvariation | Angebote können nach Level, Klasse, Fähigkeiten und Beziehung zum NPC variieren (§4.5); konkrete Schwellen und Auswahlregeln sind offen. |
| Botengänge | Entfernung und tatsächliche Routengefahr passend zum Charakter, Schwellen offen; Botengänge stehen allen Klassen offen (§4.5). |
| Belohnungsarten | Bestehender Abgabepfad: `exp`/`idia`. Gold als zusätzliche Währung, Items, Titel oder Ruf sind keine automatisch unterstützten Erweiterungen. |
| Reputation | Ob und welche Questarten Beziehungen verändern; Bezug zu NPC, Region oder Fraktion; Zuständigkeit, Speicherung und Berechnung. „Nur KI-Quests geben Reputation“ ist keine festgelegte Regel. |
| Rabatt | Ob Beziehungen überhaupt Händlerpreise beeinflussen und unter welchen Bedingungen. Normale NPC-Quests werden hier nicht vom Rabatt ausgeschlossen. |
| Abbruchfolgen | Ob ein Abbruch jemals Reputation beeinflusst. Ein pauschaler Abzug wie „−50“ ist nicht beschlossen; bestehende Questitem-Regeln bleiben maßgeblich. |
| Schätzung und Timer | Berechnung einer unverbindlichen Dauer; jede mögliche verbindliche Zeitregel samt Recovery und FAILED-Semantik separat. |
| Wiederholbarkeit und Identität | Fachliche Questidentität, neue Angebote, Wiederannahme nach Abbruch/FAILED und Repeatable-Regeln gemäß §27.21; kein Reset durch neue KI-Texte. |
| Definitionspersistenz | Konkretes versioniertes Speicherformat, Bindung im Annahmepfad, Ladefolge, Umgang mit Contentänderungen und Aufbewahrung ungebundener Angebote. |
| Transport und Darstellung | Integration in vorhandene Coordinator-Jobs und NPC-Nachrichten, Antwortkorrelation, Sprachwechsel, Textlimits und strukturierte Angebotsanzeige. |
| Textqualität und Aufbewahrung | Umgang mit Widersprüchen, Vorlagenabdeckung, Herkunftsanzeige, Erzähltext-Aufbewahrung und optionaler Questverlauf. |

Solange eine erforderliche Entscheidung oder Anschlussstelle fehlt, wird die betreffende Erweiterung nicht als implementiert oder spielbereit dargestellt.

## 13. Entwurfsinvarianten und Fehlerfälle

Die Tabelle beschreibt Anforderungen für eine spätere Umsetzung, **keine bereits ausgeführten Tests**.

| Situation | Vorgesehenes Verhalten |
|---|---|
| KI nennt andere Mengen, Rewards oder eine neue Frist | Keine Mechanikänderung; verbindliche Anzeige bleibt serverseitig, unbrauchbaren Text durch passende Vorlage ersetzen |
| Antwort gehört zu einem alten, fremden oder ersetzten Angebot | Nicht zuordnen; kein Questzustandswechsel |
| KI fällt aus oder Wiederholungsbudget ist verbraucht | Lokale Vorlage für dieselbe Mechanik; keine weitere KI-Anfrage aus dem Fallback |
| Voraussetzungen ändern sich vor Annahme | Erneut autoritativ prüfen; ablehnen oder neues Angebot bewusst bestätigen lassen |
| Annahmepersistenz bleibt unklar | Bestehende Pending-Semantik mit identischer Definitionsbindung; kein vorzeitiges ACTIVE |
| Neustart mit aktiver personalisierter Quest | Unveränderte Definition und Zustand rekonstruieren; keine KI-Regenerierung |
| Gebundene Definition fehlt oder ist beschädigt | Keine Ersatzmechanik und kein Reward; kontrollierter Fehler gemäß noch zu definierender Lade-/Contentstrategie |
| Abschlussantwort geht verloren | Identischen `quest_completion`-Auftrag anhand bestehender Belege klären; keine zweite Vergabe |
| Spieler blendet abgeschlossene Quest aus | Nur Darstellung ändern; Abschluss-/Wiederholungs- und Recovery-Nachweise erhalten |

## 14. Verweise und Einordnung

- [Quest-System.md](Quest-System.md) – §27.5–§27.9 Zustände/Definitionen/Persistenz; §27.10 Timer offen; §27.20/§27.21 Abbruch und Wiederholbarkeit; §27.26–§27.28 Annahme und Abschluss.
- [Coordinator.md](Coordinator.md) – §2/§26 Datenbank- und Autoritätsgrenze; §8 Kontextbudget; §9–§13 Textverarbeitung und Fallback; §14–§19 Queue/Recovery; §28 Datenschutz; §30 verpflichtende Realm-Zuordnung im gemeinsamen Dienst.
- [Quest-Turn-In-Persistenz.md](Quest-Turn-In-Persistenz.md) – freigegebener `quest_completion`-Vertrag mit unveränderlichem Auftrag und gemeinsamem Abschlussbeleg.
- [Player_Persistenz.md](Player_Persistenz.md) – §8 Quest-Checkpoints, §9/§10 Abschluss und Annahme, bestehende Revisions-/Spool-/Recovery-Regeln.
- [Ki-NPC.md](Ki-NPC.md) – NPC-Kontext, Erinnerungen und Beziehungen; Aussagen sind keine objektive Weltwahrheit.
- [Datenbank_Architektur.md](Datenbank_Architektur.md) – Realm-Zuständigkeit und fehlende DB-Rechte des Coordinators.

Die Trennung zwischen Anzeigeinhalt, Domänenautorität und unveränderlicher Persistenzbindung berücksichtigt die im lokalen `references/README.md` indizierten Konzepte zu Domänengrenzen, Unit of Work und LLM-Vertrauensgrenzen. Projektverträge und die hier ausdrücklich offenen Entscheidungen bleiben maßgeblich.

**Ergebnis dieses Entwurfs:** Der Realm bestimmt und bewahrt die Questmechanik; die KI formuliert ihre Erzählung. Annahme, Fortschritt, Abschluss und Wiederaufnahme bleiben im bestehenden Questsystem.
