//! Chart number decoding — `ChartBlob` → `ChartData`.
//!
//! Charts are captured verbatim as opaque bytes (see `ir::ChartBlob`) because
//! writing them back needs none of their internals. *Reading* them, though, is
//! worth real numbers: this module walks the chart part's XML — the
//! `c:chartSpace` inside `ppt/charts/chartN.xml` — and pulls out what a human
//! would call "the data": one entry per series, its name, its category labels
//! and its values. `None` values mean "the cell is empty in the source", not
//! zero; that distinction is why the vector is optional per point.
//!
//! The decoder is deliberately forgiving: a chart whose shape it does not
//! recognise (bubble, doughnut, a series with literal-only data) still yields
//! whatever series it did understand, and yields nothing at all rather than an
//! error when the XML is unreadable. The verbatim blob remains the writer's
//! source of truth either way.

use quick_xml::Reader as XmlReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;

use crate::ir::{ChartBlob, ChartPart};

/// What kind of chart the numbers came from — the coarse shape a renderer needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
    Area,
    Scatter,
    Radar,
    /// Anything the decoder does not model (doughnut, bubble, …). The data,
    /// when present, is still series × categories → values.
    #[default]
    Other,
}

/// One decoded series: a name, the category labels and one value per category.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ChartSeries {
    /// The series legend text, when the chart carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Category labels — `"Q1"`, `"2024"`, whatever the axis says.
    pub categories: Vec<String>,
    /// One value per category. `None` is an empty cell, not a zero.
    pub values: Vec<Option<f64>>,
}

/// The decoded numbers of a chart: title, kind, and every series.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ChartData {
    /// The chart's own title text, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub kind: ChartKind,
    pub series: Vec<ChartSeries>,
}

impl ChartData {
    /// Whether any series carries at least one numeric point.
    pub fn has_numbers(&self) -> bool {
        self.series
            .iter()
            .any(|s| s.values.iter().any(|v| v.is_some()))
    }

    /// A display name for a series: its legend text, else "series N" (1-based).
    pub fn series_label(&self, index: usize) -> String {
        self.series
            .get(index)
            .and_then(|s| s.name.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("series {}", index + 1))
    }
}

/// Decode the chart XML part out of a captured blob.
///
/// The blob's `chart_xml` is only the `graphicFrame` stub (it exists so the
/// writer can remap the relationship id); the real `c:chartSpace` lives among
/// the captured `parts`. This finds that part and decodes it.
pub fn decode_blob(blob: &ChartBlob) -> Option<ChartData> {
    let part = chart_xml_part(blob)?;
    decode(&String::from_utf8_lossy(&part.bytes))
}

/// The captured part that *is* the chart XML — by content type when the
/// package declared one, else by the `ppt/charts/chartN.xml` naming
/// convention.
pub(crate) fn chart_xml_part(blob: &ChartBlob) -> Option<&ChartPart> {
    blob.parts
        .iter()
        .find(|p| p.content_type.contains("chart+xml"))
        .or_else(|| {
            blob.parts.iter().find(|p| {
                p.path.contains("/charts/")
                    && p.path.ends_with(".xml")
                    && !p.path.ends_with(".rels")
            })
        })
}

