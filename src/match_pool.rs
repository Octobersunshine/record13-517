use crate::models::{Group, MatchResult, Player, QueuedPlayer, Rank, Team};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub const TEAM_SIZE: usize = 5;

pub const DEFAULT_QUEUE_TIMEOUT_SECONDS: u64 = 30 * 60;

pub struct MatchPool {
    pools: RwLock<HashMap<Rank, VecDeque<QueuedPlayer>>>,
    pub player_index: RwLock<HashMap<Uuid, Rank>>,
    groups: RwLock<HashMap<Uuid, Group>>,
    player_to_group: RwLock<HashMap<Uuid, Uuid>>,
    match_results: RwLock<HashMap<Uuid, MatchResult>>,
    queue_timeout_seconds: u64,
}

impl MatchPool {
    pub fn new() -> Self {
        Self::with_timeout(DEFAULT_QUEUE_TIMEOUT_SECONDS)
    }

    pub fn with_timeout(queue_timeout_seconds: u64) -> Self {
        let mut pools = HashMap::new();
        for rank in Rank::all_ranks() {
            pools.insert(rank, VecDeque::new());
        }

        Self {
            pools: RwLock::new(pools),
            player_index: RwLock::new(HashMap::new()),
            groups: RwLock::new(HashMap::new()),
            player_to_group: RwLock::new(HashMap::new()),
            match_results: RwLock::new(HashMap::new()),
            queue_timeout_seconds,
        }
    }

    pub fn queue_timeout_seconds(&self) -> u64 {
        self.queue_timeout_seconds
    }

    pub fn get_group(&self, group_id: &Uuid) -> Option<Group> {
        self.groups.read().unwrap().get(group_id).cloned()
    }

    pub fn get_player_group_id(&self, player_id: &Uuid) -> Option<Uuid> {
        self.player_to_group.read().unwrap().get(player_id).copied()
    }

    pub fn add_player(&self, player: Player) -> Result<(), String> {
        let player_id = player.id;
        let rank = player.rank;

        if self.player_index.read().unwrap().contains_key(&player_id) {
            return Err("玩家已在匹配队列中".to_string());
        }

        let queued = QueuedPlayer::new(player);
        self.pools
            .write()
            .unwrap()
            .get_mut(&rank)
            .unwrap()
            .push_back(queued);
        self.player_index.write().unwrap().insert(player_id, rank);

        Ok(())
    }

    pub fn add_group(&self, group: Group) -> Result<Uuid, String> {
        let group_id = group.id;
        let rank = group.rank;

        for member in &group.members {
            if self.player_index.read().unwrap().contains_key(&member.id) {
                return Err(format!("玩家 {} 已在匹配队列中", member.name));
            }
        }

        {
            let mut groups = self.groups.write().unwrap();
            let mut player_to_group = self.player_to_group.write().unwrap();
            let mut pools = self.pools.write().unwrap();
            let mut player_index = self.player_index.write().unwrap();
            let pool = pools.get_mut(&rank).unwrap();

            for member in &group.members {
                let queued = QueuedPlayer::with_group(member.clone(), group_id);
                pool.push_back(queued);
                player_index.insert(member.id, rank);
                player_to_group.insert(member.id, group_id);
            }

            groups.insert(group_id, group);
        }

        Ok(group_id)
    }

    pub fn remove_player(&self, player_id: &Uuid) -> Result<(), String> {
        let rank = {
            self.player_index
                .read()
                .unwrap()
                .get(player_id)
                .copied()
                .ok_or_else(|| "玩家不在匹配队列中".to_string())?
        };

        if let Some(group_id) = self.get_player_group_id(player_id) {
            return self.remove_group(&group_id);
        }

        let mut pools = self.pools.write().unwrap();
        let pool = pools.get_mut(&rank).unwrap();
        if let Some(pos) = pool.iter().position(|qp| qp.player.id == *player_id) {
            pool.remove(pos);
            self.player_index.write().unwrap().remove(player_id);
            Ok(())
        } else {
            Err("玩家不在匹配队列中".to_string())
        }
    }

