//! MCP HTTP server
//!
//! Provides a local HTTP server for the Model Context Protocol.
//!
//! # Security
//!
//! - Binds ONLY to localhost (127.0.0.1)
//! - No authentication (local-only assumption)
//! - All requests go through safety enforcement

use std::collections::{BTreeSet, HashMap};
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
use sha2::{Digest, Sha256};
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

#[derive(Debug, Deserialize)]
struct RepeatedTasksQuery {
    #[serde(alias = "min_frequency")]
    min_repeats: Option<usize>,
    limit: Option<usize>,
    flow_limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct DashboardAction {
    id: i64,
    action_type: String,
    node_id: String,
    source_app: Option<String>,
    target_app: Option<String>,
    element_id: Option<String>,
    element_control_type: Option<String>,
    element_automation_id: Option<String>,
    element_class_name: Option<String>,
    element_name_hash: Option<String>,
    element_interaction: Option<String>,
    element_field_type: Option<String>,
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

#[derive(Debug, Serialize)]
struct DashboardRepeatedTask {
    pattern_hash: String,
    sequence: Vec<String>,
    sequence_label: String,
    frequency: i64,
    avg_duration_ms: Option<i64>,
    confidence: f64,
    first_seen_ms: i64,
    last_seen_ms: i64,
    last_seen_iso: String,
    sample_run: Vec<DashboardAction>,
}

#[derive(Debug, Clone)]
struct ActionSignature {
    key: String,
    label: String,
}

#[derive(Debug, Clone)]
struct RepeatedSequenceStats {
    sequence_keys: Vec<String>,
    sequence_labels: Vec<String>,
    frequency: i64,
    total_duration_ms: i64,
    first_seen_ms: i64,
    last_seen_ms: i64,
    latest_start_idx: usize,
    latest_end_idx: usize,
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
            .route(
                "/api/dashboard/repeated_tasks",
                get(get_dashboard_repeated_tasks),
            )
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

    let repo = state.repository.lock().await;
    let actions = match repo.get_recent_actions_chronological(limit) {
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
            message: "Flow actions loaded (latest window, oldest to newest)".to_string(),
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

async fn get_dashboard_repeated_tasks(
    State(state): State<AppState>,
    Query(query): Query<RepeatedTasksQuery>,
) -> (StatusCode, Json<ApiResponse<Vec<DashboardRepeatedTask>>>) {
    const DEFAULT_MIN_REPEATS: usize = 3;
    const MAX_PATTERN_LENGTH_CAP: usize = 64;
    const MIN_REPEAT_OCCURRENCES: i64 = 2;

    let min_repeats = query
        .min_repeats
        .unwrap_or(DEFAULT_MIN_REPEATS)
        .clamp(2, MAX_PATTERN_LENGTH_CAP);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let flow_limit = query.flow_limit.unwrap_or(5000).clamp(200, 20000);
    let max_pattern_length = min_repeats.max(10).min(MAX_PATTERN_LENGTH_CAP);

    let repo = state.repository.lock().await;
    let flow_actions = match repo.get_recent_actions_chronological(flow_limit) {
        Ok(actions) => actions,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to load flow actions for repeated tasks: {}", err),
                    data: None,
                }),
            );
        }
    };

    let repeated_tasks = build_repeated_task_bundles(
        &flow_actions,
        min_repeats,
        MIN_REPEAT_OCCURRENCES,
        limit,
        max_pattern_length,
    );

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: "Repeated tasks loaded".to_string(),
            data: Some(repeated_tasks),
        }),
    )
}

fn format_node_id(action_type: &str, app: Option<&str>) -> String {
    let app_name = app.unwrap_or("unknown");
    format!("{}::{}", action_type, app_name)
}

