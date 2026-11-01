//! Survey datasets, defensive CSV import and validation.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
use std::collections::BTreeSet;
use cadcraft_civil_core::{Coordinate3, CoordinateError, MAX_COORDINATE};
use serde::{Deserialize, Serialize};
pub const MAX_CSV_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_CSV_ROWS: usize = 2_000_000;
pub const MAX_FIELD_BYTES: usize = 16 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurveyPoint { pub id: String, pub coordinate: Coordinate3, pub code: Option<String>, pub description: Option<String>, pub source_line: usize }
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SurveyDataset { pub name: String, pub points: Vec<SurveyPoint> }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportIssue { pub line: usize, pub message: String }
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport { pub accepted: usize, pub rejected: usize, pub issues: Vec<ImportIssue> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvColumns { pub id: usize, pub x: usize, pub y: usize, pub z: usize, pub code: Option<usize>, pub description: Option<usize> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CsvDelimiter { Comma, Semicolon, Tab }
impl CsvDelimiter { fn character(&self) -> char { match self { Self::Comma => ',', Self::Semicolon => ';', Self::Tab => '\t' } } }
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SurveyError {
    #[error("CSV exceeds the import size limit")] TooLarge,
    #[error("CSV contains too many rows")] TooManyRows,
    #[error("CSV is empty")] Empty,
    #[error("invalid CSV at line {line}: {message}")] InvalidCsv { line: usize, message: String },
    #[error("invalid column mapping: {0}")] InvalidMapping(String),
}
#[derive(Clone, Debug, PartialEq)]
pub struct ImportResult { pub dataset: SurveyDataset, pub report: ImportReport }

/// Imports CSV with explicit zero-based column mappings. The first row is a header.
/// Numeric fields use a decimal point; decimal-comma guessing is deliberately avoided.
pub fn import_csv(text: &str, name: impl Into<String>, delimiter: CsvDelimiter, columns: &CsvColumns) -> Result<ImportResult, SurveyError> {
    if text.len() > MAX_CSV_BYTES { return Err(SurveyError::TooLarge); }
    if text.trim().is_empty() { return Err(SurveyError::Empty); }
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index >= MAX_CSV_ROWS { return Err(SurveyError::TooManyRows); }
        if line.len() > MAX_FIELD_BYTES * 16 { return Err(SurveyError::InvalidCsv { line: index+1, message: "row exceeds size limit".into() }); }
        rows.push(parse_record(line, delimiter.character()).map_err(|message| SurveyError::InvalidCsv { line: index+1, message })?);
    }
    if rows.len() < 2 { return Err(SurveyError::Empty); }
    let required = [columns.id, columns.x, columns.y, columns.z];
    if required.iter().any(|i| *i >= rows[0].len()) || columns.code.is_some_and(|i| i >= rows[0].len()) || columns.description.is_some_and(|i| i >= rows[0].len()) {
        return Err(SurveyError::InvalidMapping("mapped column is outside the header".into()));
    }
    let mut dataset = SurveyDataset { name: name.into(), points: Vec::new() };
    let mut report = ImportReport::default();
    let mut ids = BTreeSet::new();
    let mut coordinates = BTreeSet::new();
    for (index,row) in rows.iter().enumerate().skip(1) {
        let line = index+1;
        if row.iter().all(|v| v.trim().is_empty()) { continue; }
        match parse_point(row,line,columns) {
            Ok(point) => {
                if !ids.insert(point.id.clone()) {
                    report.rejected += 1; report.issues.push(ImportIssue { line, message: format!("duplicate point ID: {}",point.id) }); continue;
                }
                let key = (point.coordinate.x.to_bits(),point.coordinate.y.to_bits(),point.coordinate.z.to_bits());
                if !coordinates.insert(key) {
                    report.rejected += 1; report.issues.push(ImportIssue { line, message: "point duplicates an existing XYZ coordinate".into() }); continue;
                }
                dataset.points.push(point); report.accepted += 1;
            }
            Err(message) => { report.rejected += 1; report.issues.push(ImportIssue { line,message }); }
        }
    }
    Ok(ImportResult { dataset, report })
}
fn parse_point(row: &[String], line: usize, columns: &CsvColumns) -> Result<SurveyPoint,String> {
    let field = |i:usize| row.get(i).map(|s|s.trim()).unwrap_or("");
    let id=field(columns.id); if id.is_empty() { return Err("point ID is empty".into()); }
    let number=|i:usize,label:&str| -> Result<f64,String> {
        let v=field(i).parse::<f64>().map_err(|_|format!("{label} is not a valid number"))?;
        if !v.is_finite() { return Err(format!("{label} must be finite")); }
        if v.abs()>MAX_COORDINATE { return Err(format!("{label} exceeds supported coordinate range")); }
        Ok(v)
    };
    let coordinate=Coordinate3::new(number(columns.x,"X")?,number(columns.y,"Y")?,number(columns.z,"Z")?).map_err(|e:CoordinateError|e.to_string())?;
    let optional=|i:Option<usize>| i.and_then(|n|row.get(n)).map(|s|s.trim()).filter(|s|!s.is_empty()).map(str::to_owned);
    Ok(SurveyPoint { id:id.to_owned(),coordinate,code:optional(columns.code),description:optional(columns.description),source_line:line })
}
fn parse_record(line:&str,delimiter:char)->Result<Vec<String>,String> {
    let mut fields=Vec::new(); let mut field=String::new(); let mut chars=line.chars().peekable(); let mut quoted=false;
    while let Some(ch)=chars.next() {
        if quoted {
            if ch=='"' { if chars.peek()==Some(&'"') { chars.next(); field.push('"'); } else { quoted=false; } } else { field.push(ch); }
        } else if ch==delimiter { if field.len()>MAX_FIELD_BYTES{return Err("field exceeds size limit".into());} fields.push(std::mem::take(&mut field)); }
        else if ch=='"' && field.is_empty() { quoted=true; }
        else if ch=='"' { return Err("unexpected quote in unquoted field".into()); }
        else { field.push(ch); }
        if field.len()>MAX_FIELD_BYTES{return Err("field exceeds size limit".into());}
    }
    if quoted{return Err("unterminated quoted field; multiline fields are unsupported".into());}
    fields.push(field); Ok(fields)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn cols()->CsvColumns { CsvColumns{id:0,x:1,y:2,z:3,code:Some(4),description:Some(5)} }
    #[test] fn imports_quoted_descriptions() {
        let csv="id,x,y,z,code,description\nP1,10,20,100,PT,\"marco, norte\"\nP2,11,21,101,PV,\n";
        let result=import_csv(csv,"levantamento",CsvDelimiter::Comma,&cols());
        assert!(result.is_ok());
        if let Ok(result)=result { assert_eq!(result.report.accepted,2); assert_eq!(result.dataset.points[0].description.as_deref(),Some("marco, norte")); }
    }
    #[test] fn reports_duplicate_and_invalid_rows() {
        let csv="id,x,y,z\nA,1,2,3\nA,4,5,6\nB,1,2,3\nC,NaN,0,0\n";
        let c=CsvColumns{code:None,description:None,..cols()};
        let result=import_csv(csv,"test",CsvDelimiter::Comma,&c);
        assert!(result.is_ok());
        if let Ok(result)=result { assert_eq!(result.report.accepted,1); assert_eq!(result.report.rejected,3); }
    }
    #[test] fn supports_semicolon_delimiter() {
        let csv="id;x;y;z\nP1;1.5;2.5;3.5\n";
        let c=CsvColumns{id:0,x:1,y:2,z:3,code:None,description:None};
        let result=import_csv(csv,"test",CsvDelimiter::Semicolon,&c);
        assert!(result.is_ok());
        if let Ok(result)=result { assert_eq!(result.dataset.points[0].coordinate.x,1.5); }
    }
    #[test] fn rejects_unterminated_quotes() {
        let csv="id,x,y,z\nP1,1,2,\"3\n";
        let c=CsvColumns{code:None,description:None,..cols()};
        assert!(matches!(import_csv(csv,"test",CsvDelimiter::Comma,&c),Err(SurveyError::InvalidCsv{..})));
    }
}
