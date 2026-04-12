use pulldown_cmark::{Options, Parser, Event, html};

/// Render Markdown with LaTeX math to HTML using MathML.
///
/// Math delimiters: `$...$` for inline, `$$...$$` for display.
/// LaTeX math is converted to MathML server-side for native browser rendering.
pub fn render_markdown_to_html(source: &str) -> anyhow::Result<String> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_MATH);

    let parser = Parser::new_ext(source, options);

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
}
