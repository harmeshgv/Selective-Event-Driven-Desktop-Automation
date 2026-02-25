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
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex as TokioMutex;
use tower_http::cors::{Any, CorsLayer};

use super::handlers::McpHandler;
use super::schema::{ElementNode, JsonRpcRequest, JsonRpcResponse, WindowInfo};
use crate::config::Config;
use crate::control::{CollectionController, CollectorSnapshot};
use crate::llm::{
    LlmClient, RepairPlanRequest, RepairPlanResponse, RepairToolStep, TaskSummaryRequest,
    TaskSummaryResponse,
};
use crate::observer::window_manager::WindowManager;
use crate::storage::Repository;

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Clone)]
struct AppState {
    handler: Arc<McpHandler>,
    repository: Arc<TokioMutex<Repository>>,
    collector: Option<Arc<Mutex<CollectionController>>>,
    automation: Arc<TokioMutex<AutomationRuntime>>,
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

#[derive(Debug, Deserialize)]
struct AutomationCandidatesQuery {
    min_steps: Option<usize>,
    #[serde(alias = "min_frequency")]
    min_repeats: Option<usize>,
    limit: Option<usize>,
    flow_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct AutomationSuggestRequest {
    pattern_hash: String,
    #[serde(alias = "min_frequency")]
    min_repeats: Option<usize>,
    flow_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct AutomationApprovalRequest {
    pattern_hash: String,
    approved: bool,
}

#[derive(Debug, Deserialize)]
struct AutomationStartRequest {
    pattern_hash: String,
    confirm_start: bool,
    #[serde(default = "default_true")]
    allow_llm_repair: bool,
    #[serde(alias = "min_frequency")]
    min_repeats: Option<usize>,
    flow_limit: Option<usize>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
struct AutomationProviderStatus {
    provider: String,
    model: String,
    timeout_seconds: u64,
    enabled: bool,
    disabled_reason: Option<String>,
    min_steps_threshold: usize,
}

#[derive(Debug, Serialize)]
struct AutomationCandidate {
    pattern_hash: String,
    sequence_label: String,
    frequency: i64,
    avg_duration_ms: Option<i64>,
    last_seen_ms: i64,
    step_count: usize,
}

#[derive(Debug, Serialize)]
struct AutomationSuggestionData {
    pattern_hash: String,
    note: String,
    intent: String,
    noise_explanation: String,
    suggestions: Vec<String>,
    llm_plan_steps: Vec<String>,
    step_count: usize,
    provider: String,
    model: String,
    approved: bool,
}

#[derive(Debug, Serialize)]
struct AutomationApprovalData {
    pattern_hash: String,
    approved: bool,
    ready_to_start: bool,
}

#[derive(Debug, Serialize)]
struct AutomationExecutionLog {
    stage: String,
    success: bool,
    detail: String,
}

#[derive(Debug, Serialize)]
struct AutomationRunData {
    pattern_hash: String,
    completed: bool,
    deterministic_success: bool,
    llm_repair_used: bool,
    steps_attempted: usize,
    error: Option<String>,
    logs: Vec<AutomationExecutionLog>,
}

#[derive(Debug, Clone)]
struct AutomationProposal {
    note: String,
    approved: bool,
}

struct AutomationRuntime {
    min_steps_threshold: usize,
    llm: LlmClient,
    proposals: HashMap<String, AutomationProposal>,
}

#[derive(Debug, Serialize)]
struct DashboardAction {
    id: i64,
    action_type: String,
    node_id: String,
    source_app: Option<String>,
    target_app: Option<String>,
    element_type: Option<String>,
    element_id: Option<String>,
    element_control_type: Option<String>,
    element_automation_id: Option<String>,
    element_class_name: Option<String>,
    element_name_hash: Option<String>,
    element_is_keyboard_focusable: Option<bool>,
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
    automation_steps: Vec<AutomationStep>,
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

#[derive(Debug, Serialize)]
struct AutomationStep {
    step_id: String,
    action: String,
    node_id: String,
    timestamp_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_app: Option<String>,
    selector_bundle: AutomationSelectorBundle,
    ui_context: AutomationUiContext,
    precondition: AutomationPrecondition,
    action_args: AutomationActionArgs,
    wait_rule: AutomationWaitRule,
    postcondition: AutomationPostcondition,
    on_failure: AutomationFailurePolicy,
    variables: Vec<String>,
    safety: AutomationSafety,
}

#[derive(Debug, Serialize)]
struct AutomationSelectorBundle {
    #[serde(skip_serializing_if = "Option::is_none")]
    primary: Option<AutomationSelector>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fallbacks: Vec<AutomationSelector>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AutomationSelector {
    #[serde(skip_serializing_if = "Option::is_none")]
    element_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    control_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    automation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_keyboard_focusable: Option<bool>,
}

#[derive(Debug, Serialize)]
struct AutomationUiContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    element_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    control_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interaction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    automation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_button: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_input: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_keyboard_focusable: Option<bool>,
}

#[derive(Debug, Serialize)]
struct AutomationPrecondition {
    #[serde(skip_serializing_if = "Option::is_none")]
    app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url_domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requires_element: Option<bool>,
}

#[derive(Debug, Serialize)]
struct AutomationActionArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    interaction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    field_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    website_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_engine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_shortcut_hint: Option<String>,
}

#[derive(Debug, Serialize)]
struct AutomationWaitRule {
    timeout_ms: i64,
    poll_interval_ms: i64,
    retry: u32,
}

#[derive(Debug, Serialize)]
struct AutomationPostcondition {
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_reached: Option<String>,
}

#[derive(Debug, Serialize)]
struct AutomationFailurePolicy {
    strategy: String,
    retry_max: u32,
}

#[derive(Debug, Serialize)]
struct AutomationSafety {
    destructive: bool,
    requires_confirmation: bool,
}

/// MCP server state
pub struct McpServer {
    handler: Arc<McpHandler>,
    repository: Arc<TokioMutex<Repository>>,
    port: u16,
    collector: Option<Arc<Mutex<CollectionController>>>,
    automation: Arc<TokioMutex<AutomationRuntime>>,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(
        config: Config,
        repository: Arc<TokioMutex<Repository>>,
        window_manager: Arc<TokioMutex<WindowManager>>,
    ) -> Self {
        let port = config.mcp_port;
        let handler = Arc::new(McpHandler::new(repository.clone(), window_manager));
        let llm = match LlmClient::from_config(&config) {
            Ok(client) => client,
            Err(err) => {
                tracing::error!("Failed to initialize LLM client: {}", err);
                LlmClient::disabled(config.llm_timeout_seconds)
            }
        };
        let automation = Arc::new(TokioMutex::new(AutomationRuntime {
            min_steps_threshold: config.automation_min_steps.max(2),
            llm,
            proposals: HashMap::new(),
        }));

        Self {
            handler,
            repository,
            port,
            collector: None,
            automation,
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
            automation: self.automation.clone(),
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
            .route("/api/automation/provider", get(get_automation_provider))
            .route("/api/automation/candidates", get(get_automation_candidates))
            .route("/api/automation/suggest", post(generate_automation_suggestion))
            .route("/api/automation/approve", post(approve_automation_suggestion))
            .route("/api/automation/start", post(start_automation_run))
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
    let min_repeats = query.min_repeats.unwrap_or(3).clamp(2, 64);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let flow_limit = query.flow_limit.unwrap_or(5000).clamp(200, 20000);
    let repo = state.repository.lock().await;
    let repeated_tasks = match compute_repeated_tasks(&repo, min_repeats, limit, flow_limit) {
        Ok(tasks) => tasks,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: err,
                    data: None,
                }),
            );
        }
    };

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: "Repeated tasks loaded".to_string(),
            data: Some(repeated_tasks),
        }),
    )
}

