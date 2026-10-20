# NPC-Freitext-Gespräch — Umsetzungsstand

**Status:** Nutzbarer, durchgängiger Gesprächspfad umgesetzt (Realm-Autorität +
bestehender Coordinator-/KI-Pfad + Godot-3.6-Client). **KI-/Transportantworten
in den Tests sind modelliert** (offline; kein Live-KI-Dienst angenommen).

**Datum:** 2026-10-19

**Grundlagen:** [KI_Personalisierungs_Questsystem_v2.md](KI_Personalisierungs_Questsystem_v2.md)
(§4.5 „Gesprächsbasierter Auftragsdialog"), [Coordinator.md](Coordinator.md)
(§§4/6/7/13/28/30), [Ki-NPC.md](Ki-NPC.md), [Quest-System.md](Quest-System.md)
(§27.27), [chat_system.md](chat_system.md), [communication-voice-npc-commands.md](communication-voice-npc-commands.md)

## 1. Geltungsbereich

Spieler können bei einem NPC „Gespräch beginnen" wählen, Freitext eingeben und
die Antwort über den **bestehenden** Coordinator-/KI-Pfad erhalten. Dies ist der
**Zugangsweg** zum bestehenden Quest-/NPC-Anschluss; er führt **keine** zweite
KI-Anbindung, **keine** parallele Sprachpipeline, **keine** neue Questmechanik
und **kein** Reputation-/Status-/Gilden-/Mail-System ein.

## 2. Tatsächlicher Produktionspfad

### 2.1 Client (Godot 3.6, `testclient/`, gitignored)

* `testclient/tc_conversation.gd`: NPC-Fenster mit der Schaltfläche
  **„Gespräch beginnen"** (`tc_conv_btn`). Es öffnet den Gesprächsverlauf
  (ScrollContainer), ein Freitextfeld (`LineEdit`, max. 500) und eine
  Senden-Schaltfläche.
* **Enter sendet genau einmal** (`text_entered` → `send_current`; das Feld wird
  sofort geleert, kein zweites Senden; **keine lokale NPC-Antwort**).
* **Während des Textfokus keine Bewegungs-/Hotbar-Absichten**: `tc_main` schützt
  Bewegung/Hotbar-Tasten über `conversation.has_focus()`.
* **Sichtbarer Wartezustand** (`tc_conv_wait`) und **verständliche Fehler** bei
  Nichterreichbarkeit/Ablehnung/Zeitüberschreitung (`tc_conv_err_*`).
* **Antworten werden nur dem passenden Gespräch zugeordnet**: `on_answer`
  korreliert über NPC + Wire-`seq` (+ `conversation_id`); NPC-Wechsel
  (`open_for`), Disconnect (`reset`) und verspätete Antworten werden keinem
  falschen Ziel zugeordnet.
* Deutsche/englische Texte über die bestehende i18n-Struktur (`i18n/de.json`,
  `i18n/en.json`, Schlüssel `tc_conv_*`).
* Transport: nutzt `NPC_TALK` (C2S 6) mit `action:"talk"` und `NPC_TEXT`
  (S2C 8) wieder — **kein neuer Nachrichtentyp**.

### 2.2 Realm (Rust, `src/realm-rs/src/conversation.rs`)

* **Identität und NPC-Zuordnung serverseitig bestimmt**: Ownership über
  `by_conn` (verbindungseigen), NPC aus der World, Reichweite über den
  bestehenden `loot.pickup_radius` (kein neuer Wert erfunden).
* Bestehende Ownership-/Interaktions-/Input-/Rate-/Kommunikationsregeln gelten:
  das bestehende `security`-Gate (Rate-Limit je Nachrichtentyp) und der
  Coordinator-Cooldown pro Spieler (§6) greifen; der Realm prüft das
  500-Zeichen-Input-Limit (§7) vor der KI erneut.
* **Nur begrenzter, zulässiger NPC-/Spielkontext an die KI**: Locale,
  NPC-Anzeigename/Rolle, eine stabile Situation. **Keine Secrets, keine Daten
  anderer Spieler, kein Inventar/Account/Session-Token** (Coordinator §28).
* **Keine World-Sperre über Netzwerk-/KI-Awaits**: Die World-Sperre ist auf die
  synchrone Validierung/Snapshot und den Revalidierungsblock begrenzt; der
  Coordinator-Roundtrip wartet OHNE Sperre.
* **Asynchrone Antworten nach einem Verbindungswechsel dürfen keine falsche
  Verbindung oder einen fremden Charakter erreichen**: Vor dem Senden validiert
  `correlate_ok` erneut, dass dieselbe `conn_id` noch zum selben Charakter
  gehört und der NPC noch vorhanden/lebendig ist; andernfalls wird die Antwort
  verworfen.
* Bestehende Queue-/Timeout-/Filtermechanismen werden genutzt (Coordinator
  `job_type` **`"npc"`** — die bestehende Priorität für aktive NPC-Interaktion,
  §5). Begrenztes Realm-seitiges Polling (`POLL_MAX_ATTEMPTS`); ein technisches
  Timeout ist **niemals** eine Questfrist (§11).
* **KI-Antworten als Gespräch**: Die Antwort wird als `NPC_TEXT`
  `{ok, action:"talk", npc_id, conversation_id, text, source:"ki"|"fallback"}`
  gesendet. Sie ändert **keine** Items, Idia, Reputation, Questzustände oder
  Belohnungen.
* **KI-Ausfall → lokaler Fallback** (§4/§13): bei `QUEUE_FULL`/`COOLDOWN`/
  `AI_UNAVAILABLE`/`TIMEOUT`/`VALIDATION_FAILED`/Nichterreichbarkeit sendet der
  Realm eine feste lokale Zeile (`FALLBACK_TEXT`, `source:"fallback"`) —
  **keine neue KI-Anfrage**.
* **Ein erzählerisches Versprechen ist kein verbindliches Questangebot**: Das
  Gespräch ruft niemals `quest_accept`/`quest_offers` auf; persönliche
  Questangebote bleiben ausdrücklich Folgearbeit (§4.5/§12).

### 2.3 Coordinator (Go, `src/coordinator/`)

* **Unverändert wiederverwendet**: `handleSubmitJob` → `inputValidation` (§7
  Länge, §9 Filter) → `accept` (Cooldown §6, persistente Einreihung §14–16) →
  `queue.process` (Ollama, begrenzte Korrektur §10–12) → Ergebnis über
  Result-Callback oder Polling von `GET /v1/jobs/{id}`. `job_type` `"npc"`
  existiert bereits.
* Kein neuer Job-Typ, kein neuer Endpunkt, kein neues Modell und keine neue
  Abhängigkeit.

## 3. Ausgeführte Tests (offline, modelliert)

Realm (`cargo test`, Handler-/Coordinator-Pfad am `handle_npc_talk`-Action
`"talk"`):

* `conv_valid_answers_ki_text` — gültiges Gespräch → ein `NPC_TEXT`
  `ok:true source:"ki"`, `conversation_id` zurückgegeben (modellierter KI-Text).
* `conv_unknown_npc_rejected` — ungültiger/fremder NPC →
  `ok:false reason:"unknown_npc"`.
* `conv_out_of_range_rejected` — bestehende Reichweiten-Sperre →
  `reason:"out_of_range"`.
* `conv_input_too_long_rejected_no_ki` — mehr als 500 Zeichen →
  `reason:"input_too_long"`, keine KI-Anfrage.
* `conv_ownership_lost_sends_nothing` — kein Ownership → stiller Rückzug (kein
  Frame).
* `conv_queue_full_falls_back_locally` / `conv_timeout_falls_back_locally` —
  KI-/Transport-Ausfall → `ok:true source:"fallback"` mit `FALLBACK_TEXT`.
* `conversation::tests::correlate_rejects_empty_world` + das `correlate_ok`-
  Prädikat — Antwortzuordnung/verspätete Antworten/NPC-Wechsel/Disconnect.
* `conversation::tests::submit_and_poll_mapping_is_closed`,
  `input_length_boundary_is_realm_side`, `live_client_signs_like_auth_scheme`.

Coordinator (`go test`):

* `TestSubmitConversationJob` — `job_type` `"npc"` Freitext → `QUEUED` (201).
* `TestSubmitConversationTooLong` — über `MAX_TEXT_CHARS` → 400 (§7).

## 4. Modelle und offene Grenzen (Nachweisgrenzen)

* **Modelliert/offline:** Die Realm-Tests injizieren einen
  `CoordinatorClient::modelled()`-Transport (vorgegebene Submit-/Poll-/Result-
  Antworten). **Es wird kein Live-Ollama/KI-Dienst und kein Live-Coordinator
  angenommen.** Der Live-signierte HTTP-Pfad (`submit`/`poll`/`query`) ist
  implementiert, wird hier aber **nicht gegen einen laufenden Coordinator
  geprüft**.
* **Godot-3.6-Lade-/Bedienprüfung NICHT ausgeführt:** Die Godot-Binary des
  Projekts benötigt ein X11-Display, und im Projekt ist kein Xvfb verfügbar;
  dessen Installation ist außerhalb des Umfangs. Das GDScript ist an den
  bestehenden Godot-3.6-Idiomen ausgerichtet (Tabulator-Einrückung,
  `LineEdit`/`ScrollContainer`/`Button`, `connect`), eine Laufzeit-Lade-/
  Bedienprüfung wird aber **nicht** behauptet.
* **Realm↔Coordinator-Ergebniszustellung:** Der Realm pollt
  `GET /v1/jobs/{id}` (begrenzt). Der alternative Result-Callback
  (`RealmCred.ResultURL`) ist im Realm noch nicht verdrahtet — eine offene
  Betriebsentscheidung, keine Spielregel.
* **Gesprächsidentität:** Der Realm bildet `job_id` serverseitig über
  `next_conv_job_id(realm_id)` als `conv-<realm_id>-<zeit>-<n>`: `<zeit>` ist
  der pro Aufruf aus `SystemTime::now()` gelesene Wanduhrwert in Nanosekunden,
  `<n>` der realm-lokale monotone Zähler `CONV_JOB_SEQ`. Die Client-
  `conversation_id` ist nur ein Korrelationshinweis. Eine reichere, versionierte
  Gesprächs-/Angebotsidentität bleibt Folgearbeit (§4.5/§12). **Der früher
  dokumentierte Stand `conv-<conn>-<seq>` ist überholt und beschreibt nicht die
  Implementierung.**
* **Keine DB-/Migrations-/Seed-/Dienstaktionen** wurden ausgeführt.

## 5. Ergebnis

Ein nutzbarer freier NPC-Gesprächspfad existiert: Client „Gespräch beginnen" →
Freitext → Realm-Autorität (Ownership/NPC/Reichweite/Länge) → bestehende
Coordinator-KI-Queue (`job_type` `"npc"`) → `NPC_TEXT` korreliert zum selben
Gespräch, mit lokalem Fallback bei KI-/Transport-Ausfall. Die KI hat keine
Spielautorität; nichts an Items/Idia/Reputation/Quests/Belohnungen ändert sich.

## 6. FIX (FINAL_CHECK) — konkrete Korrekturen

Die folgenden Punkte sind Bestandteil des oben beschriebenen Produktionsstands
und keine separat parallel gültige Variante.

### A — Job-Identität (serverseitig)

`conversation.rs::next_conv_job_id(realm_id)` erzeugt die maßgebliche
`conversation_id`/`job_id` als `conv-<realm_id>-<zeit>-<n>`:

- `<zeit>` = Wanduhr-Nanosekunden aus `SystemTime::now()`, **pro Aufruf**
  ermittelt (nicht einmalig beim Start),
- `<n>` = realm-lokaler monotoner `AtomicU64` (`CONV_JOB_SEQ.fetch_add`).

Unabhängig von der Client-`seq` und der zurücksetzbaren `conn_id`. Ein alleiniger
`AtomicU64` genügt über Neustarts nicht, deshalb wird der Wanduhrwert
beigemischt. **Zeitstempel plus Zähler verringern Kennungskollisionen,
garantieren aber keine globale Einmaligkeit** (derselbe Wanduhr-Nanosekundenwert
nach einem Neustart zusammen mit einem zurückgesetzten Zähler bleibt
grundsätzlich möglich). `seq` bleibt reine Korrelation. Eine verbleibende
Identitätskollision wird über `JOB_EXISTS` behandelt: Der Realm geht direkt auf
den lokalen Fallback und **pollt nicht**; ein altes/fremdes Ergebnis wird
**nicht übernommen**.

Die Ergebniszuordnung ist exakt: Beim Polling vergleicht der Live-Pfad das
zurückgegebene `job_id` mit dem übermittelten (keine verrutschte Übernahme).
`result_text` prüft `job_id` **nicht erneut**; es wird erst nach einem
bestätigten Treffer (`COMPLETED`) des Pollings erreicht.

### B — Zeitgrenzen (echte Deadline, nicht nur eine Poll-Anzahl)

- HTTP-Client mit `connect_timeout = HTTP_CONNECT_TIMEOUT_MS (1000 ms)` und
  Request-`timeout = HTTP_TIMEOUT_MS (1500 ms)` (analog `auth_api.rs`).
- Der GESAMTE Coordinator-Roundtrip (Submit + Polling + Ergebnislesen) ist in
  `tokio::time::timeout(COORDINATOR_TOTAL_BUDGET_MS = 2000 ms, …)` gefasst.
  Eine begrenzte Poll-Anzahl (`POLL_MAX_ATTEMPTS = 3`) ist keine Zeitgrenze;
  diese Deadline ist es.
- Bei Überschreiten der Deadline → dokumentierter Fallback (`FALLBACK_TEXT`,
  `source:"fallback"`), keine neue KI-Anfrage. Über keinen dieser Awaits wird
  die World-Sperre gehalten.

### C — Kommunikations-/Elternkontroll-Gate

`parental::chat_allowed(&ctx.parental, &pid)` wird geprüft, BEVOR irgendein
Spielertext den Coordinator erreicht. Bei Sperre sendet der Realm die bestehende
Ablehnungssemantik als `NPC_TEXT {ok:false, action:"talk",
reason:"chat_locked"}` und stellt KEINE KI-Anfrage. Der Client beendet seinen
Wartezustand bei dieser Ablehnung zuverlässig (`_show_reject` setzt
`_waiting=false`; i18n `tc_conv_err_parental`). Es wird keine neue
Berechtigungsregel erfunden — das bestehende Gate wird wiederverwendet.

### Ownership

`is_owner(&world, conn_id, pid)` wird zusätzlich zur `by_conn`-Zuordnung
angewendet (wie in den benachbarten `npc_quest`/`player_trade`-Handlern).

### Neue Nachweise (offline, modelliert; `cargo test conv_` = 13 grün)

- `conv_same_seq_distinct_jobs` — zwei Nachrichten, dieselbe Verbindung,
  DIESELBE `seq` → unterschiedliche serverseitige Jobs, jeweils korreliert.
- `conv_collision_never_adopts_foreign_result` — `JOB_EXISTS` → Fallback,
  niemals das alte/fremde Ergebnis.
- `conv_deadline_falls_back` — nie antwortender Transport (modelliertes HANG)
  endet innerhalb der Deadline → Fallback.
- `conv_parental_locked_no_ki` — Elternsperre → `chat_locked`, kein Transport/KI.
- `conv_takeover_no_request` — `by_conn` einem nicht-live-pid zugewiesen → kein
  Frame.
- `conv_job_id_is_server_side_and_distinct` — Identität unabhängig von
  `seq`/`conn`.
