mod handlers;
mod match_pool;
mod matchmaker;
mod models;

use crate::match_pool::{MatchPool, SharedMatchPool};
use crate::matchmaker::Matchmaker;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rank_match_server=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let pool: SharedMatchPool = Arc::new(MatchPool::new());

    let matchmaker = Matchmaker::with_interval(pool.clone(), Duration::from_secs(1));
    tokio::spawn(async move {
        matchmaker.run().await;
    });

    let app = handlers::create_router(pool);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("排位匹配服务启动，监听地址: {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
