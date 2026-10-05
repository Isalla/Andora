// security — Serverautorität & Anti-Manipulation V1.
//
// Leitgedanke: Der Client darf lügen. Der Server darf ihm nur nicht glauben.
// Der Client übermittelt Absichten/Eingaben (z. B. SPEND_ATTRIBUTE_POINT
// strength, BUY_AUCTION id), der Server berechnet Ergebnisse ausschließlich
// aus seinem autoritativen RAM-Zustand (world::Player) und persistiert über
// die bestehende Persistenzarchitektur (spool/dirty-Flags).
//
// Dieses Modul bündelt die V1-Schutzschicht bewusst klein:
// - frühe, billige Netzwerkprüfung (Größe/Format/Session/Seq/Rate),
// - einfache Ratenbegrenzung je Nachrichtentyp (kein teurer Pfad bei Flood),
// - serverautoritatives Ausgeben von Attributpunkten (+1, keine Clientwerte),
// - serverautoritatives Validieren von Auktionskäufen (Preis/Gold/Eigentum),
// - einfaches Ablehnungs-Logging (keine Cheat-Verurteilung, keine Banns).
//
// Ausdrücklich NICHT V1: externe Anti-Cheat-Software, Kernel-Treiber,
// ML-/Bot-Erkennung, permanente Banns, GM-Oberfläche, Krypto-Eigenbauten.
// zstd bleibt reine Kompression größerer Übertragungen, kein Schutz.
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use crate::world::Player;

/// Nachrichtenkategorie für gestaffelte Rate Limits (V1, bewusst grob):
/// Bewegung ist Echtzeit (häufig), Chat/Heartbeat mittel, seltene Aktionen
/// (Attribute, Käufe, Handel, Crafting, Gruppe, Loot) streng limitiert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MsgClass {
    /// MOVE: häufig, aber pro Tick ohnehin nur einmal wirksam.
    Movement,
    /// CHAT, HEARTBEAT, PARENTAL: normale Interaktionsfrequenz.
    Interactive,
    /// ATTACK, ABILITY, PICKUP: kampfrelevant, mittlere Frequenz.
    Combat,
    /// Seltene, teure Aktionen: Attribute, Auktion, Gruppe, NPC.
    Rare,
}

impl MsgClass {
    /// Ordnet eine C2S-Nachrichten-ID einer Kategorie zu. Unbekannte IDs
    /// fallen in Rare (strengstes Limit, fail-closed).
    pub fn of(msg_type: i64) -> Self {
        match msg_type {
            t if t == crate::protocol::c2s::MOVE => MsgClass::Movement,
            t if t == crate::protocol::c2s::CHAT
                || t == crate::protocol::c2s::HEARTBEAT
                || t == crate::protocol::c2s::PARENTAL =>
            {
                MsgClass::Interactive
            }
            t if t == crate::protocol::c2s::ATTACK
                || t == crate::protocol::c2s::ABILITY
                || t == crate::protocol::c2s::PICKUP =>
            {
                MsgClass::Combat
            }
            _ => MsgClass::Rare,
        }
    }
}

/// Konfiguration der V1-Schutzschicht (per config.env übersteuerbar,
/// siehe config::security_config). Alle Werte sind Mechanik, kein Balancing.
#[derive(Debug, Clone)]
pub struct SecurityCfg {
    /// Max. akzeptierte WS-Frame-Größe in Bytes (größere Frames werden
    /// verworfen, bevor JSON geparst wird).
    pub max_frame_bytes: usize,
    /// Erlaubte Requests je 1000-ms-Fenster je Kategorie.
    pub movement_per_sec: u32,
    pub interactive_per_sec: u32,
    pub combat_per_sec: u32,
    pub rare_per_sec: u32,
    /// Auffälligkeiten (Ablehnungen/Rate-Überschreitungen) je Verbindung,
    /// ab der die Verbindung getrennt wird (kein Bann, nur Disconnect).
    pub disconnect_after_violations: u32,
}

impl Default for SecurityCfg {
    fn default() -> Self {
        SecurityCfg {
            max_frame_bytes: 65536,
            movement_per_sec: 30,
            interactive_per_sec: 10,
            combat_per_sec: 10,
            rare_per_sec: 5,
            disconnect_after_violations: 50,
        }
    }
}

impl SecurityCfg {
    pub fn limit_of(&self, class: MsgClass) -> u32 {
        match class {
            MsgClass::Movement => self.movement_per_sec,
            MsgClass::Interactive => self.interactive_per_sec,
            MsgClass::Combat => self.combat_per_sec,
            MsgClass::Rare => self.rare_per_sec,
        }
    }
}

/// Ergebnis der frühen Prüfung eines Requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// Weiter zur Spiellogik.
    Allow,
    /// Verwerfen + Auffälligkeit zählen (+ ggf. loggen). Kein DB-/Kampf-/
    /// Inventar-/Welt-/KI-Pfad wird betreten.
    Drop,
    /// Wie Drop, zusätzlich Verbindung trennen (massive/wiederholte
    /// Überschreitung). Kein permanenter Bann.
    Disconnect,
}

/// Ablehnungsgrund für Logging/GM-Auswertung (V1: nur Ablehnung +
/// Zählung, keine automatische Verurteilung).
#[derive(Debug, Clone)]
pub struct RejectInfo {
    pub reason: std::borrow::Cow<'static, str>,
    pub msg_type: i64,
    pub detail: String,
}

