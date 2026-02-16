# Selective-Event-Driven-Desktop-Automation (SEDA)

Local-first desktop event collection and task-flow mining for Windows.

SEDA captures high-level OS events, transforms them into symbolic actions, stores them in SQLite, and exposes both a JSON-RPC MCP interface and a built-in dashboard for live control and visualization.

## Highlights

- Manual collection control from the dashboard (`Start Session`, `Stop Session`, `Clear Collected Data`).
- 3D directed task transition graph with rotate, pan, zoom, replay, and fullscreen.
- Repeated-task bundle detection with one-click run inspection in replay mode.
  Bundles are context-aware (app + web domain/query/path), so larger flows can be grouped as one task.
- Session timeline and recent action table.
- Browser activity capture for visited URLs and recognized search queries.
- Local-only HTTP server (`127.0.0.1`) and local SQLite storage.

## Current Behavior

- Running `cargo run` starts the backend and dashboard server.
- Data collection is idle by default.
- Collection starts only when `Start Session` is clicked in the dashboard.
- Stopping collection records a completed session summary.
- Clear removes collected actions, transitions, patterns, and dashboard session history.

## Repository Layout

- `seda-agent/` Rust application.
- `seda-agent/src/observer/` Windows event capture.
- `seda-agent/src/symbolizer/` Raw event to symbolic action transformation.
- `seda-agent/src/storage/` SQLite repository and schema types.
- `seda-agent/src/mcp/` MCP handlers, HTTP server, and dashboard UI.
- `seda-agent/src/control/` Collection lifecycle controller (start/stop/clear session handling).

## Requirements

- Windows 10/11.
- Rust stable (MSVC toolchain).
- Local browser for dashboard (`http://127.0.0.1:9315/dashboard` by default).

## Quick Start

```powershell
cd .\seda-agent
cargo build
cargo run
```

Default endpoints:

- Health: `http://127.0.0.1:9315/health`
- JSON-RPC: `http://127.0.0.1:9315/rpc`
- Dashboard: `http://127.0.0.1:9315/dashboard`

## Dashboard Guide

1. Open `/dashboard`.
2. Click `Start Session` to begin collecting.
3. Perform desktop activity.
4. Use `Stop Session` to end and finalize a session.
5. Use `Clear Collected Data` to reset collected runtime data.
6. Open `Repeated Tasks` to inspect bundles of flows you perform again and again.

### Graph Controls

- `2D Flowchart` / `3D Graph`: switch between readable 2D flow and interactive 3D view.
- In 3D mode: drag to rotate.
- In 3D mode: Shift + drag to pan.
- In 3D mode: mouse wheel to zoom.
- In 3D mode: double-click to reset camera.
- `Start From First`: jump replay to first collected action.
- `Next`: step through captured flow order.
- `Auto Play`: play replay sequence.
- `Fullscreen`: expand graph panel.
- `Repeated Tasks > Inspect Run`: load a representative linear run of a repeated bundle into replay.

## Data Captured

SEDA stores symbolic actions such as:

- App focus/open/close transitions.
- Clipboard metadata events (type only, not content).
- Shortcut-driven paste interactions.
- Browser navigation events as `VISIT_WEBSITE` (URL and domain).
- Browser search events as `SEARCH_WEB` (URL, domain, query, and inferred engine).
- UI element interactions as `INTERACT` with selector metadata (element id/control type/automation id/class/name hash when available).
- Text-input updates as `TYPE_TEXT` with field type inference and selector metadata (no raw typed text).

## Privacy Notes

- Window titles are not persisted as action fields.
- Clipboard payloads are not stored; only content type metadata is used.
- Keyboard regular typing is not recorded as raw keystroke logs.
- Browser URL/search capture is intentionally enabled in this project state.
- URLs and search queries can contain sensitive information, so treat stored data as sensitive.

## HTTP API

### Dashboard REST endpoints

- `GET /api/dashboard/status`
- `POST /api/dashboard/start`
- `POST /api/dashboard/stop`
- `POST /api/dashboard/clear`
- `GET /api/dashboard/actions?limit=120`
- `GET /api/dashboard/flow?limit=2500`
- `GET /api/dashboard/graph?min_frequency=1`
- `GET /api/dashboard/repeated_tasks?min_frequency=2&limit=25&flow_limit=5000`

### JSON-RPC methods (`/rpc`, `/mcp`)

- `list_windows`
- `get_window_tree`
- `get_patterns`
- `get_transitions`
- `activate_element`
- `press_key`
- `set_clipboard`
- `tools/list`

Example JSON-RPC call:

```powershell
curl -X POST http://127.0.0.1:9315/rpc `
  -H "Content-Type: application/json" `
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"get_transitions\",\"params\":{\"min_frequency\":1},\"id\":1}"
```

## Configuration

Environment variables:

- `SEDA_MCP_PORT` (default `9315`)
- `SEDA_DATABASE_PATH` (default `%LOCALAPPDATA%\seda-agent\seda.db`)
- `SEDA_MIN_PATTERN_FREQUENCY` (default `3`)
- `SEDA_MAX_PATTERN_LENGTH` (default `10`)
- `SEDA_ACTION_GROUPING_WINDOW_MS` (default `5000`)
- `SEDA_DEBUG` (set to any value to enable debug mode)

## Development

```powershell
cd .\seda-agent
cargo fmt
cargo check
cargo test
```

## Known Limitations

- Windows-focused implementation.
- Browser URL extraction depends on accessibility tree availability.
- Search query extraction is best-effort based on common query parameter keys.
- Dashboard session history is in-memory for the running process and resets on restart.

## Docs

- Architecture details: `docs/ARCHITECTURE.md`
- Contribution guide: `CONTRIBUTING.md`

## License

MIT. See `LICENSE`.
