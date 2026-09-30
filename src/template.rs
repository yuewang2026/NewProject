//! Reuse a user's PowerPoint template (`.potx`) as the visual chrome.
//!
//! deckr's default build writes its own minimal theme, master and layouts. A
//! `.potx` ships those same parts — a corporate colour scheme, fonts, a house
//! master. [`Template::load`] reads the template's theme, master and slide
//! layouts, then the crate-internal `Chrome` impl copies that
//! scaffolding into every generated package and binds each slide to the
//! template's own layouts. The result inherits the template's look without
//! deckr having to understand a single template-specific feature.
//!
//! The package is read, never executed: only the chrome parts — theme, master,
//! the slide layouts and their relationship graphs, plus `presProps` /
//! `viewProps` / `tableStyles` when the template carries them — are consulted
//! and copied. Everything else in the template (sample slides, notes masters,
//! media) is ignored, so the generated package carries no orphaned parts.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;

use quick_xml::Reader as XmlReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

use crate::build::{Chrome, Placed, put_bytes};
use crate::error::{Error, Result};
use crate::ir::{Role, Slide};
use crate::parts::{self, Rel};

const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const TY_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const TY_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";

/// The presentation-level chrome parts worth inheriting, beyond theme/master/
/// layouts. Each entry is the relationship-type suffix that identifies it, the
/// package part name it lives at, and its content type.
const PRES_PARTS: &[(&str, &str, &str)] = &[
    (
        "presProps",
        "ppt/presProps.xml",
        "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml",
    ),
    (
        "viewProps",
        "ppt/viewProps.xml",
        "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml",
    ),
    (
        "tableStyles",
        "ppt/tableStyles.xml",
        "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml",
    ),
];

/// A template's reusable chrome: theme, master, slide layouts and the geometry
/// of each layout's placeholders.
pub struct Template {
    /// Every part of the template package.
    entries: HashMap<String, Vec<u8>>,
    /// The slide layouts, each with the placeholders it offers.
    layouts: Vec<LayoutInfo>,
    /// The `(Type, Target)` of each presentation-level chrome part the
    /// template references (`presProps` / `viewProps` / `tableStyles`), so the
    /// generated `presentation.xml.rels` can wire them back up.
    pres_extra: Vec<(String, String)>,
    /// `(PartName, content type)` overrides for those same parts, so
    /// `[Content_Types].xml` declares them.
    extra_types: Vec<(String, String)>,
}

/// One slide layout and the placeholders it carries.
struct LayoutInfo {
    /// File name within `ppt/slideLayouts/`.
    file: String,
    placeholders: Vec<Ph>,
}

/// A placeholder on a layout, with the geometry a generated shape needs.
#[derive(Default, Clone)]
struct Ph {
    ph_type: String,
    idx: u32,
    x: i64,
    y: i64,
    cx: i64,
    cy: i64,
    anchor: String,
}

impl LayoutInfo {
    /// Whether this layout offers a placeholder whose type is in `types`.
    fn has(&self, types: &[&str]) -> bool {
        self.placeholders
            .iter()
            .any(|p| types.contains(&p.ph_type.as_str()))
    }
    /// Whether this layout offers a title placeholder (ctrTitle or title).
    fn has_title(&self) -> bool {
        self.has(&["ctrTitle", "title"])
    }
}

impl Template {
    /// Load a `.potx` (or `.pptx`) and extract its reusable chrome.
    ///
    /// Returns an error when the package has no slide master or no slide
    /// layouts — without those there is nothing to inherit.
    pub fn load(path: &Path) -> Result<Template> {
        let file = File::open(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut pkg = ZipArchive::new(file)
            .map_err(|e| Error::Other(format!("not a readable template package: {e}")))?;

        let mut entries: HashMap<String, Vec<u8>> = HashMap::new();
        for i in 0..pkg.len() {
            let mut zf = pkg.by_index(i).map_err(|e| Error::Other(e.to_string()))?;
            if zf.is_dir() {
                continue;
            }
            let name = zf.name().to_string();
            let mut buf = Vec::new();
            zf.read_to_end(&mut buf)
                .map_err(|e| Error::Other(e.to_string()))?;
            entries.insert(name, buf);
        }

        let read_str = |name: &str| -> Result<String> {
            entries
                .get(name)
                .map(|b| String::from_utf8_lossy(b).to_string())
                .ok_or_else(|| Error::Other(format!("template is missing {name}")))
        };

        let pres_rels = read_str("ppt/_rels/presentation.xml.rels")?;
        let master_rel_target = find_rels(&pres_rels)
            .into_iter()
            .find(|(ty, _)| ty.ends_with("/slideMaster"))
            .map(|(_, target)| target)
            .ok_or_else(|| Error::Other("template has no slide master relationship".into()))?;

        // Find the presentation-level chrome parts (presProps/viewProps/
        // tableStyles) the template declares, so we can copy and re-reference
        // them rather than leaving them orphaned.
        let mut pres_extra = Vec::new();
        let mut extra_types = Vec::new();
        for (suffix, part, ct) in PRES_PARTS {
            if let Some(target) = find_rels(&pres_rels)
                .into_iter()
                .find(|(ty, _)| ty.ends_with(suffix))
                .map(|(_, target)| target)
            {
                pres_extra.push((format!("{REL_NS}/{suffix}"), target.clone()));
                extra_types.push((format!("/{part}"), (*ct).to_string()));
            }
        }

        // The master's own rels live next to the master part; resolve the
        // layout targets against the master's directory.
        let master_part = resolve("ppt", &master_rel_target);
        let master_rels_name = format!(
            "{}/_rels/{}.rels",
            dir_of(&master_part),
            file_of(&master_part)
        );
        let master_rels = read_str(&master_rels_name)?;

        let mut layouts = Vec::new();
        for (_, target) in find_rels(&master_rels)
            .into_iter()
            .filter(|(ty, _)| ty.ends_with("/slideLayout"))
        {
            let pkg_path = resolve(&format!("{}/", dir_of(&master_part)), &target);
            let xml = read_str(&pkg_path)?;
            layouts.push(LayoutInfo {
                file: file_of(&pkg_path),
                placeholders: parse_layout_placeholders(&xml),
            });
        }
        if layouts.is_empty() {
            return Err(Error::Other("template has no slide layouts".into()));
        }

        Ok(Template {
            entries,
            layouts,
            pres_extra,
            extra_types,
        })
    }
}

impl Chrome for Template {
    fn layout_count(&self) -> usize {
        self.layouts.len()
    }