/// Einfaches Ablehnungs-Logging (docs-Vorgabe: Zeitpunkt, Charakter,
/// Request-Typ, Ablehnungsgrund, Wiederholungsanzahl, relevanter
/// Serverzustand). Keine großen Datenmengen; Basis für eine spätere
/// GM-/Anti-Cheat-Auswertung.
///
/// Zugangsdaten werden bewusst NICHT ausgegeben: Die vollständige Session-ID
/// wird nicht protokolliert (AUTH-05; docs/Security.md §4.5), ebenso keine
/// Handoff-Tokens oder Passwörter. Für den Test wird nur die Zeile gebaut,
/// nicht geschrieben.
pub fn reject_log_line(
    player: Option<&Player>,
    conn_id: u64,
    info: &RejectInfo,
    violations: u32,
) -> String {
    let (char_id, server_state) = match player {
        Some(p) => (
            p.id.as_str(),
            format!(
                "hp={} level={} exp={} idia={} free_attr={}",
                p.hp, p.level, p.exp, p.idia, p.free_attr_points
            ),
        ),
        None => ("-", "-".to_string()),
    };
    format!(
        "sec-reject conn={conn_id} char={char_id} type={} reason={} violations={violations} state=[{server_state}] {}",
        info.msg_type,
        info.reason,
        info.detail,
    )
}

pub fn log_reject(player: Option<&Player>, conn_id: u64, info: &RejectInfo, violations: u32) {
    log::warn!("{}", reject_log_line(player, conn_id, info, violations));
}

/// Zeile des Takeover-Ereignisses (docs/Security.md AUTH-03;
/// docs/datenschutz_zugang.md „Connection-Takeover-Logs“).
///
/// Ausgegeben werden ausschließlich Ereignisname, Account-ID, Charakter-ID,
/// alte und neue `conn_id` sowie der Zustand der verdrängten Verbindung.
/// Roh-IP-Adressen sind hier bewusst NICHT enthalten: deren dauerhafte
/// Protokollierung mit 14-Tage-Löschung ist ein eigener Auftrag (AUTH-03B)
/// und benötigt einen freigegebenen Log-Sink.
pub fn takeover_log_line(
    account_id: u32,
    player_id: &str,
    old_conn_id: u64,
    new_conn_id: u64,
) -> String {
    format!(
        "authenticated_connection_takeover account_id={account_id} char_id={player_id} \
         old_conn_id={old_conn_id} new_conn_id={new_conn_id} old_state=authenticated"
    )
}

/// Ein einzelner Takeover ist ein normales INFO-Ereignis; es erfolgt weder
/// eine Warnung noch eine automatische Sanktion (docs/Security.md AUTH-03).
pub fn log_takeover(account_id: u32, player_id: &str, old_conn_id: u64, new_conn_id: u64) {
    log::info!(
        "{}",
        takeover_log_line(account_id, player_id, old_conn_id, new_conn_id)
    );
}

/// Zustand einer Verbindung in der V1-Schutzschicht: Rate-Fenster je
/// Kategorie, Auffälligkeitszähler, letzte Sequenznummer.
#[derive(Debug, Default)]
pub struct ConnGuard {
    windows: HashMap<MsgClass, VecDeque<Instant>>,
    pub violations: u32,
    pub last_seq: i64,
    pub seen_any_seq: bool,
}

impl ConnGuard {
    /// Prüft das Rate Limit für einen Nachrichtentyp. Reine RAM-Operation
    /// (kein DB-/Logik-Zugriff). Alte Fenster-Einträge (>1000 ms) verfallen.
    pub fn check_rate(&mut self, cfg: &SecurityCfg, msg_type: i64, now: Instant) -> bool {
        let class = MsgClass::of(msg_type);
        let limit = cfg.limit_of(class);
        let window = self.windows.entry(class).or_default();
        while window
            .front()
            .is_some_and(|t| now.duration_since(*t).as_millis() > 1000)
        {
            window.pop_front();
        }
        if window.len() as u32 >= limit {
            return false;
        }
        window.push_back(now);
        true
    }

    /// Zählt eine Auffälligkeit; true = Schwelle erreicht → Disconnect.
    pub fn add_violation(&mut self, cfg: &SecurityCfg) -> bool {
        self.violations = self.violations.saturating_add(1);
        self.violations >= cfg.disconnect_after_violations.max(1)
    }

    /// Grundlegende Sequenzprüfung (Lag-tolerant): Duplikate/Out-of-Order
    /// werden nur vermerkt, nie als Cheat gewertet oder abgelehnt.
    pub fn note_seq(&mut self, seq: i64) -> bool {
        let fresh = !self.seen_any_seq || seq > self.last_seq;
        if fresh {
            self.last_seq = seq;
            self.seen_any_seq = true;
        }
        fresh
    }
}

/// Späte, billige Prüfung eines eingehenden Frames.
///
/// **Diese Funktion prüft genau zwei Stufen, in dieser Reihenfolge:**
/// **Session → Rate Limit.** Sie prüft **weder** Größe **noch** JSON-Format
/// **noch** Typ-Whitelist — diese drei liegen **beim Aufrufer** (`read_loop`
/// in `net.rs`: Größe `:582`, Parse `:592`, Whitelist `:602`).
///
/// Die tatsächliche Gesamtreihenfolge einer Nachricht lautet:
/// Größe → Parse → Whitelist → Sequenzbeobachtung → **Session → Rate
/// Limit** (hier) → Dispatch → typabhängige Fachprüfung.
///
/// Die Sequenzbeobachtung (`ConnGuard::note_seq`) ist **kein** Gate: sie
/// läuft im `dispatch` **vor** diesem Aufruf, wird dort nur vermerkt und
/// entscheidet nichts (docs/Security.md Abschnitt 4.2, normative Fassung
/// `docs/Serverautoritaet_und_Anti-Manipulation_V1.md` §3.3.1). Sie ist hier
/// deshalb bewusst nicht enthalten.
pub fn gate_frame(
    cfg: &SecurityCfg,
    guard: &mut ConnGuard,
    msg_type: i64,
    authenticated: bool,
    now: Instant,
) -> GateDecision {
    // 1) Session: HELLO darf ohne Session einsteigen (Einstieg), alles
    //    andere erfordert eine zugeordnete, eingeloggte Verbindung.
    if msg_type != crate::protocol::c2s::HELLO && !authenticated {
        guard.add_violation(cfg);
        return GateDecision::Drop;
    }
    // 2) Rate Limit (billig, vor jeder Spiellogik/DB).
    if !guard.check_rate(cfg, msg_type, now) {
        if guard.add_violation(cfg) {
            return GateDecision::Disconnect;
        }
        return GateDecision::Drop;
    }
    GateDecision::Allow
}

