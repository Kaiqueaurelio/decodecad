//! Civil/topographic drawing commands.

use cadcraft_civil_core::Coordinate3;
use cadcraft_civil_survey::{import_csv, CsvColumns, CsvDelimiter, ImportReport, SurveyDataset, SurveyPoint};
use cadcraft_civil_surface::{
    calculate_plane_volumes, generate_contours, sample_surface, stitch_contours, TinSurface,
};
use cadcraft_doc::{EntityKind, LwPolyline};
use cadcraft_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::{bad, CommandSpec};
use crate::{Result, Session};

const COMMAND: &str = "civil.contours";
const DEFAULT_STITCH_TOLERANCE: f64 = 0.001;
const MAX_DRAWN_POLYLINES: usize = 100_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(COMMAND, "Generate Civil Contours", run_contours)
            .menu(&["Civil", "Surface", "Generate Contours"])
            .params(
                "{points: [{id?, x, y, z, code?, description?}, ...], interval, datum?, tolerance?, draw?: bool (default true), referenceElevation?} -> TIN, contour polylines, CAD handles and optional cut/fill",
            ),
        CommandSpec::new("civil.sample", "Sample Civil Surface", run_sample)
            .menu(&["Civil", "Surface", "Sample Elevation"])
            .params("{points: [...], x, y} -> elevation, slope, aspect and triangle"),
        CommandSpec::new("civil.volume", "Calculate Civil Earthwork", run_volume)
            .menu(&["Civil", "Earthwork", "Volume to Datum"])
            .params("{points: [...], referenceElevation} -> cut, fill and net volumes"),
    ]
}

fn required_number(parameters: &Value, key: &str) -> Result<f64> {
    parameters
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| bad(COMMAND, format!("parameter {key} must be a finite number")))
}

fn optional_number(parameters: &Value, key: &str, default: f64) -> Result<f64> {
    match parameters.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite())
            .ok_or_else(|| bad(COMMAND, format!("parameter {key} must be a finite number"))),
    }
}

fn parse_dataset(parameters: &Value) -> Result<(SurveyDataset, Option<ImportReport>)> {
    let name = parameters
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Civil Surface");

    if let Some(csv) = parameters.get("csv").and_then(Value::as_str) {
        if parameters.get("points").is_some() {
            return Err(bad(COMMAND, "provide either csv or points, not both"));
        }
        let mapping = parameters
            .get("columns")
            .ok_or_else(|| bad(COMMAND, "CSV import requires explicit columns mapping"))?;
        let required_column = |key: &str| {
            mapping
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| bad(COMMAND, format!("CSV column {key} must be a zero-based integer")))
        };
        let optional_column = |key: &str| -> Result<Option<usize>> {
            match mapping.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(value) => value
                    .as_u64()
                    .and_then(|number| usize::try_from(number).ok())
                    .map(Some)
                    .ok_or_else(|| bad(COMMAND, format!("CSV column {key} must be a zero-based integer"))),
            }
        };
        let columns = CsvColumns {
            id: required_column("id")?,
            x: required_column("x")?,
            y: required_column("y")?,
            z: required_column("z")?,
            code: optional_column("code")?,
            description: optional_column("description")?,
        };
        let delimiter = match parameters
            .get("delimiter")
            .and_then(Value::as_str)
            .unwrap_or("comma")
        {
            "comma" => CsvDelimiter::Comma,
            "semicolon" => CsvDelimiter::Semicolon,
            "tab" => CsvDelimiter::Tab,
            _ => return Err(bad(COMMAND, "CSV delimiter must be comma, semicolon or tab")),
        };
        let imported = import_csv(csv, name, delimiter, &columns)
            .map_err(|error| bad(COMMAND, error.to_string()))?;
        return Ok((imported.dataset, Some(imported.report)));
    }

    let points = parameters
        .get("points")
        .and_then(Value::as_array)
        .ok_or_else(|| bad(COMMAND, "points must be an array or csv must be supplied"))?;
    if points.len() < 3 {
        return Err(bad(COMMAND, "at least three survey points are required"));
    }
    if points.len() > cadcraft_civil_surface::MAX_TIN_POINTS {
        return Err(bad(
            COMMAND,
            format!(
                "point count exceeds the limit of {}",
                cadcraft_civil_surface::MAX_TIN_POINTS
            ),
        ));
    }

    let mut seen_ids = std::collections::BTreeSet::new();
    let mut survey_points = Vec::with_capacity(points.len());
    for (index, value) in points.iter().enumerate() {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("P{}", index + 1));
        if !seen_ids.insert(id.clone()) {
            return Err(bad(COMMAND, format!("duplicate point ID {id}")));
        }

        let coordinate_number = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_f64)
                .filter(|number| number.is_finite())
                .ok_or_else(|| bad(COMMAND, format!("point {id} requires finite {key}")))
        };
        let coordinate = Coordinate3::new(
            coordinate_number("x")?,
            coordinate_number("y")?,
            coordinate_number("z")?,
        )
        .map_err(|error| bad(COMMAND, format!("point {id}: {error}")))?;

        let optional_text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        survey_points.push(SurveyPoint {
            id,
            coordinate,
            code: optional_text("code"),
            description: optional_text("description"),
            source_line: index + 1,
        });
    }

    Ok((
        SurveyDataset {
            name: name.to_owned(),
            points: survey_points,
        },
        None,
    ))
}

