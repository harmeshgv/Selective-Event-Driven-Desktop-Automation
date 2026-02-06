//! Sequence mining algorithm
//!
//! Implements a simplified PrefixSpan-style sequential pattern mining algorithm
//! optimized for streaming data and real-time pattern detection.
//!
//! # Algorithm Overview
//!
//! 1. Maintain a sliding window of recent actions
//! 2. Extract n-grams (sequences of length n) from the window
//! 3. Count frequencies of each n-gram
//! 4. Patterns above the support threshold become candidates
//!
//! # Privacy Note
//!
//! Only SymbolicActionType sequences are mined - no raw data.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::pattern::CandidatePattern;
use super::PatternMiner;
use crate::symbolizer::{SymbolicAction, SymbolicActionType};

/// Configuration for the sequence miner
#[derive(Debug, Clone)]
pub struct MinerConfig {
    /// Minimum occurrences for a pattern to be considered
    pub min_support: u32,
    /// Maximum length of patterns to detect
    pub max_pattern_length: usize,
    /// Time window for grouping related actions (milliseconds)
    pub time_window_ms: u64,
    /// Maximum gap between actions in a pattern (milliseconds)
    pub max_gap_ms: u64,
}

impl Default for MinerConfig {
    fn default() -> Self {
        Self {
            min_support: 3,
            max_pattern_length: 10,
            time_window_ms: 5000,
            max_gap_ms: 30000, // 30 seconds max gap
        }
    }
}

/// Sequence miner for detecting repeated action patterns
pub struct SequenceMiner {
    config: MinerConfig,
    /// N-gram counts for each pattern length
    ngram_counts: HashMap<usize, HashMap<Vec<SymbolicActionType>, PatternStats>>,
    /// Recent action buffer with timestamps
    action_buffer: Vec<(SymbolicActionType, DateTime<Utc>)>,
    /// Maximum buffer size
    max_buffer_size: usize,
}

/// Statistics for a pattern
#[derive(Debug, Clone)]
struct PatternStats {
    frequency: u32,
    total_duration_ms: u64,
    completions: u32,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
}

impl PatternStats {
    fn new(duration_ms: u64, timestamp: DateTime<Utc>) -> Self {
        Self {
            frequency: 1,
            total_duration_ms: duration_ms,
            completions: 1,
            first_seen: timestamp,
            last_seen: timestamp,
        }
    }

    fn record(&mut self, duration_ms: u64, timestamp: DateTime<Utc>, completed: bool) {
        self.frequency += 1;
        self.total_duration_ms += duration_ms;
        if completed {
            self.completions += 1;
        }
        self.last_seen = timestamp;
    }

    fn avg_duration_ms(&self) -> u64 {
        if self.frequency == 0 {
            0
        } else {
            self.total_duration_ms / self.frequency as u64
        }
    }

    fn confidence(&self) -> f64 {
        if self.frequency == 0 {
            0.0
        } else {
            self.completions as f64 / self.frequency as f64
        }
    }
}

impl SequenceMiner {
    /// Create a new sequence miner with default configuration
    pub fn new() -> Self {
        Self::with_config(MinerConfig::default())
    }

    /// Create a sequence miner with custom configuration
    pub fn with_config(config: MinerConfig) -> Self {
        Self {
            max_buffer_size: 1000,
            ngram_counts: HashMap::new(),
            action_buffer: Vec::new(),
            config,
        }
    }

    /// Process a new action and update pattern statistics
    pub fn process_action(&mut self, action: &SymbolicAction, timestamp: DateTime<Utc>) {
        let action_type = action.action_type();

        // Add to buffer
        self.action_buffer.push((action_type, timestamp));

        // Trim buffer if too large
        if self.action_buffer.len() > self.max_buffer_size {
            self.action_buffer.remove(0);
        }

        // Extract and count n-grams
        self.update_ngrams(timestamp);
    }

