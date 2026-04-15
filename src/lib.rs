pub mod convert;
pub mod heading_extract;
pub mod markdown_render;
pub mod md_plugins;
pub mod typst_render;

use std::path::Path;

pub use convert::convert_format;
pub use markdown_render::{render_markdown_to_html, render_markdown_series, render_latex_to_mathml};
pub use typst_render::{render_typst_to_html, render_typst_to_html_with_images, render_series_to_html, render_series_full_html, set_packages_dir, RenderConfig, read_chapter_order};

/// Fedi-Xanadu standard Typst library (theorem environments, layout helpers).
/// Consumers can inject this via `RenderConfig::extra_files`.
pub const FX_LIB_TYP: &str = include_str!("../typst-libs/fx/lib.typ");

/// Build a `RenderConfig` that imports the bundled `fx/lib.typ`.
pub fn fx_render_config() -> RenderConfig {
    RenderConfig {
        extra_preamble: "#import \"fx/lib.typ\": *\n".to_string(),
        extra_files: vec![("fx/lib.typ".to_string(), FX_LIB_TYP.to_string())],
    }
}

/// Map a content format identifier to its canonical file extension.
pub fn format_extension(format: &str) -> &'static str {
    match format {
        "markdown" => "md",
        "html" => "html",
        _ => "typ",
    }
}

/// Render source content to HTML based on format.
///
/// `repo_path` is used by Typst to resolve images; other formats ignore it.
pub fn render_to_html(format: &str, source: &str, repo_path: &Path) -> anyhow::Result<String> {
    match format {
        "markdown" => render_markdown_to_html(source),
        "html" => Ok(source.to_string()),
        _ => render_typst_to_html_with_images(source, repo_path),
    }
}

/// Render source content to HTML with custom render configuration.
pub fn render_to_html_with_config(format: &str, source: &str, repo_path: &Path, config: &RenderConfig) -> anyhow::Result<String> {
    match format {
        "markdown" => render_markdown_to_html(source),
        "html" => Ok(source.to_string()),
        _ => {
            let world = typst_render::RenderWorld::with_config(source, Some(repo_path), config);
            typst_render::render_world(&world)
        }
    }
}
