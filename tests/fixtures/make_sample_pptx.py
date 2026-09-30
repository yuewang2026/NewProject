"""Generate a minimal but valid .pptx used to smoke-test deckr.

Slide *presentation* order is deliberately the reverse of filename order, so a
reader that sorts `slideN.xml` instead of following `sldIdLst` gets it wrong.

Everything here resolves: `tests/fixtures/validate_pptx.py` must accept the
result, because a fixture that lies about how real packages hang together would
teach the tests the wrong lesson.
"""
import struct
import sys
import zipfile
import zlib

NS_P = 'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"'
NS_A = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"'
NS_R = 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

CONTENT_TYPES = XML + f'''<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Default Extension="png" ContentType="image/png"/>
<Default Extension="xlsx" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"/>
<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
<Override PartName="/ppt/slides/slide2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
<Override PartName="/ppt/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
</Types>'''

ROOT_RELS = XML + '''<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>'''

PRES_RELS = XML + '''<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/>
</Relationships>'''

# rId2 (slide2.xml) is listed first -> it is what a human sees first.
PRESENTATION = XML + f'''<p:presentation {NS_P} {NS_R}>
<p:sldIdLst><p:sldId id="257" r:id="rId2"/><p:sldId id="256" r:id="rId1"/></p:sldIdLst>
</p:presentation>'''

HEAD = f'''<p:cSld><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
'''

# ---------------------------------------------------------------- slide1.xml
# A table + a nested bullet list, plus two empty placeholders that must not
# leak into the Markdown export.
SLIDE1 = XML + f'''<p:sld {NS_P} {NS_A} {NS_R}>{HEAD}
<p:sp>
  <p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr txBox="1"/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
  <p:txBody>
    <a:p><a:r><a:rPr lang="en-US"/><a:t>Roadmap &amp; Milestones</a:t></a:r></a:p>
  </p:txBody>
</p:sp>
<p:sp>
  <p:nvSpPr><p:cNvPr id="3" name="Content Placeholder 2"/><p:cNvSpPr txBox="1"/><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>
  <p:txBody>
    <a:p><a:pPr lvl="0"/><a:r><a:t>Ship the reader</a:t></a:r></a:p>
    <a:p><a:pPr lvl="1"/><a:r><a:t>OOXML &amp; Deck IR</a:t></a:r></a:p>
    <a:p><a:pPr lvl="2"/><a:r><a:t>placeholders stay bound</a:t></a:r></a:p>
    <a:p><a:pPr lvl="1"/><a:r><a:t>Write the builder</a:t></a:r></a:p>
  </p:txBody>
</p:sp>
<p:graphicFrame>
  <p:nvGraphicFramePr><p:cNvPr id="4" name="Table 3" descr="release plan"/><p:nvPr/></p:nvGraphicFramePr>
  <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl>
    <a:tr><a:tc><a:txBody><a:p><a:t>Phase</a:t></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:t>Target</a:t></a:p></a:txBody></a:tc></a:tr>
    <a:tr><a:tc><a:txBody><a:p><a:t>0.1 read</a:t></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:t>2026 Q3</a:t></a:p></a:txBody></a:tc></a:tr>
    <a:tr><a:tc><a:txBody><a:p><a:t>0.2 write</a:t></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:t>2026 Q4</a:t></a:p></a:txBody></a:tc></a:tr>
  </a:tbl></a:graphicData></a:graphic>
</p:graphicFrame>
<p:sp>
  <p:nvSpPr><p:cNvPr id="5" name="Subtitle 4"/><p:nvPr><p:ph type="subTitle" idx="1"/></p:nvPr></p:nvSpPr>
  <p:txBody><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody>
</p:sp>
<p:sp>
  <p:nvSpPr><p:cNvPr id="6" name="Slide Number Placeholder 5"/><p:nvPr><p:ph type="sldNum" idx="10"/></p:nvPr></p:nvSpPr>
  <p:txBody><a:p><a:fld id="{'{1D7C8D8A-1111-4257-9F3F-1234567890AB}'}" type="slidenum"><a:rPr lang="en-US"/><a:t>1</a:t></a:fld></a:p></p:txBody>
</p:sp>
</p:spTree></p:cSld></p:sld>'''

