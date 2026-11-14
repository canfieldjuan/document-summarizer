//! Opt-in resolved geometry. Legacy OutputDev callers never enter this path.
//! Geometry formulas port PDFMiner.six 20260107 LTChar and process_page;
//! descriptors port PDFFont/FontMetricsDB. See PROVENANCE.md and NOTICE-ADOBE-AFM.
use super::*;

#[derive(Debug, Clone)]
pub struct GlyphGeometry {
    pub text: String,
    pub occurrence: usize,
    pub font_name: String,
    pub matrix: [f64; 6],
    pub bounds: [f64; 4],
    pub advance: f64,
    pub size: f64,
    pub upright: bool,
    pub vertical: bool,
    pub font_size: f64,
    pub rise: f64,
    pub horizontal_scaling: f64,
    pub ascent: f64,
    pub descent: f64,
    pub font_bbox: [f64; 4],
    pub font_scale: [f64; 2],
    pub displacement: Option<(Option<f64>, f64)>,
}

#[derive(Debug, Clone)]
pub struct PageGeometry {
    pub page_number: u32,
    pub media_box: [f64; 4],
    pub rotation: i64,
    pub transform: [f64; 6],
    pub glyphs: Vec<GlyphGeometry>,
}

/// A document parsed from original bytes with precise decimal representation.
/// Cannot be constructed from an already rounded legacy Document.
///
/// ```compile_fail
/// let legacy = pdf_extract::Document::new();
/// pdf_extract::geometry::extract_page_geometry(&legacy, 1).unwrap();
/// ```
pub struct PreciseDocument(Document);

impl PreciseDocument {
    pub fn load_mem(bytes: &[u8]) -> Result<Self, lopdf::Error> {
        Document::load_mem_with_options(
            bytes,
            lopdf::LoadOptions {
                numeric_precision: lopdf::NumericPrecision::Precise,
                ..Default::default()
            },
        )
        .map(Self)
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, OutputError> {
        Ok(Self::load_mem(&std::fs::read(path)?)?)
    }
}

/// Resolve one actual page. No text reordering, deduplication or app policy here.
pub fn extract_page_geometry(
    precise: &PreciseDocument,
    page_number: u32,
) -> Result<PageGeometry, OutputError> {
    let doc = &precise.0;
    let object_id = *doc
        .get_pages()
        .get(&page_number)
        .ok_or(lopdf::Error::PageNumberNotFound(page_number))?;
    let page = doc.get_dictionary(object_id)?;
    let bounds: Vec<f64> =
        get_inherited(doc, page, b"MediaBox").ok_or_else(|| invalid("missing MediaBox"))?;
    if bounds.len() != 4 || bounds.iter().any(|x| !x.is_finite()) {
        return Err(invalid("invalid MediaBox"));
    }
    let rotation = get_inherited::<i64>(doc, page, b"Rotate")
        .unwrap_or(0)
        .rem_euclid(360);
    let [x0, y0, x1, y1] = [bounds[0], bounds[1], bounds[2], bounds[3]];
    let transform = match rotation {
        0 => [1., 0., 0., 1., -x0, -y0],
        90 => [0., -1., 1., 0., -y0, x1],
        180 => [-1., 0., 0., -1., x1, y1],
        270 => [0., 1., -1., 0., y1, -x0],
        _ => return Err(invalid("non-quarter page rotation not qualified")),
    };
    let mut collector = Collector(PageGeometry {
        page_number,
        media_box: [x0, y0, x1, y1],
        rotation,
        transform,
        glyphs: Vec::new(),
    });
    let empty = Dictionary::new();
    let mut processor = Processor::new();
    processor.initial_ctm = Transform::row_major(
        transform[0],
        transform[1],
        transform[2],
        transform[3],
        transform[4],
        transform[5],
    );
    output_doc_inner(
        page_number,
        object_id,
        doc,
        &mut processor,
        &mut collector,
        &empty,
    )?;
    Ok(collector.0)
}

fn invalid(message: &str) -> OutputError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message).into()
}

