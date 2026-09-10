// group — Gruppensystem V1 (docs/Gruppensystem.md §§1–9).
// Laufzeit-only, kein DB-Persistenz für Gruppenzustand.
// Claim-Encoder: Spieler → rohe ID, Gruppe → "g:<id>".
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct GroupCfg {
    pub max_members: u32,
    pub range: f64,
    pub reconnect_ms: u64,
}

impl Default for GroupCfg {
    fn default() -> Self {
        Self {
            max_members: 4,
            range: 100.0,
            reconnect_ms: 300_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroupMember {
    pub player_id: String,
    pub joined_at: Instant,
    pub online: bool,
    pub disconnected_at: Option<Instant>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Modell-Metadaten (Gruppensystem.md §2); Auswertung folgt
pub struct InviteRecord {
    pub invited_by: String,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
pub struct Proposal {
    pub proposer: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub id: u64,
    pub leader_id: String,
    pub members: BTreeMap<String, GroupMember>,
    pub pending_invites: BTreeMap<String, InviteRecord>,
    pub proposals: Vec<Proposal>,
    /// Ursprünglicher Leiter bei temporärer Leitung (Disconnect §9).
    /// Some = Leiter war temporarily abgetreten, Reconnect innerhalb Frist
    /// gibt die Leitung automatisch zurück.
    pub original_leader: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupError {
    NotInGroup,
    AlreadyInGroup,
    TargetAlreadyInGroup,
    TargetAlreadyInvited,
    GroupFull,
    NotLeader,
    TargetNotInGroup,
    ProposalNotFound,
    CannotSelfAction,
    GroupDoesNotExist,
    NoGroup,
    LeaderCannotLeave,
}

impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInGroup => write!(f, "nicht in einer Gruppe"),
            Self::AlreadyInGroup => write!(f, "Bereits in einer Gruppe"),
            Self::TargetAlreadyInGroup => write!(f, "Ziel ist bereits in einer Gruppe"),
            Self::TargetAlreadyInvited => write!(f, "Ziel bereits eingeladen"),
            Self::GroupFull => write!(f, "Gruppe ist voll"),
            Self::NotLeader => write!(f, "Kein Gruppenleiter"),
            Self::TargetNotInGroup => write!(f, "Ziel nicht in der Gruppe"),
            Self::ProposalNotFound => write!(f, "Vorschlag nicht gefunden"),
            Self::CannotSelfAction => write!(f, "Aktion auf sich selbst nicht möglich"),
            Self::GroupDoesNotExist => write!(f, "Gruppe existiert nicht"),
            Self::NoGroup => write!(f, "Kein Gruppenzustand"),
            Self::LeaderCannotLeave => write!(f, "Leiter muss erst Leitung übertragen"),
        }
    }
}

// ── Claim-Encoder ──────────────────────────────────────────────────────
pub fn encode_group_claim(group_id: u64) -> String {
    format!("g:{group_id}")
}

pub fn is_group_claim(claim: &str) -> bool {
    claim.starts_with("g:")
}

pub fn decode_group_claim(claim: &str) -> Option<u64> {
    claim.strip_prefix("g:").and_then(|s| s.parse().ok())
}

/// Deterministische gleichmäßige EXP-Aufteilung (§7): Rest geht an die
/// ersten Empfänger in stabiler (BTreeMap-)Reihenfolge — kein langfristiger
/// Verlust. count=0 → leere Liste.
pub fn split_exp_equally(exp: i64, count: usize) -> Vec<i64> {
    if count == 0 {
        return Vec::new();
    }
    let exp = exp.max(0);
    let base = exp / count as i64;
    let rem = (exp % count as i64) as usize;
    (0..count)
        .map(|i| base + if i < rem { 1 } else { 0 })
        .collect()
}

// ── GroupManager ───────────────────────────────────────────────────────
pub struct GroupManager {
    groups: BTreeMap<u64, Group>,
    player_to_group: HashMap<String, u64>,
    next_id: u64,
    pub cfg: GroupCfg,
    /// Bei Auflösung (leave/kick/Reconnect-Frist) erfasste Gruppe samt
    /// letztem Mitglied — Claim-Übergang (§8), vom Tick abgeholt.
    pending_dissolutions: Vec<(u64, String)>,
}

impl GroupManager {
    pub fn new(cfg: GroupCfg) -> Self {
        Self {
            groups: BTreeMap::new(),
            player_to_group: HashMap::new(),
            next_id: 1,
            cfg,
            pending_dissolutions: Vec::new(),
        }
    }

    pub fn group_of(&self, player_id: &str) -> Option<u64> {
        self.player_to_group.get(player_id).copied()
    }

    pub fn get_group(&self, group_id: u64) -> Option<&Group> {
        self.groups.get(&group_id)
    }

    pub fn member_ids(&self, group_id: u64) -> Vec<String> {
        self.groups
            .get(&group_id)
            .map(|g| g.members.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn is_leader(&self, player_id: &str) -> bool {
        self.player_to_group
            .get(player_id)
            .and_then(|gid| self.groups.get(gid))
            .is_some_and(|g| g.leader_id == player_id)
    }

    /// Aktive Mitglieder = online + innerhalb Reichweite des Leiters.
    /// Akzeptiert eine Map von Spielerpositionen und die Leiterposition.
    pub fn active_members(
        &self,
        group_id: u64,
        player_positions: &HashMap<String, (f64, f64)>,
        range: f64,
    ) -> Vec<String> {
        let Some(group) = self.groups.get(&group_id) else {
            return Vec::new();
        };
        let leader_pos = group
            .members
            .get(&group.leader_id)
            .and_then(|m| player_positions.get(&m.player_id));
        let Some((lx, ly)) = leader_pos else {
            return Vec::new();
        };
        group
            .members
            .values()
            .filter(|m| m.online)
            .filter(|m| {
                player_positions
                    .get(&m.player_id)
                    .is_some_and(|(px, py)| dist(*px, *py, *lx, *ly) <= range)
            })
            .map(|m| m.player_id.clone())
            .collect()
    }

    // ── Mutationen ─────────────────────────────────────────────────────

    /// §1/§2: Neue Gruppe mit dem Ersteller als Leiter.
    pub fn create_group(&mut self, player_id: &str, now: Instant) -> Result<u64, GroupError> {
        if self.player_to_group.contains_key(player_id) {
            return Err(GroupError::AlreadyInGroup);
        }
        let gid = self.next_id;
        self.next_id += 1;
        let mut members = BTreeMap::new();
        members.insert(
            player_id.to_string(),
            GroupMember {
                player_id: player_id.to_string(),
                joined_at: now,
                online: true,
                disconnected_at: None,
            },
        );
        let group = Group {
            id: gid,
            leader_id: player_id.to_string(),
            members,
            pending_invites: BTreeMap::new(),
            proposals: Vec::new(),
            original_leader: None,
        };
        self.groups.insert(gid, group);
        self.player_to_group.insert(player_id.to_string(), gid);
        Ok(gid)
    }

    /// §2: Leiter lädt Spieler ein.
    pub fn invite(
        &mut self,
        group_id: u64,
        leader_id: &str,
        target_id: &str,
        now: Instant,
    ) -> Result<(), GroupError> {
        let group = self.groups.get(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        if group.leader_id != leader_id {
            return Err(GroupError::NotLeader);
        }
        if leader_id == target_id {
            return Err(GroupError::CannotSelfAction);
        }
        if self.player_to_group.contains_key(target_id) {
            return Err(GroupError::TargetAlreadyInGroup);
        }
        if group.pending_invites.contains_key(target_id) {
            return Err(GroupError::TargetAlreadyInvited);
        }
        if group.members.len() as u32 >= self.cfg.max_members {
            return Err(GroupError::GroupFull);
        }
        let group = self.groups.get_mut(&group_id).unwrap();
        group.pending_invites.insert(
            target_id.to_string(),
            InviteRecord {
                invited_by: leader_id.to_string(),
                created_at: now,
            },
        );
        Ok(())
    }

    /// §2: Spieler nimmt Einladung an.
    pub fn accept_invite(
        &mut self,
        group_id: u64,
        player_id: &str,
        now: Instant,
    ) -> Result<(), GroupError> {
        if self.player_to_group.contains_key(player_id) {
            return Err(GroupError::AlreadyInGroup);
        }
        let group = self.groups.get(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        if !group.pending_invites.contains_key(player_id) {
            return Err(GroupError::NoGroup);
        }
        if group.members.len() as u32 >= self.cfg.max_members {
            return Err(GroupError::GroupFull);
        }
        let group = self.groups.get_mut(&group_id).unwrap();
        group.pending_invites.remove(player_id);
        group.members.insert(
            player_id.to_string(),
            GroupMember {
                player_id: player_id.to_string(),
                joined_at: now,
                online: true,
                disconnected_at: None,
            },
        );
        self.player_to_group.insert(player_id.to_string(), group_id);
        Ok(())
    }

    /// §2: Spieler lehnt Einladung ab.
    pub fn reject_invite(
        &mut self,
        group_id: u64,
        player_id: &str,
    ) -> Result<(), GroupError> {
        let group = self.groups.get_mut(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        group.pending_invites.remove(player_id);
        Ok(())
    }

    /// §3: Mitglied schlägt anderen Spieler vor.
    pub fn propose(
        &mut self,
        group_id: u64,
        proposer_id: &str,
        target_id: &str,
    ) -> Result<(), GroupError> {
        let group = self.groups.get(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        if !group.members.contains_key(proposer_id) {
            return Err(GroupError::NotInGroup);
        }
        if proposer_id == target_id {
            return Err(GroupError::CannotSelfAction);
        }
        if self.player_to_group.contains_key(target_id) {
            return Err(GroupError::TargetAlreadyInGroup);
        }
        if group.members.len() as u32 >= self.cfg.max_members {
            return Err(GroupError::GroupFull);
        }
        let dominated = group
            .proposals
            .iter()
            .any(|p| p.target == target_id && p.proposer == proposer_id);
        if dominated {
            return Ok(());
        }
        let group = self.groups.get_mut(&group_id).unwrap();
        group.proposals.push(Proposal {
            proposer: proposer_id.to_string(),
            target: target_id.to_string(),
        });
        Ok(())
    }

    /// §3: Leiter nimmt Vorschlag an → erzeugt Einladung.
    pub fn approve_proposal(
        &mut self,
        group_id: u64,
        leader_id: &str,
        target_id: &str,
        now: Instant,
    ) -> Result<(), GroupError> {
        let group = self.groups.get(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        if group.leader_id != leader_id {
            return Err(GroupError::NotLeader);
        }
        let idx = group
            .proposals
            .iter()
            .position(|p| p.target == target_id)
            .ok_or(GroupError::ProposalNotFound)?;
        let group = self.groups.get_mut(&group_id).unwrap();
        group.proposals.remove(idx);
        if group.pending_invites.contains_key(target_id) {
            return Err(GroupError::TargetAlreadyInvited);
        }
        group.pending_invites.insert(
            target_id.to_string(),
            InviteRecord {
                invited_by: leader_id.to_string(),
                created_at: now,
            },
        );
        Ok(())
    }

    /// §3: Leiter lehnt Vorschlag ab.
    pub fn reject_proposal(
        &mut self,
        group_id: u64,
        leader_id: &str,
        target_id: &str,
    ) -> Result<(), GroupError> {
        let group = self.groups.get(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        if group.leader_id != leader_id {
            return Err(GroupError::NotLeader);
        }
        let group = self.groups.get_mut(&group_id).unwrap();
        let dominated = group
            .proposals
            .iter()
            .position(|p| p.target == target_id);
        if let Some(idx) = dominated {
            group.proposals.remove(idx);
        }
        Ok(())
    }

    /// §2: Spieler verlässt Gruppe (freiwillig).
    /// Leiter darf nicht einfach gehen — muss erst übertragen.
    pub fn leave(
        &mut self,
        group_id: u64,
        player_id: &str,
        _now: Instant,
    ) -> Result<(), GroupError> {
        let group = self.groups.get(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        if !group.members.contains_key(player_id) {
            return Err(GroupError::NotInGroup);
        }
        let is_leader = group.leader_id == player_id;
        if is_leader {
            if group.members.len() == 1 {
                self.dissolve(group_id);
                return Ok(());
            }
            return Err(GroupError::LeaderCannotLeave);
        }
        self.remove_player(group_id, player_id);
        Ok(())
    }

    /// §2: Leiter entfernt Mitglied.
    pub fn kick(
        &mut self,
        group_id: u64,
        leader_id: &str,
        target_id: &str,
        _now: Instant,
    ) -> Result<(), GroupError> {
        let group = self.groups.get(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        if group.leader_id != leader_id {
            return Err(GroupError::NotLeader);
        }
        if leader_id == target_id {
            return Err(GroupError::CannotSelfAction);
        }
        if !group.members.contains_key(target_id) {
            return Err(GroupError::TargetNotInGroup);
        }
        if group.members.len() == 1 {
            self.dissolve(group_id);
            return Ok(());
        }
        self.remove_player(group_id, target_id);
        Ok(())
    }

    /// §2: Leiter überträgt Leitung freiwillig.
    pub fn transfer_leader(
        &mut self,
        group_id: u64,
        leader_id: &str,
        new_leader_id: &str,
    ) -> Result<(), GroupError> {
        let group = self.groups.get(&group_id).ok_or(GroupError::GroupDoesNotExist)?;
        if group.leader_id != leader_id {
            return Err(GroupError::NotLeader);
        }
        if !group.members.contains_key(new_leader_id) {
            return Err(GroupError::TargetNotInGroup);
        }
        if leader_id == new_leader_id {
            return Err(GroupError::CannotSelfAction);
        }
        let group = self.groups.get_mut(&group_id).unwrap();
        // Temporäre Leitung (Disconnect §9): nur der EIGENTLICHE Leiter
        // gibt das Rückforderungsrecht auf; ein zwischenzeitlicher Leiter
        // überschreibt es nicht.
        let is_temp_leader = group
            .original_leader
            .as_ref()
            .is_some_and(|o| o != leader_id);
        if !is_temp_leader {
            group.original_leader = None;
        }
        group.leader_id = new_leader_id.to_string();
        Ok(())
    }

    // ── Disconnect / Reconnect (§9) ────────────────────────────────────

    /// §9: Spieler verliert Verbindung.
    pub fn on_disconnect(&mut self, player_id: &str, now: Instant) {
        let Some(gid) = self.player_to_group.get(player_id).copied() else {
            return;
        };
        let was_leader = {
            let Some(group) = self.groups.get_mut(&gid) else {
                return;
            };
            if let Some(member) = group.members.get_mut(player_id) {
                member.online = false;
                member.disconnected_at = Some(now);
            }
            let was_leader = group.leader_id == player_id;
            if was_leader {
                // Nur der ERSTE (ursprüngliche) Leiter wird als
                // original_leader geführt; der Disconnect eines
                // zwischenzeitlichen Leiters überschreibt das
                // Rückforderungsrecht nicht (§9, mehrere Disconnects).
                if group.original_leader.is_none() {
                    group.original_leader = Some(player_id.to_string());
                }
            }
            was_leader
        };
        if was_leader {
            if let Some(new_leader) = self.find_longest_standing_online(gid, Some(player_id)) {
                if let Some(group) = self.groups.get_mut(&gid) {
                    group.leader_id = new_leader;
                }
            }
        }
    }

    /// §9: Spieler reconnectet. Gibt true zurück, wenn erfolgreich.
    pub fn on_reconnect(&mut self, player_id: &str, _now: Instant) -> bool {
        let Some(gid) = self.player_to_group.get(player_id).copied() else {
            return false;
        };
        let Some(group) = self.groups.get_mut(&gid) else {
            return false;
        };
        let was_original_leader = group.original_leader.as_deref() == Some(player_id);
        if let Some(member) = group.members.get_mut(player_id) {
            member.online = true;
            member.disconnected_at = None;
        }
        if was_original_leader {
            group.original_leader = None;
            group.leader_id = player_id.to_string();
        }
        true
    }

    /// §9: Periodischer Tick — läuft abgelaufene Reconnect-Fristen ab.
    pub fn tick(&mut self, now: Instant) -> Vec<String> {
        let mut expired = Vec::new();
        let grace = Duration::from_millis(self.cfg.reconnect_ms);
        let group_ids: Vec<u64> = self.groups.keys().copied().collect();
        for gid in group_ids {
            let disconnecteds: Vec<String> = self
                .groups
                .get(&gid)
                .map(|g| {
                    g.members
                        .values()
                        .filter(|m| {
                            !m.online
                                && m.disconnected_at
                                    .is_some_and(|d| now.duration_since(d) >= grace)
                        })
                        .map(|m| m.player_id.clone())
                        .collect()
                })
                .unwrap_or_default();
            for pid in disconnecteds {
                self.remove_player(gid, &pid);
                expired.push(pid);
            }
        }
        expired
    }

    // ── Dissolution-Handling ───────────────────────────────────────────

    /// Gibt alle seit dem letzten Tick aufgelösten Gruppen samt letztem
    /// Mitglied zurück (Claim-Übergang §8). `last_member` ist leer, wenn
    /// bei der Auflösung kein Spieler mehr übrig war.
    pub fn take_dissolutions(&mut self) -> Vec<(u64, String)> {
        std::mem::take(&mut self.pending_dissolutions)
    }

    // ── Hilfsfunktionen ────────────────────────────────────────────────

    fn find_longest_standing_online(&self, group_id: u64, exclude: Option<&str>) -> Option<String> {
        let group = self.groups.get(&group_id)?;
        let mut candidates: Vec<_> = group
            .members
            .values()
            .filter(|m| m.online)
            .filter(|m| Some(m.player_id.as_str()) != exclude)
            .collect();
        candidates.sort_by_key(|m| m.joined_at);
        candidates.first().map(|m| m.player_id.clone())
    }

    fn remove_player(&mut self, group_id: u64, player_id: &str) {
        let dominated = self.groups.get(&group_id).map(|g| g.members.len());
        let dominated = dominated.unwrap_or(0);
        if let Some(group) = self.groups.get_mut(&group_id) {
            group.members.remove(player_id);
            group.pending_invites.remove(player_id);
            group.proposals.retain(|p| p.proposer != player_id && p.target != player_id);
        }
        self.player_to_group.remove(player_id);
        if dominated <= 2 {
            self.maybe_dissolve(group_id);
        } else {
            self.maybe_transfer_on_removal(group_id, player_id);
        }
    }

    fn maybe_dissolve(&mut self, group_id: u64) {
        let dominated = self
            .groups
            .get(&group_id)
            .map(|g| g.members.len())
            .unwrap_or(0);
        if dominated <= 1 {
            self.dissolve(group_id);
        }
    }

    fn maybe_transfer_on_removal(&mut self, group_id: u64, removed_id: &str) {
        let leader_removed = self
            .groups
            .get(&group_id)
            .is_some_and(|g| g.leader_id == removed_id);
        if !leader_removed {
            return;
        }
        if let Some(new_leader) = self.find_longest_standing_online(group_id, None) {
            if let Some(group) = self.groups.get_mut(&group_id) {
                group.leader_id = new_leader;
                group.original_leader = None;
            }
        }
    }

    fn dissolve(&mut self, group_id: u64) {
        if let Some(group) = self.groups.remove(&group_id) {
            let last_member = group.members.keys().next().cloned().unwrap_or_default();
            for pid in group.members.keys() {
                self.player_to_group.remove(pid);
            }
            self.pending_dissolutions.push((group_id, last_member));
        }
    }
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).hypot(ay - by)
}

pub type SharedGroups = std::sync::Arc<tokio::sync::Mutex<GroupManager>>;

pub fn new_shared_groups(cfg: GroupCfg) -> SharedGroups {
    std::sync::Arc::new(tokio::sync::Mutex::new(GroupManager::new(cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    fn cfg() -> GroupCfg {
        GroupCfg {
            max_members: 4,
            range: 100.0,
            reconnect_ms: 300_000,
        }
    }

    #[test]
    fn create_group_and_leader() {
        let mut gm = GroupManager::new(cfg());
        let gid = gm.create_group("a", now()).unwrap();
        assert_eq!(gid, 1);
        assert!(gm.is_leader("a"));
        assert_eq!(gm.group_of("a"), Some(1));
    }

    #[test]
    fn cannot_create_two_groups() {
        let mut gm = GroupManager::new(cfg());
        gm.create_group("a", now()).unwrap();
        assert_eq!(gm.create_group("a", now()), Err(GroupError::AlreadyInGroup));
    }

    #[test]
    fn invite_and_accept() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        assert_eq!(gm.group_of("b"), Some(gid));
        assert_eq!(gm.member_ids(gid).len(), 2);
    }

    #[test]
    fn invite_reject() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.reject_invite(gid, "b").unwrap();
        assert_eq!(gm.group_of("b"), None);
    }

    #[test]
    fn cannot_invite_self() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        assert_eq!(
            gm.invite(gid, "a", "a", t),
            Err(GroupError::CannotSelfAction)
        );
    }

    #[test]
    fn cannot_invite_twice() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        assert_eq!(
            gm.invite(gid, "a", "b", t),
            Err(GroupError::TargetAlreadyInvited)
        );
    }

    #[test]
    fn non_leader_cannot_invite() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        assert_eq!(gm.invite(gid, "b", "c", t), Err(GroupError::NotLeader));
    }

    #[test]
    fn group_full_rejects_invite() {
        let mut gm = GroupManager::new(GroupCfg {
            max_members: 2,
            ..cfg()
        });
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        assert_eq!(
            gm.invite(gid, "a", "c", t),
            Err(GroupError::GroupFull)
        );
    }

    #[test]
    fn target_in_other_group_rejected() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let g1 = gm.create_group("a", t).unwrap();
        let g2 = gm.create_group("b", t).unwrap();
        gm.invite(g1, "a", "c", t).unwrap();
        gm.accept_invite(g1, "c", t).unwrap();
        assert_eq!(
            gm.invite(g2, "b", "c", t),
            Err(GroupError::TargetAlreadyInGroup)
        );
    }

    #[test]
    fn propose_and_approve() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.propose(gid, "b", "c").unwrap();
        gm.approve_proposal(gid, "a", "c", t).unwrap();
        assert!(gm.groups.get(&gid).unwrap().pending_invites.contains_key("c"));
    }

    #[test]
    fn propose_reject() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.propose(gid, "b", "c").unwrap();
        gm.reject_proposal(gid, "a", "c").unwrap();
        assert!(gm.groups.get(&gid).unwrap().proposals.is_empty());
    }

    #[test]
    fn non_leader_cannot_approve_proposal() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.propose(gid, "b", "c").unwrap();
        assert_eq!(
            gm.approve_proposal(gid, "b", "c", t),
            Err(GroupError::NotLeader)
        );
    }

    #[test]
    fn leave_normal_member() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.leave(gid, "b", t).unwrap();
        assert_eq!(gm.group_of("b"), None);
        // Nur noch 1 Mitglied → Gruppe löst sich auf (§2).
        assert_eq!(gm.group_of("a"), None);
        assert!(gm.groups.is_empty());
    }

    #[test]
    fn leader_cannot_leave_without_transfer() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        assert_eq!(gm.leave(gid, "a", t), Err(GroupError::LeaderCannotLeave));
    }

    #[test]
    fn last_member_leaves_dissolves() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.leave(gid, "a", t).unwrap();
        assert!(gm.groups.is_empty());
    }

    #[test]
    fn kick_member() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.kick(gid, "a", "b", t).unwrap();
        assert_eq!(gm.group_of("b"), None);
    }

    #[test]
    fn non_leader_cannot_kick() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        assert_eq!(gm.kick(gid, "b", "a", t), Err(GroupError::NotLeader));
    }

    #[test]
    fn transfer_leader() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.transfer_leader(gid, "a", "b").unwrap();
        assert!(gm.is_leader("b"));
        assert!(!gm.is_leader("a"));
    }

