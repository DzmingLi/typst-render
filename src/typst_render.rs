use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Feature, Features, Library, LibraryExt, World};
use typst_html::{HtmlAttr, HtmlDocument, HtmlElement, HtmlNode};

/// Global packages cache directory. Set via `set_packages_dir()`.
static PACKAGES_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Set the global directory for caching Typst packages.
/// Call once at startup. Default: `{data_dir}/typst-packages`.
pub fn set_packages_dir(dir: PathBuf) {
    let _ = PACKAGES_DIR.set(dir);
}

fn packages_dir() -> Option<&'static Path> {
    PACKAGES_DIR.get().map(|p| p.as_path())
}

/// Mathyml library files, embedded at compile time.
const MATHYML_FILES: &[(&str, &str)] = &[
    ("mathyml/lib.typ", include_str!("../typst-libs/mathyml/lib.typ")),
    ("mathyml/convert.typ", include_str!("../typst-libs/mathyml/convert.typ")),
    ("mathyml/prelude.typ", include_str!("../typst-libs/mathyml/prelude.typ")),
    ("mathyml/unicode.typ", include_str!("../typst-libs/mathyml/unicode.typ")),
    ("mathyml/utils.typ", include_str!("../typst-libs/mathyml/utils.typ")),
];

/// Default preamble: import mathyml for MathML math rendering.
const DEFAULT_PREAMBLE: &str = r#"#import "mathyml/lib.typ": try-to-mathml, include-mathfont
#show math.equation: try-to-mathml
"#;

/// Extended preamble for series documents (heading numbering for cross-references).
const SERIES_PREAMBLE: &str = r#"#import "mathyml/lib.typ": try-to-mathml, include-mathfont
#show math.equation: try-to-mathml
#set heading(numbering: "1.1")
"#;

/// Configuration for customizing the Typst rendering environment.
///
/// Consumers can inject custom preamble lines and virtual library files.
/// For example, fedi-xanadu adds theorem environments via `fx/lib.typ`.
#[derive(Debug, Clone, Default)]
pub struct RenderConfig {
    /// Extra preamble lines appended after the default math preamble.
    /// Example: `#import "fx/lib.typ": *`
    pub extra_preamble: String,
    /// Extra virtual files to include in the Typst filesystem.
    /// Key: virtual path (e.g. "fx/lib.typ"), Value: file content.
    pub extra_files: Vec<(String, String)>,
}

pub struct RenderWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    sources: HashMap<FileId, Source>,
    /// Optional repo directory for resolving images and other binary files.
    repo_path: Option<PathBuf>,
}

impl RenderWorld {
    fn new(text: &str) -> Self {
        Self::with_preamble(text, DEFAULT_PREAMBLE, None, &[])
    }

    fn with_repo(text: &str, repo_path: Option<&Path>) -> Self {
        Self::with_preamble(text, DEFAULT_PREAMBLE, repo_path, &[])
    }

    pub fn with_config(text: &str, repo_path: Option<&Path>, config: &RenderConfig) -> Self {
        let preamble = format!("{}{}", DEFAULT_PREAMBLE, config.extra_preamble);
        let extra: Vec<(&str, &str)> = config.extra_files.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        Self::with_preamble(text, &preamble, repo_path, &extra)
    }

    fn with_series_preamble(text: &str, repo_path: Option<&Path>, config: &RenderConfig) -> Self {
        let preamble = format!("{}{}", SERIES_PREAMBLE, config.extra_preamble);
        let extra: Vec<(&str, &str)> = config.extra_files.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        Self::with_preamble(text, &preamble, repo_path, &extra)
    }

    fn with_preamble(text: &str, preamble: &str, repo_path: Option<&Path>, extra_files: &[(&str, &str)]) -> Self {
        let features: Features = [Feature::Html].into_iter().collect();
        let library = Library::builder().with_features(features).build();

        // Load bundled fonts
        let mut book = FontBook::new();
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            let buffer = Bytes::new(data.to_vec());
            for font in Font::iter(buffer) {
                book.push(font.info().clone());
                fonts.push(font);
            }
        }

        // Build virtual filesystem
        let mut sources = HashMap::new();

        // Add library + extra virtual files
        for (path, content) in MATHYML_FILES.iter().chain(extra_files.iter()) {
            let id = FileId::new(None, VirtualPath::new(path));
            sources.insert(id, Source::new(id, (*content).into()));
        }

        // Main source = preamble + user content
        let full_source = format!("{preamble}{text}");
        let main_id = FileId::new(None, VirtualPath::new("main.typ"));
        let main = Source::new(main_id, full_source);
        sources.insert(main_id, main.clone());

