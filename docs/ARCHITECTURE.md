# Architecture

This document describes the runtime data flow in SEDA.

## High-Level Pipeline

1. OS observers emit raw events.
2. Symbolizer converts raw events into symbolic actions.
3. Storage writes actions and transition edges to SQLite.
4. Mining and graph components update derived patterns and task graph state.
5. MCP server and dashboard expose query and control APIs.

## Runtime Components

- `observer`: Captures window, focus, clipboard, keyboard shortcut, and browser URL activity.
- `control`: Owns observer lifecycle and session state, and provides `start_collection`, `stop_collection`, and `clear_collected_data`.
- `symbolizer`: Defines `SymbolicAction`, transforms raw events to privacy-aware actions, and performs URL/query parsing.
- `storage`: Stores symbolic actions, transitions, patterns, sessions, and audit entries in SQLite.
- `graph`: Builds the in-memory task transition graph from the action stream.
- `mining`: Performs sequence mining for repeated action patterns.
- `mcp`: Exposes JSON-RPC tools, dashboard REST endpoints, and serves `dashboard.html`.

## Collection Lifecycle

Collection is controlled through `CollectionController`.

- Startup: controller is created in idle mode.
- Start: initializes all observers and opens a pending collection session.
- Stop: stops observers, closes session, and records session summary.
- Clear: clears collected runtime data and resets dashboard session summaries.

## Browser Capture Flow

1. `BrowserUrlObserver` polls foreground browser windows.
2. `extract_browser_url_for_window` reads URL-like value via UI Automation.
3. Raw event `BrowserNavigation` is emitted.
4. Transformer emits `VISIT_WEBSITE` if URL has no recognized query.
5. Transformer emits `SEARCH_WEB` when search query parameters are detected.
6. Dashboard exposes website/search fields in recent actions and flow replay.

## API Surfaces

### Dashboard REST

- `GET /api/dashboard/status`
- `POST /api/dashboard/start`
- `POST /api/dashboard/stop`
- `POST /api/dashboard/clear`
- `GET /api/dashboard/actions`
- `GET /api/dashboard/flow`
- `GET /api/dashboard/graph`

### MCP JSON-RPC

- `list_windows`
- `get_window_tree`
- `get_patterns`
- `get_transitions`
- `activate_element`
- `press_key`
- `set_clipboard`
- `tools/list`

## Storage Summary

Key tables (initial migration):

- `symbolic_actions`
- `action_transitions`
- `detected_patterns`
- `audit_log`
- `sessions`

## Security and Privacy Notes

- HTTP server binds to localhost.
- Automation methods go through safety validation.
- Clipboard content payload is not persisted.
- Browser URL/search capture is intentionally enabled and should be treated as sensitive data.
