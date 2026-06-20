use crate::match_pool::SharedMatchPool;
use crate::models::{Group, GroupMatchRequest, MatchRequest, MatchStatus, Player, PlayerMatchStatus, Rank};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub message: String,
    pub data: Option<T>,
}

#[derive(Serialize, Deserialize)]
pub struct PoolStatusResponse {
    pub pools: HashMap<String, usize>,
    pub total_queued: usize,
    pub queue_timeout_seconds: u64,
}

#[derive(Serialize, Deserialize)]
pub struct CancelRequest {
    pub player_id: Uuid,
}

#[derive(Serialize, Deserialize)]
pub struct GroupMatchResponse {
    pub group_id: Uuid,
    pub leader_id: Uuid,
    pub members: Vec<Player>,
    pub rank: Rank,
    pub member_count: usize,
}

pub fn create_router(pool: SharedMatchPool) -> Router {
    Router::new()
        .route("/api/match/join", post(join_match))
        .route("/api/match/group/join", post(join_group_match))
        .route("/api/match/status/:player_id", get(get_status))
        .route("/api/match/cancel", post(cancel_match))
        .route("/api/match/pools", get(get_pool_status))
        .route("/health", get(health_check))
        .with_state(pool)
}

async fn health_check() -> Json<ApiResponse<()>> {
    Json(ApiResponse {
        success: true,
        message: "服务运行中".to_string(),
        data: None,
    })
}

async fn join_match(
    State(pool): State<SharedMatchPool>,
    Json(payload): Json<MatchRequest>,
) -> (StatusCode, Json<ApiResponse<PlayerMatchStatus>>) {
    let player = Player::new(payload.player_id, payload.player_name, payload.score);
    let rank = player.rank;

    match pool.add_player(player) {
        Ok(_) => {
            let wait_time = pool.get_player_wait_time(&payload.player_id).unwrap_or(0);
            let timeout = pool.queue_timeout_seconds();
            let status = PlayerMatchStatus {
                player_id: payload.player_id,
                status: MatchStatus::Queued,
                rank,
                wait_time_seconds: wait_time,
                match_result: None,
            };

            (
                StatusCode::OK,
                Json(ApiResponse {
                    success: true,
                    message: format!(
                        "已加入 {} 段位匹配队列（超过 {} 秒未匹配将自动移除）",
                        rank.as_str(),
                        timeout
                    ),
                    data: Some(status),
                }),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: e,
                data: None,
            }),
        ),
    }
}

async fn join_group_match(
    State(pool): State<SharedMatchPool>,
    Json(payload): Json<GroupMatchRequest>,
) -> (StatusCode, Json<ApiResponse<GroupMatchResponse>>) {
    let mut players = Vec::new();
    for member in &payload.members {
        players.push(Player::new(member.player_id, member.player_name.clone(), member.score));
    }

    let group = match Group::new(payload.leader_id, players) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse {
                    success: false,
                    message: e,
                    data: None,
                }),
            );
        }
    };

    let group_rank = group.rank;
    let group_id = group.id;
    let leader_id = group.leader_id;
    let members = group.members.clone();
    let member_count = members.len();

    match pool.add_group(group) {
        Ok(_) => {
            let timeout = pool.queue_timeout_seconds();
            (
                StatusCode::OK,
                Json(ApiResponse {
                    success: true,
                    message: format!(
                        "组队（{}人）已加入 {} 段位匹配队列，好友将优先分配至同队（超过 {} 秒未匹配将自动移除）",
                        member_count,
                        group_rank.as_str(),
                        timeout
                    ),
                    data: Some(GroupMatchResponse {
                        group_id,
                        leader_id,
                        members,
                        rank: group_rank,
                        member_count,
                    }),
                }),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: e,
                data: None,
            }),
        ),
    }
}

async fn get_status(
    State(pool): State<SharedMatchPool>,
    axum::extract::Path(player_id): axum::extract::Path<Uuid>,
) -> (StatusCode, Json<ApiResponse<PlayerMatchStatus>>) {
    if let Some(result) = pool.get_match_result(&player_id) {
        let rank = Rank::from_score(
            result
                .team1
                .players
                .iter()
                .find(|p| p.id == player_id)
                .or_else(|| result.team2.players.iter().find(|p| p.id == player_id))
                .map(|p| p.score)
                .unwrap_or(0),
        );

        let wait_time = result
            .created_at
            .elapsed()
            .map(|d| d.as_secs())
            .unwrap_or(0);

        return (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: "匹配成功".to_string(),
                data: Some(PlayerMatchStatus {
                    player_id,
                    status: MatchStatus::Matched,
                    rank,
                    wait_time_seconds: wait_time,
                    match_result: Some(result),
                }),
            }),
        );
    }

    if pool.is_player_in_queue(&player_id) {
        let wait_time = pool.get_player_wait_time(&player_id).unwrap_or(0);
        let timeout = pool.queue_timeout_seconds();
        let rank = pool
            .player_index
            .read()
            .unwrap()
            .get(&player_id)
            .copied()
            .unwrap_or(Rank::Bronze);

        let group_info = pool
            .get_player_group_id(&player_id)
            .and_then(|gid| pool.get_group(&gid))
            .map(|group| format!("（组队模式，共{}名队友）", group.size()));

        let base_message = if wait_time as f64 > timeout as f64 * 0.8 {
            format!(
                "匹配中...已等待 {} 秒，请注意: 超过 {} 秒将自动移出队列",
                wait_time, timeout
            )
        } else {
            "匹配中...".to_string()
        };

        let message = match group_info {
            Some(info) => format!("{}{}", base_message, info),
            None => base_message,
        };

        return (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message,
                data: Some(PlayerMatchStatus {
                    player_id,
                    status: MatchStatus::Queued,
                    rank,
                    wait_time_seconds: wait_time,
                    match_result: None,
                }),
            }),
        );
    }

    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse {
            success: false,
            message: "玩家不在匹配队列中".to_string(),
            data: None,
        }),
    )
}

async fn cancel_match(
    State(pool): State<SharedMatchPool>,
    Json(payload): Json<CancelRequest>,
) -> (StatusCode, Json<ApiResponse<()>>) {
    match pool.remove_player(&payload.player_id) {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: "已取消匹配".to_string(),
                data: None,
            }),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: e,
                data: None,
            }),
        ),
    }
}

async fn get_pool_status(
    State(pool): State<SharedMatchPool>,
) -> Json<ApiResponse<PoolStatusResponse>> {
    let pools = pool.get_all_pool_sizes();
    let total_queued = pools.values().sum();
    let timeout = pool.queue_timeout_seconds();

    Json(ApiResponse {
        success: true,
        message: "获取成功".to_string(),
        data: Some(PoolStatusResponse {
            pools,
            total_queued,
            queue_timeout_seconds: timeout,
        }),
    })
}
