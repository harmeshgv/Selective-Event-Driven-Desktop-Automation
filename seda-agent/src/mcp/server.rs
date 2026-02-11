//! MCP HTTP server
//!
//! Provides a local HTTP server for the Model Context Protocol.
//!
//! # Security
//!
//! - Binds ONLY to localhost (127.0.0.1)
//! - No authentication (local-only assumption)
//! - All requests go through safety enforcement

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Query, State},
    http::{header, Method, StatusCode},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as TokioMutex;
use tower_http::cors::{Any, CorsLayer};

use super::handlers::McpHandler;
use super::schema::{JsonRpcRequest, JsonRpcResponse};
use crate::control::{CollectionController, CollectorSnapshot};
use crate::observer::window_manager::WindowManager;
use crate::storage::Repository;

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Clone)]
struct AppState {
    handler: Arc<McpHandler>,
    repository: Arc<TokioMutex<Repository>>,
    collector: Option<Arc<Mutex<CollectionController>>>,
}

#[derive(Debug, Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct ActionsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct FlowQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct GraphQuery {
    min_frequency: Option<i64>,
}

#[derive(Debug, Serialize)]
struct DashboardAction {
    id: i64,
    action_type: String,
    node_id: String,
    source_app: Option<String>,
    target_app: Option<String>,
    website_url: Option<String>,
    website_domain: Option<String>,
    search_query: Option<String>,
    search_engine: Option<String>,
    duration_ms: Option<i64>,
    session_id: String,
    timestamp_ms: i64,
    timestamp_iso: String,
}

#[derive(Debug, Serialize)]
struct GraphNode {
    id: String,
}

#[derive(Debug, Serialize)]
struct GraphEdge {
    from: String,
    to: String,
    frequency: i64,
    avg_duration_ms: f64,
    last_seen_ms: i64,
}

#[derive(Debug, Serialize)]
struct DashboardGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

/// MCP server state
pub struct McpServer {
    handler: Arc<McpHandler>,
    repository: Arc<TokioMutex<Repository>>,
    port: u16,
    collector: Option<Arc<Mutex<CollectionController>>>,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(
        port: u16,
        repository: Arc<TokioMutex<Repository>>,
        window_manager: Arc<TokioMutex<WindowManager>>,
    ) -> Self {
        let handler = Arc::new(McpHandler::new(repository.clone(), window_manager));

        Self {
            handler,
            repository,
            port,
            collector: None,
        }
    }

    /// Attach optional collector controls for dashboard start/stop.
    pub fn with_collector(mut self, collector: Arc<Mutex<CollectionController>>) -> Self {
        self.collector = Some(collector);
        self
    }

    /// Start the HTTP server
    ///
    /// # Safety
    ///
    /// The server binds ONLY to 127.0.0.1 (localhost).
    /// This ensures the MCP is only accessible from the local machine.
    pub async fn run(self) -> Result<(), std::io::Error> {
        let app_state = AppState {
            handler: self.handler.clone(),
            repository: self.repository.clone(),
            collector: self.collector.clone(),
        };

        // Build router
        let app = Router::new()
            .route("/", get(health_check))
            .route("/health", get(health_check))
            .route("/rpc", post(handle_rpc))
            .route("/mcp", post(handle_rpc))
            .route("/dashboard", get(dashboard))
            .route("/api/dashboard/status", get(get_dashboard_status))
            .route("/api/dashboard/start", post(start_collection))
            .route("/api/dashboard/stop", post(stop_collection))
            .route("/api/dashboard/clear", post(clear_collected_data))
            .route("/api/dashboard/actions", get(get_dashboard_actions))
            .route("/api/dashboard/flow", get(get_dashboard_flow))
            .route("/api/dashboard/graph", get(get_dashboard_graph))
            .with_state(app_state)
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([Method::GET, Method::POST])
                    .allow_headers([header::CONTENT_TYPE]),
            );

        // SECURITY: Bind ONLY to localhost
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));

        tracing::info!("MCP server starting on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Get the port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the handler for testing
    pub fn handler(&self) -> Arc<McpHandler> {
        self.handler.clone()
    }
}

/// Health check endpoint
async fn health_check() -> &'static str {
    "SEDA Agent MCP Server OK"
}

/// Dashboard UI endpoint
async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