fn run_sample(_session: &mut Session, parameters: &Value) -> Result<Value> {
    let (dataset, import_report) = parse_dataset(parameters)?;
    let surface = TinSurface::from_survey(&dataset)
        .map_err(|error| bad("civil.sample", error.to_string()))?;
    let x = required_number(parameters, "x")?;
    let y = required_number(parameters, "y")?;
    let sample = sample_surface(&surface, x, y)
        .map_err(|error| bad("civil.sample", error.to_string()))?;

    Ok(json!({
        "surface": dataset.name,
        "x": x,
        "y": y,
        "elevation": sample.elevation,
        "slopeDegrees": sample.slope_degrees,
        "aspectDegrees": sample.aspect_degrees,
        "triangleIndex": sample.triangle_index,
        "importReport": import_report
    }))
}

fn run_volume(_session: &mut Session, parameters: &Value) -> Result<Value> {
    let (dataset, import_report) = parse_dataset(parameters)?;
    let surface = TinSurface::from_survey(&dataset)
        .map_err(|error| bad("civil.volume", error.to_string()))?;
    let reference = required_number(parameters, "referenceElevation")?;
    let volumes = calculate_plane_volumes(&surface, reference)
        .map_err(|error| bad("civil.volume", error.to_string()))?;

    Ok(json!({
        "surface": dataset.name,
        "referenceElevation": reference,
        "cut": volumes.cut,
        "fill": volumes.fill,
        "net": volumes.net,
        "unit": "coordinate_unit^3",
        "importReport": import_report
    }))
}

