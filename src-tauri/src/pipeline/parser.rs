use crate::pipeline::contracts::{
    DocumentParser, IngestedDocument, ParsedDocument, ParsedPage, PipelineFailure, PipelineStage,
    PipelineWarning, SourceType,
};
use crate::pipeline::db::{self, StoreError};
use lopdf::{
    content::Content, decode_text_string, Dictionary as PdfDictionary, Object as PdfObject,
    ObjectId,
};
use pdf_extract::{output_doc_page, Document as PdfDocument, Error as LopdfError, PlainTextOutput};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use thiserror::Error;

const MAX_TAGGED_OCR_CONTENT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub struct PdfExtractParser;

impl PdfExtractParser {
    pub fn new() -> Self {
        Self
    }
}

impl DocumentParser for PdfExtractParser {
    fn parse(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
        catch_unwind(AssertUnwindSafe(|| self.parse_inner(document))).unwrap_or_else(|_| {
            Err(parse_failure(
                "PDF_PARSER_PANIC",
                "The native PDF parser aborted while reading malformed page data",
                false,
            ))
        })
    }

    fn id(&self) -> &'static str {
        "pdf-extract"
    }

    fn version(&self) -> &'static str {
        "0.12.0"
    }
}

#[derive(Default)]
pub struct SourceParserSet {
    native: PdfExtractParser,
    ocr: TaggedOcrParser,
}

impl SourceParserSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn select(&self, source_type: SourceType) -> &dyn DocumentParser {
        match source_type {
            SourceType::NativeText => &self.native,
            SourceType::OcrText => &self.ocr,
        }
    }
}

impl PdfExtractParser {
    fn parse_inner(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
        if document.file_type != "pdf" {
            return Err(parse_failure(
                "UNSUPPORTED_PARSER_INPUT",
                "The native PDF parser only accepts ingested PDF documents",
                false,
            ));
        }

        let source_bytes = verified_source_bytes(document)?;

        let pdf = PdfDocument::load_mem(&source_bytes).map_err(map_load_error)?;
        if pdf.was_encrypted() || pdf.is_encrypted() {
            return Err(parse_failure(
                "ENCRYPTED_PDF_UNSUPPORTED",
                "Encrypted PDFs are not supported in the native-text parser",
                false,
            ));
        }

        let source_page_numbers = pdf.get_pages().keys().copied().collect::<Vec<_>>();
        let mut parsed_pages = Vec::new();
        let mut document_warnings = Vec::new();
        let mut empty_page_count = 0;

        for (index, source_page_number) in source_page_numbers.into_iter().enumerate() {
            let mut text = String::new();
            let mut output = PlainTextOutput::new(&mut text);
            output_doc_page(&pdf, &mut output, source_page_number).map_err(|error| {
                parse_failure(
                    "PDF_TEXT_EXTRACTION_FAILED",
                    format!(
                        "Native text extraction failed on page {}: {error}",
                        index + 1
                    ),
                    false,
                )
            })?;

            let page_number = u32::try_from(index + 1).map_err(|_| {
                parse_failure(
                    "PDF_PAGE_COUNT_OVERFLOW",
                    "The PDF has more pages than the parsed-document contract supports",
                    false,
                )
            })?;
            let mut page_warnings = Vec::new();
            let requires_visual_processing = text.trim().is_empty();
            if requires_visual_processing {
                empty_page_count += 1;
                page_warnings.push(PipelineWarning {
                    code: "NO_NATIVE_TEXT".to_string(),
                    message: "NO_NATIVE_TEXT".to_string(),
                    stage: Some(PipelineStage::Parse),
                });
            }

            parsed_pages.push(ParsedPage {
                page_number,
                text,
                warnings: page_warnings,
                requires_visual_processing,
            });
        }

        if empty_page_count > 0 && empty_page_count == parsed_pages.len() {
            document_warnings.push(PipelineWarning {
                code: "NO_NATIVE_TEXT_IN_DOCUMENT".to_string(),
                message: "NO_NATIVE_TEXT_IN_DOCUMENT".to_string(),
                stage: Some(PipelineStage::Parse),
            });
        }

        Ok(ParsedDocument {
            document_id: document.document_id.clone(),
            parser_id: self.id().to_string(),
            parser_version: self.version().to_string(),
            source_type: document.source_type,
            pages: parsed_pages,
            warnings: document_warnings,
        })
    }
}

fn verified_source_bytes(document: &IngestedDocument) -> Result<Vec<u8>, PipelineFailure> {
    let source_bytes = fs::read(&document.local_source_path).map_err(|error| {
        parse_failure(
            "SOURCE_IO_ERROR",
            format!("The ingested source could not be reopened: {error}"),
            true,
        )
    })?;
    let source_size = u64::try_from(source_bytes.len()).map_err(|_| {
        parse_failure(
            "SOURCE_CONTENT_CHANGED",
            "The source no longer matches the ingested document identity",
            true,
        )
    })?;
    let source_hash = format!("{:x}", Sha256::digest(&source_bytes));
    if source_size != document.byte_size || source_hash != document.content_hash {
        return Err(parse_failure(
            "SOURCE_CONTENT_CHANGED",
            "The source no longer matches the ingested document identity",
            true,
        ));
    }
    Ok(source_bytes)
}

#[derive(Default)]
pub struct TaggedOcrParser;

impl TaggedOcrParser {
    pub fn new() -> Self {
        Self
    }

    fn parse_inner(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
        if document.file_type != "pdf"
            || document.source_type != crate::pipeline::contracts::SourceType::OcrText
        {
            return Err(parse_failure(
                "UNSUPPORTED_PARSER_INPUT",
                "The tagged OCR parser only accepts admitted OCR PDF documents",
                false,
            ));
        }
        let source_bytes = verified_source_bytes(document)?;
        let pages = parse_tagged_ocr_source(&source_bytes)?;
        Ok(ParsedDocument {
            document_id: document.document_id.clone(),
            parser_id: self.id().to_string(),
            parser_version: self.version().to_string(),
            source_type: document.source_type,
            pages,
            warnings: Vec::new(),
        })
    }
}

fn parse_tagged_ocr_source(source_bytes: &[u8]) -> Result<Vec<ParsedPage>, PipelineFailure> {
    let pdf = load_bounded_tagged_ocr_pdf(source_bytes).map_err(|_| {
        parse_failure(
            "OCR_STRUCTURE_INVALID",
            "The OCR PDF does not match the required tagged profile",
            false,
        )
    })?;
    if pdf.was_encrypted() || pdf.is_encrypted() {
        return Err(parse_failure(
            "ENCRYPTED_PDF_UNSUPPORTED",
            "Encrypted PDFs are not supported in the tagged OCR parser",
            false,
        ));
    }
    let pages = parse_tagged_ocr_pages(&pdf).map_err(|_| {
        parse_failure(
            "OCR_STRUCTURE_INVALID",
            "The OCR PDF does not match the required tagged profile",
            false,
        )
    })?;
    let text_bytes = pages
        .iter()
        .map(|page| page.text.len())
        .try_fold(0usize, |total, length| total.checked_add(length + 1))
        .ok_or_else(|| {
            parse_failure(
                "OCR_TEXT_LIMIT_EXCEEDED",
                "The OCR text exceeds the supported size",
                false,
            )
        })?;
    if text_bytes > 256 * 1024 || pages.iter().all(|page| page.text.trim().is_empty()) {
        return Err(parse_failure(
            "OCR_TEXT_LIMIT_EXCEEDED",
            "The OCR text is empty or exceeds the supported size",
            false,
        ));
    }
    Ok(pages)
}

pub(crate) fn canonical_tagged_ocr_text(source_bytes: &[u8]) -> Result<Vec<u8>, PipelineFailure> {
    let pages = parse_tagged_ocr_source(source_bytes)?;
    let mut text = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        if index > 0 {
            text.push(0x0c);
        }
        text.extend_from_slice(page.text.as_bytes());
    }
    Ok(text)
}

