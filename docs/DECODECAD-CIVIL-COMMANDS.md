# DecodeCAD Civil engine commands

These commands are registered in the CAD engine under the Civil menu. They are callable through the same command JSON interface used by engine and agent integrations.

## Generate and draw contours

Command: **civil.contours**

~~~json
{
  "name": "Levantamento do terreno",
  "points": [
    {"id": "P1", "x": 0, "y": 0, "z": 100},
    {"id": "P2", "x": 20, "y": 0, "z": 102},
    {"id": "P3", "x": 20, "y": 20, "z": 104},
    {"id": "P4", "x": 0, "y": 20, "z": 102}
  ],
  "interval": 0.5,
  "datum": 0,
  "tolerance": 0.001,
  "draw": true,
  "referenceElevation": 101
}
~~~

The command builds a TIN, extracts contour segments, stitches them into polylines and, by default, creates CAD lightweight polylines in the current space/current layer. Each resulting entity stores its contour elevation in the entity elevation field. The response includes input point count, triangle count, raw contour segment count, stitched polyline count, and created handles.

Optional referenceElevation adds an initial cut/fill calculation between the surface and a horizontal plane. Omit draw or set it to false to calculate without adding CAD entities.

## Sample a surface

Command: **civil.sample**

Use the same points array with x and y to query one position. The response includes interpolated elevation, slope in degrees, downslope aspect in degrees clockwise from north, and the triangle used.

## Calculate cut and fill to a plane

Command: **civil.volume**

Use the same points array and a required referenceElevation. The result contains cut, fill, and net volumes in the cube of the coordinate unit supplied by the user.

## Input and coordinate rules

- Every point is an object with finite numeric x, y, and z; id is optional and defaults to P1, P2, and so on.
- Duplicate IDs, duplicate XY positions, invalid coordinates and collinear survey sets are rejected.
- All horizontal, vertical, interval and reference values must use the same coordinate unit. The command does not infer CRS transformations or convert units.
- Commands currently operate on a supplied in-memory point array. A visual CSV import wizard, saved civil surface manager, breaklines, surface boundaries, and two-surface earthwork remain future work.
