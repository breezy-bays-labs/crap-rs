//! Pedagogical CRAP sample crate.
//!
//! Four modules chosen so each one isolates a different scaling
//! pattern of the CRAP formula `c² × (1 - coverage)³ + c`. See
//! `README.md` for the worked example heatmap and why each module
//! was picked.

pub mod config_merger;
pub mod csv_parser;
pub mod event_log;
pub mod string_utils;
