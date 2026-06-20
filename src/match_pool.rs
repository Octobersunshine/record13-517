use crate::models::{MatchResult, Player, QueuedPlayer, Rank, Team};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub const TEAM_SIZE: usize = 5;

pub const DEFAULT_QUEUE_TIMEOUT_SECONDS: u64 = 30 * 60;

pub struct MatchPool {
    pools: RwLock<HashMap<Rank, VecDeque<QueuedPlayer>>>,
    pub player_index: RwLock<HashMap<Uuid, Rank>>,
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
            match_results: RwLock::new(HashMap::new()),
            queue_timeout_seconds,
        }
    }

    pub fn queue_timeout_seconds(&self) -> u64 {
        self.queue_timeout_seconds
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

    pub fn remove_player(&self, player_id: &Uuid) -> Result<(), String> {
        let rank = {
            self.player_index
                .read()
                .unwrap()
                .get(player_id)
                .copied()
                .ok_or_else(|| "玩家不在匹配队列中".to_string())?
        };

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

    pub fn try_match_rank(&self, rank: Rank) -> Vec<MatchResult> {
        let mut results = Vec::new();
        let mut pools = self.pools.write().unwrap();
        let pool = pools.get_mut(&rank).unwrap();

        while pool.len() >= TEAM_SIZE * 2 {
            let mut team1_players = Vec::with_capacity(TEAM_SIZE);
            let mut team2_players = Vec::with_capacity(TEAM_SIZE);

            for _ in 0..TEAM_SIZE {
                if let Some(qp) = pool.pop_front() {
                    team1_players.push(qp.player);
                }
            }

            for _ in 0..TEAM_SIZE {
                if let Some(qp) = pool.pop_front() {
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
                    let mut results_map = self.match_results.write().unwrap();
                    for p in &match_result.team1.players {
                        index.remove(&p.id);
                        results_map.insert(p.id, match_result.clone());
                    }
                    for p in &match_result.team2.players {
                        index.remove(&p.id);
                        results_map.insert(p.id, match_result.clone());
                    }
                }

                results.push(match_result);
            }
        }

        results
    }

    pub fn clear_expired_queue_players(&self) -> Vec<Uuid> {
        let mut removed = Vec::new();
        let timeout = self.queue_timeout_seconds;

        let mut pools = self.pools.write().unwrap();
        let mut index = self.player_index.write().unwrap();

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
                    let qp = pool.remove(i).unwrap();
                    let player_id = qp.player.id;
                    index.remove(&player_id);
                    removed.push(player_id);
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