/// Handle JSON-RPC requests
async fn handle_rpc(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> (StatusCode, Json<JsonRpcResponse>) {
    // Validate JSON-RPC version
    if request.jsonrpc != "2.0" {
        return (
            StatusCode::BAD_REQUEST,
            Json(JsonRpcResponse::error(
                request.id,
                -32600,
                "Invalid JSON-RPC version",
            )),
        );
    }

    // Handle the request
    let response = state.handler.handle_request(request).await;

    // JSON-RPC responses always return 200 by convention.
    (StatusCode::OK, Json(response))
}

async fn get_dashboard_status(
    State(state): State<AppState>,
) -> (StatusCode, Json<ApiResponse<CollectorSnapshot>>) {
    let Some(collector) = state.collector else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: "Collector controls are not configured".to_string(),
                data: None,
            }),
        );
    };

    let snapshot_result = tokio::task::spawn_blocking(move || {
        let controller = collector
            .lock()
            .map_err(|e| format!("Failed to lock collector: {}", e))?;
        Ok::<CollectorSnapshot, String>(controller.snapshot())
    })
    .await;

    match snapshot_result {
        Ok(Ok(snapshot)) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: "Collector status retrieved".to_string(),
                data: Some(snapshot),
            }),
        ),
        Ok(Err(err)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: err,
                data: None,
            }),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: format!("Status task failed: {}", err),
                data: None,
            }),
        ),
    }
}

async fn start_collection(
    State(state): State<AppState>,
) -> (StatusCode, Json<ApiResponse<CollectorSnapshot>>) {
    let Some(collector) = state.collector else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: "Collector controls are not configured".to_string(),
                data: None,
            }),
        );
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut controller = collector
            .lock()
            .map_err(|e| format!("Failed to lock collector: {}", e))?;
        controller.start_collection()
    })
    .await;

    match result {
        Ok(Ok(snapshot)) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: if snapshot.collecting {
                    "Data collection is running".to_string()
                } else {
                    "Data collection did not start".to_string()
                },
                data: Some(snapshot),
            }),
        ),
        Ok(Err(err)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: err,
                data: None,
            }),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: format!("Start task failed: {}", err),
                data: None,
            }),
        ),
    }
}

async fn stop_collection(
    State(state): State<AppState>,
) -> (StatusCode, Json<ApiResponse<CollectorSnapshot>>) {
    let Some(collector) = state.collector else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: "Collector controls are not configured".to_string(),
                data: None,
            }),
        );
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut controller = collector
            .lock()
            .map_err(|e| format!("Failed to lock collector: {}", e))?;
        controller.stop_collection()
    })
    .await;

    match result {
        Ok(Ok(snapshot)) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: if snapshot.collecting {
                    "Data collection is still running".to_string()
                } else {
                    "Data collection stopped".to_string()
                },
                data: Some(snapshot),
            }),
        ),
        Ok(Err(err)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: err,
                data: None,
            }),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: format!("Stop task failed: {}", err),
                data: None,
            }),
        ),
    }
}

async fn clear_collected_data(
    State(state): State<AppState>,
) -> (StatusCode, Json<ApiResponse<CollectorSnapshot>>) {
    let Some(collector) = state.collector else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: "Collector controls are not configured".to_string(),
                data: None,
            }),
        );
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut controller = collector
            .lock()
            .map_err(|e| format!("Failed to lock collector: {}", e))?;
        controller.clear_collected_data()
    })
    .await;

    match result {
        Ok(Ok(snapshot)) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: "Collected data cleared".to_string(),
                data: Some(snapshot),
            }),
        ),
        Ok(Err(err)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: err,
                data: None,
            }),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: format!("Clear task failed: {}", err),
                data: None,
            }),
        ),
    }
}

async fn get_dashboard_actions(
    State(state): State<AppState>,
    Query(query): Query<ActionsQuery>,
) -> (StatusCode, Json<ApiResponse<Vec<DashboardAction>>>) {
    let limit = query.limit.unwrap_or(100).clamp(10, 500);

    let repo = state.repository.lock().await;
    let actions = match repo.get_recent_actions(limit) {
        Ok(actions) => actions,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to load actions: {}", err),
                    data: None,
                }),
            );
        }
    };

    let mapped_actions: Vec<DashboardAction> = actions.into_iter().map(to_dashboard_action).collect();

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: "Recent actions loaded".to_string(),
            data: Some(mapped_actions),
        }),
    )
}

