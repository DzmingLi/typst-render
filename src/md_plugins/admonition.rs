//! MkDocs-Material admonition plugin for markdown-it.
//!
//! Supports:
//! - `!!! type "title"` — always-open admonition
//! - `??? type "title"` — collapsible (closed by default)
//! - `???+ type "title"` — collapsible (open by default)
//!
//! Body is indented by 4 spaces. Nesting is supported.

use markdown_it::parser::block::{BlockRule, BlockState};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

const ADMONITION_TYPES: &[&str] = &[
    "note", "abstract", "info", "tip", "success", "question",
    "warning", "failure", "danger", "bug", "example", "quote",
];

// ── AST nodes ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Admonition {
    pub kind: String,
    pub title: String,
}

impl NodeValue for Admonition {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push(("class", format!("admonition {}", self.kind)));
        fmt.cr();
        fmt.open("div", &attrs);
        fmt.cr();
        fmt.open("p", &[("class", "admonition-title".into())]);
        fmt.text(&self.title);
        fmt.close("p");
        fmt.cr();
        fmt.contents(&node.children);
        fmt.cr();
        fmt.close("div");
        fmt.cr();
    }
}

#[derive(Debug)]
pub struct CollapsibleAdmonition {
    pub kind: String,
    pub title: String,
    pub open: bool,
}

impl NodeValue for CollapsibleAdmonition {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push(("class", format!("admonition {}", self.kind)));
        if self.open {
            attrs.push(("open", String::new()));
        }
        fmt.cr();
        fmt.open("details", &attrs);
        fmt.cr();
        fmt.open("summary", &[]);
        fmt.text(&self.title);
        fmt.close("summary");
        fmt.cr();
        fmt.contents(&node.children);
        fmt.cr();
        fmt.close("details");
        fmt.cr();
    }
}

// ── Block rule ──────────────────────────────────────────────────────────

struct AdmonitionScanner;

impl BlockRule for AdmonitionScanner {
    fn check(state: &mut BlockState) -> Option<()> {
        let line = state.get_line(state.line);
        let trimmed = line.trim_start();
        if trimmed.starts_with("!!! ") || trimmed.starts_with("??? ") || trimmed.starts_with("???+ ") {
            Some(())
        } else {
            None
        }
    }

    fn run(state: &mut BlockState) -> Option<(Node, usize)> {
        Self::check(state)?;

        let line = state.get_line(state.line).to_owned();
        let trimmed = line.trim_start();

        // Parse marker
        let (marker, rest) = if let Some(r) = trimmed.strip_prefix("???+ ") {
            ("???+", r)
        } else if let Some(r) = trimmed.strip_prefix("??? ") {
            ("???", r)
        } else if let Some(r) = trimmed.strip_prefix("!!! ") {
            ("!!!", r)
        } else {
            return None;
        };

        let rest = rest.trim();

        // Extract type
        let (ty, after_type) = match rest.find(|c: char| c.is_whitespace()) {
            Some(i) => (&rest[..i], rest[i..].trim()),
            None => (rest, ""),
        };

        let ty_lower = ty.to_lowercase();
        if !ADMONITION_TYPES.contains(&ty_lower.as_str()) {
            return None;
        }

        // Extract title
        let title = if after_type.starts_with('"') && after_type.ends_with('"') && after_type.len() >= 2 {
            after_type[1..after_type.len() - 1].to_string()
        } else if !after_type.is_empty() {
            after_type.to_string()
        } else {
            let mut chars = ty_lower.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => ty_lower.clone(),
            }
        };

        // Find extent of indented body (lines with indent >= 4 relative to current blk_indent)
        let start_line = state.line;
        let min_indent = state.blk_indent as i32 + 4;
        let mut next_line = start_line + 1;
        while next_line < state.line_max {
            if state.is_empty(next_line) {
                // Blank line: include if more indented content follows
                let mut has_more = false;
                for k in (next_line + 1)..state.line_max {
                    if state.is_empty(k) { continue; }
                    if state.line_offsets[k].indent_nonspace >= min_indent {
                        has_more = true;
                    }
                    break;
                }
                if has_more {
                    next_line += 1;
                } else {
                    break;
                }
            } else if state.line_offsets[next_line].indent_nonspace >= min_indent {
                next_line += 1;
            } else {
                break;
            }
        }

        // Create node and parse nested content with adjusted indentation
        let is_collapsible = marker != "!!!";
        let is_open = marker == "???+";

        let mut node = if is_collapsible {
            Node::new(CollapsibleAdmonition {
                kind: ty_lower,
                title,
                open: is_open,
            })
        } else {
            Node::new(Admonition {
                kind: ty_lower,
                title,
            })
        };

        // Adjust line offsets to remove 4-space indent, then tokenize nested content
        let mut old_offsets = Vec::new();
        for i in (start_line + 1)..next_line {
            old_offsets.push(state.line_offsets[i].clone());
            if !state.is_empty(i) {
                // Shift first_nonspace and indent by 4 to "remove" the leading indent
                let raw_start = state.line_offsets[i].line_start;
                let raw_text = &state.src[raw_start..state.line_offsets[i].line_end];
                if raw_text.starts_with("    ") {
                    state.line_offsets[i].first_nonspace = (state.line_offsets[i].first_nonspace).max(raw_start + 4);
                    state.line_offsets[i].indent_nonspace -= 4;
                } else if raw_text.starts_with('\t') {
                    state.line_offsets[i].first_nonspace = (state.line_offsets[i].first_nonspace).max(raw_start + 1);
                    state.line_offsets[i].indent_nonspace -= 4;
                }
            }
        }

        let old_indent = state.blk_indent;
        state.blk_indent = 0;

        let old_node = std::mem::replace(&mut state.node, node);
        let old_line_max = state.line_max;
        state.line = start_line + 1;
        state.line_max = next_line;
        state.md.block.tokenize(state);
        next_line = state.line;
        state.line = start_line;
        state.line_max = old_line_max;

        state.blk_indent = old_indent;

        // Restore offsets
        for (idx, offset) in old_offsets.into_iter().enumerate() {
            state.line_offsets[start_line + 1 + idx] = offset;
        }

        node = std::mem::replace(&mut state.node, old_node);
        Some((node, next_line - start_line))
    }
}

// ── Plugin registration ─────────────────────────────────────────────────

pub fn add(md: &mut MarkdownIt) {
    // Must run before code scanner, which would eat 4-space-indented lines
    md.block.add_rule::<AdmonitionScanner>().before_all();
}