fn load_bounded_tagged_ocr_pdf(source: &[u8]) -> Result<PdfDocument, ()> {
    require_classic_tagged_ocr_xref(source)?;
    PdfDocument::load_mem(source).map_err(|_| ())
}

fn require_classic_tagged_ocr_xref(source: &[u8]) -> Result<(), ()> {
    const STARTXREF: &[u8] = b"startxref";
    const EOF_MARKER: &[u8] = b"%%EOF";

    let marker_offset = source
        .windows(STARTXREF.len())
        .rposition(|window| window == STARTXREF)
        .ok_or(())?;
    let mut cursor = marker_offset + STARTXREF.len();
    skip_pdf_whitespace(source, &mut cursor);
    let number_start = cursor;
    let mut xref_offset = 0usize;
    while let Some(digit) = source.get(cursor).filter(|byte| byte.is_ascii_digit()) {
        xref_offset = xref_offset
            .checked_mul(10)
            .and_then(|value| value.checked_add(usize::from(*digit - b'0')))
            .ok_or(())?;
        cursor += 1;
    }
    if cursor == number_start {
        return Err(());
    }
    skip_pdf_whitespace(source, &mut cursor);
    if !source
        .get(cursor..)
        .is_some_and(|tail| tail.starts_with(EOF_MARKER))
    {
        return Err(());
    }
    cursor += EOF_MARKER.len();
    skip_pdf_whitespace(source, &mut cursor);
    if cursor != source.len() || xref_offset >= marker_offset {
        return Err(());
    }

    let xref = source.get(xref_offset..marker_offset).ok_or(())?;
    if !xref.starts_with(b"xref")
        || !xref.get(4).is_some_and(|byte| is_pdf_whitespace(*byte))
        || xref
            .windows(b"/Prev".len())
            .any(|window| window == b"/Prev")
        || xref
            .windows(b"/XRefStm".len())
            .any(|window| window == b"/XRefStm")
        || xref.contains(&b'#')
    {
        return Err(());
    }
    Ok(())
}

fn skip_pdf_whitespace(source: &[u8], cursor: &mut usize) {
    while source
        .get(*cursor)
        .is_some_and(|byte| is_pdf_whitespace(*byte))
    {
        *cursor += 1;
    }
}

fn is_pdf_whitespace(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 12 | b'\r' | b' ')
}

impl DocumentParser for TaggedOcrParser {
    fn parse(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
        catch_unwind(AssertUnwindSafe(|| self.parse_inner(document))).unwrap_or_else(|_| {
            Err(parse_failure(
                "OCR_PARSER_PANIC",
                "The tagged OCR parser aborted while reading malformed page data",
                false,
            ))
        })
    }

    fn id(&self) -> &'static str {
        "local-connect-tagged-ocr"
    }

    fn version(&self) -> &'static str {
        "1.0"
    }
}

type TaggedResult<T> = Result<T, ()>;

fn tagged_object<'a>(pdf: &'a PdfDocument, object: &'a PdfObject) -> TaggedResult<&'a PdfObject> {
    pdf.dereference(object)
        .map(|(_, value)| value)
        .map_err(|_| ())
}

fn tagged_dictionary<'a>(
    pdf: &'a PdfDocument,
    object: &'a PdfObject,
) -> TaggedResult<&'a PdfDictionary> {
    tagged_object(pdf, object)?.as_dict().map_err(|_| ())
}

fn tagged_array<'a>(
    pdf: &'a PdfDocument,
    object: &'a PdfObject,
) -> TaggedResult<&'a Vec<PdfObject>> {
    tagged_object(pdf, object)?.as_array().map_err(|_| ())
}

fn tagged_reference(object: &PdfObject) -> TaggedResult<ObjectId> {
    object.as_reference().map_err(|_| ())
}

fn tagged_child_ids(pdf: &PdfDocument, dictionary: &PdfDictionary) -> TaggedResult<Vec<ObjectId>> {
    tagged_array(pdf, dictionary.get(b"K").map_err(|_| ())?)?
        .iter()
        .map(tagged_reference)
        .collect()
}

fn tagged_role(dictionary: &PdfDictionary) -> TaggedResult<&[u8]> {
    dictionary
        .get(b"S")
        .map_err(|_| ())?
        .as_name()
        .map_err(|_| ())
}

fn tagged_binding(dictionary: &PdfDictionary, key: &[u8], expected: ObjectId) -> TaggedResult<()> {
    if tagged_reference(dictionary.get(key).map_err(|_| ())?)? == expected {
        Ok(())
    } else {
        Err(())
    }
}

fn tagged_number(object: &PdfObject) -> TaggedResult<f64> {
    match object {
        PdfObject::Integer(value) => Ok(*value as f64),
        PdfObject::Real(value) => Ok(f64::from(*value)),
        _ => Err(()),
    }
}

fn inherited_page_object<'a>(
    pdf: &'a PdfDocument,
    mut page_id: ObjectId,
    key: &[u8],
) -> TaggedResult<&'a PdfObject> {
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(page_id) {
            return Err(());
        }
        let page = pdf.get_dictionary(page_id).map_err(|_| ())?;
        if let Ok(value) = page.get(key) {
            return tagged_object(pdf, value);
        }
        page_id = tagged_reference(page.get(b"Parent").map_err(|_| ())?)?;
    }
}

fn tagged_page_box(pdf: &PdfDocument, page_id: ObjectId) -> TaggedResult<[f64; 4]> {
    let values = tagged_array(pdf, inherited_page_object(pdf, page_id, b"MediaBox")?)?;
    if values.len() != 4 {
        return Err(());
    }
    let result = [
        tagged_number(&values[0])?,
        tagged_number(&values[1])?,
        tagged_number(&values[2])?,
        tagged_number(&values[3])?,
    ];
    if !result.iter().all(|value| value.is_finite())
        || result[2] <= result[0]
        || result[3] <= result[1]
    {
        return Err(());
    }
    Ok(result)
}

fn tagged_bbox(pdf: &PdfDocument, span: &PdfDictionary, page_box: [f64; 4]) -> TaggedResult<()> {
    let attributes = tagged_dictionary(pdf, span.get(b"A").map_err(|_| ())?)?;
    if attributes
        .get(b"O")
        .map_err(|_| ())?
        .as_name()
        .map_err(|_| ())?
        != b"Layout"
    {
        return Err(());
    }
    let box_values = tagged_array(pdf, attributes.get(b"BBox").map_err(|_| ())?)?;
    if box_values.len() != 4 {
        return Err(());
    }
    let values = [
        tagged_number(&box_values[0])?,
        tagged_number(&box_values[1])?,
        tagged_number(&box_values[2])?,
        tagged_number(&box_values[3])?,
    ];
    let tolerance = 0.5;
    if !values.iter().all(|value| value.is_finite())
        || values[2] < values[0]
        || values[3] < values[1]
        || values[0] < page_box[0] - tolerance
        || values[1] < page_box[1] - tolerance
        || values[2] > page_box[2] + tolerance
        || values[3] > page_box[3] + tolerance
    {
        return Err(());
    }
    Ok(())
}

fn decode_tagged_actual_text(actual: &PdfObject) -> TaggedResult<String> {
    let bytes = actual.as_str().map_err(|_| ())?;
    if bytes.is_ascii() {
        return String::from_utf8(bytes.to_vec()).map_err(|_| ());
    }
    decode_text_string(actual).map_err(|_| ())
}