    #[test]
    fn non_leader_cannot_transfer() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        assert_eq!(
            gm.transfer_leader(gid, "b", "a"),
            Err(GroupError::NotLeader)
        );
    }

    #[test]
    fn disconnect_marks_offline_and_transfers_leadership() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.on_disconnect("a", t);
        assert!(gm.is_leader("b"));
        let group = gm.get_group(gid).unwrap();
        assert_eq!(group.original_leader.as_deref(), Some("a"));
        assert!(!group.members["a"].online);
    }

    #[test]
    fn reconnect_restores_leadership() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.on_disconnect("a", t);
        gm.on_reconnect("a", t + Duration::from_secs(10));
        assert!(gm.is_leader("a"));
        let group = gm.get_group(gid).unwrap();
        assert!(group.original_leader.is_none());
    }

    #[test]
    fn tick_expires_disconnected_players() {
        let mut gm = GroupManager::new(GroupCfg {
            reconnect_ms: 1000,
            ..cfg()
        });
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.on_disconnect("b", t);
        // Noch nicht abgelaufen.
        let expired = gm.tick(t + Duration::from_millis(500));
        assert!(expired.is_empty());
        // Abgelaufen.
        let expired = gm.tick(t + Duration::from_millis(1500));
        assert_eq!(expired, vec!["b".to_string()]);
        // Nur noch 1 Mitglied → Gruppe löst sich auf (§2).
        assert!(gm.groups.is_empty());
        assert_eq!(gm.group_of("a"), None);
    }

    #[test]
    fn disconnect_leader_three_members() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.invite(gid, "a", "c", t).unwrap();
        gm.accept_invite(gid, "c", t).unwrap();
        gm.on_disconnect("a", t);
        // a war leader → b (ältestes online) wird neuer Leiter.
        assert!(gm.is_leader("b"));
        let group = gm.get_group(gid).unwrap();
        assert_eq!(group.original_leader.as_deref(), Some("a"));
    }