/// Decode a `c:chartSpace` document into [`ChartData`].
///
/// Returns `None` when the XML is unreadable or carries no series — there is
/// nothing meaningful to show for an empty `c:barChart`.
pub fn decode(xml: &str) -> Option<ChartData> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut data = ChartData::default();
    let mut stack: Vec<String> = Vec::new();

    // Where in the series the captured text belongs. `depth` is the stack
    // length when the section opened, so nested elements know when it closes.
    #[derive(Clone, Copy, PartialEq)]
    enum Section {
        Tx,
        Cat,
        Val,
        XVal,
        YVal,
    }
    let mut section: Option<(Section, usize)> = None;
    let mut in_title = false;
    let mut in_v = false;
    let mut text_buf = String::new();
    let mut pt_idx: usize = 0;
    let mut cur: Option<ChartSeries> = None;

    // String and Option slots need different padding, so two tiny helpers.
    fn put_str(slots: &mut Vec<String>, idx: usize, value: String) {
        while slots.len() <= idx {
            slots.push(String::new());
        }
        slots[idx] = value;
    }

    fn put_val(slots: &mut Vec<Option<f64>>, idx: usize, value: Option<f64>) {
        while slots.len() <= idx {
            slots.push(None);
        }
        slots[idx] = value;
    }

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let local = local_name(e.name());
                match local.as_str() {
                    "title" => in_title = true,
                    "barChart" | "lineChart" | "pieChart" | "areaChart" | "scatterChart"
                    | "radarChart" => {
                        data.kind = match local.as_str() {
                            "barChart" => ChartKind::Bar,
                            "lineChart" => ChartKind::Line,
                            "pieChart" => ChartKind::Pie,
                            "areaChart" => ChartKind::Area,
                            "scatterChart" => ChartKind::Scatter,
                            _ => ChartKind::Radar,
                        };
                    }
                    "ser" => {
                        cur = Some(ChartSeries::default());
                    }
                    "tx" if cur.is_some() => section = Some((Section::Tx, stack.len())),
                    "cat" if cur.is_some() => section = Some((Section::Cat, stack.len())),
                    "val" if cur.is_some() => section = Some((Section::Val, stack.len())),
                    "xVal" if cur.is_some() => section = Some((Section::XVal, stack.len())),
                    "yVal" if cur.is_some() => section = Some((Section::YVal, stack.len())),
                    "pt" => {
                        pt_idx = attr(&e, "idx").and_then(|v| v.parse().ok()).unwrap_or(0);
                    }
                    "v" => {
                        in_v = true;
                        text_buf.clear();
                    }
                    _ => {}
                }
                stack.push(local);
            }
            Ok(Event::Text(t)) => {
                if in_v || in_title {
                    text_buf.push_str(&unescape(t.as_ref()));
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name());
                stack.pop();
                match local.as_str() {
                    "v" => {
                        in_v = false;
                        if let Some(ser) = cur.as_mut() {
                            let text = text_buf.trim().to_string();
                            match section {
                                // The series name lives at pt idx 0 of c:tx.
                                Some((Section::Tx, _)) if pt_idx == 0 => {
                                    ser.name = Some(text).filter(|s| !s.is_empty());
                                }
                                Some((Section::Cat, _)) | Some((Section::XVal, _)) => {
                                    put_str(&mut ser.categories, pt_idx, text);
                                }
                                Some((Section::Val, _)) | Some((Section::YVal, _)) => {
                                    put_val(&mut ser.values, pt_idx, text.parse().ok());
                                }
                                _ => {}
                            }
                        }
                        text_buf.clear();
                    }
                    "title" => {
                        in_title = false;
                        let text = text_buf.trim().to_string();
                        if !text.is_empty() {
                            data.title = Some(text);
                        }
                        text_buf.clear();
                    }
                    "tx" | "cat" | "val" | "xVal" | "yVal" => section = None,
                    "ser" => {
                        if let Some(ser) = cur.take() {
                            data.series.push(ser);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(_)) => {}
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    if data.series.is_empty() {
        None
    } else {
        // A series with neither name, categories nor values is chart junk
        // (some producers emit placeholder series); keep only real ones.
        data.series.retain(|s| {
            s.name.is_some() || !s.categories.is_empty() || s.values.iter().any(|v| v.is_some())
        });
        if data.series.is_empty() {
            None
        } else {
            Some(data)
        }
    }
}

/// Numbers the way a spreadsheet says them: `3`, `1.5`, never `3.0`. Gaps are
/// the caller's business — this formats a real value only.
pub(crate) fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Local name of a qualified XML name.
/// Local name of a qualified XML name (`c:ser` → `ser`).
fn local_name(name: QName<'_>) -> String {
    let full = name.0;
    match full.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => full.to_string(),
    }
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if local_name(a.key) == key {
            Some(unescape(&a.value))
        } else {
            None
        }
    })
}

/// Resolve the five entities XML permits — chart XML is machine-written but
/// `&amp;` in a series name is real, so it must decode once, not twice.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#10;", "\n")
        .replace("&#13;", "\r")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = r#"<?xml version="1.0"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
              xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<c:chart>
<c:title><c:tx><c:rich><a:p><a:r><a:t>Revenue vs Cost</a:t></a:r></a:p></c:rich></c:tx></c:title>
<c:plotArea><c:barChart><c:barDir val="col"/>
<c:ser>
  <c:idx val="0"/><c:order val="0"/>
  <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Revenue</c:v></c:pt></c:strCache></c:strRef></c:tx>
  <c:cat><c:strRef><c:strCache>
    <c:pt idx="0"><c:v>Q1</c:v></c:pt>
    <c:pt idx="1"><c:v>Q2</c:v></c:pt>
    <c:pt idx="2"><c:v>Q3</c:v></c:pt>
  </c:strCache></c:strRef></c:cat>
  <c:val><c:numRef><c:numCache>
    <c:formatCode>General</c:formatCode>
    <c:pt idx="0"><c:v>1.5</c:v></c:pt>
    <c:pt idx="1"><c:v>2.5</c:v></c:pt>
    <c:pt idx="2"><c:v>3.5</c:v></c:pt>
  </c:numCache></c:numRef></c:val>