        Self {
            library: LazyHash::new(library),
            book: LazyHash::new(book),
            fonts,
            main,
            sources,
            repo_path: repo_path.map(|p| p.to_path_buf()),
        }
    }
}

impl World for RenderWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if let Some(s) = self.sources.get(&id) {
            return Ok(s.clone());
        }
        let bytes = self.file(id)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text.into()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(s) = self.sources.get(&id) {
            return Ok(Bytes::new(s.text().as_bytes().to_vec()));
        }

        // Try loading from package cache
        if let Some(pkg) = id.package() {
            if let Some(pkg_dir) = resolve_package(pkg) {
                let rel = id.vpath().as_rootless_path();
                let path = pkg_dir.join(rel);
                if path.exists() {
                    let data = std::fs::read(&path)
                        .map_err(|_| FileError::NotFound(rel.into()))?;
                    return Ok(Bytes::new(data));
                }
            }
            return Err(FileError::NotFound(id.vpath().as_rootless_path().into()));
        }

        // Try loading from repo directory (for images etc.)
        if let Some(ref repo) = self.repo_path {
            let rel = id.vpath().as_rootless_path();
            let path = repo.join(rel);
            if path.exists() {
                let data = std::fs::read(&path)
                    .map_err(|_| FileError::NotFound(rel.into()))?;
                return Ok(Bytes::new(data));
            }
        }
        Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        None
    }
}

/// Resolve a Typst package to a local directory, downloading if needed.
fn resolve_package(pkg: &typst::syntax::package::PackageSpec) -> Option<PathBuf> {
    let cache_dir = packages_dir()?;
    let pkg_dir = cache_dir
        .join(pkg.namespace.as_str())
        .join(pkg.name.as_str())
        .join(pkg.version.to_string());

    if pkg_dir.join("typst.toml").exists() {
        return Some(pkg_dir);
    }

    let url = format!(
        "https://packages.typst.org/{}/{}-{}.tar.gz",
        pkg.namespace, pkg.name, pkg.version
    );
    tracing::info!("downloading typst package: {url}");

    match download_and_extract_package(&url, &pkg_dir) {
        Ok(()) => Some(pkg_dir),
        Err(e) => {
            tracing::warn!("failed to download package {pkg}: {e}");
            None
        }
    }
}

fn download_and_extract_package(url: &str, dest: &Path) -> anyhow::Result<()> {
    let response = ureq::get(url).call()
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {e}"))?;

    let reader = response.into_body().into_reader();
    let gz = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(gz);

    std::fs::create_dir_all(dest)?;
    archive.unpack(dest)?;

    Ok(())
}

/// Render Typst source to HTML, resolving images from a repo directory.
/// Inline base64 images are extracted to `{repo_path}/_rendered/` as separate files.
pub fn render_typst_to_html_with_images(source: &str, repo_path: &Path) -> anyhow::Result<String> {
    let world = RenderWorld::with_repo(source, Some(repo_path));
    render_world_with_extraction(&world, Some(repo_path))
}

/// Render Typst source to HTML using Typst's native HTML export.
///
/// Math equations are automatically converted to MathML via the mathyml library.
pub fn render_typst_to_html(source: &str) -> anyhow::Result<String> {
    let world = RenderWorld::new(source);
    render_world(&world)
}

pub fn render_world(world: &RenderWorld) -> anyhow::Result<String> {
    render_world_with_extraction(world, None)
}