fn compute_repeated_tasks(
    repo: &Repository,
    min_repeats: usize,
    limit: usize,
    flow_limit: usize,
) -> Result<Vec<DashboardRepeatedTask>, String> {
    const MAX_PATTERN_LENGTH_CAP: usize = 64;
    const MIN_REPEAT_OCCURRENCES: i64 = 2;

    let min_repeats = min_repeats.clamp(2, MAX_PATTERN_LENGTH_CAP);
    let limit = limit.clamp(1, 100);
    let flow_limit = flow_limit.clamp(200, 20000);
    let max_pattern_length = min_repeats.max(10).min(MAX_PATTERN_LENGTH_CAP);

    let flow_actions = repo
        .get_recent_actions_chronological(flow_limit)
        .map_err(|err| format!("Failed to load flow actions for repeated tasks: {}", err))?;

    Ok(build_repeated_task_bundles(
        &flow_actions,
        min_repeats,
        MIN_REPEAT_OCCURRENCES,
        limit,
        max_pattern_length,
    ))
}

async fn get_automation_provider(
    State(state): State<AppState>,
) -> (StatusCode, Json<ApiResponse<AutomationProviderStatus>>) {
    let runtime = state.automation.lock().await;
    let status = AutomationProviderStatus {
        provider: runtime.llm.provider().as_str().to_string(),
        model: runtime.llm.model().to_string(),
        timeout_seconds: runtime.llm.timeout_seconds(),
        enabled: runtime.llm.is_enabled(),
        disabled_reason: runtime.llm.disabled_reason(),
        min_steps_threshold: runtime.min_steps_threshold,
    };

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: "Automation provider status loaded".to_string(),
            data: Some(status),
        }),
    )
}

async fn get_automation_candidates(
    State(state): State<AppState>,
    Query(query): Query<AutomationCandidatesQuery>,
) -> (StatusCode, Json<ApiResponse<Vec<AutomationCandidate>>>) {
    let min_steps = {
        let runtime = state.automation.lock().await;
        query
            .min_steps
            .unwrap_or(runtime.min_steps_threshold)
            .clamp(2, 256)
    };
    let min_repeats = query.min_repeats.unwrap_or(3).clamp(2, 64);
    let limit = query.limit.unwrap_or(25).clamp(1, 100);
    let flow_limit = query.flow_limit.unwrap_or(5000).clamp(200, 20000);

    let repo = state.repository.lock().await;
    let repeated_tasks = match compute_repeated_tasks(&repo, min_repeats, limit, flow_limit) {
        Ok(tasks) => tasks,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: err,
                    data: None,
                }),
            );
        }
    };

    let candidates = repeated_tasks
        .into_iter()
        .filter(|task| task.automation_steps.len() >= min_steps)
        .map(|task| AutomationCandidate {
            pattern_hash: task.pattern_hash,
            sequence_label: task.sequence_label,
            frequency: task.frequency,
            avg_duration_ms: task.avg_duration_ms,
            last_seen_ms: task.last_seen_ms,
            step_count: task.automation_steps.len(),
        })
        .collect::<Vec<_>>();

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: "Automation candidates loaded".to_string(),
            data: Some(candidates),
        }),
    )
}