    /// Update n-gram counts from the current buffer
    fn update_ngrams(&mut self, now: DateTime<Utc>) {
        let buffer_len = self.action_buffer.len();

        // For each pattern length (3 to max)
        for n in 3..=self.config.max_pattern_length.min(buffer_len) {
            // Look at the most recent n actions
            let start = buffer_len.saturating_sub(n);
            let window = &self.action_buffer[start..];

            // Check if actions are within time constraints
            if !self.is_valid_sequence(window) {
                continue;
            }

            // Extract the sequence
            let sequence: Vec<SymbolicActionType> = window.iter().map(|(t, _)| *t).collect();

            // Calculate duration
            let duration_ms = if window.len() >= 2 {
                let start_time = window.first().unwrap().1;
                let end_time = window.last().unwrap().1;
                end_time
                    .signed_duration_since(start_time)
                    .num_milliseconds()
                    .max(0) as u64
            } else {
                0
            };

            // Update counts
            let counts = self.ngram_counts.entry(n).or_default();

            if let Some(stats) = counts.get_mut(&sequence) {
                stats.record(duration_ms, now, true);
            } else {
                counts.insert(sequence, PatternStats::new(duration_ms, now));
            }
        }
    }

    /// Check if a sequence is valid (within time constraints)
    fn is_valid_sequence(&self, sequence: &[(SymbolicActionType, DateTime<Utc>)]) -> bool {
        if sequence.len() < 2 {
            return true;
        }

        // Check gaps between consecutive actions
        for window in sequence.windows(2) {
            let gap = window[1]
                .1
                .signed_duration_since(window[0].1)
                .num_milliseconds()
                .max(0) as u64;

            if gap > self.config.max_gap_ms {
                return false;
            }
        }

        true
    }

    /// Get all patterns that meet the minimum support threshold
    pub fn get_frequent_patterns(&self) -> Vec<CandidatePattern> {
        let mut patterns = Vec::new();

        for (_, counts) in &self.ngram_counts {
            for (sequence, stats) in counts {
                if stats.frequency >= self.config.min_support {
                    let mut pattern = CandidatePattern::new(
                        sequence.clone(),
                        stats.frequency,
                        stats.avg_duration_ms(),
                        stats.confidence(),
                    );
                    pattern.first_seen = stats.first_seen;
                    pattern.last_seen = stats.last_seen;
                    patterns.push(pattern);
                }
            }
        }

        // Sort by score (highest first)
        patterns.sort_by(|a, b| b.score().partial_cmp(&a.score()).unwrap_or(std::cmp::Ordering::Equal));

        // Remove redundant patterns (subsequences of longer patterns)
        self.remove_redundant_patterns(patterns)
    }

    /// Remove patterns that are strict subsequences of other patterns
    fn remove_redundant_patterns(&self, patterns: Vec<CandidatePattern>) -> Vec<CandidatePattern> {
        let mut result = Vec::new();

        for pattern in patterns {
            // Check if this pattern is a strict prefix of any existing pattern
            let dominated = result.iter().any(|existing: &CandidatePattern| {
                existing.sequence.starts_with(&pattern.sequence)
                    && existing.sequence.len() > pattern.sequence.len()
                    && existing.frequency >= pattern.frequency
            });

            if !dominated {
                // Also remove any existing patterns that this one dominates
                result.retain(|existing| {
                    !(pattern.sequence.starts_with(&existing.sequence)
                        && pattern.sequence.len() > existing.sequence.len()
                        && pattern.frequency >= existing.frequency)
                });
                result.push(pattern);
            }
        }

        result
    }

    /// Get patterns with minimum length
    pub fn get_patterns_min_length(&self, min_length: usize) -> Vec<CandidatePattern> {
        self.get_frequent_patterns()
            .into_iter()
            .filter(|p| p.len() >= min_length)
            .collect()
    }

    /// Clear all collected statistics
    pub fn clear(&mut self) {
        self.ngram_counts.clear();
        self.action_buffer.clear();
    }

    /// Get the current configuration
    pub fn config(&self) -> &MinerConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_min_support(&mut self, min_support: u32) {
        self.config.min_support = min_support;
    }