/// Prüft die rohe Frame-Größe VOR dem JSON-Parsing (billigste Stufe).
pub fn frame_too_large(cfg: &SecurityCfg, bytes: usize) -> bool {
    bytes > cfg.max_frame_bytes
}

/// Bekannte C2S-Typen (Protokoll-Whitelist; Unbekanntes wird verworfen und
/// geloggt, erreicht aber nie die Spiellogik).
pub fn is_known_c2s(msg_type: i64) -> bool {
    use crate::protocol::c2s::*;
    matches!(
        msg_type,
        HELLO
            | MOVE
            | ATTACK
            | PICKUP
            | CHAT
            | NPC_TALK
            | AUCTION_LIST
            | AUCTION_BID
            | AUCTION_BUY
            | HEARTBEAT
            | PARENTAL
            | ABILITY
            | GROUP_INVITE
            | GROUP_INVITE_REACT
            | GROUP_SUGGEST
            | GROUP_SUGGEST_DECIDE
            | GROUP_LEAVE
            | GROUP_KICK
            | GROUP_TRANSFER
            | SPEND_ATTRIBUTE
    )
}

// ── Attribute (serverautoritativ) ─────────────────────────────────────────

/// Gültige Attributschlüssel für SPEND_ATTRIBUTE_POINT (Allowlist; alles
/// andere wird abgelehnt — der Client kann keine beliebigen Felder setzen).
pub const SPENDABLE_ATTRIBUTES: [&str; 7] = [
    "strength",
    "constitution",
    "dexterity",
    "intelligence",
    "wisdom",
    "luck",
    "endurance",
];

/// Fehler beim Ausgeben eines Attributpunkts (keine Cheat-Verurteilung:
// Lag/Doppelklick/Clientfehler sind möglich — nur ablehnen + zählen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpendAttrError {
    UnknownAttribute,
    NoPointsLeft,
}

impl SpendAttrError {
    pub fn reason(self) -> &'static str {
        match self {
            SpendAttrError::UnknownAttribute => "unknown_attribute",
            SpendAttrError::NoPointsLeft => "no_attribute_points",
        }
    }
}

/// Serverautoritatives Ausgeben EINES Attributpunkts:
///
/// 1. Charakter aus serverseitigem Zustand (RAM, nicht Clientwerte),
/// 2. verfügbare Punkte prüfen (free_attr_points > 0),
/// 3. Attributschlüssel gegen Allowlist prüfen,
/// 4. Stärke (o. a.) intern um genau +1 erhöhen,
/// 5. verfügbaren Punkt reduzieren,
/// 6. Max-Ressourcen neu berechnen, Progression dirty markieren
///    (Persistenz läuft über die bestehende Spool-Architektur).
///
/// Client-seitig manipulierte Werte (z. B. strength=999, SET-Requests mit
/// Endwerten) haben keinerlei Bedeutung: Der Client liefert nur die Aktion
/// (welches Attribut), der Server bestimmt Startwert, Schritt (+1) und
/// Ergebnis. SET_* Nachrichten mit Endwerten existieren nicht im Protokoll.
pub fn spend_attribute_point(
    player: &mut Player,
    attr: &str,
) -> Result<(), SpendAttrError> {
    let key = attr.trim().to_lowercase();
    if !SPENDABLE_ATTRIBUTES.contains(&key.as_str()) {
        return Err(SpendAttrError::UnknownAttribute);
    }
    if player.free_attr_points == 0 {
        return Err(SpendAttrError::NoPointsLeft);
    }
    let slot = match key.as_str() {
        "strength" => &mut player.attributes.strength,
        "constitution" => &mut player.attributes.constitution,
        "dexterity" => &mut player.attributes.dexterity,
        "intelligence" => &mut player.attributes.intelligence,
        "wisdom" => &mut player.attributes.wisdom,
        "luck" => &mut player.attributes.luck,
        "endurance" => &mut player.attributes.endurance,
        _ => return Err(SpendAttrError::UnknownAttribute),
    };
    *slot = slot.saturating_add(1);
    player.free_attr_points -= 1;
    crate::attributes::recompute_max_resources(player);
    player.mark_dirty(crate::persist::PersistComponent::Progression);
    Ok(())
}

// ── Auktionshaus (serverautoritativ, V1-Anschluss) ─────────────────────────

/// Serverseitige Sicht auf ein Auktionsangebot (aus der DB/dem AH-State —
/// NIEMALS aus Client-Feldern wie Preis, Verkäufer oder Goldbestand).
#[derive(Debug, Clone)]
pub struct AuctionOffer {
    /// Serverseitige Angebots-ID (vom Client als einzige Eingabe übermittelt).
    #[allow(dead_code)]
    pub auction_id: i64,
    /// Noch aktiv (nicht verkauft/abgelaufen/abgebrochen)?
    pub active: bool,
    /// Serverseitig gespeicherter Festpreis.
    pub price: i64,
    /// Verkäufer-Charakter-ID (Eigentümer des Items).
    pub seller_id: String,
}

/// Fehlergründe eines Kaufversuchs (der Client erfährt nur ok/reason;
/// decidido wird ausschließlich aus Serverwerten).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuctionBuyError {
    /// Angebot existiert nicht (mehr).
    UnknownAuction,
    /// Angebot nicht mehr aktiv.
    NotActive,
    /// Eigene Auktion kaufen ist unzulässig.
    SelfBuy,
    /// Serverseitiges Gold reicht nicht (Client-Anzeige ist irrelevant).
    InsufficientGold,
    /// Preis/Werte ungültig (defekter Content-Eintrag).
    InvalidOffer,
}