async fn generate_automation_suggestion(
    State(state): State<AppState>,
    Json(request): Json<AutomationSuggestRequest>,
) -> (StatusCode, Json<ApiResponse<AutomationSuggestionData>>) {
    let (llm, provider_name, model_name) = {
        let runtime = state.automation.lock().await;
        (
            runtime.llm.clone(),
            runtime.llm.provider().as_str().to_string(),
            runtime.llm.model().to_string(),
        )
    };

    let min_repeats = request.min_repeats.unwrap_or(3).clamp(2, 64);
    let flow_limit = request.flow_limit.unwrap_or(5000).clamp(200, 20000);
    let repeated_task = match load_repeated_task_by_hash(
        &state,
        &request.pattern_hash,
        min_repeats,
        flow_limit,
    )
    .await
    {
        Ok(task) => task,
        Err(err) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse {
                    success: false,
                    message: err,
                    data: None,
                }),
            );
        }
    };

    let step_count = repeated_task.automation_steps.len();

    let fallback_summary = build_fallback_summary(&repeated_task);
    let summary_request = TaskSummaryRequest {
        pattern_hash: repeated_task.pattern_hash.clone(),
        sequence_label: repeated_task.sequence_label.clone(),
        frequency: repeated_task.frequency,
        step_count,
        sample_apps: extract_sample_apps(&repeated_task),
        sample_domains: extract_sample_domains(&repeated_task),
        sample_queries: extract_sample_queries(&repeated_task),
        sample_steps: extract_sample_steps(&repeated_task),
    };

    let summary = if llm.is_enabled() {
        match llm.summarize_repeated_task(&summary_request).await {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!("LLM summary failed, using fallback note: {}", err);
                fallback_summary.clone()
            }
        }
    } else {
        fallback_summary.clone()
    };

    let note = if summary.summary.trim().is_empty() || is_low_level_explanation(&summary.summary) {
        fallback_summary.summary.clone()
    } else {
        truncate_note(&summary.summary, 420)
    };

    let intent = if summary.intent.trim().is_empty() || is_low_level_explanation(&summary.intent) {
        fallback_summary.intent.clone()
    } else {
        truncate_note(&summary.intent, 160)
    };

    let fallback_suggestions = build_fallback_suggestions(&repeated_task);
    let suggestions = if summary.suggestions.is_empty() {
        fallback_suggestions
    } else {
        let mut normalized = summary
            .suggestions
            .iter()
            .map(|value| truncate_note(value, 180))
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>();
        if normalized.len() < 3 {
            for fallback in build_fallback_suggestions(&repeated_task) {
                if normalized.iter().any(|item| item.eq_ignore_ascii_case(&fallback)) {
                    continue;
                }
                normalized.push(fallback);
                if normalized.len() >= 3 {
                    break;
                }
            }
        }
        normalized.into_iter().take(3).collect()
    };

    let noise_explanation = if summary.noise_explanation.trim().is_empty() {
        build_fallback_noise_explanation(&repeated_task)
    } else {
        truncate_note(&summary.noise_explanation, 260)
    };
    let llm_plan_steps = {
        let mut steps = summary
            .plan_steps
            .iter()
            .map(|value| truncate_note(value, 180))
            .filter(|value| !value.trim().is_empty())
            .filter(|value| !is_low_level_explanation(value))
            .collect::<Vec<_>>();
        if steps.len() < 3 {
            steps = build_fallback_llm_plan(&repeated_task);
        }
        steps.into_iter().take(40).collect::<Vec<_>>()
    };

    let data = AutomationSuggestionData {
        pattern_hash: repeated_task.pattern_hash.clone(),
        note,
        intent,
        noise_explanation,
        suggestions,
        llm_plan_steps,
        step_count,
        provider: provider_name,
        model: model_name,
        approved: false,
    };

    {
        let mut runtime = state.automation.lock().await;
        runtime.proposals.insert(
            repeated_task.pattern_hash.clone(),
            AutomationProposal {
                note: data.note.clone(),
                approved: false,
            },
        );
    }

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: "AI explanation ready".to_string(),
            data: Some(data),
        }),
    )
}

async fn approve_automation_suggestion(
    State(state): State<AppState>,
    Json(request): Json<AutomationApprovalRequest>,
) -> (StatusCode, Json<ApiResponse<AutomationApprovalData>>) {
    let mut runtime = state.automation.lock().await;
    let Some(proposal) = runtime.proposals.get_mut(&request.pattern_hash) else {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiResponse {
                success: false,
                message: "Suggestion not found. Generate an AI suggestion first.".to_string(),
                data: None,
            }),
        );
    };
    proposal.approved = request.approved;

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: if request.approved {
                "Suggestion approved. Ask for start confirmation before automation."
                    .to_string()
            } else {
                "Suggestion approval cleared.".to_string()
            },
            data: Some(AutomationApprovalData {
                pattern_hash: request.pattern_hash,
                approved: request.approved,
                ready_to_start: request.approved,
            }),
        }),
    )
}

async fn start_automation_run(
    State(state): State<AppState>,
    Json(request): Json<AutomationStartRequest>,
) -> (StatusCode, Json<ApiResponse<AutomationRunData>>) {
    if !request.confirm_start {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: "Start confirmation is required".to_string(),
                data: None,
            }),
        );
    }

    let (proposal, min_steps_threshold) = {
        let runtime = state.automation.lock().await;
        let Some(found) = runtime.proposals.get(&request.pattern_hash) else {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse {
                    success: false,
                    message: "No approved suggestion found. Generate and approve first."
                        .to_string(),
                    data: None,
                }),
            );
        };

        if !found.approved {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse {
                    success: false,
                    message: "Suggestion is not approved yet".to_string(),
                    data: None,
                }),
            );
        }

        (found.clone(), runtime.min_steps_threshold)
    };

    let min_repeats = request.min_repeats.unwrap_or(3).clamp(2, 64);
    let flow_limit = request.flow_limit.unwrap_or(5000).clamp(200, 20000);
    let repeated_task = match load_repeated_task_by_hash(
        &state,
        &request.pattern_hash,
        min_repeats,
        flow_limit,
    )
    .await
    {
        Ok(task) => task,
        Err(err) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse {
                    success: false,
                    message: err,
                    data: None,
                }),
            );
        }
    };

    let step_count = repeated_task.automation_steps.len();
    if step_count < min_steps_threshold {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: format!(
                    "Automation blocked: task has {} steps but minimum is {} steps",
                    step_count, min_steps_threshold
                ),
                data: None,
            }),
        );
    }

    let mut run = execute_deterministic_automation(&state.handler, &repeated_task).await;
    let mut llm_repair_used = false;
    let mut final_error = run.failure.as_ref().map(|f| f.reason.clone());

    if !run.completed && request.allow_llm_repair {
        let repair_result =
            execute_llm_repair_plan(&state, &proposal, &repeated_task, &run).await;
        match repair_result {
            Ok(mut repair_logs) => {
                llm_repair_used = true;
                run.logs.append(&mut repair_logs);
                run.completed = true;
                final_error = None;
            }
            Err(err) => {
                llm_repair_used = true;
                run.logs.push(AutomationExecutionLog {
                    stage: "llm_repair".to_string(),
                    success: false,
                    detail: err.clone(),
                });
                final_error = Some(err);
            }
        }
    }

    if run.completed {
        let repo = state.repository.lock().await;
        let _ = repo.accept_pattern(&request.pattern_hash);
    }

    let response_data = AutomationRunData {
        pattern_hash: request.pattern_hash,
        completed: run.completed,
        deterministic_success: run.failure.is_none(),
        llm_repair_used,
        steps_attempted: run.steps_attempted,
        error: final_error.clone(),
        logs: run.logs,
    };

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            message: if response_data.completed {
                "Automation run completed".to_string()
            } else {
                format!(
                    "Automation run ended with failure: {}",
                    final_error.unwrap_or_else(|| "unknown error".to_string())
                )
            },
            data: Some(response_data),
        }),
    )
}