/// Compile the series source and collect `<nbt-chapter>` and `<nbt-summary>`
/// metadata nodes in document order.
///
/// Authors annotate chapters with typst's native `#metadata()` function +
/// label; the extractor just queries the introspector after compilation.
/// The preamble for series repos (`SERIES_PREAMBLE`) is applied so the
/// source resolves the same way as during rendering.
///
/// # Example
///
/// ```typst
/// = Introduction
/// #metadata((teaches: ("intro",), prereqs: (("basics", "required"),))) <nbt-chapter>
///
/// Visible summary sentence. <nbt-summary>
/// ```
pub fn extract_series_metadata(
    repo_path: &Path,
    config: &RenderConfig,
) -> anyhow::Result<SeriesMetadata> {
    let main_path = repo_path.join("main.typ");
    let source = if main_path.exists() {
        std::fs::read_to_string(&main_path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", main_path.display()))?
    } else {
        let chapter_files = read_chapter_order(repo_path, ".typ");
        build_auto_concat_source_from_files(&chapter_files, repo_path)?
    };
    let world = RenderWorld::with_series_preamble(&source, Some(repo_path), config);
    query_labels(&world)
}

/// Chapter-level and summary metadata extracted from a compiled Typst series.
#[derive(Debug, Default, Clone)]
pub struct SeriesMetadata {
    /// `#metadata(...) <nbt-chapter>` values, serialised to JSON and ordered
    /// by document position. Each entry typically contains `teaches` (array
    /// of tag ids) and `prereqs` (array of `(tag_id, type)` tuples).
    pub chapter_metadata: Vec<serde_json::Value>,
    /// `<nbt-summary>` block contents as plain text, ordered by document
    /// position. These double as visible lead paragraphs.
    pub summaries: Vec<String>,
}

fn query_labels(world: &RenderWorld) -> anyhow::Result<SeriesMetadata> {
    use typst::foundations::Selector;
    use typst::introspection::Location;

    let warned = typst::compile::<HtmlDocument>(world);
    let document = warned.output.map_err(|diags| {
        let msgs: Vec<String> = diags.iter().map(|d| d.message.to_string()).collect();
        anyhow::anyhow!("Typst compilation errors: {}", msgs.join("; "))
    })?;

    let chapter_label = typst::foundations::Label::construct(
        typst::foundations::Str::from("nbt-chapter"),
    ).map_err(|e| anyhow::anyhow!("invalid label: {e}"))?;
    let summary_label = typst::foundations::Label::construct(
        typst::foundations::Str::from("nbt-summary"),
    ).map_err(|e| anyhow::anyhow!("invalid label: {e}"))?;

    let introspector = &document.introspector;

    // Helper: sort results by location so the output order matches document order.
    let by_location = |content: &typst::foundations::Content| -> Option<Location> {
        content.location()
    };

    // <nbt-chapter>: typst's #metadata element exposes its payload via the
    // `value` field. Iterate, serialize each.
    let mut chapters: Vec<(Option<Location>, serde_json::Value)> = Vec::new();
    for c in introspector.query(&Selector::Label(chapter_label)) {
        let value_field = c.field_by_name("value").ok();
        let loc = by_location(&c);
        if let Some(v) = value_field {
            let json = serde_json::to_value(&v).unwrap_or(serde_json::Value::Null);
            chapters.push((loc, json));
        }
    }
    chapters.sort_by_key(|(l, _)| l.map(|x| x.hash()));

    // <nbt-summary>: content blocks. Render their plain-text form.
    let mut summaries: Vec<(Option<Location>, String)> = Vec::new();
    for c in introspector.query(&Selector::Label(summary_label)) {
        let text = plain_text_of(&c);
        let loc = by_location(&c);
        summaries.push((loc, text));
    }
    summaries.sort_by_key(|(l, _)| l.map(|x| x.hash()));

    Ok(SeriesMetadata {
        chapter_metadata: chapters.into_iter().map(|(_, v)| v).collect(),
        summaries: summaries.into_iter().map(|(_, s)| s).collect(),
    })
}

/// Best-effort plain-text extraction from Typst content. Walks into sequences
/// and styled nodes; unrecognized elements contribute an empty string.
fn plain_text_of(content: &typst::foundations::Content) -> String {
    use typst::foundations::Value;
    // The easiest path is to serialize and pull out string leaves; Content
    // doesn't expose a canonical "to plain text" but `#text` elements carry
    // the string in their `text` field.
    fn walk(content: &typst::foundations::Content, out: &mut String) {
        if let Ok(Value::Str(s)) = content.field_by_name("text") {
            out.push_str(s.as_str());
            return;
        }
        if let Ok(Value::Content(body)) = content.field_by_name("body") {
            walk(&body, out);
            return;
        }
        if let Ok(Value::Array(arr)) = content.field_by_name("children") {
            for item in arr.iter() {
                if let Value::Content(c) = item {
                    walk(c, out);
                }
            }
        }
    }
    let mut out = String::new();
    walk(content, &mut out);
    out.trim().to_string()
}

pub fn render_world_with_extraction(world: &RenderWorld, repo_path: Option<&Path>) -> anyhow::Result<String> {
    let warned = typst::compile::<HtmlDocument>(&world);
    let mut document = warned.output.map_err(|diags| {
        let msgs: Vec<String> = diags.iter().map(|d| d.message.to_string()).collect();
        anyhow::anyhow!("Typst compilation errors: {}", msgs.join("; "))
    })?;

    // Extract base64-inlined images at the document tree level,
    // before serialization to HTML string.
    if let Some(repo) = repo_path {
        externalize_images(&mut document.root, repo);
    }

    let html = typst_html::html(&document).map_err(|diags| {
        let msgs: Vec<String> = diags.iter().map(|d| d.message.to_string()).collect();
        anyhow::anyhow!("Typst HTML export errors: {}", msgs.join("; "))
    })?;

    Ok(extract_body(&html))
}

/// Walk the HtmlDocument tree, find `<img src="data:...">` attributes,
/// write the decoded image data to `{repo_path}/_rendered/{hash}.{ext}`,
/// and replace the `src` value with the relative path.
fn externalize_images(element: &mut HtmlElement, repo_path: &Path) {
    let src_attr = HtmlAttr::constant("src");
    let out_dir = repo_path.join("_rendered");

    for (attr, value) in element.attrs.0.make_mut().iter_mut() {
        if *attr == src_attr && value.starts_with("data:") {
            if let Some((ext, decoded)) = parse_data_uri(value) {
                let hash = blake3::hash(&decoded);
                let hash_str = &hash.to_hex()[..16];
                let filename = format!("{hash_str}.{ext}");

                // Ensure output directory exists (once, lazily)
                let _ = std::fs::create_dir_all(&out_dir);
                let file_path = out_dir.join(&filename);
                if !file_path.exists() {
                    if let Err(e) = std::fs::write(&file_path, &decoded) {
                        tracing::warn!("failed to write extracted image {filename}: {e}");
                        continue;
                    }
                }

                *value = format!("_rendered/{filename}").into();
            }
        }
    }

    // Recurse into children
    for child in element.children.make_mut().iter_mut() {
        if let HtmlNode::Element(elem) = child {
            externalize_images(elem, repo_path);
        }
    }
}

fn parse_data_uri(uri: &str) -> Option<(&'static str, Vec<u8>)> {
    let rest = uri.strip_prefix("data:")?;
    let (mime, payload) = rest.split_once(";base64,")?;

    let ext = match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => "bin",
    };

    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD.decode(payload).ok()?;
    Some((ext, decoded))
}