</c:ser>
<c:ser>
  <c:idx val="1"/><c:order val="1"/>
  <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Cost</c:v></c:pt></c:strCache></c:strRef></c:tx>
  <c:cat><c:strRef><c:strCache>
    <c:pt idx="0"><c:v>Q1</c:v></c:pt>
    <c:pt idx="1"><c:v>Q2</c:v></c:pt>
    <c:pt idx="2"><c:v>Q3</c:v></c:pt>
  </c:strCache></c:strRef></c:cat>
  <c:val><c:numRef><c:numCache>
    <c:pt idx="0"><c:v>1.1</c:v></c:pt>
    <c:pt idx="1"><c:v></c:v></c:pt>
    <c:pt idx="2"><c:v>3.9</c:v></c:pt>
  </c:numCache></c:numRef></c:val>
</c:ser>
</c:barChart></c:plotArea>
</c:chart></c:chartSpace>"#;

    #[test]
    fn a_bar_chart_decodes_to_series_and_numbers() {
        let data = decode(BAR).expect("decodes");
        assert_eq!(data.title.as_deref(), Some("Revenue vs Cost"));
        assert_eq!(data.kind, ChartKind::Bar);
        assert_eq!(data.series.len(), 2);

        let revenue = &data.series[0];
        assert_eq!(revenue.name.as_deref(), Some("Revenue"));
        assert_eq!(revenue.categories, vec!["Q1", "Q2", "Q3"]);
        assert_eq!(revenue.values, vec![Some(1.5), Some(2.5), Some(3.5)]);

        // An empty <c:v></c:v> is an empty cell, not a zero.
        let cost = &data.series[1];
        assert_eq!(cost.values, vec![Some(1.1), None, Some(3.9)]);
        assert!(data.has_numbers());
    }

    #[test]
    fn literal_data_and_missing_points_are_tolerated() {
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
<c:chart><c:plotArea><c:lineChart>
<c:ser>
  <c:tx><c:v>lit</c:v></c:tx>
  <c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strLit></c:cat>
  <c:val><c:numLit><c:pt idx="1"><c:v>7</c:v></c:pt></c:numLit></c:val>
</c:ser>
</c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let data = decode(xml).expect("decodes");
        assert_eq!(data.kind, ChartKind::Line);
        let ser = &data.series[0];
        assert_eq!(ser.name.as_deref(), Some("lit"));
        assert_eq!(ser.categories, vec!["A", "B"]);
        // idx 0 was never written, so it is a gap, not zero.
        assert_eq!(ser.values, vec![None, Some(7.0)]);
    }

    #[test]
    fn scatter_maps_x_to_categories_and_y_to_values() {
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
<c:chart><c:plotArea><c:scatterChart>
<c:ser>
  <c:tx><c:v>pts</c:v></c:tx>
  <c:xVal><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:xVal>
  <c:yVal><c:numRef><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>20</c:v></c:pt></c:numCache></c:numRef></c:yVal>
</c:ser>
</c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#;
        let data = decode(xml).expect("decodes");
        assert_eq!(data.kind, ChartKind::Scatter);
        assert_eq!(data.series[0].categories, vec!["1", "2"]);
        assert_eq!(data.series[0].values, vec![Some(10.0), Some(20.0)]);
    }

    #[test]
    fn a_chart_without_series_decodes_to_nothing() {
        // This is exactly what the current fixture ships.
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
<c:chart><c:plotArea><c:barChart><c:barDir val="col"/></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        assert!(decode(xml).is_none());
        assert!(decode("not xml at all").is_none());
    }

    #[test]
    fn junk_series_are_dropped_not_reported() {
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
<c:chart><c:plotArea><c:barChart>
<c:ser><c:idx val="0"/></c:ser>
<c:ser><c:tx><c:v>real</c:v></c:tx><c:val><c:numLit><c:pt idx="0"><c:v>4</c:v></c:pt></c:numLit></c:val></c:ser>
</c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let data = decode(xml).expect("decodes");
        assert_eq!(data.series.len(), 1);
        assert_eq!(data.series[0].name.as_deref(), Some("real"));
    }

    #[test]
    fn the_blob_decoder_finds_the_chart_part_by_content_type() {
        let blob = ChartBlob {
            uri: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart".into(),
            chart_xml: "<c:chart r:id=\"rId1\"/>".into(),
            parts: vec![
                ChartPart {
                    path: "ppt/charts/_rels/chart1.xml.rels".into(),
                    bytes: b"<Relationships/>".to_vec(),
                    content_type: "application/xml".into(),
                },
                ChartPart {
                    path: "ppt/charts/chart1.xml".into(),
                    bytes: BAR.as_bytes().to_vec(),
                    content_type:
                        "application/vnd.openxmlformats-officedocument.drawingml.chart+xml".into(),
                },
            ],
        };
        let data = decode_blob(&blob).expect("decodes");
        assert_eq!(data.series.len(), 2);
    }
}