async fn load_repeated_task_by_hash(
    state: &AppState,
    pattern_hash: &str,
    min_repeats: usize,
    flow_limit: usize,
) -> Result<DashboardRepeatedTask, String> {
    let repo = state.repository.lock().await;
    let tasks = compute_repeated_tasks(&repo, min_repeats, 100, flow_limit)?;
    tasks
        .into_iter()
        .find(|task| task.pattern_hash == pattern_hash)
        .ok_or_else(|| format!("Repeated task not found: {}", pattern_hash))
}

fn build_fallback_summary(task: &DashboardRepeatedTask) -> TaskSummaryResponse {
    let primary_app = extract_sample_apps(task).into_iter().next();
    let primary_domain = extract_sample_domains(task).into_iter().next();
    let primary_query = extract_sample_queries(task).into_iter().next();

    let flow_hint = match (primary_app.as_ref(), primary_domain.as_ref(), primary_query.as_ref()) {
        (Some(app), Some(domain), Some(query)) => format!(
            "open {}, go to {}, and search for '{}'",
            truncate_note(app, 32),
            truncate_note(domain, 48),
            truncate_note(query, 56)
        ),
        (Some(app), Some(domain), None) => format!(
            "open {} and work on {}",
            truncate_note(app, 32),
            truncate_note(domain, 48)
        ),
        (Some(app), None, Some(query)) => format!(
            "open {} and search for '{}'",
            truncate_note(app, 32),
            truncate_note(query, 56)
        ),
        (Some(app), None, None) => format!("repeat the same routine in {}", truncate_note(app, 40)),
        (None, Some(domain), _) => format!("repeat the same routine on {}", truncate_note(domain, 56)),
        _ => "repeat the same workflow".to_string(),
    };

    let summary = format!(
        "It looks like you usually {}. I can focus on this core routine even when unrelated actions happen in the middle. Should I automate this for you?",
        flow_hint
    );
    let intent = format!("Automate routine: {}", flow_hint);
    let noise_explanation = build_fallback_noise_explanation(task);
    let suggestions = build_fallback_suggestions(task);
    let plan_steps = build_fallback_llm_plan(task);
    TaskSummaryResponse {
        summary,
        intent,
        noise_explanation,
        suggestions,
        plan_steps,
    }
}

fn build_fallback_noise_explanation(task: &DashboardRepeatedTask) -> String {
    let apps = extract_sample_apps(task);
    if apps.is_empty() {
        return "Some actions in the middle may be unrelated, but the repeated pattern still indicates one core workflow.".to_string();
    }
    format!(
        "Some middle actions may be unrelated noise, but the repeated pattern still points to one main workflow across {}.",
        truncate_note(&apps.join(", "), 80)
    )
}

fn build_fallback_suggestions(task: &DashboardRepeatedTask) -> Vec<String> {
    let domain_hint = extract_sample_domains(task).into_iter().next();
    let app_hint = extract_sample_apps(task).into_iter().next();
    let query_hint = extract_sample_queries(task).into_iter().next();
    let sequence = truncate_note(&task.sequence_label, 120);

    let mut options = Vec::new();
    if let (Some(app), Some(domain), Some(query)) =
        (app_hint.as_ref(), domain_hint.as_ref(), query_hint.as_ref())
    {
        options.push(format!(
            "Likely goal: open {} and search '{}' on {}.",
            app,
            truncate_note(query, 48),
            domain
        ));
    }
    if let (Some(app), Some(domain)) = (app_hint.as_ref(), domain_hint.as_ref()) {
        options.push(format!(
            "Likely goal: open {} and complete work on {}.",
            app, domain
        ));
    }
    if let Some(app) = app_hint.as_ref() {
        options.push(format!(
            "Likely goal: use {} for the same repeated workflow each time.",
            app
        ));
    }
    options.push(format!(
        "Likely goal: repeat this sequence reliably: {}.",
        sequence
    ));
    options.push(
        "Likely goal: ignore unrelated middle clicks and automate only the stable core steps."
            .to_string(),
    );

    options
        .into_iter()
        .map(|value| truncate_note(&value, 180))
        .filter(|value| !value.trim().is_empty())
        .take(3)
        .collect()
}

fn build_fallback_llm_plan(task: &DashboardRepeatedTask) -> Vec<String> {
    let mut plan = extract_sample_steps(task)
        .into_iter()
        .map(|step| truncate_note(&step, 180))
        .filter(|step| !step.trim().is_empty())
        .filter(|step| !is_low_level_explanation(step))
        .collect::<Vec<_>>();

    if plan.is_empty() {
        let sequence_steps = task
            .sequence
            .iter()
            .enumerate()
            .map(|(index, action)| {
                format!(
                    "Step {}: {}",
                    index + 1,
                    action.to_ascii_lowercase().replace('_', " ")
                )
            })
            .collect::<Vec<_>>();
        plan.extend(sequence_steps);
    }

    if plan.is_empty() {
        plan.push("Open the required app or website.".to_string());
        plan.push("Complete the repeated routine with your usual inputs.".to_string());
        plan.push("Submit or finish the task and verify the result.".to_string());
    }

    plan
        .into_iter()
        .enumerate()
        .map(|(index, step)| {
            if step.to_ascii_lowercase().starts_with("step ") {
                step
            } else {
                format!("Step {}: {}", index + 1, step)
            }
        })
        .take(40)
        .collect()
}

fn extract_sample_apps(task: &DashboardRepeatedTask) -> Vec<String> {
    let mut apps = BTreeSet::new();
    for action in &task.sample_run {
        if let Some(app) = action.target_app.as_ref().or(action.source_app.as_ref()) {
            if !app.trim().is_empty() {
                apps.insert(app.clone());
            }
        }
    }
    apps.into_iter().take(8).collect()
}

fn extract_sample_domains(task: &DashboardRepeatedTask) -> Vec<String> {
    let mut domains = BTreeSet::new();
    for action in &task.sample_run {
        if let Some(domain) = action.website_domain.as_ref() {
            if !domain.trim().is_empty() {
                domains.insert(domain.clone());
            }
        }
    }
    domains.into_iter().take(8).collect()
}

fn extract_sample_queries(task: &DashboardRepeatedTask) -> Vec<String> {
    let mut queries = BTreeSet::new();
    for action in &task.sample_run {
        if let Some(query) = action.search_query.as_ref() {
            let normalized = query.trim();
            if !normalized.is_empty() {
                queries.insert(normalized.to_string());
            }
        }
    }
    queries.into_iter().take(8).collect()
}