fn tagged_parent_maps(
    pdf: &PdfDocument,
    root: &PdfDictionary,
    page_count: usize,
) -> TaggedResult<BTreeMap<i64, Vec<ObjectId>>> {
    let parent_tree = tagged_dictionary(pdf, root.get(b"ParentTree").map_err(|_| ())?)?;
    let numbers = tagged_array(pdf, parent_tree.get(b"Nums").map_err(|_| ())?)?;
    let (pairs, remainder) = numbers.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(());
    }
    let mut result = BTreeMap::new();
    for pair in pairs {
        let key = pair[0].as_i64().map_err(|_| ())?;
        let values = tagged_array(pdf, &pair[1])?
            .iter()
            .map(tagged_reference)
            .collect::<TaggedResult<Vec<_>>>()?;
        if result.insert(key, values).is_some() {
            return Err(());
        }
    }
    if result.keys().copied().collect::<Vec<_>>()
        != (0..i64::try_from(page_count).map_err(|_| ())?).collect::<Vec<_>>()
    {
        return Err(());
    }
    Ok(result)
}

fn tagged_stream_content(
    stream: &lopdf::Stream,
    decoded_bytes: &mut usize,
) -> TaggedResult<Vec<u8>> {
    let remaining = MAX_TAGGED_OCR_CONTENT_BYTES
        .checked_sub(*decoded_bytes)
        .ok_or(())?;
    let bytes = if stream.dict.get(b"Filter").is_err() {
        if stream.content.len() > remaining {
            return Err(());
        }
        stream.content.clone()
    } else {
        let filters = stream.filters().map_err(|_| ())?;
        if filters.as_slice() != [b"FlateDecode"] || stream.dict.get(b"DecodeParms").is_ok() {
            return Err(());
        }
        let read_limit = u64::try_from(remaining).map_err(|_| ())? + 1;
        let mut decoder =
            flate2::read::ZlibDecoder::new(stream.content.as_slice()).take(read_limit);
        let mut output = Vec::new();
        decoder.read_to_end(&mut output).map_err(|_| ())?;
        if output.len() > remaining {
            return Err(());
        }
        output
    };
    *decoded_bytes = decoded_bytes.checked_add(bytes.len()).ok_or(())?;
    Ok(bytes)
}

fn tagged_content_mcids(
    pdf: &PdfDocument,
    page_id: ObjectId,
    decoded_bytes: &mut usize,
) -> TaggedResult<HashSet<usize>> {
    let contents = pdf.get_page_contents(page_id);
    if contents.len() < 3 {
        return Err(());
    }
    let mut decode = |id: ObjectId| -> TaggedResult<Content<Vec<lopdf::content::Operation>>> {
        let stream = pdf
            .get_object(id)
            .map_err(|_| ())?
            .as_stream()
            .map_err(|_| ())?;
        let bytes = tagged_stream_content(stream, decoded_bytes)?;
        Content::decode_strict(&bytes).map_err(|_| ())
    };
    let opening = decode(contents[0])?;
    let closing = decode(contents[contents.len() - 2])?;
    let artifact = PdfObject::Name(b"Artifact".to_vec());
    let legacy_wrapper = opening.operations.len() == 1
        && opening.operations[0].operator == "BMC"
        && opening.operations[0].operands.as_slice() == [artifact.clone()]
        && closing.operations.len() == 1
        && closing.operations[0].operator == "EMC"
        && closing.operations[0].operands.is_empty();
    let isolated_wrapper = opening.operations.len() == 2
        && opening.operations[0].operator == "q"
        && opening.operations[0].operands.is_empty()
        && opening.operations[1].operator == "BMC"
        && opening.operations[1].operands.as_slice() == [artifact]
        && closing.operations.len() == 2
        && closing.operations[0].operator == "EMC"
        && closing.operations[0].operands.is_empty()
        && closing.operations[1].operator == "Q"
        && closing.operations[1].operands.is_empty();
    if !legacy_wrapper && !isolated_wrapper {
        return Err(());
    }
    let mut found = HashSet::new();
    let mut span_open = false;
    for operation in decode(*contents.last().ok_or(())?)?.operations {
        match operation.operator.as_str() {
            "EMC" => {
                if !operation.operands.is_empty() || !span_open {
                    return Err(());
                }
                span_open = false;
                continue;
            }
            "BMC" => return Err(()),
            "BDC" => {}
            _ => continue,
        }
        if span_open
            || operation.operands.len() != 2
            || operation.operands[0].as_name().map_err(|_| ())? != b"Span"
        {
            return Err(());
        }
        let properties = operation.operands[1].as_dict().map_err(|_| ())?;
        let mcid = usize::try_from(
            properties
                .get(b"MCID")
                .map_err(|_| ())?
                .as_i64()
                .map_err(|_| ())?,
        )
        .map_err(|_| ())?;
        if !found.insert(mcid) {
            return Err(());
        }
        span_open = true;
    }
    if span_open {
        return Err(());
    }
    Ok(found)
}

fn tagged_span_text(
    pdf: &PdfDocument,
    span_id: ObjectId,
    parent_id: ObjectId,
    page_id: ObjectId,
    parent_map: &[ObjectId],
    seen: &mut HashSet<usize>,
    page_box: [f64; 4],
) -> TaggedResult<String> {
    let span = pdf.get_dictionary(span_id).map_err(|_| ())?;
    if tagged_role(span)? != b"Span" {
        return Err(());
    }
    tagged_binding(span, b"P", parent_id)?;
    tagged_binding(span, b"Pg", page_id)?;
    let actual = span.get(b"ActualText").map_err(|_| ())?;
    if !matches!(actual, PdfObject::String(_, _)) {
        return Err(());
    }
    let text = decode_tagged_actual_text(actual)?;
    if text.trim().is_empty() || text.contains('\u{000c}') {
        return Err(());
    }
    tagged_bbox(pdf, span, page_box)?;
    let marked = tagged_dictionary(pdf, span.get(b"K").map_err(|_| ())?)?;
    if marked
        .get(b"Type")
        .map_err(|_| ())?
        .as_name()
        .map_err(|_| ())?
        != b"MCR"
    {
        return Err(());
    }
    tagged_binding(marked, b"Pg", page_id)?;
    let mcid = usize::try_from(
        marked
            .get(b"MCID")
            .map_err(|_| ())?
            .as_i64()
            .map_err(|_| ())?,
    )
    .map_err(|_| ())?;
    if mcid >= parent_map.len() || parent_map[mcid] != span_id || !seen.insert(mcid) {
        return Err(());
    }
    Ok(text)
}

fn tagged_page_text(
    pdf: &PdfDocument,
    section_id: ObjectId,
    document_id: ObjectId,
    page_id: ObjectId,
    parent_map: &[ObjectId],
    decoded_bytes: &mut usize,
) -> TaggedResult<String> {
    let section = pdf.get_dictionary(section_id).map_err(|_| ())?;
    if tagged_role(section)? != b"Sect" {
        return Err(());
    }
    tagged_binding(section, b"P", document_id)?;
    tagged_binding(section, b"Pg", page_id)?;
    let page_box = tagged_page_box(pdf, page_id)?;
    let mut seen = HashSet::new();
    let mut text = String::new();
    for block_id in tagged_child_ids(pdf, section)? {
        let block = pdf.get_dictionary(block_id).map_err(|_| ())?;
        tagged_binding(block, b"P", section_id)?;
        tagged_binding(block, b"Pg", page_id)?;
        match tagged_role(block)? {
            b"P" => {
                let spans = tagged_child_ids(pdf, block)?;
                if spans.is_empty() {
                    return Err(());
                }
                for span_id in spans {
                    text.push_str(&tagged_span_text(
                        pdf, span_id, block_id, page_id, parent_map, &mut seen, page_box,
                    )?);
                }
            }
            b"Table" => {
                let rows = tagged_child_ids(pdf, block)?;
                if rows.is_empty() {
                    return Err(());
                }
                for row_id in rows {
                    let row = pdf.get_dictionary(row_id).map_err(|_| ())?;
                    if tagged_role(row)? != b"TR" {
                        return Err(());
                    }
                    tagged_binding(row, b"P", block_id)?;
                    tagged_binding(row, b"Pg", page_id)?;
                    let cells = tagged_child_ids(pdf, row)?;
                    if cells.is_empty() {
                        return Err(());
                    }
                    for cell_id in cells {
                        let cell = pdf.get_dictionary(cell_id).map_err(|_| ())?;
                        if !matches!(tagged_role(cell)?, b"TH" | b"TD") {
                            return Err(());
                        }
                        tagged_binding(cell, b"P", row_id)?;
                        tagged_binding(cell, b"Pg", page_id)?;
                        let spans = tagged_child_ids(pdf, cell)?;
                        if spans.len() != 1 {
                            return Err(());
                        }
                        text.push_str(&tagged_span_text(
                            pdf, spans[0], cell_id, page_id, parent_map, &mut seen, page_box,
                        )?);
                    }
                }
            }
            _ => return Err(()),
        }
    }
    let expected = (0..parent_map.len()).collect::<HashSet<_>>();
    if text.trim().is_empty()
        || seen != expected
        || tagged_content_mcids(pdf, page_id, decoded_bytes)? != expected
    {
        return Err(());
    }
    Ok(text)
}

