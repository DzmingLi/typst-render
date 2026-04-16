//! Markdown rendering via markdown-it with custom plugins.
//!
//! Pipeline:
//!   source → markdown-it parser (with plugins) → AST → core rules → HTML
//!
//! Plugins:
//!   - cmark: full CommonMark support
//!   - extra: tables, strikethrough, heading anchors
//!   - footnotes: [^1] syntax
//!   - front-matter: YAML front matter (stripped)
//!   - math: $...$ inline, $$...$$ display → MathML
//!   - admonition: !!!/???/???+ blocks
//!   - ial: {: .class #id} inline attribute lists
//!   - callout: > [!theorem] → theorem divs

use crate::md_plugins;

/// Build a configured markdown-it parser with all NightBoat plugins.
fn build_parser() -> markdown_it::MarkdownIt {
    let mut md = markdown_it::MarkdownIt::new();

    // Standard plugins
    markdown_it::plugins::cmark::add(&mut md);
    markdown_it::plugins::extra::add(&mut md);
    markdown_it::plugins::extra::heading_anchors::add(&mut md, slugify);
    markdown_it::plugins::html::add(&mut md);
    markdown_it_footnote::add(&mut md);
    markdown_it_front_matter::add(&mut md);

    // Custom plugins
    md_plugins::math::add(&mut md);
    md_plugins::admonition::add(&mut md);
    md_plugins::ial::add(&mut md);
    md_plugins::callout::add(&mut md);
    md_plugins::directive::add(&mut md);

    md
}

/// Slugify function for heading IDs.
/// Supports ASCII + CJK characters, lowercased, non-alnum replaced with `-`.
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c >= '\u{4e00}' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Render Markdown to HTML using the markdown-it plugin pipeline.
///
/// Supports all standard Markdown features plus:
/// - LaTeX math ($...$, $$...$$) → MathML
/// - MkDocs admonitions (!!!, ???, ???+)
/// - Inline attribute lists ({: .class #id})
/// - Theorem callouts (> [!theorem])
/// - Footnotes, tables, heading anchors, front matter
pub fn render_markdown_to_html(source: &str) -> anyhow::Result<String> {
    let md = build_parser();
    let ast = md.parse(source);
    Ok(ast.render())
}

