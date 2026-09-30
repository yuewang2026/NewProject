//! The scaffolding a minimal — but genuinely valid — `.pptx` package needs.
//!
//! Nothing here is content; it is all machinery: relationships, content types, a
//! theme, a master and three layouts. [`crate::build`] fills these parts in.
//!
//! Two properties are deliberate:
//!
//! - **The theme owns appearance.** These templates specify no fonts, colours or
//!   bullet glyphs of their own beyond what a theme must declare, so a future
//!   "use my corporate template" switch is a substitution here and nothing else.
//! - **Layouts carry the bullet definitions.** Slide paragraphs state only their
//!   indent level; the hanging indent and bullet character come from the
//!   layout's `a:lstStyle`, exactly as in a hand-made deck.

/// English Metric Units per inch — every coordinate in PresentationML is this.
pub const EMU_PER_INCH: i64 = 914_400;

pub const SLIDE_WIDTH: i64 = 12_192_000; // 13.333in
pub const SLIDE_HEIGHT: i64 = 6_858_000; // 7.5in
const NOTES_WIDTH: i64 = 6_858_000;
const NOTES_HEIGHT: i64 = 9_144_000;

const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Escape text for a character data node.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // Control characters are illegal even escaped; drop them rather
            // than shipping a file that no XML parser will open.
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Escape text for an attribute value, where quotes matter too.
pub fn esc_attr(s: &str) -> String {
    esc(s).replace('"', "&quot;")
}

/// A placeholder offered by a layout.
#[derive(Debug, Clone, Copy)]
pub struct Placeholder {
    /// `p:ph/@type`; `None` means an untyped placeholder, which is a body.
    pub ph_type: Option<&'static str>,
    pub idx: u32,
    pub x: i64,
    pub y: i64,
    pub cx: i64,
    pub cy: i64,
    /// `a:bodyPr/@anchor`: how text sits vertically inside the box.
    pub anchor: &'static str,
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub file: &'static str,
    pub name: &'static str,
    pub ty_p: &'static str,
    pub placeholders: Vec<Placeholder>,
    /// `true` when this layout suits a slide that is only a title (and maybe a
    /// subtitle) — used to decide which layout a given slide should use.
    pub for_title_only: bool,
}

impl Layout {
    pub fn placeholder(&self, ph_type: &str) -> Option<&Placeholder> {
        self.placeholders
            .iter()
            .find(|p| p.ph_type == Some(ph_type))
    }
}

fn centered(y: i64, cy: i64) -> Placeholder {
    Placeholder {
        ph_type: None,
        idx: 0,
        x: 838_200,
        y,
        cx: 10_515_600,
        cy,
        anchor: "ctr",
    }
}

pub fn layouts() -> Vec<Layout> {
    vec![
        Layout {
            file: "slideLayout1.xml",
            name: "Title Slide",
            ty_p: "title",
            for_title_only: true,
            placeholders: vec![
                Placeholder {
                    ph_type: Some("ctrTitle"),
                    idx: 0,
                    ..centered(1_325_625, 2_743_200)
                },
                Placeholder {
                    ph_type: Some("subTitle"),
                    idx: 1,
                    x: 838_200,
                    y: 4_267_200,
                    cx: 10_515_600,
                    cy: 1_600_200,
                    anchor: "ctr",
                },
            ],
        },
        Layout {
            file: "slideLayout2.xml",
            name: "Title and Content",
            ty_p: "obj",
            for_title_only: false,
            placeholders: vec![
                Placeholder {
                    ph_type: Some("title"),
                    idx: 0,
                    x: 838_200,
                    y: 457_200,
                    cx: 10_515_600,
                    cy: 1_371_600,
                    anchor: "ctr",
                },
                Placeholder {
                    ph_type: Some("body"),
                    idx: 1,
                    x: 838_200,
                    y: 2_019_300,
                    cx: 10_515_600,
                    cy: 4_241_800,
                    anchor: "t",
                },
            ],
        },
        Layout {
            file: "slideLayout3.xml",
            name: "Title Only",
            ty_p: "titleOnly",
            for_title_only: true,
            placeholders: vec![Placeholder {
                ph_type: Some("title"),
                idx: 0,
                ..centered(1_600_200, 2_194_560)
            }],
        },
    ]
}