    fn choose_layout(&self, slide: &Slide) -> usize {
        let needs_content = slide
            .blocks
            .iter()
            .filter(|b| !b.is_empty() && !b.role.is_chrome())
            .any(|b| matches!(b.role, Role::Body | Role::Object | Role::Table));
        if needs_content {
            // First layout offering both a title placeholder and a body.
            self.layouts
                .iter()
                .position(|l| l.has_title() && l.has(&["body"]))
                .unwrap_or(0)
        } else {
            // First layout offering a title placeholder.
            self.layouts.iter().position(|l| l.has_title()).unwrap_or(0)
        }
    }

    fn placeholder(&self, layout: usize, ph_type: &str) -> Option<Placed> {
        let l = self.layouts.get(layout)?;
        let p = l.placeholders.iter().find(|p| p.ph_type == ph_type)?;
        Some(Placed {
            idx: p.idx,
            x: p.x,
            y: p.y,
            cx: p.cx,
            cy: p.cy,
            anchor: p.anchor.clone(),
        })
    }

    fn write_chrome<W: Write + Seek>(
        &self,
        zip: &mut zip::ZipWriter<W>,
        opts: SimpleFileOptions,
    ) -> Result<()> {
        // Copy only the chrome parts, by name. Sorted so template builds stay
        // byte-deterministic. Everything else the template shipped — sample
        // slides, notes masters, media — is deliberately left behind, so the
        // generated package carries no orphaned parts.
        let pres_names: Vec<String> = self
            .pres_extra
            .iter()
            .map(|(_, target)| format!("ppt/{}", target.trim_start_matches('/')))
            .collect();
        let mut names: Vec<&String> = self.entries.keys().collect();
        names.sort();
        for name in names {
            if !is_chrome_part(name, &pres_names) {
                continue;
            }
            let bytes = &self.entries[name];
            put_bytes(zip, opts, name, bytes)?;
        }
        Ok(())
    }

    fn content_types(&self, slide_count: usize, extra: &[(String, String)]) -> String {
        let mut all = self.extra_types.clone();
        all.extend(extra.iter().cloned());
        parts::content_types_xml(slide_count, self.layout_count(), &all)
    }

    fn presentation_rels(&self, slide_count: usize) -> String {
        // The master is rId1; slides follow at rId2..rId(slide_count+1),
        // exactly as deckr's default rels. The presentation-level chrome parts
        // get ids past the slides, so they never collide and are still found
        // by relationship type — which is how PowerPoint resolves them.
        let mut out = String::from(xml_header());
        out.push_str(&format!("<Relationships xmlns=\"{REL_NS}\">"));
        out.push_str(&rel("rId1", TY_MASTER, "slideMasters/slideMaster1.xml"));
        let extra_start = slide_count + 2;
        for (k, (ty, target)) in self.pres_extra.iter().enumerate() {
            out.push_str(&rel(&format!("rId{}", extra_start + k), ty, target));
        }
        for i in 0..slide_count {
            out.push_str(&rel(
                &format!("rId{}", i + 2),
                TY_SLIDE,
                &format!("slides/slide{}.xml", i + 1),
            ));
        }
        out.push_str("</Relationships>");
        out
    }

    fn slide_layout_target(&self, layout: usize) -> String {
        format!("../slideLayouts/{}", self.layouts[layout].file)
    }

