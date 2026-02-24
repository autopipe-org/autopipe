mod db;
mod handlers;
mod routes;
mod web;

use std::sync::Arc;
use tokio_postgres::NoTls;
use tracing_subscriber;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let db_host = std::env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".into());
    let db_port = std::env::var("POSTGRES_PORT").unwrap_or_else(|_| "5433".into());
    let db_name = std::env::var("POSTGRES_DB").unwrap_or_else(|_| "autopipe_wf".into());
    let db_user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "autopipe".into());
    let db_pass = std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "autopipe123".into());

    let conn_str = format!(
        "host={} port={} dbname={} user={} password={}",
        db_host, db_port, db_name, db_user, db_pass
    );

    let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
        .await
        .expect("Failed to connect to PostgreSQL");

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::error!("PostgreSQL connection error: {}", e);
        }
    });

    // Ensure table exists
    db::ensure_table(&client).await.expect("Failed to create table");

    let state = Arc::new(db::DbState { client });
    let app = routes::create_router(state);

    let addr = std::env::var("REGISTRY_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".into());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    tracing::info!("AutoPipe Registry listening on {}", addr);
    axum::serve(listener, app).await.expect("Server error");
}
