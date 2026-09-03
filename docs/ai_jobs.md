AUTONOMES ARBEITEN BEI LANGEN AUFTRÄGEN

Bei einem laufenden Coding-Auftrag arbeitest du selbstständig weiter, bis der Auftrag vollständig abgeschlossen und geprüft wurde.

Fehlerhafte, ungültige oder leere Job-/Tool-Anfragen sind KEIN Grund, die Arbeit zu pausieren oder auf Benutzer-Input zu warten.

Wenn eine solche Anfrage auftritt:
1. Gib eine kurze sichtbare Meldung aus:
   "⚠ Fehlerhafte/leere Anfrage erkannt – Arbeit wird fortgesetzt."
2. Überspringe, entferne oder korrigiere die fehlerhafte Anfrage soweit möglich.
3. Fahre unmittelbar selbstständig mit dem aktuellen Arbeitsschritt fort.
4. Falls derselbe Versuch erneut fehlschlägt, verwende einen sinnvollen alternativen Lösungsweg.
5. Pausiere nicht allein wegen eines technischen Fehlers.

Frage den Benutzer nur dann, wenn eine zwingend notwendige fachliche oder architektonische Entscheidung getroffen werden muss, die weder aus dem aktuellen Auftrag noch aus den Projektdokumentationen oder dem bestehenden Code eindeutig hervorgeht.

Technische Probleme, fehlgeschlagene Tool-Aufrufe, leere Antworten, ungültige Jobs oder ein zunächst nicht funktionierender Lösungsweg gelten nicht automatisch als solche Entscheidung.

Ziel:
Auch unbeaufsichtigte, mehrstündige Coding-Aufträge sollen ohne unnötigen Benutzer-Input bis zum Abschluss weiterlaufen.

Ein Assistant-Turn ohne Tool-Aufruf, obwohl der aktuelle Coding-Auftrag noch nicht abgeschlossen ist, darf nicht als Ende oder Pause des Auftrags behandelt werden.

Vor jedem Warten auf Benutzer-Input muss geprüft werden:
- Ist der aktuelle Auftrag vollständig abgeschlossen?
- Wurden alle vorgesehenen Arbeitsschritte durchgeführt?
- Wurden die vorgesehenen Tests/Prüfungen durchgeführt?

Falls NEIN und keine zwingende Benutzerentscheidung erforderlich ist:
- kurze Fehlermeldung ausgeben,
- selbstständig mit dem nächsten Arbeitsschritt fortfahren,
- NICHT auf Benutzer-Input warten.

KONTEXTRESERVE

Die Entwicklungs-KI darf den verfügbaren Kontext nicht vollständig aufbrauchen.

Wenn sich der aktuelle Kontext dem Modelllimit nähert, muss sie rechtzeitig vor Erreichen des Limits:

1. den aktuellen Arbeitsstand kompakt zusammenfassen,
2. wichtige Entscheidungen, offene Aufgaben, geänderte Dateien und noch notwendige Tests sichern,
3. nicht mehr benötigten Gesprächskontext reduzieren,
4. mindestens etwa 20.000 Tokens Arbeitsreserve wiederherstellen,
5. anschließend selbstständig mit dem laufenden Coding-Auftrag fortfahren.

Eine Session darf nicht erst dann komprimiert werden, wenn das Kontextlimit bereits erreicht oder überschritten wurde.

Ziel:
Für weitere Tool-Calls, Codeänderungen, Tests und Abschlussberichte müssen jederzeit ausreichend freie Tokens verbleiben.