struct Collector(PageGeometry);
impl OutputDev for Collector {
    fn wants_geometry(&self) -> bool {
        true
    }
    fn output_glyph(&mut self, mut glyph: GlyphGeometry) -> Result<(), OutputError> {
        if glyph
            .bounds
            .iter()
            .chain(glyph.matrix.iter())
            .chain([glyph.advance, glyph.size].iter())
            .any(|x| !x.is_finite())
        {
            return Err(invalid("nonfinite resolved glyph geometry"));
        }
        glyph.occurrence = self.0.glyphs.len();
        self.0.glyphs.push(glyph);
        Ok(())
    }
    fn begin_page(
        &mut self,
        _: u32,
        _: &MediaBox,
        _: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn output_character(
        &mut self,
        _: &Transform,
        _: f64,
        _: f64,
        _: f64,
        _: &str,
    ) -> Result<(), OutputError> {
        Err(invalid("legacy glyph in enriched path"))
    }
    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

#[derive(Debug)]
pub(super) struct ReferenceFont<'a> {
    decoder: Rc<dyn PdfFont + 'a>,
    pub name: String,
    pub ascent: f64,
    pub descent: f64,
    pub bbox: [f64; 4],
    pub scale: [f64; 2],
    pub vertical: bool,
    widths: HashMap<u32, f64>,
    unicode_widths: HashMap<String, f64>,
    default_width: f64,
    displacements: HashMap<u32, (Option<f64>, f64)>,
    default_displacement: (Option<f64>, f64),
}

impl PdfFont for ReferenceFont<'_> {
    fn get_width(&self, code: CharCode) -> f64 {
        self.width(code) * self.scale[0] * 1000.
    }
    fn next_char(&self, iter: &mut Iter<u8>) -> Option<(CharCode, u8)> {
        self.decoder.next_char(iter)
    }
    fn decode_char(&self, code: CharCode) -> String {
        self.decoder.decode_char(code)
    }
    fn reference_geometry(&self) -> Option<&ReferenceFont<'_>> {
        Some(self)
    }
}