fn bounded_tagged_ocr_pages<I>(page_ids: I) -> TaggedResult<Vec<ObjectId>>
where
    I: Iterator<Item = ObjectId>,
{
    let mut pages = Vec::with_capacity(100);
    for page_id in page_ids {
        if pages.len() == 100 {
            return Err(());
        }
        pages.push(page_id);
    }
    if pages.is_empty() {
        return Err(());
    }
    Ok(pages)
}

fn parse_tagged_ocr_pages(pdf: &PdfDocument) -> TaggedResult<Vec<ParsedPage>> {
    let pages = bounded_tagged_ocr_pages(pdf.page_iter())?;
    let catalog = tagged_dictionary(pdf, pdf.trailer.get(b"Root").map_err(|_| ())?)?;
    let mark_info = tagged_dictionary(pdf, catalog.get(b"MarkInfo").map_err(|_| ())?)?;
    if !mark_info
        .get(b"Marked")
        .map_err(|_| ())?
        .as_bool()
        .map_err(|_| ())?
    {
        return Err(());
    }
    let root_id = tagged_reference(catalog.get(b"StructTreeRoot").map_err(|_| ())?)?;
    let root = pdf.get_dictionary(root_id).map_err(|_| ())?;
    let top = tagged_child_ids(pdf, root)?;
    if top.len() != 1 {
        return Err(());
    }
    let document_id = top[0];
    let document = pdf.get_dictionary(document_id).map_err(|_| ())?;
    if tagged_role(document)? != b"Document" {
        return Err(());
    }
    tagged_binding(document, b"P", root_id)?;
    let sections = tagged_child_ids(pdf, document)?;
    if sections.len() != pages.len() {
        return Err(());
    }
    let parent_maps = tagged_parent_maps(pdf, root, pages.len())?;
    let mut decoded_bytes = 0usize;
    pages
        .into_iter()
        .enumerate()
        .map(|(index, page_id)| {
            let page = pdf.get_dictionary(page_id).map_err(|_| ())?;
            let struct_parent = page
                .get(b"StructParents")
                .map_err(|_| ())?
                .as_i64()
                .map_err(|_| ())?;
            if struct_parent != i64::try_from(index).map_err(|_| ())? {
                return Err(());
            }
            let parent_map = parent_maps.get(&struct_parent).ok_or(())?;
            let text = tagged_page_text(
                pdf,
                sections[index],
                document_id,
                page_id,
                parent_map,
                &mut decoded_bytes,
            )?;
            Ok(ParsedPage {
                page_number: u32::try_from(index + 1).map_err(|_| ())?,
                text,
                warnings: Vec::new(),
                requires_visual_processing: false,
            })
        })
        .collect()
}

fn map_load_error(error: LopdfError) -> PipelineFailure {
    match error {
        LopdfError::IO(io_error) => parse_failure(
            "SOURCE_IO_ERROR",
            format!("The ingested source could not be reopened: {io_error}"),
            true,
        ),
        LopdfError::InvalidPassword => parse_failure(
            "ENCRYPTED_PDF_UNSUPPORTED",
            "Encrypted PDFs are not supported in the native-text parser",
            false,
        ),
        other => parse_failure(
            "MALFORMED_PDF",
            format!("The PDF structure could not be parsed: {other}"),
            false,
        ),
    }
}

fn parse_failure(
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> PipelineFailure {
    PipelineFailure {
        code: code.into(),
        message: message.into(),
        stage: Some(PipelineStage::Parse),
        recoverable,
    }
}

#[derive(Debug, Error)]
pub enum ParsePipelineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Parsing failed: {0}")]
    ParserFailed(PipelineFailure),
    #[error("Parsed artifact persistence failed: {0}")]
    ArtifactPersistence(StoreError),
    #[error("{primary}; the failure state could not be persisted: {persistence}")]
    FailurePersistence {
        primary: String,
        #[source]
        persistence: StoreError,
    },
}

impl ParsePipelineError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::ParserFailed(failure) => &failure.code,
            Self::ArtifactPersistence(_) => "PARSED_ARTIFACT_PERSISTENCE_FAILED",
            Self::FailurePersistence { .. } => "PARSE_FAILURE_PERSISTENCE_FAILED",
        }
    }
}

pub fn parse_document(
    conn: &mut Connection,
    parser: &dyn DocumentParser,
    run_id: &str,
) -> Result<ParsedDocument, ParsePipelineError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let (parsing_run, document) = db::start_parsing(conn, run_id, run.state_version)?;

    parse_started_document(conn, parser, run_id, parsing_run.state_version, &document)
}

pub(crate) fn parse_started_document(
    conn: &mut Connection,
    parser: &dyn DocumentParser,
    run_id: &str,
    parsing_state_version: u32,
    document: &IngestedDocument,
) -> Result<ParsedDocument, ParsePipelineError> {
    let parsed = match parser.parse(document) {
        Ok(parsed) => parsed,
        Err(failure) => {
            return Err(persist_parser_failure(
                conn,
                run_id,
                parsing_state_version,
                failure,
            ));
        }
    };
    if let Err(failure) = validate_parsed_document(&parsed, document, parser) {
        return Err(persist_parser_failure(
            conn,
            run_id,
            parsing_state_version,
            failure,
        ));
    }

    let warnings = parsed
        .warnings
        .iter()
        .chain(parsed.pages.iter().flat_map(|page| page.warnings.iter()))
        .cloned()
        .collect::<Vec<_>>();
    if let Err(persistence) =
        db::complete_parsing(conn, run_id, parsing_state_version, &parsed, warnings)
    {
        let failure = parse_failure(
            "PARSED_ARTIFACT_PERSISTENCE_FAILED",
            "Parsed output could not be committed atomically",
            true,
        );
        return match db::fail_parsing(conn, run_id, parsing_state_version, failure) {
            Ok(_) => Err(ParsePipelineError::ArtifactPersistence(persistence)),
            Err(failure_persistence) => Err(ParsePipelineError::FailurePersistence {
                primary: persistence.to_string(),
                persistence: failure_persistence,
            }),
        };
    }

    Ok(parsed)
}