    pub fn remove_group(&self, group_id: &Uuid) -> Result<(), String> {
        let group = self
            .groups
            .write()
            .unwrap()
            .remove(group_id)
            .ok_or_else(|| "组队不存在".to_string())?;

        let mut player_to_group = self.player_to_group.write().unwrap();
        let mut pools = self.pools.write().unwrap();
        let mut player_index = self.player_index.write().unwrap();

        let rank = group.rank;
        let pool = pools.get_mut(&rank).unwrap();

        for member in &group.members {
            player_to_group.remove(&member.id);
            player_index.remove(&member.id);
            if let Some(pos) = pool.iter().position(|qp| qp.player.id == member.id) {
                pool.remove(pos);
            }
        }

        Ok(())
    }

    pub fn is_player_in_queue(&self, player_id: &Uuid) -> bool {
        self.player_index
            .read()
            .unwrap()
            .contains_key(player_id)
    }

    pub fn get_player_wait_time(&self, player_id: &Uuid) -> Option<u64> {
        let rank = self.player_index.read().unwrap().get(player_id).copied()?;
        let pools = self.pools.read().unwrap();
        let pool = pools.get(&rank)?;
        let queued = pool.iter().find(|qp| qp.player.id == *player_id)?;
        queued
            .join_time
            .elapsed()
            .map(|d| d.as_secs())
            .ok()
    }

    pub fn get_pool_size(&self, rank: Rank) -> usize {
        self.pools
            .read()
            .unwrap()
            .get(&rank)
            .map(|p| p.len())
            .unwrap_or(0)
    }

    pub fn get_all_pool_sizes(&self) -> HashMap<String, usize> {
        let mut sizes = HashMap::new();
        let pools = self.pools.read().unwrap();
        for rank in Rank::all_ranks() {
            sizes.insert(
                rank.as_str().to_string(),
                pools.get(&rank).map(|p| p.len()).unwrap_or(0),
            );
        }
        sizes
    }

    pub fn get_match_result(&self, player_id: &Uuid) -> Option<MatchResult> {
        self.match_results
            .read()
            .unwrap()
            .get(player_id)
            .cloned()
    }

    fn collect_group_players(
        pool: &VecDeque<QueuedPlayer>,
        start_idx: usize,
        group_id: Uuid,
    ) -> Vec<usize> {
        let mut indices = Vec::new();
        for (i, qp) in pool.iter().enumerate().skip(start_idx) {
            if qp.group_id == Some(group_id) {
                indices.push(i);
            }
            if indices.len() >= TEAM_SIZE {
                break;
            }
        }
        indices
    }

    fn remove_indices(pool: &mut VecDeque<QueuedPlayer>, indices: &[usize]) -> Vec<Player> {
        let mut sorted_indices = indices.to_vec();
        sorted_indices.sort_by(|a, b| b.cmp(a));
        let mut players = Vec::with_capacity(indices.len());
        for idx in sorted_indices {
            let qp = pool.remove(idx).unwrap();
            players.push(qp.player);
        }
        players.reverse();
        players
    }

