use axum::{routing::{get}, extract::Extension, Router, response::{IntoResponse}, http::StatusCode};
use axum_login::{AuthManagerLayerBuilder};
use tower_sessions::{SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;
use std::sync::{Arc};
use tokio::sync::Mutex;
use tower_http::trace;
use tower_http::trace::TraceLayer;
use tracing::{info, Level, error, warn};
use std::net::SocketAddr;
use tower_cookies::CookieManagerLayer;
use tower_http::services::ServeDir;
use tokio::signal::unix::{signal, SignalKind};

use crate::auth::{AdminBackend, auth_router, load_credentials, generate_default_credentials, load_settings, Settings};
use crate::db::init_db;
use crate::event_manager::EventManager;

mod auth;
mod api;
mod db;
mod filters;
mod ui;
mod tinkerbell;
mod event_manager;
mod os_templates;

// Global static for accessing event manager from other modules
use std::sync::RwLock;
use once_cell::sync::Lazy;
pub static EVENT_MANAGER_REF: Lazy<RwLock<Option<std::sync::Arc<EventManager>>>> = Lazy::new(|| {
    RwLock::new(None)
});

// Application state struct
#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Mutex<Settings>>,
    pub event_manager: Arc<EventManager>,
}

pub async fn run() -> anyhow::Result<()> {
    // Initialize the database 
    let db_pool = init_db().await?;
    
    // Initialize timing database tables
    db::init_timing_tables().await?;
    
    // Load historical timing data
    tinkerbell::load_historical_timings().await?;
    
    // --- Start HookOS Artifacts Download in Background ---
    info!("Starting HookOS artifacts check and download in background...");
    tokio::spawn(async move {
        // First check if artifacts exist
        if !api::check_hookos_artifacts().await {
            info!("HookOS artifacts not found. Downloading HookOS artifacts...");
            match api::download_hookos_artifacts("v0.10.0").await {
                Ok(_) => info!("HookOS artifacts downloaded successfully"),
                Err(e) => warn!("Failed to download HookOS artifacts: {}", e),
            }
        } else {
            info!("HookOS artifacts already exist");
        }
    });
    
    // --- Start OS Templates Initialization in Background ---
    info!("Starting OS templates initialization in background...");
    tokio::spawn(async move {
        match os_templates::init_os_templates().await {
            Ok(_) => info!("OS templates initialized successfully"),
            Err(e) => warn!("Failed to initialize OS templates: {}", e),
        }
    });
    
    // --- Graceful Shutdown Setup ---
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

    // Start the timing cleanup task with shutdown signal
    tinkerbell::start_timing_cleanup_task(shutdown_rx.clone()).await;
    
    // Create event manager
    let event_manager = Arc::new(EventManager::new());
    
    // Store the event manager in the global static for access from other modules
    if let Ok(mut global_ref) = EVENT_MANAGER_REF.write() {
        *global_ref = Some(event_manager.clone());
    } else {
        error!("Failed to store event manager reference");
    }
    
    // Start the workflow polling task with shutdown signal
    tinkerbell::start_workflow_polling_task(event_manager.clone(), shutdown_rx.clone()).await;
    
    // Load or generate admin credentials
    let credentials = match load_credentials().await {
        Ok(creds) => {
            info!("Loaded existing admin credentials");
            creds
        },
        Err(_) => {
            info!("No admin credentials found, generating default credentials");
            match generate_default_credentials().await {
                Ok(creds) => creds,
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to generate default credentials: {}", e));
                }
            }
        }
    };
    
    // Load settings
    let settings = match load_settings().await {
        Ok(s) => s,
        Err(e) => {
            info!("Failed to load settings: {}, using defaults", e);
            Settings::default()
        }
    };
    
    // Create application state
    let app_state = AppState {
        settings: Arc::new(Mutex::new(settings)),
        event_manager: event_manager,
    };
    
    // Set up the persistent session store using the sqlx store
    let session_store = SqliteStore::new(db_pool.clone());
    session_store.migrate().await?; // Create the sessions table

    // Configure the session layer with the SqliteStore
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false);
    
    // Create session-based authentication
    let backend = AdminBackend::new(credentials);
    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer).build();
    
    // Build the app router with shared state
    let app = Router::new()
        .merge(auth_router())
        .merge(ui::ui_router())
        .route("/favicon.ico", get(handle_favicon))
        .route("/{mac}", get(api::ipxe_script))
        .route("/ipxe/{*path}", get(api::serve_ipxe_artifact))
        .nest("/api", api::api_router())
        .nest_service("/static", {
            let preferred_path = "/opt/dragonfly/static";
            let fallback_path = "crates/dragonfly-server/static";
            
            let static_path = if std::path::Path::new(preferred_path).exists() {
                preferred_path
            } else {
                fallback_path
            };
            
            ServeDir::new(static_path)
        })
        .layer(CookieManagerLayer::new())
        .layer(auth_layer)
        .layer(Extension(db_pool.clone())) // Pass the pool clone
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new()
                    .level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new()
                    .level(Level::INFO)),
        )
        .with_state(app_state);
    
    // --- Start Server with Graceful Shutdown ---
    info!("Starting server on 0.0.0.0:3000");
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Define the shutdown signal future
    let shutdown_signal = async move {
        let ctrl_c = async {
            tokio::signal::ctrl_c().await
                .expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal(SignalKind::terminate())
                .expect("Failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))] // Fallback for non-Unix systems
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {
                 info!("Received SIGINT (Ctrl+C), initiating shutdown...");
            },
            _ = terminate => {
                 info!("Received SIGTERM, initiating shutdown...");
            },
        }

        // Send the shutdown signal to background tasks
        info!("Sending shutdown signal to background tasks...");
        let _ = shutdown_tx.send(());
    };

    // Run the server with graceful shutdown
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    info!("Server shutdown complete.");

    Ok(())
} 

async fn handle_favicon() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "Favicon not found")
}