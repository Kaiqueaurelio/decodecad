//! Topographic 2.5D surface generation and contour extraction.
//!
//! The triangulator uses a normalized, deterministic Bowyer-Watson insertion order.
//! Its current point limit is intentionally conservative because the first version
//! scans the active triangle set for every inserted point.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use cadcraft_civil_core::{Coordinate3, MAX_COORDINATE};
use cadcraft_civil_survey::SurveyDataset;
use serde::{Deserialize, Serialize};

pub const MAX_TIN_POINTS: usize = 20_000;
pub const MAX_CONTOUR_LEVELS: usize = 10_000;
pub const MAX_CONTOUR_SEGMENTS: usize = 2_000_000;
const GEOMETRY_EPSILON: f64 = 1.0e-14;
const CIRCLE_EPSILON: f64 = 1.0e-12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceVertex {
    pub point_id: String,
    pub coordinate: Coordinate3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Triangle {
    /// Counter-clockwise indices into TinSurface::vertices.
    pub vertices: [usize; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TinSurface {
    pub name: String,
    pub vertices: Vec<SurfaceVertex>,
    pub triangles: Vec<Triangle>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanarPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContourSegment {
    pub elevation: f64,
    pub start: PlanarPoint,
    pub end: PlanarPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSample {
    pub elevation: f64,
    pub slope_degrees: f64,
    /// Downslope azimuth clockwise from north; absent on a perfectly flat triangle.
    pub aspect_degrees: Option<f64>,
    pub triangle_index: usize,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SurfaceError {
    #[error("at least three survey points are required to build a TIN")]
    TooFewPoints,
    #[error("TIN input exceeds the limit of {MAX_TIN_POINTS} points")]
    TooManyPoints,
    #[error("point {point_id} contains a non-finite or out-of-range coordinate")]
    InvalidCoordinate { point_id: String },
    #[error("points {first_id} and {second_id} share the same XY position")]
    DuplicatePlanarCoordinate { first_id: String, second_id: String },
    #[error("survey points are collinear in plan")]
    CollinearPoints,
    #[error("TIN triangulation could not construct a valid surface")]
    TriangulationFailed,
    #[error("contour interval must be finite, positive and supported")]
    InvalidContourInterval,
    #[error("contour datum must be finite and supported")]
    InvalidContourDatum,
    #[error("contour generation exceeds its level limit ({MAX_CONTOUR_LEVELS})")]
    TooManyContourLevels,
    #[error("surface contains a triangle with an invalid vertex index")]
    InvalidTriangleIndex,
    #[error("query coordinates must be finite and within the supported coordinate range")]
    InvalidQueryCoordinate,
    #[error("query position lies outside the TIN surface")]
    OutsideSurface,
    #[error("surface contains a degenerate triangle")]
    DegenerateTriangle,
    #[error("too many contour segments (limit {MAX_CONTOUR_SEGMENTS})")]
    TooManyContourSegments,
}

#[derive(Clone, Copy)]
struct Point2 {
    x: f64,
    y: f64,
}

impl TinSurface {
    /// Builds a 2.5D Delaunay TIN from a survey dataset.
    ///
    /// XY duplicates are rejected instead of being silently combined: two elevations
    /// at the same plan position cannot both represent a single-valued terrain surface.
    pub fn from_survey(dataset: &SurveyDataset) -> Result<Self, SurfaceError> {
        let point_count = dataset.points.len();
        if point_count < 3 {
            return Err(SurfaceError::TooFewPoints);
        }
        if point_count > MAX_TIN_POINTS {
            return Err(SurfaceError::TooManyPoints);
        }

        let mut vertices = Vec::with_capacity(point_count);
        let mut seen_xy = BTreeMap::new();
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for point in &dataset.points {
            let coordinate = point.coordinate;
            if !valid_coordinate(coordinate) {
                return Err(SurfaceError::InvalidCoordinate { point_id: point.id.clone() });
            }

            let key = (canonical_bits(coordinate.x), canonical_bits(coordinate.y));
            match seen_xy.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(vertices.len());
                }
                Entry::Occupied(entry) => {
                    let previous = vertices.get(*entry.get());
                    let first_id = previous.map(|vertex: &SurfaceVertex| vertex.point_id.clone()).unwrap_or_default();
                    return Err(SurfaceError::DuplicatePlanarCoordinate { first_id, second_id: point.id.clone() });
                }
            }

            min_x = min_x.min(coordinate.x);
            min_y = min_y.min(coordinate.y);
            max_x = max_x.max(coordinate.x);
            max_y = max_y.max(coordinate.y);
            vertices.push(SurfaceVertex { point_id: point.id.clone(), coordinate });
        }

        let scale = (max_x - min_x).max(max_y - min_y);
        if !scale.is_finite() || scale <= 0.0 {
            return Err(SurfaceError::CollinearPoints);
        }
        let center_x = min_x + (max_x - min_x) * 0.5;
        let center_y = min_y + (max_y - min_y) * 0.5;

        let mut points = Vec::with_capacity(point_count + 3);
        for vertex in &vertices {
            points.push(Point2 { x: (vertex.coordinate.x - center_x) / scale, y: (vertex.coordinate.y - center_y) / scale });
        }

        let first_super = points.len();
        points.push(Point2 { x: -32.0, y: -16.0 });
        points.push(Point2 { x: 32.0, y: -16.0 });
        points.push(Point2 { x: 0.0, y: 32.0 });

        let mut working = vec![[first_super, first_super + 1, first_super + 2]];
        for (point_index, inserted_point) in points.iter().copied().take(point_count).enumerate() {
            let mut bad_indices = Vec::new();
            for (triangle_index, triangle) in working.iter().enumerate() {
                let Some(a) = points.get(triangle[0]).copied() else {
                    return Err(SurfaceError::TriangulationFailed);
                };
                let Some(b) = points.get(triangle[1]).copied() else {
                    return Err(SurfaceError::TriangulationFailed);
                };
                let Some(c) = points.get(triangle[2]).copied() else {
                    return Err(SurfaceError::TriangulationFailed);
                };
                if circumcircle_contains(a, b, c, inserted_point) {
                    bad_indices.push(triangle_index);
                }
            }

            if bad_indices.is_empty() {
                return Err(SurfaceError::TriangulationFailed);
            }

            let bad_set: BTreeSet<usize> = bad_indices.iter().copied().collect();
            let mut edge_counts: BTreeMap<(usize, usize), usize> = BTreeMap::new();
            for triangle_index in &bad_indices {
                let Some(triangle) = working.get(*triangle_index).copied() else {
                    return Err(SurfaceError::TriangulationFailed);
                };
                count_edge(&mut edge_counts, triangle[0], triangle[1]);
                count_edge(&mut edge_counts, triangle[1], triangle[2]);
                count_edge(&mut edge_counts, triangle[2], triangle[0]);
            }

            let mut next = Vec::with_capacity(working.len() + edge_counts.len());
            for (index, triangle) in working.iter().copied().enumerate() {
                if !bad_set.contains(&index) {
                    next.push(triangle);
                }
            }

            for ((edge_a, edge_b), count) in edge_counts {
                if count != 1 {
                    continue;
                }
                let (Some(a), Some(b)) = (points.get(edge_a).copied(), points.get(edge_b).copied()) else {
                    return Err(SurfaceError::TriangulationFailed);
                };
                let turn = orientation(a, b, inserted_point);
                if turn.abs() <= GEOMETRY_EPSILON {
                    continue;
                }
                if turn > 0.0 {
                    next.push([edge_a, edge_b, point_index]);
                } else {
                    next.push([edge_b, edge_a, point_index]);
                }
            }
            working = next;
        }

        let mut triangles = Vec::new();
        let mut unique = BTreeSet::new();
        for triangle in working {
            if triangle.iter().any(|index| *index >= point_count) {
                continue;
            }
            let (Some(a), Some(b), Some(c)) = (points.get(triangle[0]).copied(), points.get(triangle[1]).copied(), points.get(triangle[2]).copied())
            else {
                return Err(SurfaceError::TriangulationFailed);
            };
            let turn = orientation(a, b, c);
            if turn.abs() <= GEOMETRY_EPSILON {
                continue;
            }
            let oriented = if turn > 0.0 { triangle } else { [triangle[1], triangle[0], triangle[2]] };
            let mut key = oriented;
            key.sort_unstable();
            if unique.insert(key) {
                triangles.push(Triangle { vertices: oriented });
            }
        }

        if triangles.is_empty() {
            return Err(SurfaceError::CollinearPoints);
        }
        triangles.sort_by_key(|triangle| triangle.vertices);
        Ok(Self { name: dataset.name.clone(), vertices, triangles })
    }
}

/// Interpolates elevation and computes slope/aspect at an XY position.
///
/// The query is evaluated against the first containing triangle. Aspect is the
/// downslope azimuth in degrees clockwise from north.
pub fn sample_surface(surface: &TinSurface, x: f64, y: f64) -> Result<SurfaceSample, SurfaceError> {
    if !x.is_finite() || !y.is_finite() || x.abs() > MAX_COORDINATE || y.abs() > MAX_COORDINATE {
        return Err(SurfaceError::InvalidQueryCoordinate);
    }

    let query = Point2 { x, y };
    const BARYCENTRIC_TOLERANCE: f64 = 1.0e-10;

    for (triangle_index, triangle) in surface.triangles.iter().enumerate() {
        let (Some(a), Some(b), Some(c)) =
            (surface.vertices.get(triangle.vertices[0]), surface.vertices.get(triangle.vertices[1]), surface.vertices.get(triangle.vertices[2]))
        else {
            return Err(SurfaceError::InvalidTriangleIndex);
        };
        for vertex in [a, b, c] {
            if !valid_coordinate(vertex.coordinate) {
                return Err(SurfaceError::InvalidCoordinate { point_id: vertex.point_id.clone() });
            }
        }

        let pa = Point2 { x: a.coordinate.x, y: a.coordinate.y };
        let pb = Point2 { x: b.coordinate.x, y: b.coordinate.y };
        let pc = Point2 { x: c.coordinate.x, y: c.coordinate.y };
        let denominator = orientation(pa, pb, pc);
        if !denominator.is_finite() || denominator == 0.0 {
            return Err(SurfaceError::DegenerateTriangle);
        }

        let wa = orientation(pb, pc, query) / denominator;
        let wb = orientation(pc, pa, query) / denominator;
        let wc = orientation(pa, pb, query) / denominator;
        if wa >= -BARYCENTRIC_TOLERANCE && wb >= -BARYCENTRIC_TOLERANCE && wc >= -BARYCENTRIC_TOLERANCE {
            let wa = wa.max(0.0);
            let wb = wb.max(0.0);
            let wc = wc.max(0.0);
            let weight_sum = wa + wb + wc;
            if !weight_sum.is_finite() || weight_sum <= 0.0 {
                return Err(SurfaceError::TriangulationFailed);
            }
            let wa = wa / weight_sum;
            let wb = wb / weight_sum;
            let wc = wc / weight_sum;

            let elevation = wa * a.coordinate.z + wb * b.coordinate.z + wc * c.coordinate.z;
            let dx1 = b.coordinate.x - a.coordinate.x;
            let dy1 = b.coordinate.y - a.coordinate.y;
            let dx2 = c.coordinate.x - a.coordinate.x;
            let dy2 = c.coordinate.y - a.coordinate.y;
            let dz1 = b.coordinate.z - a.coordinate.z;
            let dz2 = c.coordinate.z - a.coordinate.z;
            let dz_dx = (dz1 * dy2 - dz2 * dy1) / denominator;
            let dz_dy = (dx1 * dz2 - dx2 * dz1) / denominator;
            let gradient = dz_dx.hypot(dz_dy);
            if !elevation.is_finite() || !gradient.is_finite() {
                return Err(SurfaceError::TriangulationFailed);
            }

            let aspect_degrees = if gradient <= f64::EPSILON { None } else { Some((-dz_dx).atan2(-dz_dy).to_degrees().rem_euclid(360.0)) };
            return Ok(SurfaceSample { elevation, slope_degrees: gradient.atan().to_degrees(), aspect_degrees, triangle_index });
        }
    }

    Err(SurfaceError::OutsideSurface)
}

/// Extracts contour line segments at multiples of interval from datum elevation.
///
/// Segments are intentionally returned individually. A later surface-processing stage
/// can stitch adjacent segments into polylines while preserving branch and saddle topology.
pub fn generate_contours(surface: &TinSurface, interval: f64, datum: f64) -> Result<Vec<ContourSegment>, SurfaceError> {
    if !interval.is_finite() || interval <= 0.0 || interval > MAX_COORDINATE {
        return Err(SurfaceError::InvalidContourInterval);
    }
    if !datum.is_finite() || datum.abs() > MAX_COORDINATE {
        return Err(SurfaceError::InvalidContourDatum);
    }

    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for vertex in &surface.vertices {
        if !valid_coordinate(vertex.coordinate) {
            return Err(SurfaceError::InvalidCoordinate { point_id: vertex.point_id.clone() });
        }
        minimum = minimum.min(vertex.coordinate.z);
        maximum = maximum.max(vertex.coordinate.z);
    }
    if surface.vertices.is_empty() || surface.triangles.is_empty() {
        return Ok(Vec::new());
    }

    let first_level = ((minimum - datum) / interval).ceil();
    let last_level = ((maximum - datum) / interval).floor();
    if !first_level.is_finite() || !last_level.is_finite() {
        return Err(SurfaceError::InvalidContourInterval);
    }
    if last_level < first_level {
        return Ok(Vec::new());
    }
    let level_count = last_level - first_level + 1.0;
    if !level_count.is_finite() || level_count > MAX_CONTOUR_LEVELS as f64 {
        return Err(SurfaceError::TooManyContourLevels);
    }

    let mut segments = Vec::new();
    let level_count = level_count as usize;
    for offset in 0..level_count {
        let elevation = datum + (first_level + offset as f64) * interval;
        if !elevation.is_finite() || elevation.abs() > MAX_COORDINATE {
            return Err(SurfaceError::InvalidContourInterval);
        }

        for triangle in &surface.triangles {
            let (Some(a), Some(b), Some(c)) =
                (surface.vertices.get(triangle.vertices[0]), surface.vertices.get(triangle.vertices[1]), surface.vertices.get(triangle.vertices[2]))
            else {
                return Err(SurfaceError::InvalidTriangleIndex);
            };
            let corners = [a, b, c];
            let mut intersections = Vec::with_capacity(3);
            for (edge, start) in corners.iter().enumerate() {
                let Some(end) = corners.get((edge + 1) % 3) else {
                    continue;
                };
                let start_z = start.coordinate.z;
                let end_z = end.coordinate.z;
                let crosses = (start_z <= elevation && end_z > elevation) || (end_z <= elevation && start_z > elevation);
                if !crosses {
                    continue;
                }

                let denominator = end_z - start_z;
                if denominator == 0.0 {
                    continue;
                }
                let fraction = (elevation - start_z) / denominator;
                let intersection = PlanarPoint {
                    x: start.coordinate.x + fraction * (end.coordinate.x - start.coordinate.x),
                    y: start.coordinate.y + fraction * (end.coordinate.y - start.coordinate.y),
                };
                if !intersection.x.is_finite() || !intersection.y.is_finite() {
                    return Err(SurfaceError::TriangulationFailed);
                }
                if !intersections.iter().any(|existing: &PlanarPoint| {
                    existing.x.to_bits() == intersection.x.to_bits() && existing.y.to_bits() == intersection.y.to_bits()
                }) {
                    intersections.push(intersection);
                }
            }

            if intersections.len() == 2 {
                let Some(start) = intersections.first().copied() else {
                    continue;
                };
                let Some(end) = intersections.get(1).copied() else {
                    continue;
                };
                if start.x != end.x || start.y != end.y {
                    if segments.len() >= MAX_CONTOUR_SEGMENTS {
                        return Err(SurfaceError::TooManyContourSegments);
                    }
                    segments.push(ContourSegment { elevation, start, end });
                }
            }
        }
    }
    Ok(segments)
}

fn valid_coordinate(coordinate: Coordinate3) -> bool {
    [coordinate.x, coordinate.y, coordinate.z].iter().all(|value| value.is_finite() && value.abs() <= MAX_COORDINATE)
}

fn canonical_bits(value: f64) -> u64 {
    if value == 0.0 { 0.0_f64.to_bits() } else { value.to_bits() }
}

fn count_edge(edges: &mut BTreeMap<(usize, usize), usize>, a: usize, b: usize) {
    let key = if a < b { (a, b) } else { (b, a) };
    let count = edges.entry(key).or_insert(0);
    *count += 1;
}

fn orientation(a: Point2, b: Point2, c: Point2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn circumcircle_contains(a: Point2, b: Point2, c: Point2, point: Point2) -> bool {
    let ax = a.x - point.x;
    let ay = a.y - point.y;
    let bx = b.x - point.x;
    let by = b.y - point.y;
    let cx = c.x - point.x;
    let cy = c.y - point.y;

    let determinant =
        (ax * ax + ay * ay) * (bx * cy - cx * by) - (bx * bx + by * by) * (ax * cy - cx * ay) + (cx * cx + cy * cy) * (ax * by - bx * ay);

    determinant > -CIRCLE_EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_civil_survey::SurveyPoint;

    fn dataset(points: &[(f64, f64, f64)]) -> SurveyDataset {
        SurveyDataset {
            name: "levantamento de teste".to_owned(),
            points: points
                .iter()
                .enumerate()
                .map(|(index, (x, y, z))| SurveyPoint {
                    id: format!("P{}", index + 1),
                    coordinate: Coordinate3 { x: *x, y: *y, z: *z },
                    code: None,
                    description: None,
                    source_line: index + 2,
                })
                .collect(),
        }
    }

    #[test]
    fn triangulates_a_square_into_two_triangles() {
        let data = dataset(&[(0.0, 0.0, 10.0), (10.0, 0.0, 11.0), (10.0, 10.0, 12.0), (0.0, 10.0, 11.0)]);
        let surface = TinSurface::from_survey(&data);
        assert!(surface.is_ok());
        if let Ok(surface) = surface {
            assert_eq!(surface.vertices.len(), 4);
            assert_eq!(surface.triangles.len(), 2);
            for triangle in surface.triangles {
                assert!(triangle.vertices.iter().all(|index| *index < 4));
            }
        }
    }

    #[test]
    fn rejects_duplicate_xy_even_when_elevations_differ() {
        let data = dataset(&[(0.0, 0.0, 10.0), (1.0, 0.0, 11.0), (0.0, 1.0, 12.0), (0.0, 0.0, 15.0)]);
        assert!(matches!(TinSurface::from_survey(&data), Err(SurfaceError::DuplicatePlanarCoordinate { .. })));
    }

    #[test]
    fn rejects_collinear_points() {
        let data = dataset(&[(0.0, 0.0, 0.0), (1.0, 1.0, 1.0), (2.0, 2.0, 2.0)]);
        assert_eq!(TinSurface::from_survey(&data), Err(SurfaceError::CollinearPoints));
    }

    #[test]
    fn creates_contour_segments_for_a_sloped_surface() {
        let data = dataset(&[(0.0, 0.0, 0.0), (10.0, 0.0, 10.0), (10.0, 10.0, 20.0), (0.0, 10.0, 10.0)]);
        let surface = TinSurface::from_survey(&data);
        assert!(surface.is_ok());
        if let Ok(surface) = surface {
            let contours = generate_contours(&surface, 5.0, 0.0);
            assert!(contours.is_ok());
            if let Ok(contours) = contours {
                assert!(!contours.is_empty());
                assert!(contours.iter().all(|segment| {
                    segment.elevation.is_finite()
                        && segment.start.x.is_finite()
                        && segment.start.y.is_finite()
                        && segment.end.x.is_finite()
                        && segment.end.y.is_finite()
                }));
            }
        }
    }

    #[test]
    fn interpolates_elevation_and_downslope_aspect() {
        let data = dataset(&[(0.0, 0.0, 0.0), (10.0, 0.0, 10.0), (10.0, 10.0, 20.0), (0.0, 10.0, 10.0)]);
        let surface = TinSurface::from_survey(&data);
        assert!(surface.is_ok());
        if let Ok(surface) = surface {
            let sample = sample_surface(&surface, 5.0, 5.0);
            assert!(sample.is_ok());
            if let Ok(sample) = sample {
                assert!((sample.elevation - 10.0).abs() < 1.0e-9);
                assert!((sample.slope_degrees - 54.735_610_317_245_346).abs() < 1.0e-8);
                assert!(sample.aspect_degrees.is_some_and(|aspect| { (aspect - 225.0).abs() < 1.0e-8 }));
            }
        }
    }

    #[test]
    fn reports_queries_outside_the_surface() {
        let data = dataset(&[(0.0, 0.0, 0.0), (10.0, 0.0, 10.0), (0.0, 10.0, 10.0)]);
        let surface = TinSurface::from_survey(&data);
        assert!(surface.is_ok());
        if let Ok(surface) = surface {
            assert_eq!(sample_surface(&surface, 100.0, 100.0), Err(SurfaceError::OutsideSurface));
        }
    }

    #[test]
    fn guards_invalid_contour_parameters() {
        let data = dataset(&[(0.0, 0.0, 0.0), (1.0, 0.0, 1.0), (0.0, 1.0, 1.0)]);
        let surface = TinSurface::from_survey(&data);
        assert!(surface.is_ok());
        if let Ok(surface) = surface {
            assert_eq!(generate_contours(&surface, 0.0, 0.0), Err(SurfaceError::InvalidContourInterval));
        }
    }
}