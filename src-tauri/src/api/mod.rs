use axum::{
    extract::{Query, State},
    response::Json,
    routing::get,
    Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::search::SearchEngine;

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<usize>,
}

pub fn create_router(engine: SearchEngine) -> Router {
    Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/search", get(search_handler))
        .with_state(Arc::new(engine))
}

async fn search_handler(
    State(engine): State<Arc<SearchEngine>>,
    Query(params): Query<SearchQuery>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50);
    
    match engine.search(&params.q, limit).await {
        Ok(results) => Json(serde_json::json!({ "status": "ok", "results": results })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
    }
}
