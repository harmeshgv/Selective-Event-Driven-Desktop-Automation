//! SEDA Agent - Selective Event-Driven Desktop Automation
//!
//! A privacy-first local agent that observes OS-level user behavior,
//! discovers repeated action patterns, and exposes them via MCP for
//! AI-assisted automation suggestions.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │   Windows   │────>│  Observer   │────>│ Symbolizer  │
//! │     OS      │     │  (hooks)    │     │  (privacy)  │
//! └─────────────┘     └─────────────┘     └──────┬──────┘
//!                                                │
//!                     ┌─────────────┐     ┌──────▼──────┐
//!                     │   Mining    │<────│   Storage   │
//!                     │  (patterns) │     │  (SQLite)   │
//!                     └──────┬──────┘     └──────┬──────┘
//!                            │                   │
//!                     ┌──────▼──────┐     ┌──────▼──────┐
//!                     │    Graph    │     │     MCP     │
//!                     │   (task)    │     │   (HTTP)    │
//!                     └─────────────┘     └─────────────┘
//! ```
//!
//! # Safety
//!
//! - All events are symbolized immediately (no raw data storage)
//! - MCP enforces strict allowlist (no shell, no arbitrary mouse, etc.)
//! - All actions are audited
//! - Rate limiting prevents automation abuse