/// One `<p:sp>` placeholder carrying its layout-supplied list style.
fn placeholder_xml(id: u32, name: &str, ph: &Placeholder, for_title: bool) -> String {
    let r#type = match ph.ph_type {
        Some(t) => format!(r#"<p:ph type="{t}" idx="{idx}"/>"#, idx = ph.idx),
        None => format!(r#"<p:ph idx="{idx}"/>"#, idx = ph.idx),
    };
    // Only a content placeholder carries bullet definitions; a title does not
    // need them and they would look wrong if text ever overflowed into level 1.
    let lst_style = if for_title {
        String::new()
    } else {
        level_styles()
    };
    format!(
        concat!(
            "<p:sp>",
            "<p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/>",
            "<p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr>",
            "<p:nvPr>{ph}</p:nvPr></p:nvSpPr>",
            "<p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr>",
            "<p:txBody><a:bodyPr anchor=\"{anchor}\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\">",
            "<a:normAutofit/></a:bodyPr>{lst_style}",
            "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>",
            "</p:txBody></p:sp>"
        ),
        id = id,
        name = esc_attr(name),
        ph = r#type,
        x = ph.x,
        y = ph.y,
        cx = ph.cx,
        cy = ph.cy,
        anchor = ph.anchor,
        lst_style = lst_style,
    )
}

/// Bullet characters and hanging indents for levels 1–8.
///
/// Present on the *layout*, not on slides, so that an author-written indent
/// level renders as a real bullet without the slide repeating any styling.
fn level_styles() -> String {
    let glyphs = ["\u{2022}", "\u{2013}", "\u{2022}", "\u{2013}"];
    let mut out = String::from("<a:lstStyle>");
    for level in 1..=8usize {
        let mar_l = level as i64 * 285_750;
        let glyph = glyphs[(level - 1) % glyphs.len()];
        out.push_str(&format!(
            concat!(
                "<a:lvl{lvl}pPr marL=\"{mar_l}\" indent=\"-285750\" algn=\"l\">",
                "<a:buClrTx/><a:buFontTx/><a:buChar char=\"{glyph}\"/>",
                "</a:lvl{lvl}pPr>"
            ),
            lvl = level,
            mar_l = mar_l,
            glyph = glyph,
        ));
    }
    out.push_str("</a:lstStyle>");
    out
}

pub fn slide_layout_xml(layout: &Layout) -> String {
    let mut body = String::new();
    // Shape ids start at 2: 1 belongs to the shape tree itself.
    for (id, ph) in (2u32..).zip(&layout.placeholders) {
        let name = format!("{} Placeholder {}", layout.name, id);
        let for_title = ph.ph_type.is_some_and(|t| t == "title" || t == "ctrTitle");
        body.push_str(&placeholder_xml(id, &name, ph, for_title));
    }
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<p:sldLayout xmlns:a=\"{a}\" xmlns:r=\"{r}\" xmlns:p=\"{p}\"",
            " type=\"{ty}\" preserve=\"1\">",
            "<p:cSld name=\"{name}\"><p:spTree>",
            "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>",
            "<p:grpSpPr><a:xfrm/></p:grpSpPr>{body}</p:spTree></p:cSld>",
            "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>",
            "</p:sldLayout>"
        ),
        a = A_NS,
        r = R_NS,
        p = P_NS,
        ty = layout.ty_p,
        name = esc_attr(layout.name),
        body = body,
    )
}