fn extract_body(html: &str) -> String {
    let start = html.find("<body>").map(|i| i + "<body>".len());
    let end = html.rfind("</body>");
    match (start, end) {
        (Some(s), Some(e)) => html[s..e].trim().to_string(),
        _ => html.to_string(),
    }
}

/// Render a Typst series to per-chapter HTML.
pub fn render_series_to_html(
    chapter_ids: &[(String, usize)],
    repo_path: &Path,
) -> anyhow::Result<HashMap<String, String>> {
    render_series_to_html_with_config(chapter_ids, repo_path, &RenderConfig::default())
}

pub fn render_series_to_html_with_config(
    chapter_ids: &[(String, usize)],
    repo_path: &Path,
    config: &RenderConfig,
) -> anyhow::Result<HashMap<String, String>> {
    if chapter_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let main_path = repo_path.join("main.typ");
    let source = if main_path.exists() {
        std::fs::read_to_string(&main_path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", main_path.display()))?
    } else {
        build_auto_concat_source(chapter_ids, repo_path)?
    };

    let world = RenderWorld::with_series_preamble(&source, Some(repo_path), config);
    let html = render_world(&world)?;

    split_series_html(&html, chapter_ids)
}

/// Render full series HTML (unsplit) for heading extraction.
pub fn render_series_full_html(repo_path: &Path) -> anyhow::Result<String> {
    render_series_full_html_with_config(repo_path, &RenderConfig::default())
}

pub fn render_series_full_html_with_config(repo_path: &Path, config: &RenderConfig) -> anyhow::Result<String> {
    let main_path = repo_path.join("main.typ");
    let source = if main_path.exists() {
        std::fs::read_to_string(&main_path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", main_path.display()))?
    } else {
        let chapter_files = read_chapter_order(repo_path, ".typ");
        build_auto_concat_source_from_files(&chapter_files, repo_path)?
    };

    let world = RenderWorld::with_series_preamble(&source, Some(repo_path), config);
    render_world(&world)
}

fn build_auto_concat_source(
    chapter_ids: &[(String, usize)],
    repo_path: &Path,
) -> anyhow::Result<String> {
    let mut files = Vec::new();
    for (uri, idx) in chapter_ids {
        let tid = uri.rsplit('/').next().unwrap_or("unknown");
        if let Ok(entries) = std::fs::read_dir(repo_path) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(tid) && name.ends_with(".typ") {
                    files.push((name, *idx));
                    break;
                }
            }
        }
    }
    files.sort_by_key(|(_, idx)| *idx);

    let file_names: Vec<String> = files.iter().map(|(name, _)| name.clone()).collect();
    build_auto_concat_source_from_files(&file_names, repo_path)
}