fn extract_sample_steps(task: &DashboardRepeatedTask) -> Vec<String> {
    task.sample_run
        .iter()
        .take(20)
        .map(action_to_human_step)
        .filter(|value| !value.trim().is_empty())
        .collect()
}

fn action_to_human_step(action: &DashboardAction) -> String {
    let target_app = action
        .target_app
        .as_deref()
        .or(action.source_app.as_deref())
        .unwrap_or("the app");

    match action.action_type.as_str() {
        "OPEN_APP" => format!("open {}", target_app),
        "CLOSE_APP" => format!("close {}", target_app),
        "SWITCH_APP" => String::new(),
        "SEARCH_WEB" => {
            let query = action
                .search_query
                .as_deref()
                .map(|value| truncate_note(value, 48))
                .unwrap_or_else(|| "a query".to_string());
            let domain = action
                .website_domain
                .as_deref()
                .unwrap_or("a website");
            format!("search '{}' on {}", query, domain)
        }
        "VISIT_WEBSITE" => {
            let domain = action
                .website_domain
                .as_deref()
                .or(action.website_url.as_deref())
                .unwrap_or("a website");
            format!("visit {}", truncate_note(domain, 64))
        }
        "TYPE_TEXT" => {
            if let Some(domain) = action.website_domain.as_deref() {
                format!("fill details on {}", truncate_note(domain, 56))
            } else {
                format!("enter text in {}", target_app)
            }
        }
        "INTERACT" => {
            if let Some(domain) = action.website_domain.as_deref() {
                format!("continue workflow on {}", truncate_note(domain, 56))
            } else {
                String::new()
            }
        }
        "COPY_TEXT" => format!("copy text from {}", target_app),
        "PASTE_TEXT" => format!("paste text into {}", target_app),
        _ => format!(
            "{} in {}",
            action.action_type.to_ascii_lowercase().replace('_', " "),
            target_app
        ),
    }
}

fn is_low_level_explanation(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    const MARKERS: [&str; 15] = [
        "switch_app",
        "open_app",
        "visit_website",
        "search_web",
        "type_text",
        "copy_text",
        "paste_text",
        "interact",
        "node_id",
        "selector",
        "control_type",
        "field type",
        "automation step",
        "hwnd",
        "window tree",
    ];
    MARKERS.iter().any(|marker| lower.contains(marker))
}

fn truncate_note(value: &str, max_chars: usize) -> String {
    let max = max_chars.max(24);
    let mut text = value.trim().to_string();
    if text.chars().count() > max {
        text = text.chars().take(max).collect::<String>();
        text.push_str("...");
    }
    text
}

#[derive(Debug)]
struct AutomationFailure {
    step_id: Option<String>,
    action: String,
    reason: String,
    target_app: Option<String>,
    selector_hint: serde_json::Value,
    variables: HashMap<String, serde_json::Value>,
}

#[derive(Debug)]
struct DeterministicRun {
    completed: bool,
    steps_attempted: usize,
    logs: Vec<AutomationExecutionLog>,
    failure: Option<AutomationFailure>,
}

async fn execute_deterministic_automation(
    handler: &Arc<McpHandler>,
    task: &DashboardRepeatedTask,
) -> DeterministicRun {
    let mut logs = Vec::new();
    let mut variables: HashMap<String, serde_json::Value> = HashMap::new();
    let mut steps_attempted = 0usize;

    for step in &task.automation_steps {
        steps_attempted += 1;

        if let Some(app_hint) = step
            .precondition
            .app
            .as_deref()
            .or(step.target_app.as_deref())
            .or(step.source_app.as_deref())
        {
            if !variables.contains_key("TARGET_HWND") {
                if let Ok(Some(hwnd)) = find_target_hwnd_for_app(handler, app_hint).await {
                    variables.insert("TARGET_HWND".to_string(), json!(hwnd));
                }
            }
        }

        let step_result = execute_step_deterministic(handler, step, &mut variables).await;
        match step_result {
            Ok(detail) => logs.push(AutomationExecutionLog {
                stage: format!("deterministic:{}", step.step_id),
                success: true,
                detail,
            }),
            Err(err) => {
                logs.push(AutomationExecutionLog {
                    stage: format!("deterministic:{}", step.step_id),
                    success: false,
                    detail: err.clone(),
                });
                return DeterministicRun {
                    completed: false,
                    steps_attempted,
                    logs,
                    failure: Some(AutomationFailure {
                        step_id: Some(step.step_id.clone()),
                        action: step.action.clone(),
                        reason: err,
                        target_app: step
                            .precondition
                            .app
                            .clone()
                            .or_else(|| step.target_app.clone())
                            .or_else(|| step.source_app.clone()),
                        selector_hint: json!({
                            "primary": step.selector_bundle.primary,
                            "fallbacks": step.selector_bundle.fallbacks,
                        }),
                        variables,
                    }),
                };
            }
        }
    }

    DeterministicRun {
        completed: true,
        steps_attempted,
        logs,
        failure: None,
    }
}

async fn execute_step_deterministic(
    handler: &Arc<McpHandler>,
    step: &AutomationStep,
    variables: &mut HashMap<String, serde_json::Value>,
) -> Result<String, String> {
    match step.action.as_str() {
        "COPY_TEXT" => {
            call_mcp_tool(
                handler,
                "press_key",
                json!({"key":"C","modifiers":["Ctrl"]}),
            )
            .await?;
            Ok("Pressed Ctrl+C".to_string())
        }
        "PASTE_TEXT" => {
            call_mcp_tool(
                handler,
                "press_key",
                json!({"key":"V","modifiers":["Ctrl"]}),
            )
            .await?;
            Ok("Pressed Ctrl+V".to_string())
        }
        "TYPE_TEXT" => {
            if step
                .action_args
                .key_shortcut_hint
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case("Enter"))
                .unwrap_or(false)
            {
                call_mcp_tool(handler, "press_key", json!({"key":"Enter"})).await?;
                Ok("Pressed Enter for search submit".to_string())
            } else {
                Ok("No deterministic type-text action needed".to_string())
            }
        }
        "INTERACT" => execute_interact_step(handler, step, variables).await,
        "CLOSE_APP" => {
            call_mcp_tool(
                handler,
                "press_key",
                json!({"key":"F4","modifiers":["Alt"]}),
            )
            .await?;
            Ok("Pressed Alt+F4 for CLOSE_APP".to_string())
        }
        unsupported => Err(format!(
            "Unsupported deterministic action for MCP tools: {}",
            unsupported
        )),
    }
}

