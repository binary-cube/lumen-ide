//! An export as an Excel workbook (`.xlsx`), written a block of rows at a time: a zip of
//! the few XML parts a spreadsheet needs (the types, the relations, the workbook, a style for
//! the header, one sheet), the sheet deflated as its rows come — never all of them in memory.
//! Values keep their types (numbers, booleans; the rest as text, cut at Excel's 32 767
//! characters); `NULL` is an empty cell; the header row is bold and stays in view. Our own
//! small writer (no zip64: at most 4 GiB, and Excel's 1 048 576 rows), on `flate2`, already
//! in the tree.

use std::io::Write as _;

use flate2::Compression;
use flate2::write::DeflateEncoder;

use super::copy::kind_of;
use super::source::{CellRef, GridColumn};
use extractors::Kind;

/// The most rows a sheet holds (the header's included).
pub const MAX_ROWS: u64 = 1_048_576;
/// The most characters a cell holds.
pub const MAX_CELL: usize = 32_767;

const SHEET: &str = "xl/worksheets/sheet1.xml";
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELATIONS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// A part written: what the zip's directory says of it.
#[derive(Debug, Clone)]
struct Entry {
    name: &'static str,
    crc: u32,
    compressed: u64,
    size: u64,
    offset: u64,
    /// Its sizes come after it (the sheet, written as it goes).
    described: bool,
}

/// The sheet being written.
struct Sheet {
    encoder: DeflateEncoder<Vec<u8>>,
    crc: flate2::Crc,
    size: u64,
    compressed: u64,
    offset: u64,
}

/// A workbook written a piece at a time (see the module's doc).
pub struct XlsxWriter {
    title: String,
    columns: Vec<(String, Kind)>,
    entries: Vec<Entry>,
    /// Bytes given out before the sheet's data (where the next part starts).
    offset: u64,
    sheet: Option<Sheet>,
    /// Rows written (the header's not counted).
    written: u64,
}

impl std::fmt::Debug for XlsxWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XlsxWriter").field("written", &self.written).finish_non_exhaustive()
    }
}

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn too_large() -> String {
    i18n::t("extract.xlsx_too_large")
}

/// 1980-01-01 00:00 (a fixed date: the same rows give the same file).
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x21;