/// Read chapter order from meta.yaml's `chapters` list. Falls back to a
/// sorted directory scan when the list is missing or empty.
pub fn read_chapter_order(repo_path: &Path, ext: &str) -> Vec<String> {
    if let Ok(data) = std::fs::read_to_string(repo_path.join("meta.yaml")) {
        if let Ok(meta) = serde_yml::from_str::<serde_yml::Value>(&data) {
            if let Some(order) = meta.get("chapters").and_then(|v| v.as_sequence()) {
                let files: Vec<String> = order
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .filter(|f| f.ends_with(ext))
                    .filter(|f| repo_path.join(f).exists())
                    .collect();
                if !files.is_empty() {
                    return files;
                }
            }
        }
    }
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(repo_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(ext) {
                files.push(name);
            }
        }
    }
    files.sort();
    files
}

fn build_auto_concat_source_from_files(
    files: &[String],
    repo_path: &Path,
) -> anyhow::Result<String> {
    let mut source = String::new();

    for (i, name) in files.iter().enumerate() {
        source.push_str(&format!(
            "\n#html.elem(\"section\", attrs: (\"data-chapter\": \"{i}\"))[\n#include \"{name}\"\n]\n"
        ));
    }

    // Auto-discover .bib files in repo root
    let mut bib_files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(repo_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".bib") {
                bib_files.push(name);
            }
        }
    }
    if !bib_files.is_empty() {
        if bib_files.len() == 1 {
            source.push_str(&format!("\n#bibliography(\"{}\")\n", bib_files[0]));
        } else {
            let args: Vec<String> = bib_files.iter().map(|f| format!("\"{f}\"")).collect();
            source.push_str(&format!("\n#bibliography(({}))\n", args.join(", ")));
        }
    }

    Ok(source)
}

fn split_series_html(
    html: &str,
    chapter_ids: &[(String, usize)],
) -> anyhow::Result<HashMap<String, String>> {
    let mut result = HashMap::new();

    for (uri, idx) in chapter_ids {
        let marker = format!("data-chapter=\"{idx}\"");

        if let Some(marker_pos) = html.find(&marker) {
            let content_start = match html[marker_pos..].find('>') {
                Some(offset) => marker_pos + offset + 1,
                None => continue,
            };

            let mut depth = 1i32;
            let mut pos = 0;
            let slice = &html[content_start..];
            while pos < slice.len() && depth > 0 {
                if slice[pos..].starts_with("<section") {
                    depth += 1;
                } else if slice[pos..].starts_with("</section>") {
                    depth -= 1;
                    if depth == 0 {
                        result.insert(uri.clone(), slice[..pos].trim().to_string());
                        break;
                    }
                }
                pos += 1;
            }
        }
    }

    for (uri, _) in chapter_ids {
        result.entry(uri.clone()).or_default();
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_heading() {
        let html = render_typst_to_html("= Hello\nSome *bold* text").unwrap();
        assert!(html.contains("Hello"));
        assert!(html.contains("bold"));
        assert!(!html.contains("<!DOCTYPE"));
    }

    #[test]
    fn test_render_math() {
        let html = render_typst_to_html("The formula $x^2 + y^2 = r^2$ is a circle.").unwrap();
        assert!(html.contains("<math"));
        assert!(html.contains("circle"));
    }

    #[test]
    fn test_render_block_math() {
        let html = render_typst_to_html("Display:\n$\nE = m c^2\n$").unwrap();
        assert!(html.contains("<math"));
    }

    #[test]
    fn test_render_error() {
        let result = render_typst_to_html("#invalid-func()");
        assert!(result.is_err());
    }

    #[test]
    fn extract_metadata_from_labels() {
        // Build a self-contained source (no main.typ on disk) and drive the
        // introspector directly to avoid needing a temp repo dir.
        let src = r#"
= Introduction
#metadata((teaches: ("intro",), prereqs: (("basics", "required"),))) <nbt-chapter>

The intro chapter summary. <nbt-summary>

= Advanced
#metadata((teaches: ("advanced",))) <nbt-chapter>

Advanced stuff overview. <nbt-summary>
"#;
        let world = RenderWorld::new(src);
        let meta = query_labels(&world).expect("query should succeed");

        assert_eq!(meta.chapter_metadata.len(), 2);
        assert_eq!(
            meta.chapter_metadata[0].get("teaches").and_then(|v| v.as_array()).map(|a| a.len()),
            Some(1),
        );
        assert_eq!(meta.summaries.len(), 2);
        assert!(meta.summaries[0].contains("intro chapter summary"));
        assert!(meta.summaries[1].contains("Advanced stuff overview"));
    }
}
