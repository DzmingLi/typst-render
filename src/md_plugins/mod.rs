//! Custom markdown-it plugins for NightBoat.
//!
//! Each plugin follows the standard markdown-it pattern:
//! - Block rules for block-level syntax (admonition, display math)
//! - Inline rules for inline syntax (inline math)
//! - Core rules for AST post-processing (IAL, callout)

pub mod admonition;
pub mod callout;
pub mod ial;
pub mod math;
