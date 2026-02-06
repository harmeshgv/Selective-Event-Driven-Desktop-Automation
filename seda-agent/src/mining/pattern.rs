//! Pattern representation
//!
//! Defines the structure of detected patterns (candidate automations).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::symbolizer::SymbolicActionType;

/// A candidate pattern for automation
///
/// This represents a repeated sequence of actions that could potentially
/// be automated. It's designed to be consumed by the Python planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidatePattern {
    /// Unique identifier for this pattern
    pub id: String,
    /// SHA256 hash of the sequence (for deduplication)
    pub pattern_hash: String,
    /// Sequence of action types
    pub sequence: Vec<SymbolicActionType>,
    /// How many times this pattern was observed
    pub frequency: u32,
    /// Average total duration to complete the pattern (milliseconds)
    pub avg_total_duration_ms: u64,
    /// How consistently this pattern completes (0.0 - 1.0)
    ///
    /// A high confidence means the pattern usually completes once started.
    /// Low confidence might indicate the pattern is often interrupted.
    pub confidence: f64,
    /// Estimated time that could be saved per occurrence (milliseconds)
    ///
    /// This is a heuristic based on pattern duration and assumed automation speed.
    pub estimated_time_saved_per_occurrence_ms: u64,
    /// When this pattern was first detected
    pub first_seen: DateTime<Utc>,
    /// Most recent observation
    pub last_seen: DateTime<Utc>,
    /// Application context (if pattern is app-specific)
    pub app_context: Option<Vec<String>>,
}

impl CandidatePattern {
    /// Create a new candidate pattern
    pub fn new(
        sequence: Vec<SymbolicActionType>,
        frequency: u32,
        avg_total_duration_ms: u64,
        confidence: f64,
    ) -> Self {
        let pattern_hash = Self::compute_hash(&sequence);
        let id = format!("pattern_{}", &pattern_hash[..8]);

        // Estimate time saved: assume automation takes 10% of manual time
        // This is a rough heuristic that will be refined by the planner
        let estimated_time_saved = (avg_total_duration_ms as f64 * 0.9) as u64;

        let now = Utc::now();

        Self {
            id,
            pattern_hash,
            sequence,
            frequency,
            avg_total_duration_ms,
            confidence,
            estimated_time_saved_per_occurrence_ms: estimated_time_saved,
            first_seen: now,
            last_seen: now,
        app_context: None,
        }
    }

    /// Compute a hash of the sequence for deduplication
    pub fn compute_hash(sequence: &[SymbolicActionType]) -> String {
        let mut hasher = Sha256::new();
        for action in sequence {
            hasher.update(action.to_string().as_bytes());
            hasher.update(b"|");
        }
        hex::encode(hasher.finalize())
    }

    /// Get the pattern length (number of actions)
    pub fn len(&self) -> usize {
        self.sequence.len()
    }

    /// Check if the pattern is empty
    pub fn is_empty(&self) -> bool {
        self.sequence.is_empty()
    }

    /// Calculate a score for ranking patterns
    ///
    /// Higher scores indicate more valuable patterns.
    pub fn score(&self) -> f64 {
        // Factors:
        // 1. Frequency (log scale to avoid dominance)
        let freq_factor = (self.frequency as f64).ln_1p();

        // 2. Time savings potential
        let time_factor = (self.estimated_time_saved_per_occurrence_ms as f64 / 1000.0).ln_1p();

        // 3. Confidence
        let conf_factor = self.confidence;

        // 4. Recency bonus
        let age_hours = Utc::now()
            .signed_duration_since(self.last_seen)
            .num_hours()
            .max(1) as f64;
        let recency_factor = 1.0 / age_hours.ln_1p();

        // Combined score
        freq_factor * time_factor * conf_factor * (1.0 + recency_factor * 0.1)
    }