fn persist_parser_failure(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> ParsePipelineError {
    match db::fail_parsing(conn, run_id, expected_version, failure.clone()) {
        Ok(_) => ParsePipelineError::ParserFailed(failure),
        Err(persistence) => ParsePipelineError::FailurePersistence {
            primary: failure.to_string(),
            persistence,
        },
    }
}

fn validate_parsed_document(
    parsed: &ParsedDocument,
    document: &IngestedDocument,
    parser: &dyn DocumentParser,
) -> Result<(), PipelineFailure> {
    if parsed.document_id != document.document_id {
        return Err(parse_failure(
            "INVALID_PARSED_ARTIFACT",
            "The parser returned an artifact for a different document",
            false,
        ));
    }
    if parsed.parser_id != parser.id() || parsed.parser_version != parser.version() {
        return Err(parse_failure(
            "INVALID_PARSED_ARTIFACT",
            "The parser artifact identity does not match the selected parser",
            false,
        ));
    }
    if parsed.source_type != document.source_type {
        return Err(parse_failure(
            "INVALID_PARSED_ARTIFACT",
            "The parser changed the document source provenance",
            false,
        ));
    }

    for (index, page) in parsed.pages.iter().enumerate() {
        let expected_page_number = u32::try_from(index + 1).map_err(|_| {
            parse_failure(
                "INVALID_PARSED_ARTIFACT",
                "The parsed page count exceeds the contract's page-number range",
                false,
            )
        })?;
        if page.page_number != expected_page_number {
            return Err(parse_failure(
                "INVALID_PARSED_ARTIFACT",
                "Parsed pages are not in canonical one-based order",
                false,
            ));
        }
        if page.text.trim().is_empty()
            && (!page.requires_visual_processing
                || !page
                    .warnings
                    .iter()
                    .any(|warning| warning.code == "NO_NATIVE_TEXT"))
        {
            return Err(parse_failure(
                "INVALID_PARSED_ARTIFACT",
                "A page without native text must preserve its visual-routing marker and warning",
                false,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{PipelineState, SourceType};
    use crate::pipeline::ingest::{ingest_pdf, prepare_pdf_ingestion_with_source_type};
    use pdf_extract::content::{Content, Operation};
    use pdf_extract::{
        dictionary, EncryptionState, EncryptionVersion, Object, Permissions, Stream, StringFormat,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    enum FixturePage {
        Text(Vec<u8>),
        Empty,
        ImageOnly,
    }

    #[test]
    fn tagged_ocr_parser_bounds_structural_streams_before_lopdf_load() {
        let source = include_str!("parser.rs");
        let tagged = &source[source.find("impl TaggedOcrParser").unwrap()
            ..source
                .find("impl DocumentParser for TaggedOcrParser")
                .unwrap()];

        assert!(tagged.contains("load_bounded_tagged_ocr_pdf(source_bytes)"));
        assert!(!tagged.contains("PdfDocument::load_mem(&source_bytes)"));
    }

    fn classic_xref_source(trailer_entries: &[u8]) -> Vec<u8> {
        let mut source = b"%PDF-1.4\n".to_vec();
        let xref_offset = source.len();
        source.extend_from_slice(b"xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 1 ");
        source.extend_from_slice(trailer_entries);
        source.extend_from_slice(format!(">>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
        source
    }

    #[test]
    fn tagged_ocr_xref_profile_accepts_only_one_classic_revision() {
        assert!(require_classic_tagged_ocr_xref(&classic_xref_source(b"")).is_ok());
        assert!(require_classic_tagged_ocr_xref(&classic_xref_source(b"/Prev 9")).is_err());
        assert!(require_classic_tagged_ocr_xref(&classic_xref_source(b"/XRefStm 9")).is_err());
        assert!(require_classic_tagged_ocr_xref(&classic_xref_source(b"/Pr#65v 9")).is_err());

        let xref_stream = b"%PDF-1.5\n1 0 obj\n<< /Type /XRef >>\nstream\n\nendstream\nendobj\nstartxref\n9\n%%EOF\n";
        assert!(require_classic_tagged_ocr_xref(xref_stream).is_err());
    }

    #[test]
    fn source_parser_set_dispatches_both_durable_source_types() {
        let parsers = SourceParserSet::new();

        assert_eq!(parsers.select(SourceType::NativeText).id(), "pdf-extract");
        assert_eq!(
            parsers.select(SourceType::OcrText).id(),
            "local-connect-tagged-ocr"
        );
    }

    struct TestPath(PathBuf);

    impl TestPath {
        fn new(extension: &str) -> Self {
            Self(
                std::env::temp_dir().join(format!("doc-sum-parser-{}.{extension}", Uuid::new_v4())),
            )
        }

        fn write(extension: &str, bytes: &[u8]) -> Self {
            let path = Self::new(extension);
            fs::write(&path.0, bytes).expect("fixture should be writable");
            path
        }
    }

    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn build_pdf(pages: Vec<FixturePage>) -> PdfDocument {
        let mut document = PdfDocument::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let mut page_ids = Vec::new();

        for page in pages {
            let mut resources = dictionary! {
                "Font" => dictionary! { "F1" => font_id },
            };
            let operations = match page {
                FixturePage::Text(bytes) => vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new("Tj", vec![Object::String(bytes, StringFormat::Literal)]),
                    Operation::new("ET", vec![]),
                ],
                FixturePage::Empty => Vec::new(),
                FixturePage::ImageOnly => {
                    let image_id = document.add_object(Stream::new(
                        dictionary! {
                            "Type" => "XObject",
                            "Subtype" => "Image",
                            "Width" => 1,
                            "Height" => 1,
                            "ColorSpace" => "DeviceRGB",
                            "BitsPerComponent" => 8,
                        },
                        vec![0, 0, 0],
                    ));
                    resources.set("XObject", dictionary! { "Im1" => image_id });
                    vec![
                        Operation::new("q", vec![]),
                        Operation::new(
                            "cm",
                            vec![
                                72.into(),
                                0.into(),
                                0.into(),
                                72.into(),
                                72.into(),
                                72.into(),
                            ],
                        ),
                        Operation::new("Do", vec!["Im1".into()]),
                        Operation::new("Q", vec![]),
                    ]
                }
            };
            let content = Content { operations }
                .encode()
                .expect("content should encode");
            let content_id = document.add_object(Stream::new(dictionary! {}, content));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            page_ids.push(Object::Reference(page_id));
        }

        let page_count = page_ids.len() as i64;
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids,
                "Count" => page_count,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document
    }

    fn save_pdf(mut document: PdfDocument) -> TestPath {
        let path = TestPath::new("pdf");
        document.save(&path.0).expect("PDF fixture should save");
        path
    }

    fn ingest_fixture(
        conn: &mut Connection,
        path: &Path,
    ) -> crate::pipeline::contracts::PipelineRun {
        let (_, run) = ingest_pdf(conn, path.to_str().expect("UTF-8 path"))
            .expect("PDF candidate should ingest");
        run
    }

    #[test]
    fn tagged_ocr_parser_preserves_logical_page_text_and_source_type() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let fixture_bytes = fs::read(&path).expect("OCR fixture should be readable");
        assert!(
            require_classic_tagged_ocr_xref(&fixture_bytes).is_ok(),
            "producer fixture must satisfy the bounded structural profile"
        );
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (document, run) = prepare_pdf_ingestion_with_source_type(
            path.to_str().expect("UTF-8 path"),
            Some("recognized.pdf"),
            SourceType::OcrText,
        )
        .expect("OCR fixture should prepare");
        db::persist_ingestion(&mut conn, &document, &run).expect("OCR fixture should persist");

        let parsed = parse_document(&mut conn, &TaggedOcrParser::new(), &run.run_id)
            .expect("tagged OCR fixture should parse");

        assert_eq!(parsed.source_type, SourceType::OcrText);
        assert_eq!(parsed.parser_id, "local-connect-tagged-ocr");
        assert_eq!(parsed.pages.len(), 2);
        assert_eq!(parsed.pages[0].text, "OCR page one evidence.");
        assert_eq!(parsed.pages[1].text, "Amount$125.00");
        assert!(parsed
            .pages
            .iter()
            .all(|page| !page.requires_visual_processing && page.warnings.is_empty()));
    }

    fn tagged_fixture_with_wrapper(opening: &[u8], closing: &[u8]) -> TestPath {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
        let first_page = *pdf
            .get_pages()
            .values()
            .next()
            .expect("OCR fixture should have a page");
        let content_ids = pdf.get_page_contents(first_page);
        pdf.get_object_mut(content_ids[0])
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(opening.to_vec());
        pdf.get_object_mut(content_ids[content_ids.len() - 2])
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(closing.to_vec());
        save_pdf(pdf)
    }

    #[test]
    fn tagged_ocr_parser_accepts_only_paired_source_graphics_wrappers() {
        let isolated = tagged_fixture_with_wrapper(b"q\n/Artifact BMC\n", b"EMC\nQ\n");
        let (isolated_document, _) = prepare_pdf_ingestion_with_source_type(
            isolated.0.to_str().unwrap(),
            Some("recognized.pdf"),
            SourceType::OcrText,
        )
        .unwrap();
        TaggedOcrParser::new()
            .parse(&isolated_document)
            .expect("the current provider wrapper should parse");

        let mixed = tagged_fixture_with_wrapper(b"q\n/Artifact BMC\n", b"EMC\n");
        let (mixed_document, _) = prepare_pdf_ingestion_with_source_type(
            mixed.0.to_str().unwrap(),
            Some("recognized.pdf"),
            SourceType::OcrText,
        )
        .unwrap();
        let error = TaggedOcrParser::new()
            .parse(&mixed_document)
            .expect_err("an unmatched graphics-state wrapper must fail");
        assert_eq!(error.code, "OCR_STRUCTURE_INVALID");
    }

    #[test]
    fn tagged_ocr_parser_preserves_ascii_controls_in_actual_text() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
        let catalog = tagged_dictionary(&pdf, pdf.trailer.get(b"Root").unwrap()).unwrap();
        let root_id = tagged_reference(catalog.get(b"StructTreeRoot").unwrap()).unwrap();
        let document_id = tagged_child_ids(&pdf, pdf.get_dictionary(root_id).unwrap()).unwrap()[0];
        let section_id =
            tagged_child_ids(&pdf, pdf.get_dictionary(document_id).unwrap()).unwrap()[0];
        let block_id = tagged_child_ids(&pdf, pdf.get_dictionary(section_id).unwrap()).unwrap()[0];
        let span_id = tagged_child_ids(&pdf, pdf.get_dictionary(block_id).unwrap()).unwrap()[0];
        pdf.get_dictionary_mut(span_id).unwrap().set(
            "ActualText",
            PdfObject::String(b"line one\nline two\t".to_vec(), StringFormat::Literal),
        );
        let changed = save_pdf(pdf);
        let (document, _) = prepare_pdf_ingestion_with_source_type(
            changed.0.to_str().unwrap(),
            Some("recognized.pdf"),
            SourceType::OcrText,
        )
        .unwrap();

        let parsed = TaggedOcrParser::new()
            .parse(&document)
            .expect("profile ASCII controls should parse");

        assert!(parsed.pages[0].text.starts_with("line one\nline two\t"));
    }

    fn persist_tagged_fixture(
        conn: &mut Connection,
        path: &Path,
    ) -> crate::pipeline::contracts::PipelineRun {
        let (document, run) = prepare_pdf_ingestion_with_source_type(
            path.to_str().expect("UTF-8 path"),
            Some("recognized.pdf"),
            SourceType::OcrText,
        )
        .expect("OCR fixture should prepare");
        db::persist_ingestion(conn, &document, &run).expect("OCR fixture should persist");
        run
    }

    #[test]
    fn tagged_ocr_parser_rejects_comment_spoofed_mcid_operator() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
        let first_page = *pdf
            .get_pages()
            .values()
            .next()
            .expect("OCR fixture should have a page");
        let content_ids = pdf.get_page_contents(first_page);
        pdf.get_object_mut(*content_ids.last().expect("OCR stream should exist"))
            .expect("OCR stream should load")
            .as_stream_mut()
            .expect("OCR content should be a stream")
            .set_plain_content(b"% /Span <</MCID 0>> BDC\n".to_vec());
        let changed = TestPath::new("pdf");
        pdf.save(&changed.0)
            .expect("changed OCR fixture should save");
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = persist_tagged_fixture(&mut conn, &changed.0);

        let error = parse_document(&mut conn, &TaggedOcrParser::new(), &run.run_id)
            .expect_err("comment-spoofed MCID must fail");

        assert_eq!(error.code(), "OCR_STRUCTURE_INVALID");
    }

    #[test]
    fn tagged_ocr_parser_rejects_unbalanced_marked_content() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        for mutation in ["missing-emc", "extra-emc"] {
            let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
            let first_page = *pdf
                .get_pages()
                .values()
                .next()
                .expect("OCR fixture should have a page");
            let content_ids = pdf.get_page_contents(first_page);
            let stream = pdf
                .get_object_mut(*content_ids.last().expect("OCR stream should exist"))
                .expect("OCR stream should load")
                .as_stream_mut()
                .expect("OCR content should be a stream");
            let mut content = stream
                .get_plain_content()
                .expect("OCR stream should decode");
            if mutation == "missing-emc" {
                let position = content
                    .windows(3)
                    .rposition(|window| window == b"EMC")
                    .expect("OCR stream should contain EMC");
                content.drain(position..position + 3);
            } else {
                content.extend_from_slice(b"\nEMC");
            }
            stream.set_plain_content(content);
            let changed = TestPath::new("pdf");
            pdf.save(&changed.0)
                .expect("changed OCR fixture should save");
            let mut conn = db::init_db(":memory:").expect("schema should initialize");
            let run = persist_tagged_fixture(&mut conn, &changed.0);

            let error = parse_document(&mut conn, &TaggedOcrParser::new(), &run.run_id)
                .expect_err("unbalanced marked content must fail");

            assert_eq!(error.code(), "OCR_STRUCTURE_INVALID", "{mutation}");
        }
    }

    #[test]
    fn tagged_ocr_parser_rejects_non_string_actual_text() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
        let catalog = tagged_dictionary(&pdf, pdf.trailer.get(b"Root").unwrap()).unwrap();
        let root_id = tagged_reference(catalog.get(b"StructTreeRoot").unwrap()).unwrap();
        let document_id = tagged_child_ids(&pdf, pdf.get_dictionary(root_id).unwrap()).unwrap()[0];
        let section_id =
            tagged_child_ids(&pdf, pdf.get_dictionary(document_id).unwrap()).unwrap()[0];
        let block_id = tagged_child_ids(&pdf, pdf.get_dictionary(section_id).unwrap()).unwrap()[0];
        let span_id = tagged_child_ids(&pdf, pdf.get_dictionary(block_id).unwrap()).unwrap()[0];
        pdf.get_dictionary_mut(span_id)
            .expect("span should load")
            .set("ActualText", PdfObject::Name(b"invalid".to_vec()));
        let changed = TestPath::new("pdf");
        pdf.save(&changed.0)
            .expect("changed OCR fixture should save");
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = persist_tagged_fixture(&mut conn, &changed.0);

        let error = parse_document(&mut conn, &TaggedOcrParser::new(), &run.run_id)
            .expect_err("non-string ActualText must fail");

        assert_eq!(error.code(), "OCR_STRUCTURE_INVALID");
    }

    #[test]
    fn tagged_ocr_parser_rejects_a_blank_page_at_admission() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
        let catalog = tagged_dictionary(&pdf, pdf.trailer.get(b"Root").unwrap()).unwrap();
        let root_id = tagged_reference(catalog.get(b"StructTreeRoot").unwrap()).unwrap();
        let root = pdf.get_dictionary(root_id).unwrap();
        let document_id = tagged_child_ids(&pdf, root).unwrap()[0];
        let sections = tagged_child_ids(&pdf, pdf.get_dictionary(document_id).unwrap()).unwrap();
        let parent_tree_id = tagged_reference(root.get(b"ParentTree").unwrap()).unwrap();
        let parent_tree = pdf.get_dictionary(parent_tree_id).unwrap();
        let numbers = tagged_array(&pdf, parent_tree.get(b"Nums").unwrap()).unwrap();
        let first_parent_map = numbers[1].clone();
        let second_page = pdf.get_pages().into_values().nth(1).unwrap();
        let content_ids = pdf.get_page_contents(second_page);

        pdf.get_dictionary_mut(sections[1])
            .unwrap()
            .set("K", PdfObject::Array(Vec::new()));
        pdf.get_dictionary_mut(parent_tree_id).unwrap().set(
            "Nums",
            PdfObject::Array(vec![
                0.into(),
                first_parent_map,
                1.into(),
                PdfObject::Array(Vec::new()),
            ]),
        );
        pdf.get_object_mut(*content_ids.last().unwrap())
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(Vec::new());
        let changed = TestPath::new("pdf");
        pdf.save(&changed.0)
            .expect("blank-page fixture should save");
        let (document, _) = prepare_pdf_ingestion_with_source_type(
            changed.0.to_str().unwrap(),
            Some("recognized.pdf"),
            SourceType::OcrText,
        )
        .expect("blank-page fixture should prepare");

        let error = TaggedOcrParser::new()
            .parse(&document)
            .expect_err("blank OCR page must fail during admission");

        assert_eq!(error.code, "OCR_STRUCTURE_INVALID");
    }

    #[test]
    fn tagged_ocr_parser_rejects_content_decompression_over_budget() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
        let first_page = *pdf.get_pages().values().next().unwrap();
        let content_ids = pdf.get_page_contents(first_page);
        let stream = pdf
            .get_object_mut(*content_ids.last().unwrap())
            .unwrap()
            .as_stream_mut()
            .unwrap();
        let mut content = stream.get_plain_content().unwrap();
        content.extend(std::iter::repeat_n(b' ', 2 * 1024 * 1024 + 1));
        stream.set_plain_content(content);
        stream
            .compress()
            .expect("oversized content should compress");
        let changed = TestPath::new("pdf");
        pdf.save(&changed.0)
            .expect("compressed-content fixture should save");
        let (document, _) = prepare_pdf_ingestion_with_source_type(
            changed.0.to_str().unwrap(),
            Some("recognized.pdf"),
            SourceType::OcrText,
        )
        .expect("compressed-content fixture should prepare");

        let error = TaggedOcrParser::new()
            .parse(&document)
            .expect_err("decoded OCR content over budget must fail during admission");

        assert_eq!(error.code, "OCR_STRUCTURE_INVALID");
    }

    #[test]
    fn tagged_ocr_parser_rejects_cyclic_inherited_page_tree() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
        let first_page = *pdf
            .get_pages()
            .values()
            .next()
            .expect("OCR fixture should have a page");
        let page = pdf
            .get_dictionary_mut(first_page)
            .expect("OCR page should load");
        page.remove(b"MediaBox");
        page.set("Parent", PdfObject::Reference(first_page));

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sender
                .send(tagged_page_box(&pdf, first_page).is_err())
                .expect("cycle result should be received");
        });

        assert_eq!(
            receiver.recv_timeout(std::time::Duration::from_secs(1)),
            Ok(true),
            "cyclic inherited page lookup must terminate and fail closed"
        );
    }

    #[test]
    fn tagged_ocr_parser_rejects_cyclic_generic_reference() {
        let mut pdf = PdfDocument::new();
        let object_id = pdf.new_object_id();
        pdf.objects
            .insert(object_id, PdfObject::Reference(object_id));
        let reference = PdfObject::Reference(object_id);

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sender
                .send(tagged_object(&pdf, &reference).is_err())
                .expect("cycle result should be received");
        });

        assert_eq!(
            receiver.recv_timeout(std::time::Duration::from_secs(1)),
            Ok(true),
            "cyclic generic dereferencing must terminate and fail closed"
        );
    }

    #[test]
    fn tagged_ocr_page_bound_does_not_consult_or_exhaust_the_tail() {
        struct Pages {
            yielded: usize,
        }

        impl Iterator for Pages {
            type Item = ObjectId;

            fn next(&mut self) -> Option<Self::Item> {
                self.yielded += 1;
                if self.yielded > 101 {
                    panic!("page enumeration crossed the rejecting boundary");
                }
                Some((self.yielded as u32, 0))
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                panic!("bounded page admission must not enumerate the page tree for a hint");
            }
        }

        assert!(bounded_tagged_ocr_pages(Pages { yielded: 0 }).is_err());
    }

    #[test]
    fn tagged_ocr_parser_rejects_a_cyclic_pages_hierarchy() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut pdf = PdfDocument::load(&fixture).expect("OCR fixture should load");
        let catalog = tagged_dictionary(&pdf, pdf.trailer.get(b"Root").unwrap()).unwrap();
        let pages_id = tagged_reference(catalog.get(b"Pages").unwrap()).unwrap();
        pdf.get_dictionary_mut(pages_id)
            .unwrap()
            .get_mut(b"Kids")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(PdfObject::Reference(pages_id));
        let changed = TestPath::new("pdf");
        pdf.save(&changed.0)
            .expect("cyclic OCR fixture should save");
        let (document, _) = prepare_pdf_ingestion_with_source_type(
            changed.0.to_str().unwrap(),
            Some("recognized.pdf"),
            SourceType::OcrText,
        )
        .expect("cyclic OCR fixture should prepare");

        let error = TaggedOcrParser::new()
            .parse(&document)
            .expect_err("a cyclic page hierarchy must fail during admission");

        assert_eq!(error.code, "OCR_STRUCTURE_INVALID");
    }

    #[test]
    fn native_multi_page_pdf_preserves_page_order_text_and_unicode() {
        let pdf = save_pdf(build_pdf(vec![
            FixturePage::Text(b"PAGE_ONE".to_vec()),
            FixturePage::Text(b"caf\xe9 r\xe9sum\xe9".to_vec()),
            FixturePage::Text(b"PAGE_THREE".to_vec()),
        ]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("PDF should parse");

        assert_eq!(parsed.pages.len(), 3);
        assert_eq!(
            parsed
                .pages
                .iter()
                .map(|page| page.page_number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(parsed.pages[0].text.contains("PAGE_ONE"));
        assert!(parsed.pages[1].text.contains("café résumé"));
        assert!(parsed.pages[2].text.contains("PAGE_THREE"));
        assert_eq!(parsed.parser_id, "pdf-extract");
        assert_eq!(parsed.parser_version, "0.12.0");

        let persisted_run = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(persisted_run.state, PipelineState::Parsed);
        assert_eq!(persisted_run.state_version, 5);
        let events = db::list_pipeline_events(&conn, &run.run_id).expect("events should load");
        assert_eq!(
            events
                .iter()
                .map(|event| event.next_state.clone())
                .collect::<Vec<_>>(),
            vec![
                PipelineState::Received,
                PipelineState::Ingesting,
                PipelineState::Ingested,
                PipelineState::Parsing,
                PipelineState::Parsed,
            ]
        );
    }

    #[test]
    fn empty_page_is_preserved_with_visual_routing_marker() {
        let pdf = save_pdf(build_pdf(vec![
            FixturePage::Text(b"BEFORE_EMPTY".to_vec()),
            FixturePage::Empty,
            FixturePage::Text(b"AFTER_EMPTY".to_vec()),
        ]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("PDF should parse");

        assert_eq!(parsed.pages.len(), 3);
        assert_eq!(parsed.pages[1].page_number, 2);
        assert!(parsed.pages[1].text.trim().is_empty());
        assert!(parsed.pages[1].requires_visual_processing);
        assert!(parsed.pages[1]
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_NATIVE_TEXT"));
        assert!(parsed.pages[2].text.contains("AFTER_EMPTY"));
    }

    #[test]
    fn image_only_pdf_reaches_parsed_with_no_native_text_warnings() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::ImageOnly]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("image-only PDF should still parse");

        assert_eq!(parsed.pages.len(), 1);
        assert!(parsed.pages[0].text.trim().is_empty());
        assert!(parsed.pages[0].requires_visual_processing);
        assert!(parsed
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_NATIVE_TEXT_IN_DOCUMENT"));
        assert_eq!(
            db::get_pipeline_run(&conn, &run.run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Parsed
        );
    }

    #[test]
    fn malformed_pdf_fails_structurally_and_never_persists_an_artifact() {
        let pdf = TestPath::write("pdf", b"%PDF-1.7\nnot a structurally valid document");
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("malformed PDF should fail");

        assert_eq!(error.code(), "MALFORMED_PDF");
        let failed_run = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed_run.state, PipelineState::Failed);
        assert_eq!(
            failed_run.failure.expect("failure should persist").code,
            "MALFORMED_PDF"
        );
        assert!(db::get_parsed_document(&conn, &run.run_id)
            .expect("artifact query should work")
            .is_none());
        assert!(!db::list_pipeline_events(&conn, &run.run_id)
            .expect("events should load")
            .iter()
            .any(|event| event.next_state == PipelineState::Parsed));
    }

    #[test]
    fn encrypted_pdf_is_rejected_deterministically() {
        let mut document = build_pdf(vec![FixturePage::Text(b"ENCRYPTED".to_vec())]);
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal("fixture-id-one"),
                Object::string_literal("fixture-id-two"),
            ]),
        );
        let encryption = EncryptionState::try_from(EncryptionVersion::V1 {
            document: &document,
            owner_password: "owner-password",
            user_password: "user-password",
            permissions: Permissions::PRINTABLE,
        })
        .expect("encryption state should build");
        document
            .encrypt(&encryption)
            .expect("fixture should encrypt");
        let pdf = save_pdf(document);
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("encrypted PDF should be unsupported");

        assert_eq!(error.code(), "ENCRYPTED_PDF_UNSUPPORTED");
        assert_eq!(
            db::get_pipeline_run(&conn, &run.run_id)
                .expect("run should load")
                .expect("run should exist")
                .failure
                .expect("failure should persist")
                .code,
            "ENCRYPTED_PDF_UNSUPPORTED"
        );
    }

    #[test]
    fn missing_source_during_parse_becomes_a_structured_failed_run() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"DISAPPEARS".to_vec())]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);
        fs::remove_file(&pdf.0).expect("fixture should be removable");

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("missing source should fail");

        assert_eq!(error.code(), "SOURCE_IO_ERROR");
        let failed = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert!(failed.failure.expect("failure should persist").recoverable);
    }

    #[test]
    fn same_size_source_replacement_is_rejected_against_ingested_identity() {
        let original_pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"ORIGINAL_A".to_vec())]));
        let replacement_pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"MODIFIED_B".to_vec())]));
        let original_bytes = fs::read(&original_pdf.0).expect("original fixture should read");
        let replacement_bytes =
            fs::read(&replacement_pdf.0).expect("replacement fixture should read");
        assert_eq!(original_bytes.len(), replacement_bytes.len());
        assert_ne!(original_bytes, replacement_bytes);

        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (ingested, run) = ingest_pdf(
            &mut conn,
            original_pdf.0.to_str().expect("UTF-8 fixture path"),
        )
        .expect("original PDF should ingest");
        fs::write(&original_pdf.0, &replacement_bytes)
            .expect("fixture replacement should be writable");

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("changed source bytes must not parse under the original identity");

        assert_eq!(error.code(), "SOURCE_CONTENT_CHANGED");
        let failed = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert!(failed.failure.expect("failure should persist").recoverable);
        let persisted_document = db::get_document(&conn, &ingested.document_id)
            .expect("document should load")
            .expect("document should exist");
        assert_eq!(persisted_document.content_hash, ingested.content_hash);
        assert_eq!(persisted_document.byte_size, ingested.byte_size);
        assert!(db::get_parsed_document(&conn, &run.run_id)
            .expect("artifact query should work")
            .is_none());
    }

    #[test]
    fn parsed_artifact_and_history_survive_connection_reopen() {
        let pdf = save_pdf(build_pdf(vec![
            FixturePage::Text(b"PERSIST_ONE".to_vec()),
            FixturePage::Empty,
        ]));
        let database = TestPath::new("db");
        let run_id;
        let expected_artifact;
        let expected_events;
        {
            let mut conn = db::init_db(&database.0).expect("schema should initialize");
            let run = ingest_fixture(&mut conn, &pdf.0);
            expected_artifact = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
                .expect("PDF should parse");
            expected_events =
                db::list_pipeline_events(&conn, &run.run_id).expect("events should load");
            run_id = run.run_id;
        }

        let reopened = db::init_db(&database.0).expect("database should reopen");
        let artifact = db::get_parsed_document(&reopened, &run_id)
            .expect("artifact should load")
            .expect("artifact should exist");
        assert_eq!(artifact, expected_artifact);
        assert_eq!(
            db::list_pipeline_events(&reopened, &run_id).expect("events should load"),
            expected_events
        );
        assert_eq!(
            db::get_pipeline_run(&reopened, &run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Parsed
        );
    }

    #[test]
    fn parsed_event_failure_rolls_back_artifact_before_marking_run_failed() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::Text(
            b"ROLLBACK_PARSE".to_vec(),
        )]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);
        conn.execute_batch(
            "CREATE TRIGGER test_fail_parsed_event
             BEFORE INSERT ON pipeline_events
             WHEN NEW.next_state = '\"Parsed\"'
             BEGIN
                 SELECT RAISE(ABORT, 'injected parsed event failure');
             END;",
        )
        .expect("failure trigger should install");

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("parsed event failure should fail the run");

        assert_eq!(error.code(), "PARSED_ARTIFACT_PERSISTENCE_FAILED");
        assert!(db::get_parsed_document(&conn, &run.run_id)
            .expect("artifact query should work")
            .is_none());
        let failed = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(
            failed.failure.expect("failure should persist").code,
            "PARSED_ARTIFACT_PERSISTENCE_FAILED"
        );
    }

    #[test]
    fn parsing_cannot_bypass_state_machine_or_reparse_a_parsed_run() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"ONCE_ONLY".to_vec())]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);
        parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("first parse should succeed");
        let expected_events =
            db::list_pipeline_events(&conn, &run.run_id).expect("events should load");

        let second = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id);
        assert!(matches!(second, Err(ParsePipelineError::Store(_))));
        assert_eq!(
            db::get_pipeline_run(&conn, &run.run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Parsed
        );
        assert_eq!(
            db::list_pipeline_events(&conn, &run.run_id).expect("events should load"),
            expected_events
        );
    }

    struct InvalidOrderParser;

    impl DocumentParser for InvalidOrderParser {
        fn parse(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
            Ok(ParsedDocument {
                document_id: document.document_id.clone(),
                parser_id: self.id().to_string(),
                parser_version: self.version().to_string(),
                source_type: document.source_type,
                pages: vec![ParsedPage {
                    page_number: 2,
                    text: "wrong ordinal".to_string(),
                    warnings: Vec::new(),
                    requires_visual_processing: false,
                }],
                warnings: Vec::new(),
            })
        }

        fn id(&self) -> &'static str {
            "invalid-order-test-parser"
        }

        fn version(&self) -> &'static str {
            "test"
        }
    }

    #[test]
    fn invalid_parser_artifact_is_rejected_before_persistence() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"VALID_SOURCE".to_vec())]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let error = parse_document(&mut conn, &InvalidOrderParser, &run.run_id)
            .expect_err("invalid page order should fail validation");

        assert_eq!(error.code(), "INVALID_PARSED_ARTIFACT");
        assert!(db::get_parsed_document(&conn, &run.run_id)
            .expect("artifact query should work")
            .is_none());
        assert_eq!(
            db::get_pipeline_run(&conn, &run.run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Failed
        );
    }
}