use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use anyhow::Result;
use parking_lot::RwLock;
use tokio::signal;
use tokio::sync::Mutex as TokioMutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use seda_agent::config::Config;
use seda_agent::control::CollectionController;
use seda_agent::graph::TaskGraphBuilder;
use seda_agent::mcp::McpServer;
use seda_agent::mining::SequenceMiner;
use seda_agent::observer::events::RawOsEvent;
use seda_agent::observer::window_manager::WindowManager;
use seda_agent::storage::Repository;
use seda_agent::symbolizer::transformer::{EventTransformer, TimestampedAction};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "seda_agent=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("SEDA Agent starting...");
    tracing::info!("Privacy-first desktop automation agent");

    // Load configuration
    let config = Config::from_env();
    tracing::info!("Configuration loaded: {:?}", config);

    // Initialize storage
    // We use std::sync::Mutex for Repository because rusqlite::Connection is not Sync
    tracing::info!("Initializing storage at {:?}", config.database_path);
    let repository = Arc::new(Mutex::new(
        Repository::open(&config.database_path)
            .map_err(|e| anyhow::anyhow!("Failed to open database: {}", e))?,
    ));
    tracing::info!("Storage initialized");

    // Create a separate repository instance for MCP (needs TokioMutex for async)
    let mcp_repository = Arc::new(TokioMutex::new(
        Repository::open(&config.database_path)
            .map_err(|e| anyhow::anyhow!("Failed to open MCP database: {}", e))?,
    ));

    // Initialize window manager (used by observers)
    let _window_manager = Arc::new(RwLock::new(WindowManager::new()));
    
    // Create a separate window manager for MCP (needs TokioMutex for async)
    let mcp_window_manager = Arc::new(TokioMutex::new(WindowManager::new()));

    // Initialize task graph builder
    let graph_builder = Arc::new(RwLock::new(TaskGraphBuilder::new()));

    // Initialize sequence miner
    let miner = Arc::new(RwLock::new(SequenceMiner::new()));

    // Set up event channels
    let (event_tx, event_rx) = mpsc::channel::<RawOsEvent>();
    let (action_tx, action_rx) = mpsc::channel::<TimestampedAction>();

    // Initialize collector controller (observer start/stop lifecycle)
    let collector = Arc::new(Mutex::new(CollectionController::new(
        event_tx,
        Arc::clone(&repository),
    )));
    tracing::info!("Data collection is idle at startup (manual start via dashboard)");

    // Start the event transformer (symbolizer)
    tracing::info!("Starting event transformer...");
    let _transformer_handle = {
        let action_tx = action_tx;
        thread::spawn(move || {
            let mut transformer = EventTransformer::new();

            for event in event_rx {
                if let Some(action) = transformer.transform(event) {
                    if action_tx.send(action).is_err() {
                        break;
                    }
                }
            }

            tracing::info!("Event transformer stopped");
        })
    };

    // Start the action processor (storage, graph, mining)
    tracing::info!("Starting action processor...");
    let _processor_handle = {
        let repository = Arc::clone(&repository);
        let graph_builder = Arc::clone(&graph_builder);
        let miner = Arc::clone(&miner);
        let config = config.clone();

        thread::spawn(move || {
            let mut last_action: Option<seda_agent::symbolizer::SymbolicAction> = None;

            for timestamped_action in action_rx {
                let action = &timestamped_action.action;
                let timestamp = timestamped_action.timestamp;
                let duration_ms = timestamped_action.duration_ms;

                // Store the action
                if let Ok(repo) = repository.lock() {
                    if let Err(e) = repo.store_action(action, timestamp, duration_ms) {
                        tracing::error!("Failed to store action: {}", e);
                    }

                    // Record transition
                    if let Some(ref prev_action) = last_action {
                        if let Err(e) = repo.record_transition(prev_action, action, duration_ms) {
                            tracing::error!("Failed to record transition: {}", e);
                        }
                    }
                }

                // Update task graph
                graph_builder.write().observe(action, timestamp);

                // Update miner
                miner.write().process_action(action, timestamp);

                // Periodically check for patterns and store them
                let miner_stats = miner.read().stats();
                if miner_stats.buffer_size % 10 == 0 && miner_stats.buffer_size > 0 {
                    let patterns = miner.read().get_frequent_patterns();
                    for pattern in patterns {
                        if pattern.frequency >= config.min_pattern_frequency {
                            if let Ok(repo) = repository.lock() {
                                if let Err(e) = repo.store_pattern(
                                    &pattern.sequence,
                                    pattern.frequency as i64,
                                    Some(pattern.avg_total_duration_ms as i64),
                                    pattern.confidence,
                                ) {
                                    tracing::error!("Failed to store pattern: {}", e);
                                }
                            }
                        }
                    }
                }

                last_action = Some(action.clone());
            }

            tracing::info!("Action processor stopped");
        })
    };

    // Start MCP server
    tracing::info!("Starting MCP server on port {}...", config.mcp_port);
    let mcp_server = McpServer::new(config.clone(), mcp_repository, mcp_window_manager)
        .with_collector(Arc::clone(&collector));

    // Run MCP server in background
    let _mcp_handle = tokio::spawn(async move {
        if let Err(e) = mcp_server.run().await {
            tracing::error!("MCP server error: {}", e);
        }
    });

    tracing::info!("===========================================");
    tracing::info!("SEDA Agent is running!");
    tracing::info!("MCP Server: http://127.0.0.1:{}", config.mcp_port);
    tracing::info!("Dashboard: http://127.0.0.1:{}/dashboard", config.mcp_port);
    tracing::info!("Database: {:?}", config.database_path);
    tracing::info!("Press Ctrl+C to stop");
    tracing::info!("===========================================");

    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            tracing::info!("Shutdown signal received");
        }
        Err(err) => {
            tracing::error!("Unable to listen for shutdown signal: {}", err);
        }
    }

    // Cleanup
    tracing::info!("Shutting down...");

    // Stop observers via controller
    if let Ok(mut controller) = collector.lock() {
        if let Err(e) = controller.stop_collection() {
            tracing::error!("Error stopping data collection: {}", e);
        }
    } else {
        tracing::error!("Failed to lock collector controller during shutdown");
    }

    // Log final statistics
    let action_count = repository.lock().ok().map(|r| r.count_actions().unwrap_or(0)).unwrap_or(0);
    let graph = graph_builder.read();
    let miner_stats = miner.read().stats();

    tracing::info!("Final statistics:");
    tracing::info!("  Total actions recorded: {}", action_count);
    tracing::info!("  Graph nodes: {}", graph.graph().node_count());
    tracing::info!("  Graph edges: {}", graph.graph().edge_count());
    tracing::info!("  Patterns detected: {}", miner_stats.frequent_patterns);

    tracing::info!("SEDA Agent stopped");

    Ok(())
}
