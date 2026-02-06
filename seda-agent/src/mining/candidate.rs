//! Candidate pattern output format
//!
//! Defines the JSON output format for patterns to be consumed
//! by the Python planner.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::pattern::CandidatePattern;

/// Report containing discovered patterns
///
/// This is the main output format for the Python planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternReport {
    /// List of candidate patterns
    pub patterns: Vec<CandidatePattern>,
    /// When this report was generated
    pub generated_at: DateTime<Utc>,
    /// How far back the analysis looked (hours)
    pub analysis_window_hours: u64,
    /// Total actions analyzed
    pub total_actions_analyzed: u64,
    /// Version of the report format
    pub version: String,
}

impl PatternReport {
    /// Create a new pattern report
    pub fn new(
        patterns: Vec<CandidatePattern>,
        analysis_window_hours: u64,
        total_actions_analyzed: u64,
    ) -> Self {
        Self {
            patterns,
            generated_at: Utc::now(),
            analysis_window_hours,
            total_actions_analyzed,
            version: "1.0".to_string(),
        }
    }

    /// Create an empty report
    pub fn empty() -> Self {
        Self {
            patterns: Vec::new(),
            generated_at: Utc::now(),
            analysis_window_hours: 0,
            total_actions_analyzed: 0,
            version: "1.0".to_string(),
        }
    }

    /// Get patterns sorted by score
    pub fn sorted_by_score(&self) -> Vec<&CandidatePattern> {
        let mut sorted: Vec<_> = self.patterns.iter().collect();
        sorted.sort_by(|a, b| b.score().partial_cmp(&a.score()).unwrap_or(std::cmp::Ordering::Equal));
        sorted
    }

    /// Get top N patterns by score
    pub fn top_patterns(&self, n: usize) -> Vec<&CandidatePattern> {
        self.sorted_by_score().into_iter().take(n).collect()
    }

    /// Filter patterns by minimum frequency
    pub fn filter_by_frequency(&self, min_freq: u32) -> Vec<&CandidatePattern> {
        self.patterns.iter().filter(|p| p.frequency >= min_freq).collect()
    }

    /// Filter patterns by minimum confidence
    pub fn filter_by_confidence(&self, min_confidence: f64) -> Vec<&CandidatePattern> {
        self.patterns
            .iter()
            .filter(|p| p.confidence >= min_confidence)
            .collect()
    }

    /// Filter patterns by minimum length
    pub fn filter_by_length(&self, min_length: usize) -> Vec<&CandidatePattern> {
        self.patterns.iter().filter(|p| p.len() >= min_length).collect()
    }

    /// Get summary statistics
    pub fn summary(&self) -> ReportSummary {
        let total_patterns = self.patterns.len();
        let avg_frequency = if total_patterns > 0 {
            self.patterns.iter().map(|p| p.frequency as f64).sum::<f64>() / total_patterns as f64
        } else {
            0.0
        };
        let avg_confidence = if total_patterns > 0 {
            self.patterns.iter().map(|p| p.confidence).sum::<f64>() / total_patterns as f64
        } else {
            0.0
        };
        let total_time_savings_ms: u64 = self
            .patterns
            .iter()
            .map(|p| p.estimated_time_saved_per_occurrence_ms * p.frequency as u64)
            .sum();

        ReportSummary {
            total_patterns,
            avg_frequency,
            avg_confidence,
            total_time_savings_ms,
            analysis_window_hours: self.analysis_window_hours,
        }
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Summary statistics for a report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    /// Total number of patterns
    pub total_patterns: usize,
    /// Average frequency across patterns
    pub avg_frequency: f64,
    /// Average confidence across patterns
    pub avg_confidence: f64,
    /// Total estimated time savings (milliseconds)
    pub total_time_savings_ms: u64,
    /// Analysis window in hours
    pub analysis_window_hours: u64,
}

impl ReportSummary {
    /// Get a human-readable summary
    pub fn description(&self) -> String {
        format!(
            "Found {} patterns (avg freq: {:.1}, avg conf: {:.0}%, ~{:.1}min total savings)",
            self.total_patterns,
            self.avg_frequency,
            self.avg_confidence * 100.0,
            self.total_time_savings_ms as f64 / 60000.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolizer::SymbolicActionType;

    #[test]
    fn test_pattern_report() {
        let patterns = vec![
            CandidatePattern::new(
                vec![SymbolicActionType::CopyText, SymbolicActionType::PasteText],
                10,
                2000,
                0.9,
            ),
            CandidatePattern::new(
                vec![
                    SymbolicActionType::CopyText,
                    SymbolicActionType::SwitchApp,
                    SymbolicActionType::PasteText,
                ],
                5,
                3000,
                0.8,
            ),
        ];

        let report = PatternReport::new(patterns, 24, 100);

        assert_eq!(report.patterns.len(), 2);
        assert_eq!(report.analysis_window_hours, 24);

        let top = report.top_patterns(1);
        assert_eq!(top.len(), 1);
    }

    #[test]
    fn test_report_serialization() {
        let patterns = vec![CandidatePattern::new(
            vec![SymbolicActionType::CopyText, SymbolicActionType::PasteText],
            10,
            2000,
            0.9,
        )];

        let report = PatternReport::new(patterns, 24, 100);

        let json = report.to_json().unwrap();
        let parsed = PatternReport::from_json(&json).unwrap();

        assert_eq!(parsed.patterns.len(), 1);
        assert_eq!(parsed.analysis_window_hours, 24);
    }

    #[test]
    fn test_report_summary() {
        let patterns = vec![
            CandidatePattern::new(
                vec![SymbolicActionType::CopyText, SymbolicActionType::PasteText],
                10,
                2000,
                0.9,
            ),
            CandidatePattern::new(
                vec![SymbolicActionType::SwitchApp, SymbolicActionType::Navigate],
                6,
                1000,
                0.7,
            ),
        ];

        let report = PatternReport::new(patterns, 24, 100);
        let summary = report.summary();

        assert_eq!(summary.total_patterns, 2);
        assert_eq!(summary.avg_frequency, 8.0);
        assert!((summary.avg_confidence - 0.8).abs() < 0.01);
    }
}