pub fn slide_master_xml(layout_count: usize) -> String {
    let mut ids = String::new();
    for (i, _) in (0..layout_count).enumerate() {
        ids.push_str(&format!(
            r#"<p:sldLayoutId id="{id}" r:id="rId{rid}"/>"#,
            id = 2_147_483_649 + i as u32,
            rid = i + 2,
        ));
    }
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<p:sldMaster xmlns:a=\"{a}\" xmlns:r=\"{r}\" xmlns:p=\"{p}\">",
            "<p:cSld><p:spTree>",
            "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>",
            "<p:grpSpPr><a:xfrm/></p:grpSpPr></p:spTree></p:cSld>",
            "<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\"",
            " accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\"",
            " accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\"",
            " hlink=\"hlink\" folHlink=\"folHlink\"/>",
            "<p:sldLayoutIdLst>{ids}</p:sldLayoutIdLst>",
            "</p:sldMaster>"
        ),
        a = A_NS,
        r = R_NS,
        p = P_NS,
        ids = ids,
    )
}

pub fn presentation_xml(slide_count: usize) -> String {
    let mut ids = String::new();
    for i in 0..slide_count {
        ids.push_str(&format!(
            r#"<p:sldId id="{id}" r:id="rId{rid}"/>"#,
            id = 256 + i as u32,
            // rId1 is the master; slides start at rId2.
            rid = i + 2,
        ));
    }
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<p:presentation xmlns:a=\"{a}\" xmlns:r=\"{r}\" xmlns:p=\"{p}\" saveSubsetFonts=\"1\">",
            "<p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>",
            "<p:sldIdLst>{ids}</p:sldIdLst>",
            "<p:sldSz cx=\"{w}\" cy=\"{h}\"/>",
            "<p:notesSz cx=\"{nw}\" cy=\"{nh}\"/>",
            "</p:presentation>"
        ),
        a = A_NS,
        r = R_NS,
        p = P_NS,
        ids = ids,
        w = SLIDE_WIDTH,
        h = SLIDE_HEIGHT,
        nw = NOTES_WIDTH,
        nh = NOTES_HEIGHT,
    )
}

/// Relationships of one part, in file order.
#[derive(Clone)]
pub struct Rel {
    pub id: String,
    pub ty: &'static str,
    pub target: String,
    pub external: bool,
}

impl Rel {
    pub fn internal(id: String, ty: &'static str, target: String) -> Self {
        Self {
            id,
            ty,
            target,
            external: false,
        }
    }
}

const TY_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const TY_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const TY_LAYOUT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const TY_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";

pub fn root_rels_xml() -> String {
    xml_header()
        + &rel_package_xml(&[Rel::internal(
            "rId1".into(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
            "ppt/presentation.xml".into(),
        )])
}

pub fn presentation_rels_xml(slide_count: usize) -> String {
    let mut rels = vec![Rel::internal(
        "rId1".into(),
        TY_MASTER,
        "slideMasters/slideMaster1.xml".into(),
    )];
    for i in 0..slide_count {
        rels.push(Rel::internal(
            format!("rId{}", i + 2),
            TY_SLIDE,
            format!("slides/slide{}.xml", i + 1),
        ));
    }
    xml_header() + &rel_package_xml(&rels)
}

pub fn layout_rels_xml() -> String {
    xml_header()
        + &rel_package_xml(&[Rel::internal(
            "rId1".into(),
            TY_MASTER,
            "../slideMasters/slideMaster1.xml".into(),
        )])
}

pub fn master_rels_xml(layout_count: usize) -> String {
    let mut rels = vec![Rel::internal(
        "rId1".into(),
        TY_THEME,
        "../theme/theme1.xml".into(),
    )];
    for i in 0..layout_count {
        rels.push(Rel::internal(
            format!("rId{}", i + 2),
            TY_LAYOUT,
            format!("../slideLayouts/slideLayout{}.xml", i + 1),
        ));
    }
    xml_header() + &rel_package_xml(&rels)
}

pub fn slide_rels_xml(layout_index: usize, rels: &[Rel]) -> String {
    let mut all = vec![Rel::internal(
        "rId1".into(),
        TY_LAYOUT,
        format!("../slideLayouts/slideLayout{}.xml", layout_index + 1),
    )];
    all.extend(rels.iter().cloned());
    xml_header() + &rel_package_xml(&all)
}

fn rel_package_xml(rels: &[Rel]) -> String {
    let mut out = String::from(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for r in rels {
        let mode = if r.external {
            r#" TargetMode="External""#
        } else {
            ""
        };
        out.push_str(&format!(
            r#"<Relationship Id="{id}" Type="{ty}" Target="{target}"{mode}/>"#,
            id = esc_attr(&r.id),
            ty = r.ty,
            target = esc_attr(&r.target),
            mode = mode,
        ));
    }
    out.push_str("</Relationships>");
    out
}

pub fn content_types_xml(
    slide_count: usize,
    layout_count: usize,
    extra: &[(String, String)],
) -> String {
    let mut out = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
        "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
        "<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>",
        "<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>",
        "<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>",
        "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
        "<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>",
    ));
    for i in 0..layout_count {
        out.push_str(&format!(
            concat!(
                "<Override PartName=\"/ppt/slideLayouts/slideLayout{i}.xml\"",
                " ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>"
            ),
            i = i + 1
        ));
    }
    for i in 0..slide_count {
        out.push_str(&format!(
            concat!(
                "<Override PartName=\"/ppt/slides/slide{i}.xml\"",
                " ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
            ),
            i = i + 1
        ));
    }
    for (partname, ct) in extra {
        out.push_str(&format!(
            concat!("<Override PartName=\"/{name}\"", " ContentType=\"{ct}\"/>"),
            name = partname.trim_start_matches('/'),
            ct = ct,
        ));
    }
    out.push_str("</Types>");
    out
}