fn run_contours(session: &mut Session, parameters: &Value) -> Result<Value> {
    let interval = required_number(parameters, "interval")?;
    let datum = optional_number(parameters, "datum", 0.0)?;
    let tolerance = optional_number(parameters, "tolerance", DEFAULT_STITCH_TOLERANCE)?;
    let draw = parameters
        .get("draw")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let (dataset, import_report) = parse_dataset(parameters)?;
    let surface = TinSurface::from_survey(&dataset)
        .map_err(|error| bad(COMMAND, error.to_string()))?;
    let segments = generate_contours(&surface, interval, datum)
        .map_err(|error| bad(COMMAND, error.to_string()))?;
    let polylines = stitch_contours(&segments, tolerance)
        .map_err(|error| bad(COMMAND, error.to_string()))?;

    if polylines.len() > MAX_DRAWN_POLYLINES {
        return Err(bad(
            COMMAND,
            format!("contour count exceeds the drawing limit of {MAX_DRAWN_POLYLINES}"),
        ));
    }

    let volumes = match parameters.get("referenceElevation") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let reference = required_number(parameters, "referenceElevation")?;
            Some(
                calculate_plane_volumes(&surface, reference)
                    .map_err(|error| bad(COMMAND, error.to_string()))?,
            )
        }
    };

    let mut handles = Vec::new();
    if draw {
        for contour in &polylines {
            if contour.points.len() < 2 || (contour.closed && contour.points.len() < 3) {
                continue;
            }
            let vertices = contour
                .points
                .iter()
                .map(|point| PolyVertex::new(Vec2::new(point.x, point.y)))
                .collect();
            let entity = EntityKind::LwPolyline(LwPolyline {
                vertices,
                closed: contour.closed,
                const_width: 0.0,
                elevation: contour.elevation,
                plinegen: false,
            });
            let handle = session.add_entity(entity)?;
            handles.push(handle.hex());
        }
    }

    let cut_fill = volumes.map(|volume| {
        json!({
            "cut": volume.cut,
            "fill": volume.fill,
            "net": volume.net,
            "unit": "coordinate_unit^3"
        })
    });

    Ok(json!({
        "surface": dataset.name,
        "inputPoints": surface.vertices.len(),
        "triangles": surface.triangles.len(),
        "contourSegments": segments.len(),
        "contourPolylines": polylines.len(),
        "drawnEntities": handles.len(),
        "handles": handles,
        "drawn": draw,
        "cutFill": cut_fill,
        "importReport": import_report
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_point_objects_and_generates_default_ids() {
        let parameters = json!({
            "name": "Levantamento",
            "points": [
                {"x": 0.0, "y": 0.0, "z": 10.0},
                {"id": "A", "x": 10.0, "y": 0.0, "z": 11.0},
                {"x": 0.0, "y": 10.0, "z": 12.0, "code": "PT"}
            ]
        });
        let result = parse_dataset(&parameters);
        assert!(result.is_ok());
        if let Ok((dataset, report)) = result {
            assert_eq!(dataset.name, "Levantamento");
            assert_eq!(dataset.points[0].id, "P1");
            assert_eq!(dataset.points[1].id, "A");
            assert_eq!(dataset.points[2].code.as_deref(), Some("PT"));
            assert!(report.is_none());
        }
    }

    #[test]
    fn rejects_duplicate_ids_and_missing_coordinates() {
        let duplicate = json!({
            "points": [
                {"id": "A", "x": 0.0, "y": 0.0, "z": 0.0},
                {"id": "A", "x": 1.0, "y": 0.0, "z": 0.0},
                {"id": "C", "x": 0.0, "y": 1.0, "z": 0.0}
            ]
        });
        assert!(parse_dataset(&duplicate).is_err());

        let missing = json!({
            "points": [
                {"id": "A", "x": 0.0, "y": 0.0, "z": 0.0},
                {"id": "B", "x": 1.0, "y": 0.0, "z": 0.0},
                {"id": "C", "x": 0.0, "y": 1.0}
            ]
        });
        assert!(parse_dataset(&missing).is_err());
    }

    #[test]
    fn imports_csv_with_explicit_column_mapping() {
        let parameters = json!({
            "name": "CSV surface",
            "csv": "id,easting,northing,level\nP1,0,0,10\nP2,10,0,11\nP3,0,10,12\n",
            "delimiter": "comma",
            "columns": {"id": 0, "x": 1, "y": 2, "z": 3}
        });
        let result = parse_dataset(&parameters);
        assert!(result.is_ok());
        if let Ok((dataset, report)) = result {
            assert_eq!(dataset.points.len(), 3);
            assert_eq!(dataset.points[2].coordinate.z, 12.0);
            assert!(report.is_some_and(|summary| summary.accepted == 3 && summary.rejected == 0));
        }
    }

    #[test]
    fn rejects_csv_without_explicit_column_mapping() {
        let parameters = json!({
            "csv": "id,x,y,z\nP1,0,0,10\nP2,10,0,11\nP3,0,10,12\n"
        });
        assert!(parse_dataset(&parameters).is_err());
    }

    #[test]
    fn civil_commands_are_registered() {
        let ids: Vec<&str> = specs().iter().map(|spec| spec.id).collect();
        assert!(ids.contains(&COMMAND));
        assert!(ids.contains(&"civil.sample"));
        assert!(ids.contains(&"civil.volume"));
    }
}