    pub fn try_match_rank(&self, rank: Rank) -> Vec<MatchResult> {
        let mut results = Vec::new();
        let mut pools = self.pools.write().unwrap();
        let pool = pools.get_mut(&rank).unwrap();

        while pool.len() >= TEAM_SIZE * 2 {
            let mut team1_players = Vec::with_capacity(TEAM_SIZE);
            let mut team2_players = Vec::with_capacity(TEAM_SIZE);

            let first_qp = &pool[0];
            if let Some(group_id) = first_qp.group_id {
                let group_indices = Self::collect_group_players(pool, 0, group_id);
                let group_size = group_indices.len();

                if group_size <= TEAM_SIZE {
                    let group_players = Self::remove_indices(pool, &group_indices);
                    team1_players.extend(group_players);

                    while team1_players.len() < TEAM_SIZE && !pool.is_empty() {
                        let qp = pool.pop_front().unwrap();
                        if let Some(gid) = qp.group_id {
                            let other_group_indices =
                                Self::collect_group_players(pool, 0, gid);
                            let other_group_size = other_group_indices.len() + 1;

                            if team1_players.len() + other_group_size <= TEAM_SIZE {
                                team1_players.push(qp.player);
                                let others = Self::remove_indices(pool, &other_group_indices);
                                team1_players.extend(others);
                            } else {
                                let mut other_players = vec![qp.player];
                                other_players
                                    .extend(Self::remove_indices(pool, &other_group_indices));
                                team2_players.extend(other_players);
                            }
                        } else {
                            team1_players.push(qp.player);
                        }
                    }
                } else {
                    let group_players = Self::remove_indices(pool, &group_indices);
                    for (i, p) in group_players.into_iter().enumerate() {
                        if i < TEAM_SIZE {
                            team1_players.push(p);
                        } else {
                            team2_players.push(p);
                        }
                    }
                }
            } else {
                for _ in 0..TEAM_SIZE {
                    if let Some(qp) = pool.pop_front() {
                        team1_players.push(qp.player);
                    }
                }
            }

            while team2_players.len() < TEAM_SIZE && !pool.is_empty() {
                let qp = pool.pop_front().unwrap();
                if let Some(gid) = qp.group_id {
                    let other_group_indices = Self::collect_group_players(pool, 0, gid);
                    let other_group_size = other_group_indices.len() + 1;

                    if team2_players.len() + other_group_size <= TEAM_SIZE {
                        team2_players.push(qp.player);
                        let others = Self::remove_indices(pool, &other_group_indices);
                        team2_players.extend(others);
                    } else {
                        let idx_to_remove: Vec<usize> = (0..TEAM_SIZE - team2_players.len() - 1)
                            .filter(|&i| i < other_group_indices.len())
                            .map(|i| other_group_indices[i])
                            .collect();

                        team2_players.push(qp.player);
                        let partial = Self::remove_indices(pool, &idx_to_remove);
                        team2_players.extend(partial);
                        break;
                    }
                } else {
                    team2_players.push(qp.player);
                }
            }

            if team1_players.len() == TEAM_SIZE && team2_players.len() == TEAM_SIZE {
                let team1 = Team::new(team1_players);
                let team2 = Team::new(team2_players);

                let match_result = MatchResult {
                    match_id: Uuid::new_v4(),
                    team1,
                    team2,
                    created_at: std::time::SystemTime::now(),
                };

                {
                    let mut index = self.player_index.write().unwrap();
                    let mut player_to_group = self.player_to_group.write().unwrap();
                    let mut groups = self.groups.write().unwrap();
                    let mut results_map = self.match_results.write().unwrap();

                    let mut group_ids_to_remove = Vec::new();
                    for p in &match_result.team1.players {
                        index.remove(&p.id);
                        if let Some(gid) = player_to_group.remove(&p.id) {
                            group_ids_to_remove.push(gid);
                        }
                        results_map.insert(p.id, match_result.clone());
                    }
                    for p in &match_result.team2.players {
                        index.remove(&p.id);
                        if let Some(gid) = player_to_group.remove(&p.id) {
                            group_ids_to_remove.push(gid);
                        }
                        results_map.insert(p.id, match_result.clone());
                    }

                    for gid in group_ids_to_remove {
                        groups.remove(&gid);
                    }
                }

                results.push(match_result);
            } else {
                break;
            }
        }

        results
    }

    pub fn clear_expired_queue_players(&self) -> Vec<Uuid> {
        let mut removed = Vec::new();
        let timeout = self.queue_timeout_seconds;

        let mut pools = self.pools.write().unwrap();
        let mut index = self.player_index.write().unwrap();
        let mut player_to_group = self.player_to_group.write().unwrap();
        let mut groups = self.groups.write().unwrap();

        for rank in Rank::all_ranks() {
            let pool = pools.get_mut(&rank).unwrap();
            let mut i = 0;
            while i < pool.len() {
                let expired = pool[i]
                    .join_time
                    .elapsed()
                    .map(|d| d.as_secs() >= timeout)
                    .unwrap_or(false);

                if expired {
                    let player_id = pool[i].player.id;
                    let group_id = pool[i].group_id;

                    if let Some(gid) = group_id {
                        if let Some(group) = groups.remove(&gid) {
                            for member in &group.members {
                                player_to_group.remove(&member.id);
                                index.remove(&member.id);
                                removed.push(member.id);
                                if let Some(pos) =
                                    pool.iter().position(|qp| qp.player.id == member.id)
                                {
                                    if pos >= i {
                                        pool.remove(pos);
                                    }
                                }
                            }
                        }
                    } else {
                        pool.remove(i);
                        index.remove(&player_id);
                        removed.push(player_id);
                    }
                } else {
                    i += 1;
                }
            }
        }

        removed
    }

    pub fn clear_expired_results(&self, max_age_seconds: u64) {
        let mut results = self.match_results.write().unwrap();
        results.retain(|_, r| {
            r.created_at
                .elapsed()
                .map(|d| d.as_secs() < max_age_seconds)
                .unwrap_or(false)
        });
    }
}

impl Default for MatchPool {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedMatchPool = Arc<MatchPool>;