/// Render a markdown series by concatenating all chapters.
pub fn render_markdown_series(chapters: &[(String, String)]) -> anyhow::Result<String> {
    let md = build_parser();
    let mut full_html = String::new();
    for (_uri, source) in chapters {
        let ast = md.parse(source);
        full_html.push_str(&ast.render());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_markdown() {
        let html = render_markdown_to_html("# Hello\n\nSome **bold** text").unwrap();
        assert!(html.contains("Hello"));
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn test_inline_math() {
        let html = render_markdown_to_html("The formula $x^2 + y^2 = r^2$ is a circle.").unwrap();
        assert!(html.contains("<math"), "should contain MathML: {html}");
    }

    #[test]
    fn test_display_math() {
        let html = render_markdown_to_html("Display:\n\n$$\nE = mc^2\n$$").unwrap();
        assert!(html.contains("<math"), "should contain MathML: {html}");
        assert!(html.contains(r#"display="block""#), "should be block display: {html}");
    }

    #[test]
    fn test_code_block() {
        let html = render_markdown_to_html("```rust\nfn main() {}\n```").unwrap();
        assert!(html.contains("<pre"), "should contain pre: {html}");
        assert!(html.contains("fn main()") || html.contains("fn </span>"), "should contain code content: {html}");
    }

    #[test]
    fn test_table() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<table"), "should contain table: {html}");
    }

    #[test]
    fn test_footnotes() {
        let md = "Text[^1].\n\n[^1]: Footnote content.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("Footnote content"), "should contain footnote: {html}");
    }

    #[test]
    fn test_heading_id() {
        let md = "## Hello World\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("id=\""), "should have heading ID: {html}");
    }

    #[test]
    fn test_front_matter_stripped() {
        let md = "---\ntitle: Test\n---\n\n# Hello\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(!html.contains("title: Test"), "front matter should be stripped: {html}");
        assert!(html.contains("Hello"), "content should remain: {html}");
    }

    // ── Admonition tests ──

    #[test]
    fn test_admonition_basic() {
        let md = "!!! note \"Important\"\n    This is a note.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="admonition note""#), "missing class: {html}");
        assert!(html.contains("admonition-title"), "missing title: {html}");
        assert!(html.contains("Important"), "missing title text: {html}");
        assert!(html.contains("This is a note."), "missing body: {html}");
    }

    #[test]
    fn test_admonition_collapsible() {
        let md = "??? tip \"Hint\"\n    Some hint.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<details"), "should be details: {html}");
        assert!(html.contains(r#"class="admonition tip""#), "missing class: {html}");
        assert!(html.contains("<summary>Hint</summary>"), "missing summary: {html}");
    }

    #[test]
    fn test_admonition_collapsible_open() {
        let md = "???+ example \"Demo\"\n    Example content.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<details"), "should be details: {html}");
        assert!(html.contains("open"), "should be open: {html}");
    }

    #[test]
    fn test_admonition_nested() {
        let md = "!!! note \"Outer\"\n    Outer text.\n\n    !!! tip \"Inner\"\n        Inner text.\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"admonition note"#), "missing outer: {html}");
        assert!(html.contains(r#"admonition tip"#), "missing inner: {html}");
    }

    // ── IAL tests ──

    #[test]
    fn test_ial_class() {
        let md = "Caption text\n{: .caption }\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="caption""#), "missing class: {html}");
    }

    #[test]
    fn test_ial_id() {
        let md = "Some paragraph.\n\n{: #my-para }\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"id="my-para""#), "missing id: {html}");
    }

    // ── Callout tests ──

    #[test]
    fn test_theorem_callout() {
        let md = "> [!theorem] Pythagorean\n> $a^2 + b^2 = c^2$";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"thm-block thm-thm"#), "missing thm class: {html}");
        assert!(html.contains("Pythagorean"), "missing name: {html}");
    }

    #[test]
    fn test_definition_callout() {
        let md = "> [!definition]\n> A group is a set with a binary operation.";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"thm-block thm-defn"#), "missing defn class: {html}");
    }

    #[test]
    fn test_regular_blockquote_unchanged() {
        let md = "> This is a normal blockquote.";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<blockquote>"), "blockquote should remain: {html}");
    }

    // ── Directive tests ──

    #[test]
    fn test_colon_admonition() {
        let md = ":::{note}\nThis is a note.\n:::\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"class="admonition note""#), "missing class: {html}");
        assert!(html.contains("This is a note."), "missing body: {html}");
    }

    #[test]
    fn test_colon_admonition_with_title() {
        let md = ":::{warning} Be careful!\nDangerous stuff here.\n:::\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"admonition warning"#), "missing class: {html}");
        assert!(html.contains("Be careful!"), "missing title: {html}");
    }

    #[test]
    fn test_youtube_embed() {
        let md = ":::{youtube} dQw4w9WgXcQ\n:::\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("youtube-nocookie.com/embed/dQw4w9WgXcQ"), "missing embed: {html}");
        assert!(html.contains("iframe"), "missing iframe: {html}");
    }

    #[test]
    fn test_figure() {
        let md = ":::{figure} https://example.com/img.png\n:alt: A photo\n:width: 80%\n\nThis is the caption.\n:::\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains("<figure"), "missing figure: {html}");
        assert!(html.contains(r#"src="https://example.com/img.png""#), "missing src: {html}");
        assert!(html.contains(r#"alt="A photo""#), "missing alt: {html}");
        assert!(html.contains("width:80%"), "missing width: {html}");
        assert!(html.contains("<figcaption>"), "missing figcaption: {html}");
        assert!(html.contains("This is the caption."), "missing caption: {html}");
    }

    #[test]
    fn test_nested_directives() {
        let md = "::::{note}\nOuter\n\n:::{tip}\nInner\n:::\n\n::::\n";
        let html = render_markdown_to_html(md).unwrap();
        assert!(html.contains(r#"admonition note"#), "missing outer: {html}");
        assert!(html.contains(r#"admonition tip"#), "missing inner: {html}");
    }

    // ── Integration test ──

    #[test]
    fn test_linux101_renders() {
        let source = include_str!("../test_data/linux101-ch1.md");
        let html = render_markdown_to_html(source).unwrap();
        assert!(html.len() > 5000, "should produce substantial HTML");
        assert!(!html.contains("!!!"), "admonitions should be processed");
        // Count remaining IAL markers
        let ial_count = html.matches("{:").count();
        eprintln!("Remaining IAL markers: {ial_count}");
        if ial_count > 0 {
            for (i, line) in html.lines().enumerate() {
                if line.contains("{:") {
                    eprintln!("  IAL at html line {i}: {}", &line[..line.len().min(200)]);
                }
            }
        }
        // 1 remaining is the inline image IAL `{: .img-inline }` which is not block-level
        assert!(ial_count <= 1, "block IAL should be processed ({ial_count} remaining)");
        assert!(html.contains("admonition"), "should have admonition blocks");
    }
}