async fn execute_interact_step(
    handler: &Arc<McpHandler>,
    step: &AutomationStep,
    variables: &mut HashMap<String, serde_json::Value>,
) -> Result<String, String> {
    let app_hint = step
        .precondition
        .app
        .as_deref()
        .or(step.target_app.as_deref())
        .or(step.source_app.as_deref());

    let hwnd = if let Some(hwnd) = variables
        .get("TARGET_HWND")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
    {
        hwnd
    } else if let Some(app) = app_hint {
        match find_target_hwnd_for_app(handler, app).await? {
            Some(found) => {
                variables.insert("TARGET_HWND".to_string(), json!(found.clone()));
                found
            }
            None => return Err(format!("No window found for target app: {}", app)),
        }
    } else {
        return Err("No target app or hwnd available for INTERACT step".to_string());
    };

    let tree_value = call_mcp_tool(
        handler,
        "get_window_tree",
        json!({"hwnd": hwnd, "max_depth": 6}),
    )
    .await?;
    let tree: ElementNode = serde_json::from_value(tree_value)
        .map_err(|err| format!("Failed to parse window tree: {}", err))?;

    let element_id = if let Some(primary) = step.selector_bundle.primary.as_ref() {
        primary.element_id.clone().or_else(|| find_matching_element_id(&tree, primary))
    } else {
        None
    }
    .or_else(|| {
        for selector in &step.selector_bundle.fallbacks {
            if let Some(found) = find_matching_element_id(&tree, selector) {
                return Some(found);
            }
        }
        None
    })
    .ok_or_else(|| "No matching element found for INTERACT step selectors".to_string())?;

    variables.insert("TARGET_ELEMENT_ID".to_string(), json!(element_id.clone()));
    let element_action =
        map_interaction_to_element_action(step.action_args.interaction.as_deref());
    call_mcp_tool(
        handler,
        "activate_element",
        json!({
            "hwnd": hwnd,
            "element_id": element_id,
            "action": element_action,
        }),
    )
    .await?;

    Ok(format!(
        "Activated element {} with {}",
        variables
            .get("TARGET_ELEMENT_ID")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown"),
        element_action
    ))
}

async fn find_target_hwnd_for_app(
    handler: &Arc<McpHandler>,
    app_name: &str,
) -> Result<Option<String>, String> {
    let windows_value =
        call_mcp_tool(handler, "list_windows", json!({"include_hidden": false})).await?;
    let windows: Vec<WindowInfo> = serde_json::from_value(windows_value)
        .map_err(|err| format!("Failed to parse list_windows response: {}", err))?;

    let app_lower = app_name.to_ascii_lowercase();
    let focused = windows
        .iter()
        .find(|window| window.process_name.eq_ignore_ascii_case(&app_lower) && window.is_focused)
        .map(|window| window.hwnd.clone());
    if focused.is_some() {
        return Ok(focused);
    }

    Ok(windows
        .iter()
        .find(|window| window.process_name.eq_ignore_ascii_case(&app_lower))
        .map(|window| window.hwnd.clone()))
}

fn find_matching_element_id(root: &ElementNode, selector: &AutomationSelector) -> Option<String> {
    if let Some(id) = selector.element_id.as_ref() {
        return Some(id.clone());
    }
    if selector_matches(root, selector) {
        return Some(root.element_id.clone());
    }

    for child in &root.children {
        if let Some(found) = find_matching_element_id(child, selector) {
            return Some(found);
        }
    }
    None
}

fn selector_matches(node: &ElementNode, selector: &AutomationSelector) -> bool {
    let mut matchable = 0usize;

    if let Some(control_type) = selector.control_type.as_ref() {
        matchable += 1;
        if !node.control_type.eq_ignore_ascii_case(control_type) {
            return false;
        }
    }
    if let Some(name_hash) = selector.name_hash.as_ref() {
        matchable += 1;
        if node.name_hash.as_deref() != Some(name_hash.as_str()) {
            return false;
        }
    }
    if let Some(is_focusable) = selector.is_keyboard_focusable {
        matchable += 1;
        if node.is_keyboard_focusable != is_focusable {
            return false;
        }
    }

    matchable > 0
}

fn map_interaction_to_element_action(interaction: Option<&str>) -> &'static str {
    let normalized = interaction
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "click".to_string());

    if normalized.contains("focus") {
        "Focus"
    } else if normalized.contains("expand") {
        "Expand"
    } else if normalized.contains("collapse") {
        "Collapse"
    } else if normalized.contains("select") {
        "Select"
    } else {
        "Click"
    }
}

async fn execute_llm_repair_plan(
    state: &AppState,
    proposal: &AutomationProposal,
    task: &DashboardRepeatedTask,
    run: &DeterministicRun,
) -> Result<Vec<AutomationExecutionLog>, String> {
    let failure = run
        .failure
        .as_ref()
        .ok_or_else(|| "Cannot run LLM repair without deterministic failure context".to_string())?;
    let llm = {
        let runtime = state.automation.lock().await;
        runtime.llm.clone()
    };

    if !llm.is_enabled() {
        return Err(
            llm.disabled_reason()
                .unwrap_or_else(|| "LLM provider is not available".to_string()),
        );
    }

    let variables_json = serde_json::Value::Object(
        failure
            .variables
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    );
    let repair_request = RepairPlanRequest {
        pattern_hash: task.pattern_hash.clone(),
        failed_step_id: failure.step_id.clone(),
        failed_action: failure.action.clone(),
        failure_reason: failure.reason.clone(),
        target_app: failure.target_app.clone(),
        selector_hint: failure.selector_hint.clone(),
        variables: variables_json,
    };

    let plan: RepairPlanResponse = llm
        .create_repair_plan(&repair_request)
        .await
        .map_err(|err| format!("LLM repair request failed: {}", err))?;

    if plan.steps.is_empty() {
        return Err(format!(
            "LLM did not return actionable repair steps. Note: {}",
            proposal.note
        ));
    }

    let mut variables = failure.variables.clone();
    let mut logs = Vec::new();
    for (index, step) in plan.steps.iter().take(12).enumerate() {
        let step_name = format!("llm:{}:{}", index + 1, step.tool);
        let repair_outcome =
            execute_llm_repair_step(&state.handler, step, failure, &mut variables).await;
        match repair_outcome {
            Ok(detail) => logs.push(AutomationExecutionLog {
                stage: step_name,
                success: true,
                detail,
            }),
            Err(err) => {
                logs.push(AutomationExecutionLog {
                    stage: step_name,
                    success: false,
                    detail: err.clone(),
                });
                return Err(err);
            }
        }
    }

    Ok(logs)
}