# ---------------------------------------------------------------- slide2.xml
# Presented first: a title slide, a chart frame and a picture with alt text.
SLIDE2 = XML + f'''<p:sld {NS_P} {NS_A} {NS_R}>{HEAD}
<p:sp>
  <p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr txBox="1"/><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr>
  <p:txBody><a:p><a:r><a:t>deckr</a:t></a:r><a:r><a:t> — a bidirectional deck engine</a:t></a:r></a:p></p:txBody>
</p:sp>
<p:sp>
  <p:nvSpPr><p:cNvPr id="3" name="Subtitle 2"/><p:nvPr><p:ph type="subTitle" idx="1"/></p:nvPr></p:nvSpPr>
  <p:txBody><a:p><a:r><a:t>Read it. Diff it. Build it back.</a:t></a:r></a:p></p:txBody>
</p:sp>
<p:graphicFrame>
  <p:nvGraphicFramePr><p:cNvPr id="4" name="Chart 3" descr="growth"/><p:nvPr/></p:nvGraphicFramePr>
  <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">
    <c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId9"/>
  </a:graphicData></a:graphic>
</p:graphicFrame>
<p:pic>
  <p:nvPicPr><p:cNvPr id="5" name="Picture 4" descr="architecture diagram"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>
  <p:blipFill><a:blip r:embed="rId8"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>
</p:pic>
<p:sp>
  <p:nvSpPr><p:cNvPr id="6" name="Freeform 5"/><p:nvPr/></p:nvSpPr>
  <p:txBody><a:p><a:r><a:t>internal only</a:t></a:r></a:p></p:txBody>
</p:sp>
</p:spTree></p:cSld></p:sld>'''

# slide2 carries a chart frame and a picture, so it needs real relationships —
# a package that names r:id without declaring it is one PowerPoint repairs.
SLIDE2_RELS = XML + '''<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>
<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/>
</Relationships>'''

CHART1 = XML + f'''<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" {NS_A} {NS_R}>
<c:chart><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>growth</a:t></a:r></a:p></c:rich></c:tx></c:title>
<c:plotArea><c:barChart><c:barDir val="col"/></c:barChart></c:plotArea>
<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>
</c:chart></c:chartSpace>'''

# The chart points at an embedded workbook, so its relationship graph reaches
# a second part — exactly the walk `capture_chart_subgraph` is built to make
# verbatim. Without this the BFS would only ever see the chart XML.
CHART1_RELS = XML + '''<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/package" Target="../embeddings/workbook1.xlsx"/>
</Relationships>'''


def png_1x1() -> bytes:
    """A genuine 1x1 greyscale PNG, because a fake one is one failure away."""

    def chunk(tag: bytes, data: bytes) -> bytes:
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", 1, 1, 8, 0, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(b"\x00\x00"))
        + chunk(b"IEND", b"")
    )


def make_minimal_xlsx() -> bytes:
    """A genuinely openable single-sheet workbook.

    Charts keep their numbers in an embedded `.xlsx`; deckr copies it verbatim,
    so the fixture ships a real one rather than a placeholder. It is tiny but
    structurally complete enough that Excel would open it.
    """
    import io

    ct = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
        '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        "</Types>"
    )
    root_rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
        "</Relationships>"
    )
    workbook = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>'
        "</workbook>"
    )
    book_rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
        "</Relationships>"
    )
    sheet = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        "<sheetData/></worksheet>"
    )

    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", ct)
        z.writestr("_rels/.rels", root_rels)
        z.writestr("xl/workbook.xml", workbook)
        z.writestr("xl/_rels/workbook.xml.rels", book_rels)
        z.writestr("xl/worksheets/sheet1.xml", sheet)
    return buf.getvalue()


PARTS = [
    ("[Content_Types].xml", CONTENT_TYPES),
    ("_rels/.rels", ROOT_RELS),
    ("ppt/presentation.xml", PRESENTATION),
    ("ppt/_rels/presentation.xml.rels", PRES_RELS),
    ("ppt/slides/slide1.xml", SLIDE1),
    ("ppt/slides/slide2.xml", SLIDE2),
    ("ppt/slides/_rels/slide2.xml.rels", SLIDE2_RELS),
    ("ppt/charts/chart1.xml", CHART1),
    ("ppt/charts/_rels/chart1.xml.rels", CHART1_RELS),
    ("ppt/embeddings/workbook1.xlsx", make_minimal_xlsx()),
]


# ---------------------------------------------------------------------------
# A minimal but valid .potx used to prove deckr reuses a template's chrome.
# Its theme is deliberately *not* Office's: accent1 is C00000, so a build that
# inherits the template must emit a theme containing C00000 and not the default
# Office accent1 (4472C4).
# ---------------------------------------------------------------------------
TEMPLATE_THEME = XML + '''<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="deckr Test Template">
<a:themeElements><a:clrScheme name="TestScheme"><a:dk1><a:srgbClr val="1F1F1F"/></a:dk1><a:lt1><a:srgbClr val="F2F2F2"/></a:lt1><a:dk2><a:srgbClr val="C00000"/></a:dk2><a:lt2><a:srgbClr val="FFE0E0"/></a:lt2><a:accent1><a:srgbClr val="C00000"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>
<a:fontScheme name="Test"><a:majorFont><a:latin typeface="Georgia"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>
<a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="15000"/><a:satMod val="350000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="1"/></a:gradFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst/><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>'''

TEMPLATE_CONTENT_TYPES = XML + f'''<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.template.main+xml"/>
<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
<Override PartName="/ppt/slideLayouts/slideLayout2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
<Override PartName="/ppt/presProps.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presProps+xml"/>
<Override PartName="/ppt/tableStyles.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml"/>
<Override PartName="/ppt/viewProps.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml"/>
<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>
</Types>'''

