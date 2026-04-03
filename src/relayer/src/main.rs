mod auth;
mod config;
mod enclave;
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
use enclave::{enclave_health, get_attestation, get_logs, LogBuffer};
use handlers::health::health_check;
use handlers::messages::{create_message, delete_message, get_messages, update_message};
use nautilus_enclave::EnclaveKeyPair;
use state::AppState;
use storage::create_storage;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

// Import auth middleware components
use auth::{auth_middleware, create_membership_store, AuthState};

// Import background services
use services::{MembershipSyncService, WalrusSyncService};

// Import Walrus client
use walrus::WalrusClient;

// ── Dual-output tracing: stdout (VSOCK) + in-memory ring buffer ───────

struct LogBufferLayer {
    buffer: Arc<LogBuffer>,
}

impl<S> tracing_subscriber::Layer<S> for LogBufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = LogVisitor::default();
        event.record(&mut visitor);
        let level = event.metadata().level();
        let target = event.metadata().target();
        let line = format!(
            "{} {:>5} {} {}",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            level,
            target,
            visitor.message
        );
        self.buffer.push(line);
    }
}

#[derive(Default)]
struct LogVisitor {
    message: String,
}

impl tracing::field::Visit for LogVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else if !self.message.is_empty() {
            self.message
                .push_str(&format!(" {}={:?}", field.name(), value));
        } else {
            self.message = format!("{}={:?}", field.name(), value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else if !self.message.is_empty() {
            self.message
                .push_str(&format!(" {}={}", field.name(), value));
        } else {
            self.message = format!("{}={}", field.name(), value);
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Load .env file if it exists (before reading config)
    dotenvy::dotenv().ok();

    // Set up dual tracing: stdout (for VSOCK streaming) + in-memory buffer (for /logs)
    // Keep dependency noise low so relayer/Walrus activity remains visible in /logs.
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "info,messaging_relayer=info,h2=warn,hyper=warn,reqwest=warn,sui_rpc=warn,tower_http=warn"
        )
    });
    let log_buffer = Arc::new(LogBuffer::new(1000));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .with(LogBufferLayer {
            buffer: log_buffer.clone(),
        })
        .init();

    // Generate ephemeral Ed25519 keypair — NSM entropy in enclave, OsRng locally
    let eph_kp = Arc::new(EnclaveKeyPair::generate());
    info!(
        "Enclave keypair generated. Public key: {}",
        hex::encode(eph_kp.public_key_bytes())
    );

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

    let app_state = AppState::new(
        storage.clone(),
        config.clone(),
        sync_tx,
        eph_kp,
        log_buffer,
    );

    // Initialize membership store (shared between auth middleware and sync service)
    let membership_store = create_membership_store(config.membership_store_type.clone());

    // Start the membership sync service (runs in background, updates cache from Sui events)
    let mut sync_service = MembershipSyncService::new(&config, membership_store.clone());
    tokio::spawn(async move {
        sync_service.run().await;
    });

    // Start the Walrus sync service (runs in background, uploads pending messages)
    let mut walrus_sync_service =
        WalrusSyncService::new(&config, storage, walrus_client, sync_rx);
    tokio::spawn(async move {
        walrus_sync_service.run().await;
    });

    // Create auth state for middleware
    let auth_state = AuthState {
        membership_store,
        config: config.clone(),
    };

    // Nautilus enclave routes — attestation, enclave health, live logs
    let enclave_routes = Router::new()
        .route("/health", get(enclave_health))
        .route("/get_attestation", get(get_attestation))
        .route("/logs", get(get_logs))
        .with_state(app_state.clone());

    // Routes that require authentication (GET, POST, PUT, DELETE)
    let authenticated_routes = Router::new()
        .route(
            "/messages",
            get(get_messages).post(create_message).put(update_message),
        )
        .route("/messages/:message_id", delete(delete_message))
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware))
        .with_state(app_state.clone());

    // Routes that don't require authentication
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
        .merge(enclave_routes)
        .merge(public_routes)
        .merge(authenticated_routes)
        .layer(cors);

    let addr = format!("0.0.0.0:{}", config.port);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|_| panic!("Failed to bind to {}", addr));

    info!(
        "Nautilus Messaging Relayer listening on {}",
        listener.local_addr().unwrap()
    );

    axum::serve(listener, app.into_make_service())
        .await
        .expect("Server error");
}
