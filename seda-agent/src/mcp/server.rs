//! MCP HTTP server
//!
//! Provides a local HTTP server for the Model Context Protocol.
//!
//! # Security
//!
//! - Binds ONLY to localhost (127.0.0.1)
//! - No authentication (local-only assumption)
//! - All requests go through safety enforcement

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, Method, StatusCode},
    routing::{get, post},
    Json, Router,
};
use tokio::sync::Mutex as TokioMutex;
use tower_http::cors::{Any, CorsLayer};

use super::handlers::McpHandler;
use super::schema::{JsonRpcRequest, JsonRpcResponse};
use crate::observer::window_manager::WindowManager;
use crate::storage::Repository;

/// MCP server state
pub struct McpServer {
    handler: Arc<McpHandler>,
    port: u16,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(
        port: u16,
        repository: Arc<TokioMutex<Repository>>,
        window_manager: Arc<TokioMutex<WindowManager>>,
    ) -> Self {
        let handler = Arc::new(McpHandler::new(repository, window_manager));

        Self { handler, port }
    }

    /// Start the HTTP server
    ///
    /// # Safety
    ///
    /// The server binds ONLY to 127.0.0.1 (localhost).
    /// This ensures the MCP is only accessible from the local machine.
    pub async fn run(self) -> Result<(), std::io::Error> {
        let handler = self.handler.clone();

        // Build router
        let app = Router::new()
            .route("/", get(health_check))
            .route("/health", get(health_check))
            .route("/rpc", post(handle_rpc))
            .route("/mcp", post(handle_rpc))
            .with_state(handler)
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

/// Handle JSON-RPC requests
async fn handle_rpc(
    State(handler): State<Arc<McpHandler>>,
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
    let response = handler.handle_request(request).await;

    // Determine status code based on response
    let status = if response.error.is_some() {
        StatusCode::OK // JSON-RPC errors still return 200
    } else {
        StatusCode::OK
    };

    (status, Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let result = health_check().await;
        assert!(result.contains("OK"));
    }

    // Integration tests would go here
}
