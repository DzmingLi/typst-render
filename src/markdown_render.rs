use pulldown_cmark::{Options, Parser, Event, html};

/// Render Markdown with LaTeX math to HTML using MathML.
///
/// Math delimiters: `$...$` for inline, `$$...$$` for display.
/// LaTeX math is converted to MathML server-side for native browser rendering.
///
/// Also supports MkDocs-style admonitions:
/// - `!!! type "title"` — always-open admonition
/// - `??? type "title"` — collapsible (closed by default)
/// - `???+ type "title"` — collapsible (open by default)
pub fn render_markdown_to_html(source: &str) -> anyhow::Result<String> {
    let preprocessed = preprocess_ial_owned(&preprocess_admonitions(source));

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_MATH);

    let parser = Parser::new_ext(&preprocessed, options);

    // Transform math events into MathML
    let events: Vec<Event<'_>> = parser.map(|event| match event {
        Event::InlineMath(math) => {
            match latex2mathml::latex_to_mathml(&math, latex2mathml::DisplayStyle::Inline) {
                Ok(mathml) => Event::Html(mathml.into()),
                Err(_) => {
                    let escaped = html_escape(&math);
                    Event::Html(format!(r#"<code class="math-error">{escaped}</code>"#).into())
                }
            }
        }
        Event::DisplayMath(math) => {
            match latex2mathml::latex_to_mathml(&math, latex2mathml::DisplayStyle::Block) {
                Ok(mathml) => Event::Html(mathml.into()),
                Err(_) => {
                    let escaped = html_escape(&math);
                    Event::Html(format!(r#"<div class="math-error"><code>{escaped}</code></div>"#).into())
                }
            }
        }
        other => other,
    }).collect();

    let mut html_output = String::new();
    html::push_html(&mut html_output, events.into_iter());

    Ok(convert_callouts(&html_output))
}

// ── Admonition pre-processing ────────────────────────────────────────────
//
// MkDocs-Material admonition syntax is not part of CommonMark, so we convert
// it to raw HTML before handing the source to pulldown-cmark.
//
// Supported forms:
//   !!! note "Title"          → <div class="admonition note"><p class="admonition-title">Title</p>...
//   ??? warning "Title"       → <details class="admonition warning"><summary>Title</summary>...
//   ???+ tip "Title"          → <details class="admonition tip" open><summary>Title</summary>...
//   !!! note                  → title defaults to capitalised type name
//
// Body lines must be indented by 4 spaces. Nesting is supported.

/// Recognised admonition type names (matches MkDocs-Material).
const ADMONITION_TYPES: &[&str] = &[
    "note", "abstract", "info", "tip", "success", "question",
    "warning", "failure", "danger", "bug", "example", "quote",
];

fn admonition_type_valid(ty: &str) -> bool {
    ADMONITION_TYPES.contains(&ty.to_lowercase().as_str())
}

/// Parse an admonition header line. Returns (is_collapsible, is_open, type, title).
fn parse_admonition_header(line: &str) -> Option<(bool, bool, String, String)> {
    let trimmed = line.trim_end();
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
    // Extract type (first word)
    let (ty, after_type) = match rest.find(|c: char| c.is_whitespace()) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };

    if !admonition_type_valid(ty) {
        return None;
    }

    let ty_lower = ty.to_lowercase();

    // Extract title: either "quoted" or plain text, or default to type name
    let title = if after_type.starts_with('"') && after_type.ends_with('"') && after_type.len() >= 2 {
        after_type[1..after_type.len() - 1].to_string()
    } else if !after_type.is_empty() {
        after_type.to_string()
    } else {
        // Default title: capitalised type
        let mut chars = ty_lower.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            None => ty_lower.clone(),
        }
    };

    let is_collapsible = marker != "!!!";
    let is_open = marker == "???+";

    Some((is_collapsible, is_open, ty_lower, title))
}