fn build_repeated_task_bundles(
    actions: &[crate::storage::StoredAction],
    min_pattern_length: usize,
    min_occurrences: i64,
    limit: usize,
    max_pattern_length: usize,
) -> Vec<DashboardRepeatedTask> {
    const ABS_MIN_PATTERN_LENGTH: usize = 2;
    let min_len = min_pattern_length.max(ABS_MIN_PATTERN_LENGTH);

    if actions.len() < min_len {
        return Vec::new();
    }

    let signatures: Vec<ActionSignature> =
        actions.iter().map(build_action_signature).collect();

    let mut counts: HashMap<String, RepeatedSequenceStats> = HashMap::new();
    let max_len = max_pattern_length.max(min_len).min(actions.len());

    for span in min_len..=max_len {
        for start in 0..=actions.len() - span {
            let end = start + span - 1;
            let keys_slice = &signatures[start..start + span];
            let sequence_key = keys_slice
                .iter()
                .map(|sig| sig.key.clone())
                .collect::<Vec<_>>()
                .join("||");
            let duration_ms = (actions[end].timestamp_ms - actions[start].timestamp_ms).max(0);

            let entry = counts.entry(sequence_key).or_insert_with(|| RepeatedSequenceStats {
                sequence_keys: keys_slice.iter().map(|sig| sig.key.clone()).collect(),
                sequence_labels: keys_slice.iter().map(|sig| sig.label.clone()).collect(),
                frequency: 0,
                total_duration_ms: 0,
                first_seen_ms: actions[start].timestamp_ms,
                last_seen_ms: actions[end].timestamp_ms,
                latest_start_idx: start,
                latest_end_idx: end,
            });

            entry.frequency += 1;
            entry.total_duration_ms += duration_ms;
            entry.first_seen_ms = entry.first_seen_ms.min(actions[start].timestamp_ms);

            if actions[end].timestamp_ms >= entry.last_seen_ms {
                entry.last_seen_ms = actions[end].timestamp_ms;
                entry.latest_start_idx = start;
                entry.latest_end_idx = end;
            }
        }
    }

    let mut candidates: Vec<RepeatedSequenceStats> = counts
        .into_values()
        .filter(|stats| stats.frequency >= min_occurrences.max(2))
        .collect();

    candidates.sort_by(|a, b| {
        b.frequency
            .cmp(&a.frequency)
            .then_with(|| b.sequence_keys.len().cmp(&a.sequence_keys.len()))
            .then_with(|| b.last_seen_ms.cmp(&a.last_seen_ms))
    });

    let mut selected: Vec<RepeatedSequenceStats> = Vec::new();
    for candidate in candidates {
        let dominated = selected.iter().any(|existing| {
            existing.frequency >= candidate.frequency
                && existing.sequence_keys.len() >= candidate.sequence_keys.len()
                && contains_contiguous_subsequence(
                    &existing.sequence_keys,
                    &candidate.sequence_keys,
                )
        });
        if !dominated {
            selected.push(candidate);
        }
        if selected.len() >= limit {
            break;
        }
    }

    selected
        .into_iter()
        .map(|stats| {
            let signature_joined = stats.sequence_keys.join("||");
            let pattern_hash = {
                let mut hasher = Sha256::new();
                hasher.update(signature_joined.as_bytes());
                hex::encode(hasher.finalize())
            };

            let span = stats.sequence_keys.len() as i64;
            let avg_duration_ms = if stats.frequency > 0 {
                Some((stats.total_duration_ms / stats.frequency).max(span))
            } else {
                None
            };

            let run = actions[stats.latest_start_idx..=stats.latest_end_idx]
                .iter()
                .cloned()
                .map(to_dashboard_action)
                .collect::<Vec<_>>();

            DashboardRepeatedTask {
                pattern_hash,
                sequence: stats.sequence_labels.clone(),
                sequence_label: stats.sequence_labels.join(" -> "),
                frequency: stats.frequency,
                avg_duration_ms,
                confidence: 1.0,
                first_seen_ms: stats.first_seen_ms,
                last_seen_ms: stats.last_seen_ms,
                last_seen_iso: DateTime::<Utc>::from_timestamp_millis(stats.last_seen_ms)
                    .map(|ts| ts.to_rfc3339())
                    .unwrap_or_else(|| "invalid-timestamp".to_string()),
                sample_run: run,
            }
        })
        .collect()
}

