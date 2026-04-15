//! LaTeX math plugin for markdown-it.
//!
//! Converts `$...$` (inline) and `$$...$$` (display) to MathML using latex2mathml.

use markdown_it::parser::inline::{InlineRule, InlineState};
use markdown_it::parser::block::{BlockRule, BlockState};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

// ── Inline math: $...$ ──────────────────────────────────────────────────

#[derive(Debug)]
pub struct InlineMath {
    pub formula: String,
}

impl NodeValue for InlineMath {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        match latex2mathml::latex_to_mathml(&self.formula, latex2mathml::DisplayStyle::Inline) {
            Ok(mathml) => fmt.text_raw(&mathml),
            Err(_) => {
                let mut attrs = node.attrs.clone();
                attrs.push(("class", "math-error".into()));
                fmt.open("code", &attrs);
                fmt.text(&self.formula);
                fmt.close("code");
            }
        }
    }
}

struct InlineMathScanner;

impl InlineRule for InlineMathScanner {
    const MARKER: char = '$';

    fn run(state: &mut InlineState) -> Option<(Node, usize)> {
        let input = &state.src[state.pos..state.pos_max];
        if input.starts_with("$$") {
            return None; // let block math handle $$
        }
        if !input.starts_with('$') {
            return None;
        }

        // Find closing $
        let rest = &input[1..];
        let close = rest.find('$')?;
        if close == 0 {
            return None; // empty $$
        }

        let formula = &rest[..close];
        // Don't match if formula contains newlines
        if formula.contains('\n') {
            return None;
        }

        let node = Node::new(InlineMath {
            formula: formula.to_string(),
        });
        Some((node, close + 2)) // $formula$
    }
}

// ── Display math: $$...$$ ───────────────────────────────────────────────

#[derive(Debug)]
pub struct DisplayMath {
    pub formula: String,
}

impl NodeValue for DisplayMath {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        match latex2mathml::latex_to_mathml(&self.formula, latex2mathml::DisplayStyle::Block) {
            Ok(mathml) => {
                fmt.cr();
                fmt.text_raw(&mathml);
                fmt.cr();
            }
            Err(_) => {
                let mut attrs = node.attrs.clone();
                attrs.push(("class", "math-error".into()));
                fmt.cr();
                fmt.open("div", &attrs);
                fmt.open("code", &[]);
                fmt.text(&self.formula);
                fmt.close("code");
                fmt.close("div");
                fmt.cr();
            }
        }
    }
}

struct DisplayMathScanner;

impl BlockRule for DisplayMathScanner {
    fn run(state: &mut BlockState) -> Option<(Node, usize)> {
        let line = state.get_line(state.line);
        if !line.trim_start().starts_with("$$") {
            return None;
        }

        // Check if it's a single-line $$formula$$
        let trimmed = line.trim();
        if trimmed.len() > 4 && trimmed.starts_with("$$") && trimmed.ends_with("$$") {
            let formula = &trimmed[2..trimmed.len() - 2];
            return Some((
                Node::new(DisplayMath { formula: formula.to_string() }),
                1,
            ));
        }

        // Multi-line: find closing $$
        let mut end_line = state.line + 1;
        while end_line < state.line_max {
            let l = state.get_line(end_line).trim();
            if l == "$$" {
                break;
            }
            end_line += 1;
        }

        if end_line >= state.line_max {
            return None; // no closing $$
        }

        // Collect formula lines
        let mut formula = String::new();
        for i in (state.line + 1)..end_line {
            if !formula.is_empty() {
                formula.push('\n');
            }
            formula.push_str(state.get_line(i));
        }

        Some((
            Node::new(DisplayMath { formula }),
            end_line - state.line + 1,
        ))
    }
}

// ── Plugin registration ─────────────────────────────────────────────────

pub fn add(md: &mut MarkdownIt) {
    md.inline.add_rule::<InlineMathScanner>();
    md.block.add_rule::<DisplayMathScanner>();
}
