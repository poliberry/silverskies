//! GRLevelX placefiles: the plain-text overlay format spotter networks,
//! the IEM and many hobby feeds publish (spotter positions, reports,
//! warning tracks, station plots, custom annotations).
//!
//! Supported: `Title`, `Refresh`/`RefreshSeconds`, `Threshold`, `Color`,
//! `Font`, `IconFile`, `Icon`, `Text`, `Place`, `Line`, `Polygon`,
//! `Triangles` and `Object` blocks (items placed by pixel offsets from a
//! point). `Image`, `Raster` and `TimeRange` lines are skipped.

/// A colour, RGBA.
pub type Rgba = [u8; 4];

/// Where an item sits: a map position, or pixels from an object's anchor
/// (x right, y up, as GR draws them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pt {
    Geo {
        lat: f64,
        lon: f64,
    },
    Offset {
        lat: f64,
        lon: f64,
        dx: f32,
        dy: f32,
    },
}

impl Pt {
    /// The map position (the anchor for offsets).
    pub fn latlon(&self) -> (f64, f64) {
        match *self {
            Pt::Geo { lat, lon } | Pt::Offset { lat, lon, .. } => (lat, lon),
        }
    }

    /// Screen offset in pixels (x right, y down).
    pub fn pixels(&self) -> (f32, f32) {
        match *self {
            Pt::Geo { .. } => (0.0, 0.0),
            Pt::Offset { dx, dy, .. } => (dx, -dy),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Line {
        width: f32,
        label: Option<String>,
        pts: Vec<Pt>,
    },
    /// A filled polygon; the first vertex's colour fills it.
    Polygon { pts: Vec<Pt>, fill: Rgba },
    /// Filled triangles, three vertices each.
    Triangles { pts: Vec<Pt>, fill: Rgba },
    Text {
        at: Pt,
        font: u8,
        text: String,
        hover: Option<String>,
    },
    Icon {
        at: Pt,
        /// Degrees clockwise.
        angle: f32,
        file: u8,
        /// 1-based, left to right then top to bottom in the sheet.
        index: u16,
        hover: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub color: Rgba,
    /// Shown only when the map shows at most this many nautical miles from
    /// its centre to its edge.
    pub threshold_nm: f32,
}

/// An icon sheet: icons of `w`×`h` px, drawn with (`hot_x`, `hot_y`) on
/// the point.
#[derive(Debug, Clone, PartialEq)]
pub struct IconFile {
    pub number: u8,
    pub w: u32,
    pub h: u32,
    pub hot_x: f32,
    pub hot_y: f32,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Placefile {
    pub title: String,
    /// How often to re-read it, seconds.
    pub refresh_s: Option<u64>,
    pub icon_files: Vec<IconFile>,
    /// Font number → size in points.
    pub fonts: Vec<(u8, f32)>,
    pub items: Vec<Item>,
}

impl Placefile {
    pub fn font_size(&self, font: u8) -> f32 {
        self.fonts
            .iter()
            .find(|(n, _)| *n == font)
            .map_or(11.0, |(_, s)| *s)
    }
}

/// Split a directive's arguments at top-level commas; quoted strings keep
/// their commas, and adjacent quoted strings (`"a" "b"`) are joined.
/// Returns (field, was quoted).
fn fields(s: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut in_q = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                // Spaces before an opening quote aren't part of the text.
                if !in_q && !quoted && cur.trim().is_empty() {
                    cur.clear();
                }
                in_q = !in_q;
                quoted = true;
            }
            '\\' if in_q => match chars.next() {
                Some('n') => cur.push('\n'),
                Some(o) => cur.push(o),
                None => {}
            },
            ',' if !in_q => {
                out.push((finish(&cur, quoted), quoted));
                cur.clear();
                quoted = false;
            }
            // Between two quoted parts: drop the whitespace.
            c if !in_q && quoted && c.is_whitespace() => {}
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || quoted {
        out.push((finish(&cur, quoted), quoted));
    }
    out
}

fn finish(s: &str, quoted: bool) -> String {
    if quoted {
        s.to_string()
    } else {
        s.trim().to_string()
    }
}

fn num<T: std::str::FromStr>(f: &[(String, bool)], i: usize) -> Option<T> {
    f.get(i)?.0.trim().parse().ok()
}

fn text_at(f: &[(String, bool)], i: usize) -> Option<String> {
    f.get(i)
        .map(|(s, _)| s.trim_end_matches('\n').to_string())
        .filter(|s| !s.trim().is_empty())
}

/// `r g b [a]` (spaces or commas).
fn parse_color(s: &str) -> Option<Rgba> {
    let v: Vec<u8> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    match v.as_slice() {
        [r, g, b] => Some([*r, *g, *b, 255]),
        [r, g, b, a, ..] => Some([*r, *g, *b, *a]),
        _ => None,
    }
}

/// Parse a placefile; unknown or malformed lines are skipped.
pub fn parse(text: &str) -> Placefile {
    let mut pf = Placefile::default();
    let mut color: Rgba = [255, 255, 255, 255];
    let mut threshold: f32 = 999.0;
    let mut anchor: Option<(f64, f64)> = None;
    // An open Line/Polygon/Triangles block and its points.
    enum Block {
        Line(f32, Option<String>),
        Polygon,
        Triangles,
    }
    let mut block: Option<(Block, Vec<Pt>, Option<Rgba>)> = None;
    // Thresholds set inside an object apply to it only.
    let mut outer_threshold = threshold;

    let point = |f: &[(String, bool)], anchor: Option<(f64, f64)>| -> Option<Pt> {
        let a: f64 = num(f, 0)?;
        let b: f64 = num(f, 1)?;
        Some(match anchor {
            Some((lat, lon)) => Pt::Offset {
                lat,
                lon,
                dx: a as f32,
                dy: b as f32,
            },
            None => Pt::Geo { lat: a, lon: b },
        })
    };

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with("//") {
            continue;
        }
        let (key, rest) = match line.split_once(':') {
            // A coordinate line ("35.1, -97.4") has no directive.
            Some((k, r)) if k.chars().all(|c| c.is_ascii_alphabetic()) => {
                (k.to_ascii_lowercase(), r.trim())
            }
            _ => (String::new(), line),
        };
        if key == "end" {
            if let Some((b, pts, fill)) = block.take() {
                let kind = match b {
                    Block::Line(width, label) => Kind::Line { width, label, pts },
                    Block::Polygon => Kind::Polygon {
                        pts,
                        fill: fill.unwrap_or(color),
                    },
                    Block::Triangles => Kind::Triangles {
                        pts,
                        fill: fill.unwrap_or(color),
                    },
                };
                pf.items.push(Item {
                    kind,
                    color,
                    threshold_nm: threshold,
                });
            } else if anchor.take().is_some() {
                threshold = outer_threshold;
            }
            continue;
        }
        if key.is_empty() {
            // A vertex of the open block.
            if let Some((_, pts, fill)) = block.as_mut() {
                let f = fields(rest);
                if let Some(p) = point(&f, anchor) {
                    pts.push(p);
                    if fill.is_none() && f.len() >= 5 {
                        let c: Vec<String> = f[2..].iter().map(|(s, _)| s.clone()).collect();
                        *fill = color_from(&c.join(" "));
                    }
                }
            }
            continue;
        }
        let f = fields(rest);
        match key.as_str() {
            "title" => pf.title = rest.to_string(),
            "refresh" => pf.refresh_s = rest.parse::<f64>().ok().map(|m| (m * 60.0) as u64),
            "refreshseconds" => pf.refresh_s = rest.parse::<f64>().ok().map(|s| s as u64),
            "threshold" => {
                if let Ok(t) = rest.parse() {
                    threshold = t;
                    if anchor.is_none() {
                        outer_threshold = t;
                    }
                }
            }
            "color" => {
                if let Some(c) = parse_color(rest) {
                    color = c;
                }
            }
            "font" => {
                if let (Some(n), Some(size)) = (num::<u8>(&f, 0), num::<f32>(&f, 1)) {
                    pf.fonts.push((n, size));
                }
            }
            "iconfile" => {
                if let (Some(number), Some(w), Some(h), Some(hot_x), Some(hot_y), Some(url)) = (
                    num(&f, 0),
                    num(&f, 1),
                    num(&f, 2),
                    num(&f, 3),
                    num(&f, 4),
                    text_at(&f, 5),
                ) {
                    pf.icon_files.push(IconFile {
                        number,
                        w,
                        h,
                        hot_x,
                        hot_y,
                        url: url.trim().to_string(),
                    });
                }
            }
            "object" => {
                if let (Some(lat), Some(lon)) = (num(&f, 0), num(&f, 1)) {
                    anchor = Some((lat, lon));
                    outer_threshold = threshold;
                }
            }
            "icon" => {
                if let (Some(at), Some(angle), Some(file), Some(index)) =
                    (point(&f, anchor), num(&f, 2), num(&f, 3), num(&f, 4))
                {
                    pf.items.push(Item {
                        kind: Kind::Icon {
                            at,
                            angle,
                            file,
                            index,
                            hover: text_at(&f, 5),
                        },
                        color,
                        threshold_nm: threshold,
                    });
                }
            }
            "text" => {
                if let (Some(at), Some(font), Some(text)) =
                    (point(&f, anchor), num(&f, 2), f.get(3).map(|s| s.0.clone()))
                {
                    pf.items.push(Item {
                        kind: Kind::Text {
                            at,
                            font,
                            text,
                            hover: text_at(&f, 4),
                        },
                        color,
                        threshold_nm: threshold,
                    });
                }
            }
            "place" => {
                if let (Some(at), Some(text)) = (point(&f, anchor), text_at(&f, 2)) {
                    pf.items.push(Item {
                        kind: Kind::Text {
                            at,
                            font: 1,
                            text,
                            hover: None,
                        },
                        color,
                        threshold_nm: threshold,
                    });
                }
            }
            "line" => {
                let width = num(&f, 0).unwrap_or(1.0);
                block = Some((Block::Line(width, text_at(&f, 2)), Vec::new(), None));
            }
            "polygon" => block = Some((Block::Polygon, Vec::new(), None)),
            "triangles" => block = Some((Block::Triangles, Vec::new(), None)),
            _ => {}
        }
    }
    pf
}

fn color_from(s: &str) -> Option<Rgba> {
    parse_color(s)
}

/// Resolve an icon sheet URL against the placefile's own URL.
pub fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if let Some(rest) = href.strip_prefix('/') {
        // Scheme and host of the base.
        let scheme_end = base.find("://").map_or(0, |i| i + 3);
        let host_end = base[scheme_end..]
            .find('/')
            .map_or(base.len(), |i| scheme_end + i);
        return format!("{}/{rest}", &base[..host_end]);
    }
    match base.rfind('/') {
        Some(i) if i > base.find("://").map_or(0, |j| j + 2) => format!("{}/{href}", &base[..i]),
        _ => format!("{}/{href}", base.trim_end_matches('/')),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"Refresh: 1
Threshold: 999
Title: Spotters
Font: 1, 11, 0, "Courier New"
IconFile: 1, 30, 30, 15, 15, "https://example.org/sheet.png"
; a comment

Object: 35.3465,-82.4318
Icon: 0,0,090,1,6,"Jo Doe\n2026-11-20 20:45 UTC" "\nSTATIONARY"
Threshold: 150
Text: 15, 10, 1, "Jo, Doe"
End:
Color: 255 255 0
Line: 2,0,"Track\n223/34"
34.05,-95.04
34.15, -94.65
END:
Color: 255 0 0 128
Polygon:
40.0, -100.0, 0, 0, 255, 100
41.0, -100.0
41.0, -99.0
End:
Icon: 36.06,-120.28,121,1,10,"+15 min"
Place: 36.1, -120.3, "Town"
Image: "x.png"
"#;

    #[test]
    fn parses_a_placefile() {
        let pf = parse(SAMPLE);
        assert_eq!(pf.title, "Spotters");
        assert_eq!(pf.refresh_s, Some(60));
        assert_eq!(pf.icon_files.len(), 1);
        assert_eq!(pf.icon_files[0].url, "https://example.org/sheet.png");
        assert_eq!(pf.font_size(1), 11.0);
        assert_eq!(pf.items.len(), 6, "{:#?}", pf.items);

        let Kind::Icon {
            at,
            angle,
            index,
            hover,
            ..
        } = &pf.items[0].kind
        else {
            panic!()
        };
        assert_eq!(
            *at,
            Pt::Offset {
                lat: 35.3465,
                lon: -82.4318,
                dx: 0.0,
                dy: 0.0
            }
        );
        assert_eq!((*angle, *index), (90.0, 6));
        assert_eq!(
            hover.as_deref(),
            Some("Jo Doe\n2026-11-20 20:45 UTC\nSTATIONARY")
        );
        assert_eq!(pf.items[0].threshold_nm, 999.0);

        let Kind::Text { at, text, .. } = &pf.items[1].kind else {
            panic!()
        };
        assert_eq!(text, "Jo, Doe");
        assert_eq!(at.pixels(), (15.0, -10.0));
        assert_eq!(pf.items[1].threshold_nm, 150.0);

        let Kind::Line { width, label, pts } = &pf.items[2].kind else {
            panic!()
        };
        assert_eq!(
            (*width, label.as_deref(), pts.len()),
            (2.0, Some("Track\n223/34"), 2)
        );
        // The object's threshold ended with it.
        assert_eq!(pf.items[2].threshold_nm, 999.0);
        assert_eq!(pf.items[2].color, [255, 255, 0, 255]);

        let Kind::Polygon { pts, fill } = &pf.items[3].kind else {
            panic!()
        };
        assert_eq!((pts.len(), *fill), (3, [0, 0, 255, 100]));

        let Kind::Icon { at, .. } = &pf.items[4].kind else {
            panic!()
        };
        assert_eq!(at.latlon(), (36.06, -120.28));
        let Kind::Text { text, .. } = &pf.items[5].kind else {
            panic!()
        };
        assert_eq!(text, "Town");
    }

    #[test]
    fn resolves_icon_urls() {
        let b = "https://h.org/feeds/gr.txt";
        assert_eq!(resolve_url(b, "sheet.png"), "https://h.org/feeds/sheet.png");
        assert_eq!(resolve_url(b, "/i/s.png"), "https://h.org/i/s.png");
        assert_eq!(resolve_url(b, "http://o.org/s.png"), "http://o.org/s.png");
    }

    /// Real files: `SS_PLACEFILES=/dir` of `.txt` placefiles.
    #[test]
    fn parses_real_files_when_available() {
        let Ok(dir) = std::env::var("SS_PLACEFILES") else {
            return;
        };
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let pf = parse(&String::from_utf8_lossy(&std::fs::read(e.path()).unwrap()));
            let mut counts = std::collections::BTreeMap::new();
            for it in &pf.items {
                let k = match it.kind {
                    Kind::Line { .. } => "line",
                    Kind::Polygon { .. } => "polygon",
                    Kind::Triangles { .. } => "triangles",
                    Kind::Text { .. } => "text",
                    Kind::Icon { .. } => "icon",
                };
                *counts.entry(k).or_insert(0) += 1;
            }
            println!(
                "{:?}: {:?} {:?} icons {}",
                e.file_name(),
                pf.title,
                counts,
                pf.icon_files.len()
            );
        }
    }
}
