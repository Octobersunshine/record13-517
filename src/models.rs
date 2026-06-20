use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rank {
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
    Master,
    Challenger,
}

impl Rank {
    pub fn from_score(score: u32) -> Self {
        match score {
            0..=999 => Rank::Bronze,
            1000..=1999 => Rank::Silver,
            2000..=2999 => Rank::Gold,
            3000..=3999 => Rank::Platinum,
            4000..=4999 => Rank::Diamond,
            5000..=5999 => Rank::Master,
            _ => Rank::Challenger,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Rank::Bronze => "bronze",
            Rank::Silver => "silver",
            Rank::Gold => "gold",
            Rank::Platinum => "platinum",
            Rank::Diamond => "diamond",
            Rank::Master => "master",
            Rank::Challenger => "challenger",
        }
    }

    pub fn all_ranks() -> Vec<Rank> {
        vec![
            Rank::Bronze,
            Rank::Silver,
            Rank::Gold,
            Rank::Platinum,
            Rank::Diamond,
            Rank::Master,
            Rank::Challenger,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: Uuid,
    pub name: String,
    pub score: u32,
    pub rank: Rank,
}

impl Player {
    pub fn new(id: Uuid, name: String, score: u32) -> Self {
        let rank = Rank::from_score(score);
        Self { id, name, score, rank }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchRequest {
    pub player_id: Uuid,
    pub player_name: String,
    pub score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedPlayer {
    pub player: Player,
    pub join_time: SystemTime,
}

impl QueuedPlayer {
    pub fn new(player: Player) -> Self {
        Self {
            player,
            join_time: SystemTime::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Uuid,
    pub players: Vec<Player>,
    pub created_at: SystemTime,
    pub average_score: f64,
}

impl Team {
    pub fn new(players: Vec<Player>) -> Self {
        let average_score = if players.is_empty() {
            0.0
        } else {
            players.iter().map(|p| p.score as f64).sum::<f64>() / players.len() as f64
        };
        Self {
            id: Uuid::new_v4(),
            players,
            created_at: SystemTime::now(),
            average_score,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub match_id: Uuid,
    pub team1: Team,
    pub team2: Team,
    pub created_at: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchStatus {
    Queued,
    Matched,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerMatchStatus {
    pub player_id: Uuid,
    pub status: MatchStatus,
    pub rank: Rank,
    pub wait_time_seconds: u64,
    pub match_result: Option<MatchResult>,
}