impl<'a> ReferenceFont<'a> {
    pub(super) fn new(
        doc: &'a Document,
        font: &'a Dictionary,
    ) -> Result<Rc<dyn PdfFont + 'a>, OutputError> {
        let subtype = get_name_string(doc, font, b"Subtype");
        let empty = Dictionary::new();
        let mut dict = font;
        let mut vertical = false;
        if subtype == "Type0" {
            let descendants = maybe_get_array(doc, font, b"DescendantFonts")
                .ok_or_else(|| invalid("missing descendants"))?;
            dict = maybe_deref(
                doc,
                descendants
                    .first()
                    .ok_or_else(|| invalid("empty descendants"))?,
            )
            .as_dict()?;
            vertical = match maybe_get_name(doc, font, b"Encoding") {
                Some(b"Identity-H") => false,
                Some(b"Identity-V") => true,
                _ => return Err(invalid("CMap writing-mode resolution needs qualification")),
            };
        }
        let descriptor = maybe_get_obj(doc, dict, b"FontDescriptor")
            .and_then(|x| x.as_dict().ok())
            .unwrap_or(&empty);
        let bbox = maybe_get::<Vec<f64>>(doc, descriptor, b"FontBBox").unwrap_or(vec![0.; 4]);
        if bbox.len() != 4 {
            return Err(invalid("invalid font bbox"));
        }
        let decoder: Rc<dyn PdfFont> = match subtype.as_str() {
            "Type0" => Rc::new(PdfCIDFont::new(doc, font)),
            "Type3" => Rc::new(PdfType3Font::new(doc, font)),
            _ => Rc::new(PdfSimpleFont::new_with_geometry(doc, font, true)),
        };
        let mut resolved = Self {
            decoder,
            name: maybe_get_name_string(doc, descriptor, b"FontName")
                .unwrap_or_else(|| "unknown".into()),
            ascent: maybe_get(doc, descriptor, b"Ascent").unwrap_or(0.),
            descent: -maybe_get::<f64>(doc, descriptor, b"Descent")
                .unwrap_or(0.)
                .abs(),
            bbox: [bbox[0], bbox[1], bbox[2], bbox[3]],
            scale: [0.001, 0.001],
            vertical,
            widths: HashMap::new(),
            unicode_widths: HashMap::new(),
            default_width: maybe_get(doc, descriptor, b"MissingWidth").unwrap_or(0.),
            displacements: HashMap::new(),
            default_displacement: (None, 880.),
        };
        if subtype == "Type0" {
            if vertical {
                let dw = maybe_get::<Vec<f64>>(doc, dict, b"DW2").unwrap_or(vec![880., -1000.]);
                if dw.len() != 2 {
                    return Err(invalid("invalid DW2"));
                }
                resolved.default_displacement = (None, dw[0]);
                resolved.default_width = dw[1];
            } else {
                resolved.default_width = maybe_get(doc, dict, b"DW").unwrap_or(1000.);
            }
            if let Some(entries) = maybe_get_array(doc, dict, if vertical { b"W2" } else { b"W" }) {
                let mut i = 0;
                while i < entries.len() {
                    let first = maybe_deref(doc, &entries[i]).as_i64()? as u32;
                    let next = entries
                        .get(i + 1)
                        .ok_or_else(|| invalid("incomplete CID widths"))?;
                    if let Ok(array) = maybe_deref(doc, next).as_array() {
                        let step = if vertical { 3 } else { 1 };
                        if array.len() % step != 0 {
                            return Err(invalid("incomplete W2 tuple"));
                        }
                        for (offset, values) in array.chunks(step).enumerate() {
                            let code = first + offset as u32;
                            resolved
                                .widths
                                .insert(code, as_num(maybe_deref(doc, &values[0])));
                            if vertical {
                                resolved.displacements.insert(
                                    code,
                                    (
                                        Some(as_num(maybe_deref(doc, &values[1]))),
                                        as_num(maybe_deref(doc, &values[2])),
                                    ),
                                );
                            }
                        }
                        i += 2;
                    } else {
                        let last = maybe_deref(doc, next).as_i64()? as u32;
                        let count = if vertical { 3 } else { 1 };
                        let values = entries
                            .get(i + 2..i + 2 + count)
                            .ok_or_else(|| invalid("incomplete CID width range"))?;
                        for code in first..=last {
                            resolved
                                .widths
                                .insert(code, as_num(maybe_deref(doc, &values[0])));
                            if vertical {
                                resolved.displacements.insert(
                                    code,
                                    (
                                        Some(as_num(maybe_deref(doc, &values[1]))),
                                        as_num(maybe_deref(doc, &values[2])),
                                    ),
                                );
                            }
                        }
                        i += 2 + count;
                    }
                }
            }
        } else {
            let base = maybe_get_name_string(doc, font, b"BaseFont").unwrap_or_default();
            if let Some((canonical, ascent, descent, bbox)) =
                super::reference_metrics::metrics(&base).filter(|_| subtype != "Type3")
            {
                resolved.name = canonical.into();
                resolved.ascent = ascent;
                resolved.descent = descent;
                resolved.bbox = bbox;
                resolved.default_width = 0.;
                for &(code, width) in super::reference_metrics::widths(canonical) {
                    if let Some(ch) = char::from_u32(code) {
                        resolved.unicode_widths.insert(ch.to_string(), width);
                    }
                }
            } else {
                let first = maybe_get::<i64>(doc, font, b"FirstChar").unwrap_or(0);
                let widths = maybe_get::<Vec<f64>>(doc, font, b"Widths").unwrap_or(vec![0.; 256]);
                for (index, width) in widths.into_iter().enumerate() {
                    resolved.widths.insert((first + index as i64) as u32, width);
                }
            }
            if subtype == "Type3" {
                if descriptor.is_empty() {
                    let bbox = maybe_get::<Vec<f64>>(doc, font, b"FontBBox")
                        .ok_or_else(|| invalid("missing Type3 bbox"))?;
                    if bbox.len() != 4 {
                        return Err(invalid("invalid Type3 bbox"));
                    }
                    resolved.bbox = [bbox[0], bbox[1], bbox[2], bbox[3]];
                }
                resolved.descent = resolved.bbox[1];
                resolved.ascent = resolved.bbox[3];
                let matrix = maybe_get::<Vec<f64>>(doc, font, b"FontMatrix")
                    .ok_or_else(|| invalid("missing Type3 matrix"))?;
                if matrix.len() != 6 {
                    return Err(invalid("invalid Type3 matrix"));
                }
                resolved.scale = [matrix[0] + matrix[2], matrix[1] + matrix[3]];
            }
        }
        Ok(Rc::new(resolved))
    }

    fn width(&self, code: u32) -> f64 {
        self.widths
            .get(&code)
            .copied()
            .or_else(|| self.unicode_widths.get(&self.decode_char(code)).copied())
            .unwrap_or(self.default_width)
    }

    pub(super) fn emit(
        &self,
        code: u32,
        ts: &TextState,
        ctm: &Transform,
        output: &mut dyn OutputDev,
    ) -> Result<f64, OutputError> {
        let matrix = ts
            .tm
            .pre_transform(&Transform::create_translation(
                ts.geometry_position.0,
                ts.geometry_position.1,
            ))
            .post_transform(ctm);
        let advance = self.width(code) * self.scale[0] * ts.font_size * ts.horizontal_scaling;
        let displacement = if self.vertical {
            Some(
                *self
                    .displacements
                    .get(&code)
                    .unwrap_or(&self.default_displacement),
            )
        } else {
            None
        };
        let bbox = if let Some((vx, vy)) = displacement {
            let vx = vx
                .map(|x| x * ts.font_size * 0.001)
                .unwrap_or(ts.font_size * 0.5);
            let vy = (1000. - vy) * ts.font_size * 0.001;
            [
                -vx,
                vy + ts.rise + advance,
                -vx + ts.font_size,
                vy + ts.rise,
            ]
        } else {
            let descent = self.descent * self.scale[1] * ts.font_size;
            [
                0.,
                descent + ts.rise,
                advance,
                descent + ts.rise + ts.font_size,
            ]
        };
        let points = [
            (bbox[0], bbox[1]),
            (bbox[0], bbox[3]),
            (bbox[2], bbox[1]),
            (bbox[2], bbox[3]),
        ]
        .map(|(x, y)| matrix.transform_point(point2(x, y)));
        let bounds = [
            points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
            points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
        ];
        output.output_glyph(GlyphGeometry {
            text: self.decode_char(code),
            occurrence: 0,
            font_name: self.name.clone(),
            matrix: [
                matrix.m11, matrix.m12, matrix.m21, matrix.m22, matrix.m31, matrix.m32,
            ],
            bounds,
            advance,
            size: if self.vertical {
                bounds[2] - bounds[0]
            } else {
                bounds[3] - bounds[1]
            },
            upright: matrix.m11 * matrix.m22 * ts.horizontal_scaling > 0.
                && matrix.m12 * matrix.m21 <= 0.,
            vertical: self.vertical,
            font_size: ts.font_size,
            rise: ts.rise,
            horizontal_scaling: ts.horizontal_scaling,
            ascent: self.ascent,
            descent: self.descent,
            font_bbox: self.bbox,
            font_scale: self.scale,
            displacement,
        })?;
        Ok(advance)
    }
}

