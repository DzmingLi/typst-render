//! Callout plugin for markdown-it.
//!
//! Converts blockquotes with `> [!type] Optional Name` syntax into theorem
//! environment divs, matching Typst's fx/lib.typ CSS classes.
//!
//! Implemented as a core rule that walks the AST after parsing and transforms
//! matching blockquote nodes.

use markdown_it::parser::core::CoreRule;
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

const CALLOUT_TYPES: &[(&str, &str, &str)] = &[
    ("theorem", "Theorem", "thm"),
    ("thm",     "Theorem", "thm"),
    ("lemma",   "Lemma",   "thm"),
    ("corollary","Corollary","thm"),
    ("proposition","Proposition","thm"),
    ("definition","Definition","defn"),
    ("def",     "Definition","defn"),
    ("proof",   "Proof",   "proof"),
    ("remark",  "Remark",  "remark"),
    ("example", "Example", "example"),
    ("solution","Solution","example"),
];

#[derive(Debug)]
pub struct TheoremBlock {
    pub label: String,
    pub css_class: String,
    pub name: Option<String>,
}

impl NodeValue for TheoremBlock {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push(("class", format!("thm-block thm-{}", self.css_class)));
        fmt.cr();
        fmt.open("div", &attrs);
        fmt.cr();

        // Header
        let header = if let Some(ref name) = self.name {
            format!("<strong>{} ({})</strong>. ", self.label, name)
        } else {
            format!("<strong>{}</strong>. ", self.label)
        };
        fmt.text_raw(&format!("<p>{header}"));

        // Render remaining children inline
        fmt.contents(&node.children);
        fmt.text_raw("</p>");
        fmt.cr();
        fmt.close("div");
        fmt.cr();
    }
}

struct CalloutCoreRule;

impl CoreRule for CalloutCoreRule {
    fn run(root: &mut Node, _md: &MarkdownIt) {
        transform_callouts(root);
    }
}

fn transform_callouts(node: &mut Node) {
    let mut replacements: Vec<(usize, String, String, Option<String>)> = Vec::new();

    for (i, child) in node.children.iter().enumerate() {
        use markdown_it::plugins::cmark::block::blockquote::Blockquote;
        if !child.is::<Blockquote>() {
            continue;
        }

        // Check if the first child paragraph starts with [!type]
        if child.children.is_empty() {
            continue;
        }

        let first_text = child.children[0].collect_text();
        let trimmed = first_text.trim_start();

        if !trimmed.starts_with("[!") {
            continue;
        }

        let close = match trimmed.find(']') {
            Some(p) => p,
            None => continue,
        };

        let kind_str = &trimmed[2..close];
        let after = trimmed[close + 1..].trim();

        let (label, css_class) = match CALLOUT_TYPES.iter().find(|(k, _, _)| k.eq_ignore_ascii_case(kind_str)) {
            Some((_, l, c)) => (l.to_string(), c.to_string()),
            None => continue,
        };

        let name = if after.is_empty() { None } else { Some(after.to_string()) };

        replacements.push((i, label, css_class, name));
    }

    // Apply replacements in reverse
    for (idx, label, css_class, name) in replacements.into_iter().rev() {
        let old = &mut node.children[idx];
        // Remove the first child (the [!type] paragraph), keep the rest
        if !old.children.is_empty() {
            old.children.remove(0);
        }
        let children = std::mem::take(&mut old.children);
        let mut new_node = Node::new(TheoremBlock { label, css_class, name });
        new_node.children = children;
        node.children[idx] = new_node;
    }

    // Recurse
    for child in &mut node.children {
        transform_callouts(child);
    }
}

pub fn add(md: &mut MarkdownIt) {
    md.add_rule::<CalloutCoreRule>();
}