impl AuctionBuyError {
    pub fn reason(self) -> &'static str {
        match self {
            AuctionBuyError::UnknownAuction => "auction_unknown",
            AuctionBuyError::NotActive => "auction_inactive",
            AuctionBuyError::SelfBuy => "auction_self_buy",
            AuctionBuyError::InsufficientGold => "insufficient_gold",
            AuctionBuyError::InvalidOffer => "auction_invalid",
        }
    }
}

/// Serverautoritatives Prüfen eines Auktionskaufs. Der Client übermittelt
/// nur die gewünschte Auktion (`auction_id`); ALLES andere ermittelt der
/// Server selbst: Existenz, Aktivität, Preis, Käufer-Gold (RAM, nicht
/// Clientanzeige), Eigentum, Selbstkauf-Verbot. Erst bei Ok darf die
/// Transaktion (Goldtransfer + Eigentumswechsel) ausgeführt werden.
///
/// `lookup` löst die Angebots-ID serverseitig auf (DB/AH-State); gibt None
/// bei unbekannter ID zurück. Kein DB-Zugriff in dieser Funktion selbst —
/// der Aufrufer lädt das Angebot einmal und übergibt es (kein Trust in
/// Client-Preis/-Verkäufer/-Gold).
pub fn validate_auction_buy(
    buyer_id: &str,
    buyer_gold: i64,
    offer: Option<&AuctionOffer>,
) -> Result<i64, AuctionBuyError> {
    let Some(o) = offer else {
        return Err(AuctionBuyError::UnknownAuction);
    };
    if !o.active {
        return Err(AuctionBuyError::NotActive);
    }
    if o.price <= 0 {
        return Err(AuctionBuyError::InvalidOffer);
    }
    if o.seller_id == buyer_id {
        return Err(AuctionBuyError::SelfBuy);
    }
    // Entscheidend: der SERVERSEITIGE Goldstand. Zeigt ein manipulierter
    // Client 99.999 Gold bei serverseitigen 243 Gold, wird anhand der 243
    // abgelehnt.
    if buyer_gold < o.price {
        return Err(AuctionBuyError::InsufficientGold);
    }
    Ok(o.price)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn test_player() -> Player {
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        Player {
            id: "hero".into(),
            name: "hero".into(),
            x: 0.0,
            y: 0.0,
            face: 0.0,
            ping_ms: 0,
            zone_id: 0,
            hp: 100,
            max_hp: 100,
            lang: "de".into(),
            account_id: 7,
            session_id: "sess-1".into(),
            entities: HashSet::new(),
            last_activity: Instant::now(),
            tx,
            char_class: "Adventurer".into(),
            class: crate::class::ClassStatus::Adventurer,
            faction_transition: false,
            level: 5,
            exp: 0,
            free_attr_points: 3,
            rested_pool: 0,
            idia: 243,
            armor: 0,
            weapon_skill: 1,
            combat: None,
            last_strike: None,
            mana: 50,
            max_mana: 50,
            effects: Vec::new(),
            cooldowns: Default::default(),
            active_cast: None,
            learned_abilities: HashSet::new(),
            attributes: crate::attributes::Attributes {
                strength: 10,
                constitution: 10,
                dexterity: 10,
                intelligence: 10,
                wisdom: 10,
                luck: 10,
                endurance: 10,
            },
            max_hp_base: 100,
            max_mana_base: 50,
            sitting: false,
            hp_regen_bonus: 0.0,
            mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0,
            mana_regen_carry: 0.0,
            inventory: Default::default(),
            quests: Default::default(),
            dirty: Default::default(),
            persist_generation: 0,
            persist_revision: 0,
        }
    }

    fn offer(price: i64, active: bool, seller: &str) -> AuctionOffer {
        AuctionOffer {
            auction_id: 4711,
            active,
            price,
            seller_id: seller.into(),
        }
    }

    // 1) Manipulierter Attributwert (strength=999 im Client) ist bedeutungslos:
    //    Der Server erhöht nur um +1 ab dem SERVERSEITIGEN Wert.
    #[test]
    fn manipulated_attribute_value_is_ignored() {
        let mut p = test_player();
        // Client behauptet lokal strength=999 — der Server kennt nur 10.
        assert_eq!(p.attributes.strength, 10);
        spend_attribute_point(&mut p, "strength").unwrap();
        assert_eq!(p.attributes.strength, 11);
        assert_eq!(p.free_attr_points, 2);
    }

    // 1b) Unbekannte/manipulierte Attributnamen werden abgelehnt, ohne zu mutieren.
    #[test]
    fn unknown_attribute_rejected_without_mutation() {
        let mut p = test_player();
        assert_eq!(
            spend_attribute_point(&mut p, "strength999"),
            Err(SpendAttrError::UnknownAttribute)
        );
        assert_eq!(
            spend_attribute_point(&mut p, "gold"),
            Err(SpendAttrError::UnknownAttribute)
        );
        assert_eq!(p.attributes.strength, 10);
        assert_eq!(p.free_attr_points, 3);
        assert!(!p.dirty.any());
    }

    // 2) Kein verfügbarer Punkt → Ablehnung, keine Mutation, kein DB-Pfad.
    #[test]
    fn spend_without_points_rejected() {
        let mut p = test_player();
        p.free_attr_points = 0;
        assert_eq!(
            spend_attribute_point(&mut p, "strength"),
            Err(SpendAttrError::NoPointsLeft)
        );
        assert_eq!(p.attributes.strength, 10);
        assert!(!p.dirty.any());
        assert_eq!(p.persist_generation, 0);
    }

    // 2b) Alle sieben Attribute sind ausgebbar (je genau +1).
    #[test]
    fn all_attributes_spendable_one_step() {
        let mut p = test_player();
        p.free_attr_points = 7;
        for a in SPENDABLE_ATTRIBUTES {
            spend_attribute_point(&mut p, a).unwrap();
        }
        assert_eq!(p.free_attr_points, 0);
        assert_eq!(p.attributes.strength, 11);
        assert_eq!(p.attributes.endurance, 11);
        // Constitution +1 → Max-HP neu berechnet (100 Basis + 11*10).
        assert_eq!(p.max_hp, 100 + 11 * 10);
    }

    // 3) Manipuliertes Client-Gold (99.999 angezeigt, 243 serverseitig):
    //    Kauf zu 500 wird anhand der 243 abgelehnt.
    #[test]
    fn manipulated_gold_does_not_buy() {
        let p = test_player();
        let o = offer(500, true, "seller-1");
        assert_eq!(
            validate_auction_buy(&p.id, p.idia, Some(&o)),
            Err(AuctionBuyError::InsufficientGold)
        );
    }

    // 3b) Ehrlicher Kauf mit ausreichendem Server-Gold wird freigegeben.
    #[test]
    fn affordable_auction_passes_with_server_price() {
        let p = test_player();
        let o = offer(200, true, "seller-1");
        assert_eq!(validate_auction_buy(&p.id, p.idia, Some(&o)), Ok(200));
    }

    // 4) Ungültige Käufe: unbekannt, inaktiv, Selbstkauf, defekter Preis.
    #[test]
    fn invalid_auction_buys_rejected() {
        let p = test_player();
        assert_eq!(
            validate_auction_buy(&p.id, p.idia, None),
            Err(AuctionBuyError::UnknownAuction)
        );
        assert_eq!(
            validate_auction_buy(&p.id, p.idia, Some(&offer(100, false, "s"))),
            Err(AuctionBuyError::NotActive)
        );
        assert_eq!(
            validate_auction_buy(&p.id, 99999, Some(&offer(100, true, "hero"))),
            Err(AuctionBuyError::SelfBuy)
        );
        assert_eq!(
            validate_auction_buy(&p.id, 99999, Some(&offer(0, true, "s"))),
            Err(AuctionBuyError::InvalidOffer)
        );
    }

    // 7) Flood: 500 identische Rare-Requests in 1 s führen nicht zu 500
    //    Logikoperationen — nur `rare_per_sec` (Default 5) passieren das Gate.
    #[test]
    fn flood_of_rare_requests_is_capped() {
        let cfg = SecurityCfg::default();
        let mut g = ConnGuard::default();
        let now = Instant::now();
        let mut allowed = 0;
        for _ in 0..500 {
            if gate_frame(
                &cfg,
                &mut g,
                crate::protocol::c2s::SPEND_ATTRIBUTE,
                true,
                now,
            ) == GateDecision::Allow
            {
                allowed += 1;
            }
        }
        assert_eq!(allowed, cfg.rare_per_sec as usize);
    }

    // 7b) Bewegung darf deutlich häufiger als seltene Aktionen sein.
    #[test]
    fn movement_limit_exceeds_rare_limit() {
        let cfg = SecurityCfg::default();
        assert!(cfg.movement_per_sec > cfg.rare_per_sec);
        let mut g = ConnGuard::default();
        let now = Instant::now();
        let mut allowed = 0;
        for _ in 0..500 {
            if gate_frame(&cfg, &mut g, crate::protocol::c2s::MOVE, true, now)
                == GateDecision::Allow
            {
                allowed += 1;
            }
        }
        assert_eq!(allowed, cfg.movement_per_sec as usize);
    }

    // 7c) Wiederholte Überschreitung → Disconnect (kein permanenter Bann).
    #[test]
    fn repeated_violations_disconnect() {
        let mut cfg = SecurityCfg::default();
        cfg.disconnect_after_violations = 3;
        cfg.rare_per_sec = 1;
        let mut g = ConnGuard::default();
        let now = Instant::now();
        assert_eq!(
            gate_frame(&cfg, &mut g, crate::protocol::c2s::SPEND_ATTRIBUTE, true, now),
            GateDecision::Allow
        );
        assert_eq!(
            gate_frame(&cfg, &mut g, crate::protocol::c2s::SPEND_ATTRIBUTE, true, now),
            GateDecision::Drop
        );
        assert_eq!(
            gate_frame(&cfg, &mut g, crate::protocol::c2s::SPEND_ATTRIBUTE, true, now),
            GateDecision::Drop
        );
        assert_eq!(
            gate_frame(&cfg, &mut g, crate::protocol::c2s::SPEND_ATTRIBUTE, true, now),
            GateDecision::Disconnect
        );
    }

    // 8) Unauthentifizierte Requests (außer HELLO) erreichen nie die Logik.
    #[test]
    fn unauthenticated_requests_dropped_before_logic() {
        let cfg = SecurityCfg::default();
        let mut g = ConnGuard::default();
        let now = Instant::now();
        assert_eq!(
            gate_frame(&cfg, &mut g, crate::protocol::c2s::ATTACK, false, now),
            GateDecision::Drop
        );
        assert_eq!(
            gate_frame(&cfg, &mut g, crate::protocol::c2s::HELLO, false, now),
            GateDecision::Allow
        );
    }

    // ── Audit 4.4: Rate-Fenster, geteilte Budgets, Verletzungszweig ──────
    //
    // Diese Tests rufen die **Produktionsfunktionen** `check_rate` und
    // `gate_frame` mit kontrolliertem `now` auf; es wird keine Rate-Formel
    // nachgebaut. Der Burst-Nachweis „genau `limit` sofort zulässig" ist für
    // Selten bereits durch `flood_of_rare_requests_is_capped` und für
    // Bewegung durch `movement_limit_exceeds_rare_limit` erbracht und wird hier
    // **nicht** wiederholt.

    /// L-2: Die Fenstergrenze ist **exklusiv bei 1000 ms**. Ein Eintrag bleibt
    /// exakt 1000 ms belegt und verfällt erst ab 1001 ms (Ganzmillisekunden-
    /// Trunkierung von `duration_since(...).as_millis() > 1000`).
    #[test]
    fn rate_window_boundary_is_exclusive_at_1000_ms() {
        let mut cfg = SecurityCfg::default();
        cfg.movement_per_sec = 1;
        let mut g = ConnGuard::default();
        let t0 = Instant::now();
        assert!(g.check_rate(&cfg, crate::protocol::c2s::MOVE, t0));
        assert!(
            !g.check_rate(
                &cfg,
                crate::protocol::c2s::MOVE,
                t0 + std::time::Duration::from_millis(1000)
            ),
            "exakt 1000 ms bleibt noch belegt"
        );
        assert!(
            g.check_rate(
                &cfg,
                crate::protocol::c2s::MOVE,
                t0 + std::time::Duration::from_millis(1001)
            ),
            "ab 1001 ms ist der Slot wieder frei"
        );
    }

    /// L-2: **Abgewiesene** Versuche werden nicht in die Queue geschrieben und
    /// verlängern das Fenster daher nicht. Wäre `push_back` auch im
    /// Abweisungszweig, bliebe der Slot bis 1600 ms belegt.
    #[test]
    fn rejected_rate_attempts_do_not_extend_the_window() {
        let mut cfg = SecurityCfg::default();
        cfg.rare_per_sec = 2;
        let mut g = ConnGuard::default();
        let t0 = Instant::now();
        let t = crate::protocol::c2s::SPEND_ATTRIBUTE;
        assert!(g.check_rate(&cfg, t, t0));
        assert!(g.check_rate(&cfg, t, t0));
        for _ in 0..50 {
            assert!(
                !g.check_rate(&cfg, t, t0 + std::time::Duration::from_millis(600)),
                "Budget nach zwei erlaubten Versuchen erschöpft"
            );
        }
        assert!(
            g.check_rate(&cfg, t, t0 + std::time::Duration::from_millis(1001)),
            "die 50 Abweisungen bei 600 ms dürfen das Fenster nicht verlängert haben"
        );
    }

    /// L-3: Typen **derselben** Klasse teilen **ein** Budget. ATTACK, ABILITY
    /// und PICKUP liegen alle in `MsgClass::Combat` und verbrauchen dieselbe
    /// `VecDeque`.
    #[test]
    fn two_types_of_same_class_share_one_budget() {
        let mut cfg = SecurityCfg::default();
        cfg.combat_per_sec = 2;
        let mut g = ConnGuard::default();
        let now = Instant::now();
        assert!(g.check_rate(&cfg, crate::protocol::c2s::ATTACK, now));
        assert!(g.check_rate(&cfg, crate::protocol::c2s::ABILITY, now));
        assert!(
            !g.check_rate(&cfg, crate::protocol::c2s::ATTACK, now),
            "ATTACK und ABILITY verbrauchen dasselbe Combat-Budget"
        );
        assert!(!g.check_rate(&cfg, crate::protocol::c2s::PICKUP, now));
    }

    /// L-3: Eine erschöpfte Klasse bleibt ohne Wirkung auf andere Klassen —
    /// jede Klasse hat ihre eigene Queue.
    #[test]
    fn exhausted_class_does_not_affect_other_classes() {
        let mut cfg = SecurityCfg::default();
        cfg.movement_per_sec = 1;
        cfg.rare_per_sec = 1;
        let mut g = ConnGuard::default();
        let now = Instant::now();
        assert!(g.check_rate(&cfg, crate::protocol::c2s::MOVE, now));
        assert!(!g.check_rate(&cfg, crate::protocol::c2s::MOVE, now));
        assert!(
            g.check_rate(&cfg, crate::protocol::c2s::SPEND_ATTRIBUTE, now),
            "Selten bleibt von einem leeren Bewegungsbudget unberührt"
        );
        assert!(!g.check_rate(&cfg, crate::protocol::c2s::SPEND_ATTRIBUTE, now));
    }

    /// L-5: Der Session-Zweig **zählt, trennt aber nicht**. Der Rückgabewert
    /// von `add_violation` wird dort bewusst verworfen (`security.rs`), daher
    /// bleibt der `GateDecision` auch oberhalb der Schwelle `Drop`. Zusätzlich
    /// belegt: Session-Verstöße verbrauchen **kein** Klassenbudget, weil
    /// `check_rate` erst nach der Session-Prüfung läuft.
    ///
    /// **Offene Schutzentscheidung** (Audit 4.4, R-5): Der Test hält das
    /// Bestandsverhalten fest. Er ist **keine** Aussage, dass ein nicht
    /// angemeldeter Flood dadurch ausreichend begrenzt wird, und **keine**
    /// Empfehlung, eine Straf-, Bann- oder Disconnect-Regel zu ergänzen.
    #[test]
    fn session_gate_violations_never_return_disconnect() {
        let mut cfg = SecurityCfg::default();
        cfg.disconnect_after_violations = 1;
        let mut g = ConnGuard::default();
        let now = Instant::now();
        for i in 1..=5u32 {
            assert_eq!(
                gate_frame(&cfg, &mut g, crate::protocol::c2s::ATTACK, false, now),
                GateDecision::Drop,
                "Session-Verstoß {i} muss Drop bleiben, auch über der Schwelle"
            );
        }
        assert_eq!(g.violations, 5, "jeder Session-Verstoß wird gezählt");
        // Budget der betroffenen Klasse ist unberührt: genau `combat_per_sec`
        // nachgelagerte, nun angemeldete Versuche passieren.
        let mut allowed = 0;
        for _ in 0..500 {
            if gate_frame(&cfg, &mut g, crate::protocol::c2s::ATTACK, true, now)
                == GateDecision::Allow
            {
                allowed += 1;
            }
        }
        assert_eq!(
            allowed, cfg.combat_per_sec as usize,
            "Session-Verstöße dürfen kein Klassenbudget verbrauchen"
        );
    }

    /// L-4 (Bestand, keine Empfehlung): `rare_per_sec = 0` **sperrt** die
    /// gesamte Selten-Klasse, weil `window.len() >= 0` immer gilt. Ein `0` ist
    /// damit **keine** Abschaltung, sondern eine Blockade — auch für HELLO.
    #[test]
    fn zero_rate_limit_locks_the_whole_class() {
        let mut cfg = SecurityCfg::default();
        cfg.rare_per_sec = 0;
        let mut g = ConnGuard::default();
        let now = Instant::now();
        for t in [
            crate::protocol::c2s::HELLO,
            crate::protocol::c2s::SPEND_ATTRIBUTE,
            crate::protocol::c2s::GROUP_INVITE,
            crate::protocol::c2s::AUCTION_BUY,
        ] {
            assert_eq!(
                gate_frame(&cfg, &mut g, t, true, now),
                GateDecision::Drop,
                "Typ {t} muss bei Limit 0 verworfen werden"
            );
        }
        assert_eq!(
            g.violations, 4,
            "das gesperrte Budget erzeugt weiterhin Auffälligkeiten"
        );
    }

    /// L-4 (Bestand): `disconnect_after_violations = 0` wirkt über `.max(1)` als
    /// **1** — die erste Auffälligkeit trennt. Geprüft wird über den Rate-Pfad,
    /// weil **erlaubte** Nachrichten den Zähler nicht verändern (erst ein
    /// tatsächlich abgewiesener Versuch erzeugt eine Auffälligkeit).
    #[test]
    fn zero_disconnect_threshold_acts_as_one() {
        let mut cfg = SecurityCfg::default();
        cfg.disconnect_after_violations = 0;
        cfg.rare_per_sec = 1;
        let mut g = ConnGuard::default();
        let now = Instant::now();
        assert_eq!(
            gate_frame(&cfg, &mut g, crate::protocol::c2s::SPEND_ATTRIBUTE, true, now),
            GateDecision::Allow
        );
        assert_eq!(
            gate_frame(&cfg, &mut g, crate::protocol::c2s::SPEND_ATTRIBUTE, true, now),
            GateDecision::Disconnect,
            "0 wird über max(1) als Schwelle 1 wirksam"
        );
    }

    // 8b) Übergröße wird vor dem Parsen erkannt.
    //
    // Geltungsgrenze: dieser Test belegt **nur die Grenzfunktion**
    // `frame_too_large`. Die tatsächliche Anordnung „Größe vor Parse" im
    // Empfangspfad ist eine Eigenschaft des Aufrufers (`read_loop` in
    // `net.rs`, Größenprüfung `:582` mit `continue` vor dem Parse `:592`)
    // und wird dort belegt: `read_loop_drops_oversize_text_before_handler_
    // effect` (net.rs). Ebenso ist die test-eigene `pipeline()` unten eine
    // Modellierung, nicht der Produktionspfad.
    #[test]
    fn oversize_frames_rejected_before_parse() {
        let cfg = SecurityCfg::default();
        assert!(frame_too_large(&cfg, cfg.max_frame_bytes + 1));
        assert!(!frame_too_large(&cfg, cfg.max_frame_bytes));
    }

    // 9) AUTH-05: Die Ablehnungszeile enthält die vollständige Session-ID
    //    NICHT (auch nicht als Teilstring) — nur Charakter und Serverzustand.
    #[test]
    fn reject_log_line_contains_no_session_id() {
        let p = test_player(); // session_id == "sess-1"
        let info = RejectInfo {
            reason: "rate_limited".into(),
            msg_type: crate::protocol::c2s::MOVE,
            detail: String::new(),
        };
        let line = reject_log_line(Some(&p), 7, &info, 3);
        assert!(line.contains("sec-reject conn=7"), "{line}");
        assert!(line.contains("char=hero"), "{line}");
        assert!(!line.contains("sess-1"), "Session-ID im Log: {line}");
        assert!(!line.contains("session="), "Session-Feld im Log: {line}");
        // Auch ohne Player (unauthentifiziert) wird nichts ausgegeben.
        let line = reject_log_line(None, 8, &info, 1);
        assert!(line.contains("char=-"), "{line}");
        assert!(!line.to_lowercase().contains("session"), "{line}");
    }

    // 10) AUTH-03: Die Takeover-Zeile führt genau die zulässigen Felder und
    //     keine Zugangsdaten; eine Roh-IP wird nicht ausgegeben.
    #[test]
    fn takeover_log_line_has_only_allowed_fields() {
        let line = takeover_log_line(7, "hero", 11, 12);
        assert!(
            line.starts_with("authenticated_connection_takeover "),
            "{line}"
        );
        assert!(line.contains("account_id=7"), "{line}");
        assert!(line.contains("char_id=hero"), "{line}");
        assert!(line.contains("old_conn_id=11"), "{line}");
        assert!(line.contains("new_conn_id=12"), "{line}");
        assert!(line.contains("old_state=authenticated"), "{line}");
        for forbidden in ["session", "token", "password", "ip=", "peer"] {
            assert!(
                !line.contains(forbidden),
                "unzulässiges Feld {forbidden}: {line}"
            );
        }
    }

    // Sequenz: Duplikat/Lag ist kein Cheat — nur Vermerk, keine Ablehnung.
    #[test]
    fn seq_duplicates_are_tolerated() {
        let mut g = ConnGuard::default();
        assert!(g.note_seq(10));
        assert!(!g.note_seq(10));
        assert!(!g.note_seq(5));
        assert!(g.note_seq(11));
    }

    // ── Handler-Ebene (Serverautorität gegen manipulierte Clients) ──────

    async fn world_with_duel() -> crate::world::Shared {
        let shared = crate::world::new_shared();
        let mut w = shared.lock().await;
        let (ta, _) = tokio::sync::mpsc::unbounded_channel();
        let (tb, _) = tokio::sync::mpsc::unbounded_channel();
        for (id, tx, x) in [("hero", ta, 0.0), ("boss", tb, 1.0)] {
            let mut p = test_player();
            p.id = id.into();
            p.name = id.into();
            p.tx = tx;
            p.x = x;
            p.hp = if id == "hero" { 3567 } else { 5000 };
            p.max_hp = 5000;
            w.players.insert(id.into(), p);
        }
        w.by_conn.insert(7, "hero".into());
        w.by_conn.insert(8, "boss".into());
        drop(w);
        shared
    }

    fn combat_cfg() -> crate::config::CombatCfg {
        crate::config::CombatCfg {
            weapon_skill_id: "schwerter".into(),
            weapon_damage: 10,
            weapon_duration_ms: 2000,
            weapon_range: 2.0,
            hit_miss_permille: 0,
            hit_dodge_permille: 0,
            hit_parry_permille: 0,
            hit_block_permille: 0,
            hit_crit_permille: 0,
            hit_crit_mult_percent: 150,
            hit_block_reduce_percent: 50,
            armor_pct_per_point: 0,
            armor_cap_tank: 50,
            armor_cap_mage: 20,
            armor_cap_default: 30,
            skill_hit_bonus_permille: 0,
        }
    }

    fn npc_cfg() -> crate::config::NpcCfg {
        crate::config::NpcCfg {
            social_aggro_radius: 15.0,
            no_link_ms: 5000,
            return_speed: 5.0,
            persist_interval_ms: 30000,
        }
    }

    // 5) Lokal manipuliertes HP (140000 statt 3567) ist bedeutungslos:
    //    MOVE/ATTACK lesen kein HP-Feld; der Serverstand bleibt 3567.
    //    Ein mitgesendetes damage=50000 wird ebenfalls ignoriert (der Server
    //    würfelt den Schaden aus Config + Attributen).
    #[tokio::test]
    async fn manipulated_hp_and_damage_are_ignored() {
        let shared = world_with_duel().await;
        crate::handlers::handle_move(
            &shared,
            7,
            &serde_json::json!({"dir": [1.0, 0.0], "hp": 140000, "max_hp": 140000}),
            100,
        )
        .await;
        crate::handlers::handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "boss", "damage": 50000, "hp": 140000}),
            &combat_cfg(),
            &npc_cfg(),
        )
        .await;
        let w = shared.lock().await;
        assert_eq!(w.players["hero"].hp, 3567);
        assert_eq!(w.players["boss"].hp, 5000);
        // Angriff wurde bewaffnet (gültiges Ziel in Reichweite), aber mit
        // Server-Schaden — das mitgesendete damage=50000 existiert nirgends.
        assert_eq!(w.players["hero"].combat.as_ref().unwrap().target_id, "boss");
    }

    // 6) Serverseitig toter Charakter (hp<=0): weitere Kampfaktionen werden
    //    verworfen und verändern nichts — der Rest des Raids spielt weiter.
    #[tokio::test]
    async fn dead_character_attacks_are_dropped() {
        let shared = world_with_duel().await;
        {
            let mut w = shared.lock().await;
            w.players.get_mut("hero").unwrap().hp = 0;
        }
        crate::handlers::handle_attack(
            &shared,
            7,
            &serde_json::json!({"target_id": "boss"}),
            &combat_cfg(),
            &npc_cfg(),
        )
        .await;
        let w = shared.lock().await;
        assert!(w.players["hero"].combat.is_none());
        assert_eq!(w.players["boss"].hp, 5000);
        drop(w);
        // Fähigkeiten: toter Caster wird abgelehnt (kein Cast, kein Mana).
        let shared2 = world_with_duel().await;
        {
            let mut w = shared2.lock().await;
            w.players.get_mut("hero").unwrap().hp = 0;
            let r = crate::combat::targeting::validate_caster_status(&w, "hero");
            assert!(!r.valid);
        }
    }

    // 8) Ungültige Requests erreichen keine teuren Systeme: Die Pipeline
    //    (Größe → Format → Typ → Session → Rate) läuft vor jeder Logik.
    //    `expensive_calls` steht für DB/Kampf/Inventar/Welt/KI.
    //
    // **Diese Funktion ist eine Modellierung, kein Produktionspfad.** Sie
    // bildet die Aufrufer-Stufen (Größe/Parse/Whitelist) vereinfacht nach und
    // ruft nur `gate_frame` real auf; sie enthält bewusst **keinen** echten
    // JSON-Parse und keine echte Sequenzbeobachtung. Der Nachweis am
    // tatsächlichen Empfangs- und Dispatch-Pfad steht in `net.rs`
    // (z. B. `read_loop_drops_oversize_text_before_handler_effect`).
    fn pipeline(
        cfg: &SecurityCfg,
        guard: &mut ConnGuard,
        bytes: usize,
        known: bool,
        authenticated: bool,
        msg_type: i64,
        expensive_calls: &mut u32,
    ) -> GateDecision {
        if frame_too_large(cfg, bytes) {
            return GateDecision::Drop;
        }
        if !known {
            return GateDecision::Drop;
        }
        let d = gate_frame(cfg, guard, msg_type, authenticated, Instant::now());
        if d == GateDecision::Allow {
            *expensive_calls += 1;
        }
        d
    }

    #[test]
    fn invalid_requests_never_reach_expensive_systems() {
        let cfg = SecurityCfg::default();
        let mut expensive_calls = 0u32;
        let mut g = ConnGuard::default();
        // Übergröße → kein Parsen, keine Logik.
        assert_eq!(
            pipeline(&cfg, &mut g, cfg.max_frame_bytes + 1, true, true, 2, &mut expensive_calls),
            GateDecision::Drop
        );
        // Unbekannter Typ → keine Logik.
        assert_eq!(
            pipeline(&cfg, &mut g, 10, false, true, 999, &mut expensive_calls),
            GateDecision::Drop
        );
        // Keine Session → keine Logik.
        assert_eq!(
            pipeline(
                &cfg,
                &mut g,
                10,
                true,
                false,
                crate::protocol::c2s::ATTACK,
                &mut expensive_calls
            ),
            GateDecision::Drop
        );
        assert_eq!(expensive_calls, 0);
        // Gültiger Request → genau ein Logikaufruf.
        assert_eq!(
            pipeline(
                &cfg,
                &mut g,
                10,
                true,
                true,
                crate::protocol::c2s::MOVE,
                &mut expensive_calls
            ),
            GateDecision::Allow
        );
        assert_eq!(expensive_calls, 1);
    }
}