pub(super) fn adjust_text(ts: &mut TextState, adjustment: f64) -> Result<(), OutputError> {
    let font = ts
        .font
        .as_ref()
        .and_then(|f| f.reference_geometry())
        .ok_or_else(|| invalid("missing resolved font before TJ"))?;
    let amount = adjustment * 0.001 * ts.font_size * ts.horizontal_scaling;
    if font.vertical {
        ts.geometry_position.1 -= amount;
    } else {
        ts.geometry_position.0 -= amount;
    }
    Ok(())
}

pub(super) fn show_text(
    gs: &mut GraphicsState,
    text: &[u8],
    output: &mut dyn OutputDev,
) -> Result<(), OutputError> {
    let ts = &mut gs.ts;
    let owner = ts
        .font
        .clone()
        .ok_or_else(|| invalid("missing font before text"))?;
    let font = owner
        .reference_geometry()
        .ok_or_else(|| invalid("legacy font in enriched path"))?;
    let mut needs_space = if font.vertical {
        ts.geometry_position.1 != 0.
    } else {
        ts.geometry_position.0 != 0.
    };
    for (code, length) in owner.char_codes(text) {
        let space = if needs_space {
            ts.character_spacing * ts.horizontal_scaling
        } else {
            0.
        };
        if font.vertical {
            ts.geometry_position.1 += space;
        } else {
            ts.geometry_position.0 += space;
        }
        let advance = font.emit(code, ts, &gs.ctm, output)?;
        let word_space = if code == 32 && length == 1 {
            ts.word_spacing * ts.horizontal_scaling
        } else {
            0.
        };
        if font.vertical {
            ts.geometry_position.1 += advance + word_space;
        } else {
            ts.geometry_position.0 += advance + word_space;
        }
        needs_space = true;
    }
    Ok(())
}