#[test]
    fn claim_encoder_roundtrip() {
        let g = encode_group_claim(42);
        assert!(is_group_claim(&g));
        assert_eq!(decode_group_claim(&g), Some(42));
        assert!(!is_group_claim("42"));
        assert_eq!(decode_group_claim("42"), None);
        assert_eq!(decode_group_claim("g:x"), None);
    }

    #[test]
    fn split_exp_equally_even_and_remainder() {
        assert_eq!(split_exp_equally(100, 1), vec![100]);
        assert_eq!(split_exp_equally(100, 2), vec![50, 50]);
        assert_eq!(split_exp_equally(100, 4), vec![25, 25, 25, 25]);
        // Rest geht deterministisch an die ersten Empfänger.
        assert_eq!(split_exp_equally(100, 3), vec![34, 33, 33]);
        assert_eq!(split_exp_equally(7, 3), vec![3, 2, 2]);
        assert_eq!(split_exp_equally(2, 3), vec![1, 1, 0]);
        assert_eq!(split_exp_equally(100, 0), Vec::<i64>::new());
    }

    #[test]
    fn active_members_filter() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        let mut positions = HashMap::new();
        positions.insert("a".to_string(), (0.0, 0.0));
        positions.insert("b".to_string(), (5.0, 0.0));
        let active = gm.active_members(gid, &positions, 100.0);
        assert_eq!(active.len(), 2);
        positions.insert("b".to_string(), (200.0, 0.0));
        let active = gm.active_members(gid, &positions, 100.0);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0], "a");
    }

    #[test]
    fn fresh_solo_group_survives_take_dissolutions() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        // Noch nicht angenommen → Gruppe besteht nur aus dem Gründer.
        assert!(gm.take_dissolutions().is_empty());
        assert!(gm.get_group(gid).is_some());
        assert!(gm.is_leader("a"));
    }

    #[test]
    fn dissolution_records_last_member_for_claim() {
        let mut gm = GroupManager::new(cfg());
        let t = now();
        let gid = gm.create_group("a", t).unwrap();
        gm.invite(gid, "a", "b", t).unwrap();
        gm.accept_invite(gid, "b", t).unwrap();
        gm.invite(gid, "a", "c", t).unwrap();
        gm.accept_invite(gid, "c", t).unwrap();
        // c geht → len 2, keine Auflösung.
        gm.leave(gid, "c", t).unwrap();
        assert!(gm.get_group(gid).is_some());
        assert!(gm.take_dissolutions().is_empty());
        // b geht → nur noch a → Auflösung mit "a" als letztem Mitglied.
        gm.leave(gid, "b", t).unwrap();
        assert_eq!(gm.take_dissolutions(), vec![(gid, "a".to_string())]);
    }
}
