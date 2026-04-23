//! Inline Attribute Lists (IAL) plugin for markdown-it.
//!
//! kramdown / PHP Markdown Extra syntax: `{: .class #id }`
//!
//! A line containing only `{: ... }` applies CSS classes and IDs to the
//! preceding block element. Implemented as a core rule that walks the AST
//! after parsing and merges IAL paragraphs into their preceding sibling.

use markdown_it::parser::core::CoreRule;
use markdown_it::{MarkdownIt, Node};

// Note: an earlier iteration wrapped each IAL-attributed block in a custom
// `IalWrapper` NodeValue with its own `render` method. That approach was
// abandoned — the current rule mutates `node.attrs` on the preceding/current
// sibling directly, so no wrapper type is needed.

/// Regex for a paragraph that contains only `{: .class #id }`.
fn parse_ial_text(text: &str) -> Option<(Vec<String>, Option<String>)> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix("{:")?.strip_suffix('}')?;
    let inner = inner.trim();

    let mut classes = Vec::new();
    let mut id = None;

    for part in inner.split_whitespace() {
        if let Some(cls) = part.strip_prefix('.') {
            classes.push(cls.to_string());
        } else if let Some(i) = part.strip_prefix('#') {
            id = Some(i.to_string());
        }
    }

    if classes.is_empty() && id.is_none() {
        return None;
    }

    Some((classes, id))
}

/// Check if a node is a paragraph containing only IAL text.
fn extract_ial_from_node(node: &Node) -> Option<(Vec<String>, Option<String>)> {
    use markdown_it::plugins::cmark::block::paragraph::Paragraph;
    if !node.is::<Paragraph>() {
        return None;
    }

    let text = node.collect_text();
    parse_ial_text(&text)
}

struct IalCoreRule;

impl CoreRule for IalCoreRule {
    fn run(root: &mut Node, _md: &MarkdownIt) {
        apply_ial_recursive(root);
    }
}

fn apply_ial_recursive(node: &mut Node) {
    use markdown_it::plugins::cmark::block::paragraph::Paragraph;
    use markdown_it::plugins::cmark::inline::newline::Softbreak;

    let mut removals = Vec::new();

    for i in 0..node.children.len() {
        // Case 1: standalone IAL paragraph → apply to preceding sibling
        if let Some((classes, id)) = extract_ial_from_node(&node.children[i]) {
            if i > 0 {
                for cls in &classes {
                    node.children[i - 1].attrs.push(("class", cls.clone()));
                }
                if let Some(ref id_val) = id {
                    node.children[i - 1].attrs.push(("id", id_val.clone()));
                }
                removals.push(i);
            }
            continue;
        }

        // Case 2: IAL at the end of a paragraph (e.g. "text\n{: .class }")
        if !node.children[i].is::<Paragraph>() {
            continue;
        }
        let para = &node.children[i];
        let text = para.collect_text();

        // Check if last line is IAL
        if let Some(nl_pos) = text.rfind('\n') {
            let last_line = text[nl_pos + 1..].trim();
            if let Some((classes, id)) = parse_ial_text(last_line) {
                // Apply to THIS paragraph and remove IAL text from children
                for cls in &classes {
                    node.children[i].attrs.push(("class", cls.clone()));
                }
                if let Some(ref id_val) = id {
                    node.children[i].attrs.push(("id", id_val.clone()));
                }
                // Remove the softbreak + IAL text node from children
                let children = &mut node.children[i].children;
                // Find and remove trailing IAL: typically the last 1-2 children
                // (a Softbreak + text node containing "{: ...}")
                while let Some(last) = children.last() {
                    let t = last.collect_text();
                    if parse_ial_text(t.trim()).is_some() || last.is::<Softbreak>() {
                        children.pop();
                    } else {
                        break;
                    }
                }
            }
        }
    }

    for &idx in removals.iter().rev() {
        node.children.remove(idx);
    }

    for child in &mut node.children {
        apply_ial_recursive(child);
    }
}

// ── Plugin registration ─────────────────────────────────────────────────

pub fn add(md: &mut MarkdownIt) {
    md.add_rule::<IalCoreRule>();
}
