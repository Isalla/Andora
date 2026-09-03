## Temporäre Dateien

Für sämtliche temporären Dateien des Projekts ist ausschließlich der vorhandene projektinterne Ordner

`/home/pi/Projekt/pimmo/.tmp`

zu verwenden.

Systemweite temporäre Verzeichnisse wie `/tmp`, `/var/tmp` oder vergleichbare Verzeichnisse außerhalb des Projekts dürfen nicht verwendet werden.

Dies gilt insbesondere für:

* temporäre Arbeitsdateien
* Zwischenstände
* atomare Schreibvorgänge
* generierte temporäre Konfigurationen
* Build-/Hilfsdateien, sofern deren Speicherort steuerbar ist
* temporäre Queue- und Recovery-Dateien

Bei Bedarf sind innerhalb von `.tmp/` geeignete Unterverzeichnisse anzulegen.

Temporäre Dateien dürfen den Projektordner `/home/pi/Projekt/pimmo` nicht verlassen.