async fn get_dashboard_flow(
    State(state): State<AppState>,
    Query(query): Query<FlowQuery>,
) -> (StatusCode, Json<ApiResponse<Vec<DashboardAction>>>) {
    let limit = query.limit.unwrap_or(1200).clamp(50, 5000);
    let now_ms = Utc::now().timestamp_millis();

    let repo = state.repository.lock().await;
    let actions = match repo.get_actions(0, now_ms, Some(limit)) {
        Ok(actions) => actions,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to load flow actions: {}", err),
                    data: None,
                }),
            );
        }
    };

    let mapped_actions: Vec<DashboardAction> = actions.into_iter().map(to_dashboard_action).collect();

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: "Flow actions loaded (oldest to newest)".to_string(),
            data: Some(mapped_actions),
        }),
    )
}

async fn get_dashboard_graph(
    State(state): State<AppState>,
    Query(query): Query<GraphQuery>,
) -> (StatusCode, Json<ApiResponse<DashboardGraph>>) {
    let min_frequency = query.min_frequency.unwrap_or(1).max(1);

    let repo = state.repository.lock().await;
    let transitions = match repo.get_frequent_transitions(min_frequency) {
        Ok(transitions) => transitions,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to load graph transitions: {}", err),
                    data: None,
                }),
            );
        }
    };

    let mut node_ids = BTreeSet::new();
    let mut edges = Vec::new();

    for transition in transitions {
        let from = format_node_id(&transition.from_action_type, transition.from_app.as_deref());
        let to = format_node_id(&transition.to_action_type, transition.to_app.as_deref());
        node_ids.insert(from.clone());
        node_ids.insert(to.clone());

        edges.push(GraphEdge {
            from,
            to,
            frequency: transition.frequency,
            avg_duration_ms: transition.avg_duration_ms(),
            last_seen_ms: transition.last_seen_ms,
        });
    }

    let nodes = node_ids.into_iter().map(|id| GraphNode { id }).collect();
    let graph = DashboardGraph { nodes, edges };

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: "Graph data loaded".to_string(),
            data: Some(graph),
        }),
    )
}

fn format_node_id(action_type: &str, app: Option<&str>) -> String {
    let app_name = app.unwrap_or("unknown");
    format!("{}::{}", action_type, app_name)
}

fn primary_app_for_action<'a>(
    action_type: &str,
    source_app: Option<&'a str>,
    target_app: Option<&'a str>,
) -> Option<&'a str> {
    match action_type {
        "COPY_TEXT" => source_app.or(target_app),
        "SWITCH_APP" | "OPEN_APP" | "CLOSE_APP" | "PASTE_TEXT" | "TYPE_TEXT" | "NAVIGATE"
        | "INTERACT" | "VISIT_WEBSITE" | "SEARCH_WEB" => target_app.or(source_app),
        _ => target_app.or(source_app),
    }
}

fn to_dashboard_action(action: crate::storage::StoredAction) -> DashboardAction {
    let mut website_url: Option<String> = None;
    let mut website_domain: Option<String> = None;
    let mut search_query: Option<String> = None;
    let mut search_engine: Option<String> = None;

    if let Ok(symbolic_action) =
        serde_json::from_str::<crate::symbolizer::SymbolicAction>(&action.action_data)
    {
        match symbolic_action {
            crate::symbolizer::SymbolicAction::VisitWebsite { url, domain, .. } => {
                website_url = Some(url);
                website_domain = Some(domain);
            }
            crate::symbolizer::SymbolicAction::SearchWeb {
                url,
                domain,
                query,
                engine,
                ..
            } => {
                website_url = Some(url);
                website_domain = Some(domain);
                search_query = Some(query);
                search_engine = engine;
            }
            _ => {}
        }
    }

    let node_id = format_node_id(
        &action.action_type,
        primary_app_for_action(
            &action.action_type,
            action.source_app.as_deref(),
            action.target_app.as_deref(),
        ),
    );

    DashboardAction {
        id: action.id,
        action_type: action.action_type,
        node_id,
        source_app: action.source_app,
        target_app: action.target_app,
        website_url,
        website_domain,
        search_query,
        search_engine,
        duration_ms: action.duration_ms,
        session_id: action.session_id,
        timestamp_ms: action.timestamp_ms,
        timestamp_iso: DateTime::<Utc>::from_timestamp_millis(action.timestamp_ms)
            .map(|ts| ts.to_rfc3339())
            .unwrap_or_else(|| "invalid-timestamp".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let result = health_check().await;
        assert!(result.contains("OK"));
    }
}
