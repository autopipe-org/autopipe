use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use common::models::{Pipeline, SearchQuery};

use crate::db::DbState;

pub async fn list_or_search(
    State(state): State<Arc<DbState>>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let result = if let Some(ref q) = params.q {
        crate::db::search_pipelines(&state.client, q).await
    } else {
        crate::db::list_pipelines(&state.client).await
    };

    match result {
        Ok(pipelines) => Json(serde_json::json!(pipelines)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_pipeline(
    State(state): State<Arc<DbState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    match crate::db::get_pipeline(&state.client, id).await {
        Ok(Some(pipeline)) => Json(serde_json::json!(pipeline)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Pipeline not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn create_pipeline(
    State(state): State<Arc<DbState>>,
    Json(mut pipeline): Json<Pipeline>,
) -> impl IntoResponse {
    pipeline.clean_file_contents();
    match crate::db::insert_pipeline(&state.client, &pipeline).await {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"pipeline_id": id})),
        )
            .into_response(),
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("duplicate key") || msg.contains("unique") {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(serde_json::json!({"error": msg}))).into_response()
        }
    }
}

pub async fn update_pipeline(
    State(state): State<Arc<DbState>>,
    Path(id): Path<i32>,
    Json(mut pipeline): Json<Pipeline>,
) -> impl IntoResponse {
    pipeline.clean_file_contents();
    match crate::db::update_pipeline(&state.client, id, &pipeline).await {
        Ok(true) => Json(serde_json::json!({"updated": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Pipeline not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_pipeline(
    State(state): State<Arc<DbState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    match crate::db::delete_pipeline(&state.client, id).await {
        Ok(true) => Json(serde_json::json!({"deleted": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Pipeline not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
