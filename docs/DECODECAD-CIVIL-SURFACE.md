# DecodeCAD Civil — TIN and contour processing

This module adds the first computational surface pipeline for topographic surveys.

## Current capabilities

- Build a 2.5D Delaunay TIN from a validated `SurveyDataset`.
- Preserve survey point identifiers and source coordinates in surface vertices.
- Reject duplicate XY positions, invalid coordinates, collinear input and oversized datasets.
- Extract contour line segments at a fixed elevation interval and datum.
- Interpolate elevation and calculate slope plus downslope aspect inside a triangle.
- Guard malformed triangle indices and cap contour-level generation.

## Current limits

- The initial Bowyer-Watson implementation scans the active triangle set for each input point. It is suitable for small and medium preliminary datasets, with a hard limit of 20,000 points; it is not yet a large-survey optimized triangulator.
- Contours are returned as independent segments and capped at two million segments per request. Polyline stitching, labels, smoothing, breaklines, boundaries, holes, and constrained Delaunay triangulation are not implemented yet.
- The TIN is a 2.5D single-valued terrain model: duplicate XY positions are rejected, and overhangs/vertical faces are outside its scope.
- Numerical predicates use normalized f64 coordinates and deterministic tolerances. High-precision adaptive predicates and independent geospatial validation remain future work.

## Rust API

Use `TinSurface::from_survey(&dataset)` to generate the surface,
`sample_surface(&surface, x, y)` to query elevation/slope/aspect, and
`generate_contours(&surface, interval, datum)` to extract level segments.