async fn execute_llm_repair_step(
    handler: &Arc<McpHandler>,
    step: &RepairToolStep,
    failure: &AutomationFailure,
    variables: &mut HashMap<String, serde_json::Value>,
) -> Result<String, String> {
    if !super::tools::is_allowed_tool(&step.tool) {
        return Err(format!("LLM requested forbidden tool: {}", step.tool));
    }

    let mut params = resolve_placeholders(&step.params, variables);
    if !params.is_object() {
        params = json!({});
    }
    if let Some(params_obj) = params.as_object_mut() {
        if step.tool == "activate_element" {
            if !params_obj.contains_key("hwnd") {
                if let Some(hwnd) = variables.get("TARGET_HWND") {
                    params_obj.insert("hwnd".to_string(), hwnd.clone());
                }
            }
            if !params_obj.contains_key("element_id") {
                if let Some(element_id) = variables.get("TARGET_ELEMENT_ID") {
                    params_obj.insert("element_id".to_string(), element_id.clone());
                }
            }
            if !params_obj.contains_key("action") {
                params_obj.insert("action".to_string(), json!("Click"));
            }
        }
    }

    let result = call_mcp_tool(handler, &step.tool, params).await?;
    if let Some(save_as) = step.save_as.as_ref() {
        if !save_as.trim().is_empty() {
            variables.insert(save_as.clone(), result.clone());
        }
    }

    if step.tool == "list_windows" && !variables.contains_key("TARGET_HWND") {
        if let Some(app) = failure.target_app.as_deref() {
            if let Ok(windows) = serde_json::from_value::<Vec<WindowInfo>>(result.clone()) {
                if let Some(hwnd) = windows
                    .into_iter()
                    .find(|window| window.process_name.eq_ignore_ascii_case(app))
                    .map(|window| window.hwnd)
                {
                    variables.insert("TARGET_HWND".to_string(), json!(hwnd));
                }
            }
        }
    }

    if step.tool == "get_window_tree" && !variables.contains_key("TARGET_ELEMENT_ID") {
        let selectors = selectors_from_hint(&failure.selector_hint);
        if !selectors.is_empty() {
            if let Ok(tree) = serde_json::from_value::<ElementNode>(result.clone()) {
                for selector in selectors {
                    if let Some(element_id) = find_matching_element_id(&tree, &selector) {
                        variables.insert("TARGET_ELEMENT_ID".to_string(), json!(element_id));
                        break;
                    }
                }
            }
        }
    }

    Ok(step
        .note
        .clone()
        .unwrap_or_else(|| format!("Executed {}", step.tool)))
}

fn resolve_placeholders(
    value: &serde_json::Value,
    variables: &HashMap<String, serde_json::Value>,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => {
            if let Some(key) = text.strip_prefix('$') {
                variables
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| serde_json::Value::String(text.clone()))
            } else {
                serde_json::Value::String(text.clone())
            }
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| resolve_placeholders(item, variables))
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), resolve_placeholders(item, variables)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn selectors_from_hint(hint: &serde_json::Value) -> Vec<AutomationSelector> {
    let mut selectors = Vec::new();

    if let Some(primary) = hint.get("primary") {
        if !primary.is_null() {
            if let Ok(selector) = serde_json::from_value::<AutomationSelector>(primary.clone()) {
                selectors.push(selector);
            }
        }
    }
    if let Some(fallbacks) = hint.get("fallbacks").and_then(|value| value.as_array()) {
        for fallback in fallbacks {
            if let Ok(selector) = serde_json::from_value::<AutomationSelector>(fallback.clone()) {
                selectors.push(selector);
            }
        }
    }

    selectors
}

async fn call_mcp_tool(
    handler: &Arc<McpHandler>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: method.to_string(),
        params,
    };

    let response = handler.handle_request(request).await;
    if let Some(error) = response.error {
        return Err(format!("{} ({})", error.message, error.code));
    }
    Ok(response.result.unwrap_or(serde_json::Value::Null))
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
            let automation_steps = build_automation_steps(&run);

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
                automation_steps,
            }
        })
        .collect()
}

