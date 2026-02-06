//! Pattern/Sequence Mining
//!
//! Discovers repeated action sequences from the Personal Task Graph.
//! Uses a simplified PrefixSpan-style algorithm optimized for streaming data.
//!
//! # Output
//!
//! Candidate patterns are output in JSON format for consumption by
//! the Python planner (future component).

pub mod candidate;
pub mod pattern;
pub mod sequence;

pub use candidate::PatternReport;
pub use pattern::CandidatePattern;
pub use sequence::SequenceMiner;

/// Trait for pluggable mining algorithms
pub trait PatternMiner: Send + Sync {
    /// Mine patterns from a sequence of actions
    fn mine(&self, actions: &[crate::symbolizer::SymbolicAction]) -> Vec<CandidatePattern>;

    /// Get the minimum support threshold
    fn min_support(&self) -> u32;

    /// Get the maximum pattern length
    fn max_pattern_length(&self) -> usize;
}