/// A fixed timestamp keeps two builds of the same deck byte-identical, which
/// makes generated decks diffable — the whole reason an IR exists.
const FROZEN: &str = "2020-01-01T00:00:00Z";

pub fn core_props_xml(title: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<cp:coreProperties",
            " xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\"",
            " xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
            " xmlns:dcterms=\"http://purl.org/dc/terms/\"",
            " xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
            "<dc:title>{title}</dc:title>",
            "<dc:creator>deckr</dc:creator>",
            "<cp:lastModifiedBy>deckr</cp:lastModifiedBy>",
            "<dcterms:created xsi:type=\"dcterms:W3CDTF\">{frozen}</dcterms:created>",
            "<dcterms:modified xsi:type=\"dcterms:W3CDTF\">{frozen}</dcterms:modified>",
            "</cp:coreProperties>"
        ),
        title = esc(title),
        frozen = FROZEN,
    )
}

pub fn app_props_xml(slide_count: usize) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<Properties",
            " xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"",
            " xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">",
            "<Application>deckr</Application>",
            "<Company>deckr</Company>",
            "<AppVersion>0.2.0</AppVersion>",
            "<Slides>{n}</Slides>",
            "<PresentationFormat>Widescreen</PresentationFormat>",
            "</Properties>"
        ),
        n = slide_count,
    )
}

pub fn xml_header() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n".to_string()
}

/// A Theme small enough to read, complete enough for PowerPoint.
///
/// The colour scheme is Office's own (kept because it is what people expect a
/// "default deck" to look like); everything else is reduced to the elements the
/// schema actually requires.
pub const THEME_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="deckr Theme"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="35000"><a:schemeClr val="phClr"><a:tint val="37000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="15000"/><a:satMod val="350000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="1"/></a:gradFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:shade val="51000"/><a:satMod val="130000"/></a:schemeClr></a:gs><a:gs pos="80000"><a:schemeClr val="phClr"><a:shade val="93000"/><a:satMod val="130000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="94000"/><a:satMod val="135000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="1"/></a:gradFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst><a:outerShdw blurRad="57150" dist="19050" dir="5400000" algn="ctr" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="63000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:tint val="95000"/><a:satMod val="170000"/></a:schemeClr></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="93000"/><a:satMod val="150000"/><a:shade val="98000"/><a:lumMod val="102000"/></a:schemeClr></a:gs><a:gs pos="50000"><a:schemeClr val="phClr"><a:tint val="98000"/><a:satMod val="130000"/><a:shade val="90000"/><a:lumMod val="103000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="63000"/><a:satMod val="120000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="1"/></a:gradFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#;