TEMPLATE_PRES = XML + f'''<p:presentation {NS_P} {NS_R} saveSubsetFonts="1">
<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst/>
<p:sldSz cx="12192000" cy="6858000"/>
<p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>'''

TEMPLATE_PRES_RELS = XML + '''<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps" Target="presProps.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps" Target="viewProps.xml"/>
<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles" Target="tableStyles.xml"/>
</Relationships>'''

TEMPLATE_MASTER = XML + f'''<p:sldMaster {NS_P} {NS_A} {NS_R}>
<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm/></p:grpSpPr></p:spTree></p:cSld>
<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId2"/><p:sldLayoutId id="2147483650" r:id="rId3"/></p:sldLayoutIdLst>
</p:sldMaster>'''

TEMPLATE_MASTER_RELS = XML + '''<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout2.xml"/>
</Relationships>'''


def _layout(file, name, ltype, phs_xml):
    return XML + f'''<p:sldLayout {NS_P} {NS_A} {NS_R} type="{ltype}" preserve="1">
<p:cSld name="{name}"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm/></p:grpSpPr>{phs_xml}</p:spTree></p:cSld>
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>''', file


def _sp(ph_type, idx, x, y, cx, cy):
    return f'''<p:sp><p:nvSpPr><p:cNvPr id="2" name="{ph_type} {idx}"/><p:cNvSpPr/><p:nvPr><p:ph type="{ph_type}" idx="{idx}"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>
<p:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp>'''


TEMPLATE_LAYOUT1, TEMPLATE_LAYOUT1_FILE = _layout(
    "slideLayout1.xml", "Title Slide", "title",
    _sp("ctrTitle", 0, 838200, 1325625, 10515600, 2743200)
    + _sp("subTitle", 1, 838200, 4267200, 10515600, 1600200),
)
TEMPLATE_LAYOUT2, TEMPLATE_LAYOUT2_FILE = _layout(
    "slideLayout2.xml", "Title and Content", "obj",
    _sp("title", 0, 838200, 457200, 10515600, 1371600)
    + _sp("body", 1, 838200, 2019300, 10515600, 4241800),
)

TEMPLATE_LAYOUT1_RELS = XML + '''<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>'''
TEMPLATE_LAYOUT2_RELS = TEMPLATE_LAYOUT1_RELS

TEMPLATE_PRES_PROPS = XML + '<p:presentationPr xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>'
TEMPLATE_TABLE_STYLES = XML + '<a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"/>'
TEMPLATE_VIEW_PROPS = XML + '<p:viewPr xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>'
TEMPLATE_CORE = XML + '''<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>deckr template</dc:title><dc:creator>deckr</dc:creator><cp:lastModifiedBy>deckr</cp:lastModifiedBy><dcterms:created xsi:type="dcterms:W3CDTF">2020-01-01T00:00:00Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2020-01-01T00:00:00Z</dcterms:modified></cp:coreProperties>'''
TEMPLATE_APP = XML + '''<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Application>deckr</Application><Company>deckr</Company><PresentationFormat>Widescreen</PresentationFormat></Properties>'''


def make_template(out):
    """Write a minimal .potx with a custom (non-Office) theme."""
    parts = [
        ("[Content_Types].xml", TEMPLATE_CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("ppt/presentation.xml", TEMPLATE_PRES),
        ("ppt/_rels/presentation.xml.rels", TEMPLATE_PRES_RELS),
        ("ppt/theme/theme1.xml", TEMPLATE_THEME),
        ("ppt/slideMasters/slideMaster1.xml", TEMPLATE_MASTER),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", TEMPLATE_MASTER_RELS),
        ("ppt/slideLayouts/slideLayout1.xml", TEMPLATE_LAYOUT1),
        ("ppt/slideLayouts/slideLayout1.xml.rels", TEMPLATE_LAYOUT1_RELS),
        ("ppt/slideLayouts/slideLayout2.xml", TEMPLATE_LAYOUT2),
        ("ppt/slideLayouts/slideLayout2.xml.rels", TEMPLATE_LAYOUT2_RELS),
        ("ppt/presProps.xml", TEMPLATE_PRES_PROPS),
        ("ppt/tableStyles.xml", TEMPLATE_TABLE_STYLES),
        ("ppt/viewProps.xml", TEMPLATE_VIEW_PROPS),
        ("docProps/core.xml", TEMPLATE_CORE),
        ("docProps/app.xml", TEMPLATE_APP),
    ]
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for name, body in parts:
            z.writestr(name, body)
    print("wrote template", out)


if __name__ == "__main__":
    args = sys.argv[1:]
    if args and args[0] == "--template":
        out = args[1] if len(args) > 1 else "template.pptx"
        make_template(out)
    else:
        out = args[0] if args else "sample.pptx"
        with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
            for name, body in PARTS:
                z.writestr(name, body)
            # Written as bytes rather than text: a PNG is not UTF-8 and must
            # not be mangled on the way into the archive.
            z.writestr("ppt/media/image1.png", png_1x1())
        print("wrote", out)