    /// Update the pattern with a new observation
    pub fn record_observation(&mut self, duration_ms: u64, completed: bool) {
        // Update frequency
        self.frequency += 1;

        // Update average duration (running average)
        let old_total = self.avg_total_duration_ms * (self.frequency - 1) as u64;
        self.avg_total_duration_ms = (old_total + duration_ms) / self.frequency as u64;

        // Update confidence (running average)
        let completed_factor = if completed { 1.0 } else { 0.0 };
        self.confidence = (self.confidence * (self.frequency - 1) as f64 + completed_factor)
            / self.frequency as f64;

        // Update timestamps
        self.last_seen = Utc::now();

        // Recalculate time savings estimate
        self.estimated_time_saved_per_occurrence_ms =
            (self.avg_total_duration_ms as f64 * 0.9) as u64;
    }

    /// Check if this pattern matches a sequence prefix
    pub fn matches_prefix(&self, sequence: &[SymbolicActionType]) -> bool {
        if sequence.len() > self.sequence.len() {
            return false;
        }

        self.sequence.iter().zip(sequence.iter()).all(|(a, b)| a == b)
    }

    /// Check if this pattern is a subsequence of another
    pub fn is_subsequence_of(&self, other: &[SymbolicActionType]) -> bool {
        if self.sequence.len() > other.len() {
            return false;
        }

        let mut other_iter = other.iter();
        for action in &self.sequence {
            if !other_iter.any(|a| a == action) {
                return false;
            }
        }
        true
    }

    /// Get a human-readable description of the pattern
    pub fn description(&self) -> String {
        let actions: Vec<String> = self.sequence.iter().map(|a| a.to_string()).collect();
        format!(
            "Pattern: {} (freq: {}, confidence: {:.0}%, ~{:.1}s saved)",
            actions.join(" → "),
            self.frequency,
            self.confidence * 100.0,
            self.estimated_time_saved_per_occurrence_ms as f64 / 1000.0
        )
    }
}

impl PartialEq for CandidatePattern {
    fn eq(&self, other: &Self) -> bool {
        self.pattern_hash == other.pattern_hash
    }
}

impl Eq for CandidatePattern {}

impl std::hash::Hash for CandidatePattern {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pattern_hash.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_creation() {
        let sequence = vec![
            SymbolicActionType::CopyText,
            SymbolicActionType::SwitchApp,
            SymbolicActionType::PasteText,
        ];

        let pattern = CandidatePattern::new(sequence.clone(), 5, 2000, 0.9);

        assert_eq!(pattern.len(), 3);
        assert_eq!(pattern.frequency, 5);
        assert_eq!(pattern.confidence, 0.9);
        assert!(pattern.estimated_time_saved_per_occurrence_ms > 0);
    }

    #[test]
    fn test_pattern_hash() {
        let seq1 = vec![SymbolicActionType::CopyText, SymbolicActionType::PasteText];
        let seq2 = vec![SymbolicActionType::CopyText, SymbolicActionType::PasteText];
        let seq3 = vec![SymbolicActionType::PasteText, SymbolicActionType::CopyText];

        let hash1 = CandidatePattern::compute_hash(&seq1);
        let hash2 = CandidatePattern::compute_hash(&seq2);
        let hash3 = CandidatePattern::compute_hash(&seq3);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_pattern_score() {
        let high_freq = CandidatePattern::new(
            vec![SymbolicActionType::CopyText, SymbolicActionType::PasteText],
            100,
            5000,
            0.95,
        );

        let low_freq = CandidatePattern::new(
            vec![SymbolicActionType::CopyText, SymbolicActionType::PasteText],
            5,
            5000,
            0.95,
        );

        assert!(high_freq.score() > low_freq.score());
    }

    #[test]
    fn test_matches_prefix() {
        let pattern = CandidatePattern::new(
            vec![
                SymbolicActionType::CopyText,
                SymbolicActionType::SwitchApp,
                SymbolicActionType::PasteText,
            ],
            5,
            2000,
            0.9,
        );

        assert!(pattern.matches_prefix(&[SymbolicActionType::CopyText]));
        assert!(pattern.matches_prefix(&[
            SymbolicActionType::CopyText,
            SymbolicActionType::SwitchApp
        ]));
        assert!(!pattern.matches_prefix(&[SymbolicActionType::PasteText]));
    }
}