fn local_header(out: &mut Vec<u8>, entry: &Entry) -> Result<(), String> {
    u32le(out, 0x0403_4b50);
    u16le(out, 20);
    u16le(out, if entry.described { 0x0008 } else { 0 });
    u16le(out, 8);
    u16le(out, DOS_TIME);
    u16le(out, DOS_DATE);
    let (crc, compressed, size) = if entry.described {
        (0, 0, 0)
    } else {
        (
            entry.crc,
            u32::try_from(entry.compressed).map_err(|_| too_large())?,
            u32::try_from(entry.size).map_err(|_| too_large())?,
        )
    };
    u32le(out, crc);
    u32le(out, compressed);
    u32le(out, size);
    u16le(out, entry.name.len() as u16);
    u16le(out, 0);
    out.extend_from_slice(entry.name.as_bytes());
    Ok(())
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

/// `text` for XML: escaped, without the characters XML can't hold.
fn xml_text(text: &str, out: &mut String) {
    out.push_str(&extractors::formats::xml_escape(text));
}

/// A column's letters (`A`, `Z`, `AA`…).
fn column_letters(mut at: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(b'A' + (at % 26) as u8);
        if at < 26 {
            break;
        }
        at = at / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// A sheet's name as Excel takes it (at most 31 characters, none of `[]:*?/\`).
fn sheet_name(title: &str) -> String {
    let name: String = title
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect();
    if name.trim().is_empty() { "Sheet1".to_owned() } else { name }
}

impl XlsxWriter {
    /// A workbook of `columns`, its sheet named after `title`.
    pub fn new(title: &str, columns: &[GridColumn]) -> Self {
        Self {
            title: sheet_name(title),
            columns: columns.iter().map(|c| (c.name.clone(), kind_of(c.class))).collect(),
            entries: Vec::new(),
            offset: 0,
            sheet: None,
            written: 0,
        }
    }

    /// Rows written so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    fn whole_part(
        &mut self,
        out: &mut Vec<u8>,
        name: &'static str,
        xml: &str,
    ) -> Result<(), String> {
        let data = xml.as_bytes();
        let compressed = deflate(data);
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let entry = Entry {
            name,
            crc: crc.sum(),
            compressed: compressed.len() as u64,
            size: data.len() as u64,
            offset: self.offset,
            described: false,
        };
        let start = out.len();
        local_header(out, &entry)?;
        out.extend_from_slice(&compressed);
        self.offset += (out.len() - start) as u64;
        self.entries.push(entry);
        Ok(())
    }

    /// The workbook's fixed parts, the sheet's start and its header row.
    pub fn begin(&mut self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let head = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
        let types = format!(
            "{head}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
             <Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
             <Override PartName=\"/{SHEET}\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
             <Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>\
             </Types>"
        );
        self.whole_part(&mut out, "[Content_Types].xml", &types)?;
        let rels = format!(
            "{head}<Relationships xmlns=\"{PACKAGE}\"><Relationship Id=\"rId1\" \
             Type=\"{RELATIONS}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
        );
        self.whole_part(&mut out, "_rels/.rels", &rels)?;
        let mut title = String::new();
        xml_text(&self.title, &mut title);
        let workbook = format!(
            "{head}<workbook xmlns=\"{MAIN}\" xmlns:r=\"{RELATIONS}\"><sheets>\
             <sheet name=\"{title}\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>"
        );
        self.whole_part(&mut out, "xl/workbook.xml", &workbook)?;
        let workbook_rels = format!(
            "{head}<Relationships xmlns=\"{PACKAGE}\">\
             <Relationship Id=\"rId1\" Type=\"{RELATIONS}/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"{RELATIONS}/styles\" Target=\"styles.xml\"/></Relationships>"
        );
        self.whole_part(&mut out, "xl/_rels/workbook.xml.rels", &workbook_rels)?;
        let styles = format!(
            "{head}<styleSheet xmlns=\"{MAIN}\"><fonts count=\"2\"><font><sz val=\"11\"/>\
             <name val=\"Calibri\"/></font><font><b/><sz val=\"11\"/><name val=\"Calibri\"/></font>\
             </fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill>\
             <patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border>\
             <left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\">\
             <xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
             <cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>\
             <xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/>\
             </cellXfs></styleSheet>"
        );
        self.whole_part(&mut out, "xl/styles.xml", &styles)?;
        // The sheet: its sizes after it.
        let entry = Entry {
            name: SHEET,
            crc: 0,
            compressed: 0,
            size: 0,
            offset: self.offset,
            described: true,
        };
        let start = out.len();
        local_header(&mut out, &entry)?;
        self.offset += (out.len() - start) as u64;
        self.entries.push(entry);
        let mut sheet = Sheet {
            encoder: DeflateEncoder::new(Vec::new(), Compression::fast()),
            crc: flate2::Crc::new(),
            size: 0,
            compressed: 0,
            offset: self.offset,
        };
        let mut xml = format!(
            "{head}<worksheet xmlns=\"{MAIN}\"><sheetViews><sheetView workbookViewId=\"0\">\
             <pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/>\
             </sheetView></sheetViews><sheetData><row r=\"1\">"
        );
        for (i, (name, _)) in self.columns.iter().enumerate() {
            xml.push_str(&format!(
                "<c r=\"{}1\" s=\"1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">",
                column_letters(i)
            ));
            xml_text(name, &mut xml);
            xml.push_str("</t></is></c>");
        }
        xml.push_str("</row>");
        Self::feed(&mut sheet, xml.as_bytes()).map_err(|e| e.to_string())?;
        out.extend(Self::drain(&mut sheet));
        self.sheet = Some(sheet);
        Ok(out)
    }

    fn feed(sheet: &mut Sheet, data: &[u8]) -> std::io::Result<()> {
        sheet.crc.update(data);
        sheet.size += data.len() as u64;
        sheet.encoder.write_all(data)
    }

    /// What the encoder made so far, taken (counted as written).
    fn drain(sheet: &mut Sheet) -> Vec<u8> {
        let made = std::mem::take(sheet.encoder.get_mut());
        sheet.compressed += made.len() as u64;
        made
    }

    /// A row's cells (`NULL`: none written).
    fn row_xml(&self, number: u64, row: &[CellRef<'_>], xml: &mut String) {
        xml.push_str(&format!("<row r=\"{number}\">"));
        for (i, value) in row.iter().enumerate() {
            let at = format!("{}{number}", column_letters(i));
            let kind = self.columns.get(i).map_or(Kind::Text, |c| c.1);
            match value {
                CellRef::Null => {}
                CellRef::Bool(b) => {
                    xml.push_str(&format!("<c r=\"{at}\" t=\"b\"><v>{}</v></c>", u8::from(*b)));
                }
                CellRef::Int(i) => xml.push_str(&format!("<c r=\"{at}\"><v>{i}</v></c>")),
                CellRef::Float(f) if f.is_finite() => {
                    xml.push_str(&format!("<c r=\"{at}\"><v>{f}</v></c>"));
                }
                CellRef::Other(t)
                    if kind.is_number() && t.trim().parse::<f64>().is_ok_and(f64::is_finite) =>
                {
                    xml.push_str(&format!("<c r=\"{at}\"><v>{}</v></c>", t.trim()));
                }
                other => {
                    let text = extractors::Cell::text(&super::copy::cell_of(*other));
                    let cut: String = text.chars().take(MAX_CELL).collect();
                    xml.push_str(&format!(
                        "<c r=\"{at}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">"
                    ));
                    xml_text(&cut, xml);
                    xml.push_str("</t></is></c>");
                }
            }
        }
        xml.push_str("</row>");
    }

    /// The next rows (their bytes, compressed).
    pub fn rows(&mut self, rows: &[Vec<CellRef<'_>>]) -> Result<Vec<u8>, String> {
        if self.written + rows.len() as u64 + 1 > MAX_ROWS {
            return Err(i18n::tr!("extract.xlsx_too_many_rows", count = MAX_ROWS - 1));
        }
        let mut xml = String::new();
        for row in rows {
            self.written += 1;
            self.row_xml(self.written + 1, row, &mut xml);
        }
        let Some(sheet) = &mut self.sheet else { return Ok(Vec::new()) };
        Self::feed(sheet, xml.as_bytes()).map_err(|e| e.to_string())?;
        Ok(Self::drain(sheet))
    }

    /// The sheet closed, its sizes, the zip's directory.
    pub fn finish(&mut self) -> Result<Vec<u8>, String> {
        let Some(mut sheet) = self.sheet.take() else { return Ok(Vec::new()) };
        Self::feed(&mut sheet, b"</sheetData></worksheet>").map_err(|e| e.to_string())?;
        let rest = sheet.encoder.finish().map_err(|e| e.to_string())?;
        sheet.compressed += rest.len() as u64;
        let mut out = rest;
        let crc = sheet.crc.sum();
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == SHEET) {
            entry.crc = crc;
            entry.compressed = sheet.compressed;
            entry.size = sheet.size;
        }
        u32le(&mut out, 0x0807_4b50);
        u32le(&mut out, crc);
        u32le(&mut out, u32::try_from(sheet.compressed).map_err(|_| too_large())?);
        u32le(&mut out, u32::try_from(sheet.size).map_err(|_| too_large())?);
        let directory_at = sheet.offset + sheet.compressed + 16;
        let mut directory = Vec::new();
        for entry in &self.entries {
            u32le(&mut directory, 0x0201_4b50);
            u16le(&mut directory, 20);
            u16le(&mut directory, 20);
            u16le(&mut directory, if entry.described { 0x0008 } else { 0 });
            u16le(&mut directory, 8);
            u16le(&mut directory, DOS_TIME);
            u16le(&mut directory, DOS_DATE);
            u32le(&mut directory, entry.crc);
            u32le(&mut directory, u32::try_from(entry.compressed).map_err(|_| too_large())?);
            u32le(&mut directory, u32::try_from(entry.size).map_err(|_| too_large())?);
            u16le(&mut directory, entry.name.len() as u16);
            u16le(&mut directory, 0);
            u16le(&mut directory, 0);
            u16le(&mut directory, 0);
            u16le(&mut directory, 0);
            u32le(&mut directory, 0);
            u32le(&mut directory, u32::try_from(entry.offset).map_err(|_| too_large())?);
            directory.extend_from_slice(entry.name.as_bytes());
        }
        let count = self.entries.len() as u16;
        let size = directory.len() as u32;
        out.extend(directory);
        u32le(&mut out, 0x0605_4b50);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, count);
        u16le(&mut out, count);
        u32le(&mut out, size);
        u32le(&mut out, u32::try_from(directory_at).map_err(|_| too_large())?);
        u16le(&mut out, 0);
        Ok(out)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/xlsx.rs"]
mod tests;
