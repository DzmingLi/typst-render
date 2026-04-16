//! MyST-style colon directive plugin for markdown-it.
//!
//! Supports:
//! - `:::{note}` / `:::{warning}` etc. — admonition (same types as MkDocs `!!!`)
//! - `:::{youtube} VIDEO_ID` — YouTube embed
//! - `:::{figure} URL` — figure with caption
//!
//! Directives are fenced by `:::` (3+ colons). Nesting uses more colons.
//!
//! Examples:
//! ```markdown
//! :::{note}
//! This is a note.
//! :::
//!
//! :::{youtube} dQw4w9WgXcQ
//! :::
//!
//! :::{figure} https://example.com/img.png
//! :alt: Description
//! :width: 80%
//!
//! This is the caption.
//! :::
//! ```

use markdown_it::parser::block::{BlockRule, BlockState};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

const ADMONITION_TYPES: &[&str] = &[
    "note", "abstract", "info", "tip", "success", "question",
    "warning", "failure", "danger", "bug", "example", "quote",
    "admonition", "attention", "caution", "error", "hint", "important",
    "seealso",
];

// ── AST nodes ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ColonAdmonition {
    pub kind: String,
    pub title: String,
}

impl NodeValue for ColonAdmonition {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let css_kind = match self.kind.as_str() {
            "attention" | "caution" => "warning",
            "error" => "danger",
            "hint" | "important" | "seealso" => "tip",
            "admonition" => "note",
            k => k,
        };
        let mut attrs = node.attrs.clone();
        attrs.push(("class", format!("admonition {css_kind}")));
        fmt.cr();
        fmt.open("div", &attrs);
        fmt.cr();
        if !self.title.is_empty() {
            fmt.open("p", &[("class", "admonition-title".into())]);
            fmt.text(&self.title);
            fmt.close("p");
            fmt.cr();
        }
        fmt.contents(&node.children);
        fmt.cr();
        fmt.close("div");
        fmt.cr();
    }
}

#[derive(Debug)]
pub struct YouTubeEmbed {
    pub video_id: String,
}

impl NodeValue for YouTubeEmbed {
    fn render(&self, _node: &Node, fmt: &mut dyn Renderer) {
        fmt.cr();
        fmt.text_raw(&format!(
            r#"<div class="video-embed"><iframe src="https://www.youtube-nocookie.com/embed/{}" frameborder="0" allowfullscreen loading="lazy" style="aspect-ratio:16/9;width:100%;max-width:720px"></iframe></div>"#,
            self.video_id
        ));
        fmt.cr();
    }
}

#[derive(Debug)]
pub struct Figure {
    pub src: String,
    pub alt: String,
    pub width: String,
}

impl NodeValue for Figure {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push(("class", "figure".into()));
        fmt.cr();
        fmt.open("figure", &attrs);
        fmt.cr();

        let mut img_attrs: Vec<(&str, String)> = vec![
            ("src", self.src.clone()),
            ("alt", self.alt.clone()),
            ("loading", "lazy".into()),
        ];
        if !self.width.is_empty() {
            img_attrs.push(("style", format!("width:{}", self.width)));
        }
        fmt.self_close("img", &img_attrs);
        fmt.cr();

        // Caption from children
        if !node.children.is_empty() {
            fmt.open("figcaption", &[]);
            fmt.contents(&node.children);
            fmt.close("figcaption");
            fmt.cr();
        }

        fmt.close("figure");
        fmt.cr();
    }
}

// ── Block rule ──────────────────────────────────────────────────────────

struct DirectiveScanner;

impl DirectiveScanner {
    /// Count leading colons. Returns (colon_count, rest_of_line).
    fn parse_fence(line: &str) -> Option<(usize, &str)> {
        let trimmed = line.trim_start();
        let colons = trimmed.bytes().take_while(|&b| b == b':').count();
        if colons >= 3 {
            Some((colons, trimmed[colons..].trim()))
        } else {
            None
        }
    }

    /// Parse `{type} argument` from the rest after colons.
    fn parse_directive(rest: &str) -> Option<(&str, &str)> {
        if !rest.starts_with('{') {
            return None;
        }
        let close = rest.find('}')?;
        let dtype = &rest[1..close];
        let arg = rest[close + 1..].trim();
        Some((dtype, arg))
    }
}