fn build_action_signature(action: &crate::storage::StoredAction) -> ActionSignature {
    let primary_app = primary_app_for_action(
        &action.action_type,
        action.source_app.as_deref(),
        action.target_app.as_deref(),
    )
    .unwrap_or("unknown");
    let normalized_app = normalize_token(primary_app, 3, 48);

    if let Ok(symbolic_action) =
        serde_json::from_str::<crate::symbolizer::SymbolicAction>(&action.action_data)
    {
        match symbolic_action {
            crate::symbolizer::SymbolicAction::VisitWebsite { domain, url, .. } => {
                let domain_hint = normalize_domain_hint(&domain);
                let path_hint = extract_url_path_hint(&url);
                return ActionSignature {
                    key: format!("VISIT_WEBSITE::{}::{}", domain_hint, path_hint),
                    label: format!("VISIT {} /{}", domain_hint, path_hint),
                };
            }
            crate::symbolizer::SymbolicAction::SearchWeb {
                domain, query, engine, ..
            } => {
                let domain_hint = normalize_domain_hint(&domain);
                let query_hint = normalize_token(&query, 5, 48);
                let engine_hint = engine
                    .as_deref()
                    .map(|value| normalize_token(value, 2, 24))
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "unknown".to_string());
                return ActionSignature {
                    key: format!(
                        "SEARCH_WEB::{}::{}::{}",
                        domain_hint, engine_hint, query_hint
                    ),
                    label: format!(
                        "SEARCH {} [{}] q:{}",
                        domain_hint, engine_hint, query_hint
                    ),
                };
            }
            crate::symbolizer::SymbolicAction::TypeText { field_type, .. } => {
                let field = format!("{:?}", field_type).to_uppercase();
                return ActionSignature {
                    key: format!("TYPE_TEXT::{}::{}", normalized_app, field),
                    label: format!("TYPE {} @ {}", field, normalized_app),
                };
            }
            crate::symbolizer::SymbolicAction::Interact {
                element_type,
                interaction,
                ..
            } => {
                let element = format!("{:?}", element_type).to_uppercase();
                let kind = format!("{:?}", interaction).to_uppercase();
                return ActionSignature {
                    key: format!(
                        "INTERACT::{}::{}::{}",
                        normalized_app, element, kind
                    ),
                    label: format!(
                        "INTERACT {} {} @ {}",
                        element, kind, normalized_app
                    ),
                };
            }
            _ => {}
        }
    }

    ActionSignature {
        key: format!("{}::{}", action.action_type, normalized_app),
        label: format!(
            "{} @ {}",
            action.action_type.replace('_', " "),
            normalized_app
        ),
    }
}

fn normalize_domain_hint(domain: &str) -> String {
    let trimmed = domain.trim().to_lowercase();
    if trimmed.is_empty() {
        return "unknown-domain".to_string();
    }
    let host = trimmed.trim_start_matches("www.");
    let normalized: String = host
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(*c, '.' | '-'))
        .take(64)
        .collect();
    if normalized.is_empty() {
        "unknown-domain".to_string()
    } else {
        normalized
    }
}

fn extract_url_path_hint(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return "root".to_string();
    }

    let without_scheme = if let Some((_, rest)) = trimmed.split_once("://") {
        rest
    } else {
        trimmed
    };
    let path = without_scheme
        .split_once('/')
        .map(|(_, rest)| rest)
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_matches('/');

    if path.is_empty() {
        return "root".to_string();
    }

    let segments = path
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .take(2)
        .map(|segment| normalize_token(segment, 2, 24))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    if segments.is_empty() {
        "root".to_string()
    } else {
        segments.join("/")
    }
}

fn normalize_token(value: &str, max_words: usize, max_chars: usize) -> String {
    let lowered = value.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();

    let mut token = cleaned
        .split_whitespace()
        .take(max_words.max(1))
        .collect::<Vec<_>>()
        .join("_");
    if token.is_empty() {
        token = "unknown".to_string();
    }
    token.chars().take(max_chars.max(1)).collect()
}

