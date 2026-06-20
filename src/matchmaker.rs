use crate::match_pool::SharedMatchPool;
use crate::models::Rank;
use tokio::time::{self, Duration};
use tracing::info;

pub struct Matchmaker {
    pool: SharedMatchPool,
    interval: Duration,
}

impl Matchmaker {
    pub fn new(pool: SharedMatchPool) -> Self {
        Self {
            pool,
            interval: Duration::from_secs(1),
        }
    }

    pub fn with_interval(pool: SharedMatchPool, interval: Duration) -> Self {
        Self { pool, interval }
    }

    pub async fn run(self) {
        let mut interval = time::interval(self.interval);

        loop {
            interval.tick().await;
            self.tick();
        }
    }

    fn tick(&self) {
        let mut total_matches = 0;

        for rank in Rank::all_ranks() {
            let results = self.pool.try_match_rank(rank);
            if !results.is_empty() {
                total_matches += results.len();
                for r in &results {
                    info!(
                        "段位 {} 匹配成功 - 比赛ID: {}, 队伍1: {}人, 队伍2: {}人",
                        rank.as_str(),
                        r.match_id,
                        r.team1.players.len(),
                        r.team2.players.len()
                    );
                }
            }
        }

        if total_matches > 0 {
            let sizes = self.pool.get_all_pool_sizes();
            info!("本轮匹配完成，共 {} 场比赛，当前各段位队列: {:?}", total_matches, sizes);
        }

        self.pool.clear_expired_results(300);
    }
}