impl BlockRule for DirectiveScanner {
    fn check(state: &mut BlockState) -> Option<()> {
        let line = state.get_line(state.line);
        let (colons, rest) = Self::parse_fence(line)?;
        if colons < 3 { return None; }
        let (dtype, _) = Self::parse_directive(rest)?;
        let dtype_lower = dtype.to_lowercase();
        if ADMONITION_TYPES.contains(&dtype_lower.as_str())
            || dtype_lower == "youtube"
            || dtype_lower == "figure"
        {
            Some(())
        } else {
            None
        }
    }

    fn run(state: &mut BlockState) -> Option<(Node, usize)> {
        Self::check(state)?;

        let open_line = state.get_line(state.line).to_owned();
        let (open_colons, rest) = Self::parse_fence(&open_line)?;
        let (dtype, arg) = Self::parse_directive(rest)?;
        let dtype_lower = dtype.to_lowercase();

        // Find closing fence (same or more colons, no directive)
        let start_line = state.line;
        let mut end_line = start_line + 1;
        while end_line < state.line_max {
            let l = state.get_line(end_line);
            if let Some((c, r)) = Self::parse_fence(l) {
                if c >= open_colons && r.is_empty() {
                    end_line += 1; // consume closing fence
                    break;
                }
            }
            end_line += 1;
        }

        // Collect body lines (between open and close)
        let body_start = start_line + 1;
        let body_end = if end_line > start_line + 1 {
            end_line - 1 // exclude closing fence
        } else {
            end_line
        };

        // Parse options (`:key: value` lines at start of body) and caption
        let mut options: Vec<(String, String)> = Vec::new();
        let mut content_start = body_start;
        for i in body_start..body_end {
            let l = state.get_line(i).trim().to_owned();
            if l.starts_with(':') && l.len() > 1 {
                if let Some(colon2) = l[1..].find(':') {
                    let key = l[1..1 + colon2].trim().to_string();
                    let val = l[2 + colon2..].trim().to_string();
                    options.push((key, val));
                    content_start = i + 1;
                    continue;
                }
            }
            // Skip leading blank lines after options
            if l.is_empty() && content_start == i {
                content_start = i + 1;
                continue;
            }
            break;
        }

        let lines_consumed = end_line - start_line;

        // ── YouTube ──
        if dtype_lower == "youtube" {
            let video_id = arg.to_string();
            if video_id.is_empty() {
                return None;
            }
            let node = Node::new(YouTubeEmbed { video_id });
            return Some((node, lines_consumed));
        }

        // ── Figure ──
        if dtype_lower == "figure" {
            let src = arg.to_string();
            let alt = options.iter()
                .find(|(k, _)| k == "alt")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let width = options.iter()
                .find(|(k, _)| k == "width")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();

            let mut node = Node::new(Figure { src, alt, width });

            // Parse caption from remaining body
            if content_start < body_end {
                let old_node = std::mem::replace(&mut state.node, node);
                let old_line_max = state.line_max;
                state.line = content_start;
                state.line_max = body_end;
                state.md.block.tokenize(state);
                state.line = start_line;
                state.line_max = old_line_max;
                node = std::mem::replace(&mut state.node, old_node);
            }

            return Some((node, lines_consumed));
        }

        // ── Admonition ──
        let title = if !arg.is_empty() {
            arg.to_string()
        } else {
            // Default title from type name
            let mut chars = dtype_lower.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => dtype_lower.clone(),
            }
        };

        let mut node = Node::new(ColonAdmonition {
            kind: dtype_lower,
            title,
        });

        // Parse nested content
        if content_start < body_end {
            let old_node = std::mem::replace(&mut state.node, node);
            let old_line_max = state.line_max;
            state.line = content_start;
            state.line_max = body_end;
            state.md.block.tokenize(state);
            state.line = start_line;
            state.line_max = old_line_max;
            node = std::mem::replace(&mut state.node, old_node);
        }

        Some((node, lines_consumed))
    }
}

// ── Plugin registration ─────────────────────────────────────────────────

pub fn add(md: &mut MarkdownIt) {
    md.block.add_rule::<DirectiveScanner>().before_all();
}
