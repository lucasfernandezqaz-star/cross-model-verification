//! Audit stream-JSON transcripts and extract the last final answer.
//! Historical compatibility is recorded in PARITY.md; public fixtures are synthetic.

pub mod audit;
pub mod pyjson;
pub mod pytext;
mod unicode16;

pub use audit::{audit, run, strict_reasons, Audit, Run, StrictFacts};
