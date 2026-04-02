mod auth;
mod config;
mod handlers;
mod models;
mod services;
mod state;
mod storage;
mod walrus;

use std::sync::Arc;

use axum::{
    middleware,
    routing::{delete, get},
    Router,
};
use config::Config;
use handlers::health::health_check;
use handlers::messages::{create_message, delete_message, get_messages, update_message};
use state::AppState;
use storage::create_storage;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

// Import auth middleware components
use auth::{auth_middleware, create_membership_store, AuthState};

// Import background services
use services::{MembershipSyncService, WalrusSyncService};

// Import Walrus client
use walrus::WalrusClient;

#[tokio::main]
async fn main() {
    // Load .env file if it exists (before reading config)
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt::init();

    // Load configuration from environment
    let config = Config::from_env();

    // Initialize storage backend based on STORAGE_TYPE env var
    let storage = create_storage(config.storage_type.clone());

    // Create the shared Walrus HTTP client from config URLs
    let walrus_client = Arc::new(WalrusClient::new(
        &config.walrus_publisher_url,
        &config.walrus_aggregator_url,
    ));

    let (sync_tx, sync_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    let app_state = AppState::new(storage.clone(), config.clone(), sync_tx);

    // Initialize membership store (shared between auth middleware and sync service)
    let membership_store = create_membership_store(config.membership_store_type.clone());

    // Start the membership sync service (runs in background, updates cache from Sui events)
    let mut sync_service = MembershipSyncService::new(&config, membership_store.clone());
    tokio::spawn(async move {
        sync_service.run().await;
    });

    // Start the Walrus sync service (runs in background, uploads pending messages)
    let mut walrus_sync_service = WalrusSyncService::new(&config, storage, walrus_client, sync_rx);
    tokio::spawn(async move {
        walrus_sync_service.run().await;
    });

    // Create auth state for middleware
    let auth_state = AuthState {
        membership_store,
        config: config.clone(),
    };

    // Routes that require authentication (GET, POST, PUT, DELETE)
    let authenticated_routes = Router::new()
        .route(
            "/messages",
            get(get_messages).post(create_message).put(update_message),
        )
        .route("/messages/:message_id", delete(delete_message))
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware))
        .with_state(app_state.clone());

    // Routes that don't require authentication (health check only)
    let public_routes = Router::new()
        .route("/health_check", get(health_check))
        .with_state(app_state);

    // WARNING: This permissive CORS configuration is for development/demo purposes only.
    // In production, restrict allow_origin to specific trusted domains.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Combine all routes
    let app = Router::new()
        .merge(public_routes)
        .merge(authenticated_routes)
        .layer(cors);

    let addr = format!("0.0.0.0:{}", config.port);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|_| panic!("Failed to bind to {}", addr));

    info!(
        "Messaging Relayer listening on {}",
        listener.local_addr().unwrap()
    );

    axum::serve(listener, app.into_make_service())
        .await
        .expect("Server error");
}