fn contains_contiguous_subsequence(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }

    haystack
        .windows(needle.len())
        .any(|window| window == needle)
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
    let mut element_id: Option<String> = None;
    let mut element_control_type: Option<String> = None;
    let mut element_automation_id: Option<String> = None;
    let mut element_class_name: Option<String> = None;
    let mut element_name_hash: Option<String> = None;
    let mut element_interaction: Option<String> = None;
    let mut element_field_type: Option<String> = None;
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
            crate::symbolizer::SymbolicAction::TypeText {
                field_type,
                selector,
                ..
            } => {
                element_field_type = Some(format!("{:?}", field_type));
                if let Some(sel) = selector {
                    element_id = Some(sel.element_id);
                    element_control_type = Some(sel.control_type);
                    element_automation_id = sel.automation_id;
                    element_class_name = sel.class_name;
                    element_name_hash = sel.name_hash;
                }
            }
            crate::symbolizer::SymbolicAction::Interact {
                interaction,
                selector,
                ..
            } => {
                element_interaction = Some(format!("{:?}", interaction));
                if let Some(sel) = selector {
                    element_id = Some(sel.element_id);
                    element_control_type = Some(sel.control_type);
                    element_automation_id = sel.automation_id;
                    element_class_name = sel.class_name;
                    element_name_hash = sel.name_hash;
                }
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
        element_id,
        element_control_type,
        element_automation_id,
        element_class_name,
        element_name_hash,
        element_interaction,
        element_field_type,
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
    use chrono::{DateTime, Utc};
    use crate::storage::StoredAction;
    use crate::symbolizer::{AppIdentifier, SymbolicAction};

    #[tokio::test]
    async fn test_health_check() {
        let result = health_check().await;
        assert!(result.contains("OK"));
    }

    #[test]
    fn test_build_repeated_task_bundles_detects_contextual_web_flow() {
        let browser = AppIdentifier::new("chrome.exe");
        let actions = vec![
            make_stored_action(
                1,
                SymbolicAction::OpenApp {
                    app: browser.clone(),
                },
            ),
            make_stored_action(
                2,
                SymbolicAction::SearchWeb {
                    browser_app: browser.clone(),
                    engine: Some("google".to_string()),
                    query: "lpu ums placement".to_string(),
                    url: "https://www.google.com/search?q=lpu+ums+placement".to_string(),
                    domain: "google.com".to_string(),
                },
            ),
            make_stored_action(
                3,
                SymbolicAction::VisitWebsite {
                    browser_app: browser.clone(),
                    url: "https://ums.lpu.in/home".to_string(),
                    domain: "ums.lpu.in".to_string(),
                },
            ),
            make_stored_action(
                4,
                SymbolicAction::VisitWebsite {
                    browser_app: browser.clone(),
                    url: "https://ums.lpu.in/placements/portal".to_string(),
                    domain: "ums.lpu.in".to_string(),
                },
            ),
            make_stored_action(
                10,
                SymbolicAction::OpenApp {
                    app: browser.clone(),
                },
            ),
            make_stored_action(
                11,
                SymbolicAction::SearchWeb {
                    browser_app: browser.clone(),
                    engine: Some("google".to_string()),
                    query: "lpu ums placement".to_string(),
                    url: "https://www.google.com/search?q=lpu+ums+placement".to_string(),
                    domain: "google.com".to_string(),
                },
            ),
            make_stored_action(
                12,
                SymbolicAction::VisitWebsite {
                    browser_app: browser.clone(),
                    url: "https://ums.lpu.in/home".to_string(),
                    domain: "ums.lpu.in".to_string(),
                },
            ),
            make_stored_action(
                13,
                SymbolicAction::VisitWebsite {
                    browser_app: browser.clone(),
                    url: "https://ums.lpu.in/placements/portal".to_string(),
                    domain: "ums.lpu.in".to_string(),
                },
            ),
        ];

        let bundles = build_repeated_task_bundles(&actions, 2, 10, 8);
        assert!(!bundles.is_empty());

        let best = &bundles[0];
        assert!(best.frequency >= 2);
        assert!(best.sequence_label.contains("SEARCH google.com"));
        assert!(best.sequence_label.contains("VISIT ums.lpu.in"));
        let sample_ids: Vec<i64> = best.sample_run.iter().map(|action| action.id).collect();
        assert_eq!(sample_ids, vec![10, 11, 12, 13]);
    }

    #[test]
    fn test_build_repeated_task_bundles_keeps_different_queries_separate() {
        let browser = AppIdentifier::new("chrome.exe");
        let actions = vec![
            make_stored_action(
                1,
                SymbolicAction::OpenApp {
                    app: browser.clone(),
                },
            ),
            make_stored_action(
                2,
                SymbolicAction::SearchWeb {
                    browser_app: browser.clone(),
                    engine: Some("google".to_string()),
                    query: "lpu ums".to_string(),
                    url: "https://www.google.com/search?q=lpu+ums".to_string(),
                    domain: "google.com".to_string(),
                },
            ),
            make_stored_action(
                3,
                SymbolicAction::VisitWebsite {
                    browser_app: browser.clone(),
                    url: "https://ums.lpu.in/placements/portal".to_string(),
                    domain: "ums.lpu.in".to_string(),
                },
            ),
            make_stored_action(
                4,
                SymbolicAction::OpenApp {
                    app: browser.clone(),
                },
            ),
            make_stored_action(
                5,
                SymbolicAction::SearchWeb {
                    browser_app: browser.clone(),
                    engine: Some("google".to_string()),
                    query: "lpu admission portal".to_string(),
                    url: "https://www.google.com/search?q=lpu+admission+portal".to_string(),
                    domain: "google.com".to_string(),
                },
            ),
            make_stored_action(
                6,
                SymbolicAction::VisitWebsite {
                    browser_app: browser.clone(),
                    url: "https://www.lpu.in/admissions".to_string(),
                    domain: "lpu.in".to_string(),
                },
            ),
        ];

        let bundles = build_repeated_task_bundles(&actions, 2, 10, 8);
        assert!(bundles.is_empty());
    }

    fn make_stored_action(id: i64, action: SymbolicAction) -> StoredAction {
        let timestamp_ms = 1_700_000_000_000_i64 + id;
        let timestamp =
            DateTime::<Utc>::from_timestamp_millis(timestamp_ms).expect("valid timestamp");
        let mut stored = StoredAction::from_symbolic(&action, timestamp, "test-session", None)
            .expect("serialize action");
        stored.id = id;
        stored
    }
}
