-- SEDA Agent Initial Schema
-- 
-- SAFETY DECISIONS:
-- - No raw_content columns: We never store clipboard text, window content, or keystrokes
-- - No window_title columns: May contain document names, URLs, or PII
-- - Only symbolic actions are persisted
-- - All timestamps are in milliseconds since Unix epoch

-- Symbolic actions only, no raw data
CREATE TABLE IF NOT EXISTS symbolic_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    action_type TEXT NOT NULL,          -- e.g., "SWITCH_APP", "COPY_TEXT", "PASTE_TEXT", "OPEN_APP"
    action_data TEXT NOT NULL,          -- JSON of SymbolicAction (sanitized, no PII)
    timestamp_ms INTEGER NOT NULL,      -- When the action occurred
    session_id TEXT NOT NULL,           -- Groups actions within a session
    duration_ms INTEGER,                -- Duration of the action (if applicable)
    source_app TEXT,                    -- Process name of source app (for transitions)
    target_app TEXT,                    -- Process name of target app (for transitions)
    
    -- SAFETY: Explicit check that action_data is valid JSON
    CHECK(json_valid(action_data))
);

-- Task graph edges: tracks transitions between action types
CREATE TABLE IF NOT EXISTS action_transitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_action_type TEXT NOT NULL,
    to_action_type TEXT NOT NULL,
    from_app TEXT,                      -- Source application (process name only)
    to_app TEXT,                        -- Target application (process name only)
    frequency INTEGER DEFAULT 1,        -- How often this transition occurs
    total_duration_ms INTEGER DEFAULT 0, -- Sum of all transition durations
    last_seen_ms INTEGER NOT NULL,      -- Last time this transition was observed
    
    UNIQUE(from_action_type, to_action_type, from_app, to_app)
);

-- Detected patterns from sequence mining
CREATE TABLE IF NOT EXISTS detected_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_hash TEXT UNIQUE NOT NULL,  -- SHA256 hash of the pattern sequence
    sequence TEXT NOT NULL,             -- JSON array of symbolic action types
    frequency INTEGER NOT NULL,         -- How often pattern was observed
    avg_duration_ms INTEGER,            -- Average time to complete pattern
    confidence REAL DEFAULT 0.0,        -- How consistently pattern completes (0-1)
    first_seen_ms INTEGER NOT NULL,     -- When pattern was first detected
    last_seen_ms INTEGER NOT NULL,      -- Most recent observation
    user_dismissed INTEGER DEFAULT 0,   -- 1 if user dismissed this pattern
    user_accepted INTEGER DEFAULT 0     -- 1 if user accepted this pattern
);

-- Audit log for MCP actions (safety tracking)
CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    audit_id TEXT UNIQUE NOT NULL,      -- UUID for external reference
    timestamp_ms INTEGER NOT NULL,
    operation TEXT NOT NULL,            -- MCP operation name
    parameters_hash TEXT,               -- SHA256 hash of parameters (not actual values)
    result TEXT NOT NULL,               -- "success", "error", "denied"
    error_message TEXT,                 -- Error details if result is "error"
    caller TEXT                         -- Identifier of the MCP client
);

-- Sessions for grouping related actions
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,                -- UUID session ID
    started_ms INTEGER NOT NULL,
    ended_ms INTEGER,
    action_count INTEGER DEFAULT 0
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_actions_timestamp ON symbolic_actions(timestamp_ms);
CREATE INDEX IF NOT EXISTS idx_actions_session ON symbolic_actions(session_id);
CREATE INDEX IF NOT EXISTS idx_actions_type ON symbolic_actions(action_type);
CREATE INDEX IF NOT EXISTS idx_transitions_freq ON action_transitions(frequency DESC);
CREATE INDEX IF NOT EXISTS idx_transitions_from ON action_transitions(from_action_type);
CREATE INDEX IF NOT EXISTS idx_transitions_to ON action_transitions(to_action_type);
CREATE INDEX IF NOT EXISTS idx_patterns_freq ON detected_patterns(frequency DESC);
CREATE INDEX IF NOT EXISTS idx_patterns_hash ON detected_patterns(pattern_hash);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp_ms);
CREATE INDEX IF NOT EXISTS idx_audit_operation ON audit_log(operation);