fn build_automation_steps(run: &[DashboardAction]) -> Vec<AutomationStep> {
    run.iter()
        .enumerate()
        .map(|(index, action)| {
            let selector_primary = selector_from_dashboard_action(action);
            let selector_fallbacks = selector_fallbacks_from_dashboard_action(action);
            let has_selector = selector_primary.is_some() || !selector_fallbacks.is_empty();
            let expected_app = action
                .target_app
                .clone()
                .or_else(|| action.source_app.clone());
            let destructive = matches!(action.action_type.as_str(), "CLOSE_APP");
            let control_type_lower = action
                .element_control_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase());
            let element_type_lower = action
                .element_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase());
            let is_button = element_type_lower
                .as_deref()
                .map(|value| value.contains("button"))
                .or_else(|| {
                    control_type_lower
                        .as_deref()
                        .map(|value| value.contains("button"))
                });
            let is_input = action
                .element_field_type
                .as_deref()
                .map(|_| true)
                .or_else(|| {
                    element_type_lower
                        .as_deref()
                        .map(|value| value.contains("textfield"))
                })
                .or_else(|| {
                    control_type_lower.as_deref().map(|value| {
                        value.contains("edit")
                            || value.contains("text")
                            || value.contains("document")
                    })
                });

            AutomationStep {
                step_id: format!("s{}", index + 1),
                action: action.action_type.clone(),
                node_id: action.node_id.clone(),
                timestamp_ms: action.timestamp_ms,
                source_app: action.source_app.clone(),
                target_app: action.target_app.clone(),
                selector_bundle: AutomationSelectorBundle {
                    primary: selector_primary,
                    fallbacks: selector_fallbacks,
                },
                ui_context: AutomationUiContext {
                    element_type: action.element_type.clone(),
                    control_type: action.element_control_type.clone(),
                    interaction: action.element_interaction.clone(),
                    class_name: action.element_class_name.clone(),
                    automation_id: action.element_automation_id.clone(),
                    name_hash: action.element_name_hash.clone(),
                    is_button,
                    is_input,
                    is_keyboard_focusable: action.element_is_keyboard_focusable,
                },
                precondition: AutomationPrecondition {
                    app: expected_app.clone(),
                    url_domain: action.website_domain.clone(),
                    requires_element: has_selector.then_some(true),
                },
                action_args: AutomationActionArgs {
                    interaction: action.element_interaction.clone(),
                    field_type: action.element_field_type.clone(),
                    website_url: action.website_url.clone(),
                    search_query: action.search_query.clone(),
                    search_engine: action.search_engine.clone(),
                    duration_ms: action.duration_ms,
                    key_shortcut_hint: keyboard_shortcut_hint_for_action(
                        &action.action_type,
                        action.element_field_type.as_deref(),
                    ),
                },
                wait_rule: AutomationWaitRule {
                    timeout_ms: 5_000,
                    poll_interval_ms: 200,
                    retry: 2,
                },
                postcondition: AutomationPostcondition {
                    expected_app,
                    expected_domain: action.website_domain.clone(),
                    node_reached: Some(action.node_id.clone()),
                },
                on_failure: AutomationFailurePolicy {
                    strategy: "retry_then_abort".to_string(),
                    retry_max: 2,
                },
                variables: Vec::new(),
                safety: AutomationSafety {
                    destructive,
                    requires_confirmation: destructive,
                },
            }
        })
        .collect()
}

fn selector_from_dashboard_action(action: &DashboardAction) -> Option<AutomationSelector> {
    if action.element_id.is_none()
        && action.element_control_type.is_none()
        && action.element_automation_id.is_none()
        && action.element_class_name.is_none()
        && action.element_name_hash.is_none()
        && action.element_is_keyboard_focusable.is_none()
    {
        return None;
    }

    Some(AutomationSelector {
        element_id: action.element_id.clone(),
        control_type: action.element_control_type.clone(),
        automation_id: action.element_automation_id.clone(),
        class_name: action.element_class_name.clone(),
        name_hash: action.element_name_hash.clone(),
        is_keyboard_focusable: action.element_is_keyboard_focusable,
    })
}

fn selector_fallbacks_from_dashboard_action(action: &DashboardAction) -> Vec<AutomationSelector> {
    let mut fallbacks = Vec::new();

    if action.element_control_type.is_some() || action.element_class_name.is_some() {
        fallbacks.push(AutomationSelector {
            element_id: None,
            control_type: action.element_control_type.clone(),
            automation_id: None,
            class_name: action.element_class_name.clone(),
            name_hash: None,
            is_keyboard_focusable: action.element_is_keyboard_focusable,
        });
    }

    if action.element_name_hash.is_some() {
        fallbacks.push(AutomationSelector {
            element_id: None,
            control_type: action.element_control_type.clone(),
            automation_id: None,
            class_name: None,
            name_hash: action.element_name_hash.clone(),
            is_keyboard_focusable: action.element_is_keyboard_focusable,
        });
    }

    fallbacks
}

fn keyboard_shortcut_hint_for_action(
    action_type: &str,
    field_type: Option<&str>,
) -> Option<String> {
    match action_type {
        "COPY_TEXT" => Some("Ctrl+C".to_string()),
        "PASTE_TEXT" => Some("Ctrl+V".to_string()),
        "TYPE_TEXT" => {
            if field_type
                .map(|value| value.eq_ignore_ascii_case("Search"))
                .unwrap_or(false)
            {
                Some("Enter".to_string())
            } else {
                None
            }
        }
        _ => None,
    }
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
    let mut element_type: Option<String> = None;
    let mut element_id: Option<String> = None;
    let mut element_control_type: Option<String> = None;
    let mut element_automation_id: Option<String> = None;
    let mut element_class_name: Option<String> = None;
    let mut element_name_hash: Option<String> = None;
    let mut element_is_keyboard_focusable: Option<bool> = None;
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
                element_type = Some("TextField".to_string());
                if let Some(sel) = selector {
                    element_id = Some(sel.element_id);
                    element_control_type = Some(sel.control_type);
                    element_automation_id = sel.automation_id;
                    element_class_name = sel.class_name;
                    element_name_hash = sel.name_hash;
                    element_is_keyboard_focusable = Some(sel.is_keyboard_focusable);
                }
            }
            crate::symbolizer::SymbolicAction::Interact {
                element_type: interaction_element_type,
                interaction,
                selector,
                ..
            } => {
                element_type = Some(format!("{:?}", interaction_element_type));
                element_interaction = Some(format!("{:?}", interaction));
                if let Some(sel) = selector {
                    element_id = Some(sel.element_id);
                    element_control_type = Some(sel.control_type);
                    element_automation_id = sel.automation_id;
                    element_class_name = sel.class_name;
                    element_name_hash = sel.name_hash;
                    element_is_keyboard_focusable = Some(sel.is_keyboard_focusable);
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
        element_type,
        element_id,
        element_control_type,
        element_automation_id,
        element_class_name,
        element_name_hash,
        element_is_keyboard_focusable,
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

        let bundles = build_repeated_task_bundles(&actions, 3, 2, 10, 8);
        assert!(!bundles.is_empty());

        let best = &bundles[0];
        assert!(best.frequency >= 2);
        assert!(best.sequence_label.contains("SEARCH google.com"));
        assert!(best.sequence_label.contains("VISIT ums.lpu.in"));
        let sample_ids: Vec<i64> = best.sample_run.iter().map(|action| action.id).collect();
        assert_eq!(sample_ids, vec![10, 11, 12, 13]);
        assert_eq!(best.automation_steps.len(), best.sample_run.len());
        assert_eq!(best.automation_steps[0].step_id, "s1");
        assert_eq!(best.automation_steps[0].action, "OPEN_APP");
        assert_eq!(best.automation_steps[0].wait_rule.timeout_ms, 5_000);
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

        let bundles = build_repeated_task_bundles(&actions, 3, 2, 10, 8);
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