    /// Get statistics about the miner state
    pub fn stats(&self) -> MinerStats {
        let total_patterns: usize = self.ngram_counts.values().map(|m| m.len()).sum();
        let frequent_patterns = self.get_frequent_patterns().len();

        MinerStats {
            buffer_size: self.action_buffer.len(),
            total_patterns,
            frequent_patterns,
            min_support: self.config.min_support,
        }
    }
}

impl Default for SequenceMiner {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternMiner for SequenceMiner {
    fn mine(&self, actions: &[SymbolicAction]) -> Vec<CandidatePattern> {
        // Create a temporary miner to process the actions
        let mut temp_miner = SequenceMiner::with_config(self.config.clone());

        for action in actions {
            temp_miner.process_action(action, Utc::now());
        }

        temp_miner.get_frequent_patterns()
    }

    fn min_support(&self) -> u32 {
        self.config.min_support
    }

    fn max_pattern_length(&self) -> usize {
        self.config.max_pattern_length
    }
}

/// Statistics about the miner
#[derive(Debug, Clone)]
pub struct MinerStats {
    pub buffer_size: usize,
    pub total_patterns: usize,
    pub frequent_patterns: usize,
    pub min_support: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolizer::{AppIdentifier, ContentType};

    fn make_copy_action() -> SymbolicAction {
        SymbolicAction::CopyText {
            source_app: AppIdentifier::new("chrome.exe"),
            content_type: ContentType::PlainText,
        }
    }

    fn make_switch_action() -> SymbolicAction {
        SymbolicAction::SwitchApp {
            from_app: AppIdentifier::new("chrome.exe"),
            to_app: AppIdentifier::new("code.exe"),
        }
    }

    fn make_paste_action() -> SymbolicAction {
        SymbolicAction::PasteText {
            target_app: AppIdentifier::new("code.exe"),
        }
    }

    #[test]
    fn test_detect_repeated_pattern() {
        let mut miner = SequenceMiner::new();
        miner.set_min_support(3);

        // Repeat a pattern 5 times
        for _ in 0..5 {
            miner.process_action(&make_copy_action(), Utc::now());
            miner.process_action(&make_switch_action(), Utc::now());
            miner.process_action(&make_paste_action(), Utc::now());
        }

        let patterns = miner.get_frequent_patterns();

        assert!(!patterns.is_empty());

        // Should find the copy-switch-paste pattern
        let found = patterns.iter().any(|p| {
            p.sequence
                == vec![
                    SymbolicActionType::CopyText,
                    SymbolicActionType::SwitchApp,
                    SymbolicActionType::PasteText,
                ]
        });

        assert!(found, "Expected copy-switch-paste pattern");
    }

    #[test]
    fn test_minimum_support() {
        let mut miner = SequenceMiner::new();
        miner.set_min_support(5);

        // Only repeat 3 times
        for _ in 0..3 {
            miner.process_action(&make_copy_action(), Utc::now());
            miner.process_action(&make_paste_action(), Utc::now());
        }

        let patterns = miner.get_frequent_patterns();

        // Should not find any patterns (support threshold not met)
        assert!(patterns.is_empty() || patterns.iter().all(|p| p.frequency < 5));
    }

    #[test]
    fn test_pattern_scoring() {
        let mut miner = SequenceMiner::new();
        miner.set_min_support(2);

        // Create patterns with different frequencies
        for _ in 0..10 {
            miner.process_action(&make_copy_action(), Utc::now());
            miner.process_action(&make_paste_action(), Utc::now());
        }

        for _ in 0..3 {
            miner.process_action(&make_switch_action(), Utc::now());
            miner.process_action(&make_paste_action(), Utc::now());
        }

        let patterns = miner.get_frequent_patterns();

        // Higher frequency pattern should have higher score
        if patterns.len() >= 2 {
            assert!(patterns[0].score() >= patterns[1].score());
        }
    }
}