    fn slide_rels(&self, layout: usize, rels: &[Rel]) -> String {
        let mut out = String::from(xml_header());
        out.push_str(&format!("<Relationships xmlns=\"{REL_NS}\">"));
        out.push_str(&rel(
            "rId1",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            &self.slide_layout_target(layout),
        ));
        for r in rels {
            let mode = if r.external {
                " TargetMode=\"External\""
            } else {
                ""
            };
            out.push_str(&format!(
                "<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{target}\"{mode}/>",
                id = r.id,
                ty = r.ty,
                target = r.target,
                mode = mode
            ));
        }
        out.push_str("</Relationships>");
        out
    }
}

// ---------------------------------------------------------------------------
// path + relationship helpers
// ---------------------------------------------------------------------------

/// Whether `name` is a chrome part we should copy from the template.
fn is_chrome_part(name: &str, pres_names: &[String]) -> bool {
    name == "ppt/theme/theme1.xml"
        || name == "ppt/theme/_rels/theme1.xml.rels"
        || name.starts_with("ppt/slideMasters/")
        || name.starts_with("ppt/slideLayouts/")
        || pres_names.contains(&name.to_string())
}

/// Directory portion of a package path (`ppt/slideMasters/slideMaster1.xml` →
/// `ppt/slideMasters`).
fn dir_of(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(d, _)| d.to_string())
        .unwrap_or_default()
}

/// File portion of a package path (`ppt/slideLayouts/slideLayout1.xml` →
/// `slideLayout1.xml`).
fn file_of(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(_, f)| f.to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Join `target` onto `base_dir` and normalise `.` / `..`. `base_dir` is a
/// directory (with or without a trailing slash); relationship targets are
/// relative to the *part's* directory, which is what `base_dir` is here.
fn resolve(base_dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = base_dir
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for seg in target.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Every `(Type, Target)` relationship declared in a `.rels` document.
fn find_rels(rels: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for seg in rels.split("<Relationship") {
        let ty = attr_val(seg, "Type");
        let target = attr_val(seg, "Target");
        if let (Some(t), Some(tg)) = (ty, target) {
            out.push((t, tg));
        }
    }
    out
}

fn xml_header() -> &'static str {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n"
}

/// A single `<Relationship>` element.
fn rel(id: &str, ty: &str, target: &str) -> String {
    format!("<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{target}\"/>")
}

/// Pull a single double-quoted attribute out of a `<Relationship .../>` tail.
fn attr_val(seg: &str, name: &str) -> Option<String> {
    let pat = format!("{name}=\"");
    let start = seg.find(&pat)? + pat.len();
    let end = seg[start..].find('"')? + start;
    Some(seg[start..end].to_string())
}

/// Extract every placeholder (`<p:ph>`) a layout offers, with its geometry.
fn parse_layout_placeholders(xml: &str) -> Vec<Ph> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut sp_depth: i32 = 0;
    let mut cur: Option<Ph> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            // Leaf elements (p:ph, a:off, a:ext, a:bodyPr) are self-closing in
            // real layouts, so quick-xml reports them as `Empty`, not `Start`.
            // Both carry the same BytesStart, so one arm serves the two.
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let local = local_name(e.name());
                match local.as_str() {
                    "sp" => {
                        sp_depth += 1;
                        cur = Some(Ph::default());
                    }
                    "ph" if sp_depth > 0 => {
                        if let Some(c) = cur.as_mut() {
                            // An untyped placeholder is a body placeholder.
                            c.ph_type = attr(&e, "type").unwrap_or_else(|| "body".to_string());
                            c.idx = attr(&e, "idx").and_then(|v| v.parse().ok()).unwrap_or(0);
                        }
                    }
                    "off" if sp_depth > 0 => {
                        if let Some(c) = cur.as_mut() {
                            if let Some(v) = attr(&e, "x") {
                                c.x = v.parse().unwrap_or(0);
                            }
                            if let Some(v) = attr(&e, "y") {
                                c.y = v.parse().unwrap_or(0);
                            }
                        }
                    }
                    "ext" if sp_depth > 0 => {
                        if let Some(c) = cur.as_mut() {
                            if let Some(v) = attr(&e, "cx") {
                                c.cx = v.parse().unwrap_or(0);
                            }
                            if let Some(v) = attr(&e, "cy") {
                                c.cy = v.parse().unwrap_or(0);
                            }
                        }
                    }
                    "bodyPr" if sp_depth > 0 => {
                        if let Some(c) = cur.as_mut() {
                            c.anchor = attr(&e, "anchor").unwrap_or_else(|| "t".to_string());
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name());
                if local == "sp" {
                    sp_depth -= 1;
                    if let Some(c) = cur.take() {
                        if !c.ph_type.is_empty() {
                            out.push(c);
                        }
                    }
                }
            }
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Local name of a qualified XML name.
fn local_name(name: QName<'_>) -> String {
    let full = name.0;
    match full.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => full.to_string(),
    }
}

/// Value of an attribute by local name.
fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    for a in e.attributes().flatten() {
        if local_name(a.key) == key {
            return Some(a.value.to_string());
        }
    }
    None
}