/// Pre-process admonition blocks in markdown source, converting them to HTML.
fn preprocess_admonitions(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut result = String::with_capacity(source.len());
    let mut i = 0;

    while i < lines.len() {
        if let Some((is_collapsible, is_open, ty, title)) = parse_admonition_header(lines[i]) {
            // Collect indented body lines (4 spaces)
            let mut body_lines: Vec<&str> = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let line = lines[j];
                if line.starts_with("    ") {
                    body_lines.push(&line[4..]);
                    j += 1;
                } else if line.trim().is_empty() {
                    // Blank line within block — keep it if more indented lines follow
                    let mut has_more = false;
                    for k in (j + 1)..lines.len() {
                        if lines[k].starts_with("    ") {
                            has_more = true;
                            break;
                        } else if !lines[k].trim().is_empty() {
                            break;
                        }
                    }
                    if has_more {
                        body_lines.push("");
                        j += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Recursively process the body (supports nested admonitions)
            let body_source = body_lines.join("\n");
            let body_processed = preprocess_admonitions(&body_source);

            // Render body markdown to HTML inline
            let title_escaped = html_escape(&title);

            if is_collapsible {
                let open_attr = if is_open { " open" } else { "" };
                result.push_str(&format!(
                    "\n<details class=\"admonition {ty}\"{open_attr}>\n<summary>{title_escaped}</summary>\n\n{body_processed}\n\n</details>\n\n"
                ));
            } else {
                result.push_str(&format!(
                    "\n<div class=\"admonition {ty}\">\n<p class=\"admonition-title\">{title_escaped}</p>\n\n{body_processed}\n\n</div>\n\n"
                ));
            }

            i = j;
        } else {
            result.push_str(lines[i]);
            result.push('\n');
            i += 1;
        }
    }

    result
}

// ── Inline Attribute Lists (IAL) pre-processing ──────────────────────────
//
// kramdown / PHP Markdown Extra syntax: `{: .class #id }`
// A line containing only `{: ... }` applies attributes to the preceding block.
// We handle this by removing the IAL line from source and wrapping the
// preceding paragraph/element in a `<div>` with the specified attributes,
// or for inline images, by injecting class/id into the image markdown.

/// Pre-process IAL `{: .class #id }` lines in markdown source.
/// Strips IAL lines and wraps the preceding block in an HTML `<div>`.
fn preprocess_ial_owned(source: &str) -> String {
    let ial_re = regex::Regex::new(r"^\{:\s*([^}]+)\}\s*$").unwrap();
    let lines: Vec<&str> = source.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());

    let mut i = 0;
    while i < lines.len() {
        if let Some(caps) = ial_re.captures(lines[i].trim()) {
            let attrs_str = caps.get(1).unwrap().as_str().trim();
            let mut classes = Vec::new();
            let mut id: Option<&str> = None;
            for part in attrs_str.split_whitespace() {
                if let Some(cls) = part.strip_prefix('.') {
                    classes.push(cls);
                } else if let Some(i_val) = part.strip_prefix('#') {
                    id = Some(i_val);
                }
            }

            if classes.is_empty() && id.is_none() {
                out.push(lines[i].to_string());
                i += 1;
                continue;
            }

            let mut attr_parts = Vec::new();
            if !classes.is_empty() {
                attr_parts.push(format!("class=\"{}\"", classes.join(" ")));
            }
            if let Some(id_val) = id {
                attr_parts.push(format!("id=\"{id_val}\""));
            }
            let attr_str = attr_parts.join(" ");

            // Find preceding non-empty content and wrap it
            let mut prev_idx = None;
            for j in (0..out.len()).rev() {
                if !out[j].trim().is_empty() {
                    prev_idx = Some(j);
                    break;
                }
            }

            if let Some(idx) = prev_idx {
                let prev = out[idx].clone();
                out[idx] = format!("<div {attr_str}>\n\n{prev}\n\n</div>");
            }
            // Skip the IAL line
            i += 1;
        } else {
            out.push(lines[i].to_string());
            i += 1;
        }
    }

    out.join("\n")
}

/// Convert callout-style blockquotes to theorem environment divs.
///
/// Syntax: `> [!type] Optional Name`
///
/// Supported types: theorem, lemma, corollary, proposition, definition,
/// proof, remark, example, solution.
///
/// Renders with the same CSS classes as Typst's fx/lib.typ:
/// `<div class="thm-block thm-thm">...</div>`
fn convert_callouts(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut pos = 0;

    while pos < html.len() {
        if let Some(bq_start) = html[pos..].find("<blockquote>") {
            let abs_start = pos + bq_start;
            result.push_str(&html[pos..abs_start]);

            if let Some(bq_end) = html[abs_start..].find("</blockquote>") {
                let abs_end = abs_start + bq_end + "</blockquote>".len();
                let inner = &html[abs_start + "<blockquote>".len()..abs_start + bq_end];

                if let Some(callout) = parse_callout(inner) {
                    result.push_str(&callout);
                } else {
                    result.push_str(&html[abs_start..abs_end]);
                }
                pos = abs_end;
            } else {
                result.push_str(&html[abs_start..]);
                break;
            }
        } else {
            result.push_str(&html[pos..]);
            break;
        }
    }

    result
}

fn parse_callout(inner_html: &str) -> Option<String> {
    // Inner starts with \n<p>[!type]... Look for the pattern
    let trimmed = inner_html.trim();
    let content = trimmed.strip_prefix("<p>")?;

    // Match [!type] at the start
    let after_bracket = content.strip_prefix("[!")?;
    let close = after_bracket.find(']')?;
    let kind_str = &after_bracket[..close];
    let rest = &after_bracket[close + 1..];

    let (label, css_class) = match kind_str.to_lowercase().as_str() {
        "theorem" | "thm" => ("Theorem", "thm"),
        "lemma" => ("Lemma", "thm"),
        "corollary" => ("Corollary", "thm"),
        "proposition" => ("Proposition", "thm"),
        "definition" | "def" => ("Definition", "defn"),
        "proof" => ("Proof", "proof"),
        "remark" => ("Remark", "remark"),
        "example" => ("Example", "example"),
        "solution" => ("Solution", "example"),
        _ => return None,
    };

    // Extract optional name (text after ] on the same line, before </p> or newline)
    let rest = rest.trim_start();
    let (name, body) = if let Some(p_end) = rest.find("</p>") {
        let first_line = rest[..p_end].trim();
        let remaining = rest[p_end + "</p>".len()..].trim();
        if first_line.is_empty() {
            (None, remaining.to_string())
        } else {
            (Some(first_line.to_string()), remaining.to_string())
        }
    } else {
        (None, rest.to_string())
    };

    let header = if let Some(name) = &name {
        format!("<strong>{label} ({name})</strong>. ")
    } else {
        format!("<strong>{label}</strong>. ")
    };

    // Strip leading <p> from body if present, merge with header
    let body = body.trim();
    let body_html = if let Some(stripped) = body.strip_prefix("<p>") {
        format!("<p>{header}{stripped}")
    } else if body.is_empty() {
        format!("<p>{header}</p>")
    } else {
        format!("<p>{header}</p>\n{body}")
    };

    Some(format!(
        r#"<div class="thm-block thm-{css_class}">{body_html}</div>"#
    ))
}

/// Render a markdown series by concatenating all chapters.
/// Each chapter is rendered individually and the results are joined.
/// Returns the full HTML.
pub fn render_markdown_series(chapters: &[(String, String)]) -> anyhow::Result<String> {
    let mut full_html = String::new();
    for (_uri, source) in chapters {
        let chapter_html = render_markdown_to_html(source)?;
        full_html.push_str(&chapter_html);
        full_html.push('\n');
    }
    Ok(full_html)
}

/// Render a single LaTeX math formula to MathML HTML.
pub fn render_latex_to_mathml(formula: &str, display: bool) -> anyhow::Result<String> {
    let style = if display {
        latex2mathml::DisplayStyle::Block
    } else {
        latex2mathml::DisplayStyle::Inline
    };
    latex2mathml::latex_to_mathml(formula, style)
        .map_err(|e| anyhow::anyhow!("LaTeX to MathML conversion failed: {e}"))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_markdown() {
        let html = render_markdown_to_html("# Hello\n\nSome **bold** text").unwrap();
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn test_inline_math_mathml() {
        let html = render_markdown_to_html("The formula $x^2 + y^2 = r^2$ is a circle.").unwrap();
        assert!(html.contains("<math"));
        assert!(html.contains("</math>"));
    }

    #[test]
    fn test_display_math_mathml() {
        let html = render_markdown_to_html("Display:\n\n$$\nE = mc^2\n$$").unwrap();
        assert!(html.contains("<math"));
        assert!(html.contains(r#"display="block""#));
    }

    #[test]
    fn test_code_block() {
        let html = render_markdown_to_html("```rust\nfn main() {}\n```").unwrap();
        assert!(html.contains("<code"));
        assert!(html.contains("fn main()"));
    }

    #[test]
    fn test_table() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<table>"));
    }

    #[test]
    fn test_theorem_callout() {
        let md = "> [!theorem] Pythagorean\n> For a right triangle, $a^2 + b^2 = c^2$.";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="thm-block thm-thm""#), "missing thm class: {html}");
        assert!(html.contains("Theorem (Pythagorean)"), "missing name: {html}");
    }

    #[test]
    fn test_definition_callout() {
        let md = "> [!definition]\n> A group is a set with a binary operation.";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="thm-block thm-defn""#), "missing defn class: {html}");
        assert!(html.contains("<strong>Definition</strong>"), "missing label: {html}");
    }

    #[test]
    fn test_proof_callout() {
        let md = "> [!proof]\n> Obvious.";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="thm-block thm-proof""#), "missing proof class: {html}");
    }

    #[test]
    fn test_regular_blockquote_unchanged() {
        let md = "> This is a normal blockquote.";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<blockquote>"), "blockquote should remain: {html}");
        assert!(!html.contains("thm-block"), "should not be a theorem: {html}");
    }

    // ── Admonition tests ──

    #[test]
    fn test_admonition_basic() {
        let md = "!!! note \"Important\"\n    This is a note.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="admonition note""#), "missing class: {html}");
        assert!(html.contains(r#"class="admonition-title">Important"#), "missing title: {html}");
        assert!(html.contains("This is a note."), "missing body: {html}");
    }

    #[test]
    fn test_admonition_default_title() {
        let md = "!!! warning\n    Be careful.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="admonition warning""#), "missing class: {html}");
        assert!(html.contains("Warning"), "missing default title: {html}");
    }

    #[test]
    fn test_admonition_collapsible_closed() {
        let md = "??? tip \"Hint\"\n    Some hint.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<details"), "should be details: {html}");
        assert!(html.contains(r#"class="admonition tip""#), "missing class: {html}");
        assert!(html.contains("<summary>Hint</summary>"), "missing summary: {html}");
        assert!(!html.contains("open"), "should not be open: {html}");
    }

    #[test]
    fn test_admonition_collapsible_open() {
        let md = "???+ example \"Demo\"\n    Example content.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<details"), "should be details: {html}");
        assert!(html.contains("open"), "should be open: {html}");
        assert!(html.contains("<summary>Demo</summary>"), "missing summary: {html}");
    }

    #[test]
    fn test_admonition_multiline_body() {
        let md = "!!! info \"Multi\"\n    Line one.\n\n    Line two.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("Line one."), "missing line 1: {html}");
        assert!(html.contains("Line two."), "missing line 2: {html}");
    }

    #[test]
    fn test_admonition_with_markdown_body() {
        let md = "!!! note \"Rich\"\n    Some **bold** and `code`.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<strong>bold</strong>"), "missing bold: {html}");
        assert!(html.contains("<code>code</code>"), "missing code: {html}");
    }

    #[test]
    fn test_admonition_nested() {
        let md = "!!! note \"Outer\"\n    Outer text.\n\n    !!! tip \"Inner\"\n        Inner text.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="admonition note""#), "missing outer: {html}");
        assert!(html.contains(r#"class="admonition tip""#), "missing inner: {html}");
        assert!(html.contains("Outer text."), "missing outer body: {html}");
        assert!(html.contains("Inner text."), "missing inner body: {html}");
    }

    #[test]
    fn test_admonition_does_not_match_invalid_type() {
        let md = "!!! foobar \"Title\"\n    Body.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(!html.contains("admonition"), "should not match invalid type: {html}");
    }

    #[test]
    fn test_heading_attributes() {
        let md = "## Hello {#my-id}\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"id="my-id""#), "missing heading id: {html}");
    }

    #[test]
    fn test_footnotes() {
        let md = "Text[^1].\n\n[^1]: Footnote content.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("Footnote content"), "missing footnote: {html}");
    }

    // ── IAL tests ──

    #[test]
    fn test_ial_class() {
        let md = "![img](url)\n\nCaption text\n{: .caption }\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="caption""#), "missing class: {html}");
    }

    #[test]
    fn test_ial_id() {
        let md = "Some paragraph.\n{: #my-para }\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"id="my-para""#), "missing id: {html}");
    }

    #[test]
    fn test_ial_multiple_classes() {
        let md = "Text\n{: .img-inline .centered }\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("img-inline"), "missing class: {html}");
        assert!(html.contains("centered"), "missing class: {html}");
    }

    #[test]
    fn test_ial_no_match() {
        let md = "Normal text.\n\nMore text.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(!html.contains("{:"), "IAL should not appear in output: {html}");
    }
}
