# Artwork

| File | Purpose |
| --- | --- |
| `logo.svg` | Editable CAE Ferris mark used by the repository README. |
| `conversion-flow.svg` | Editable flowchart of format groups and supported conversion directions. |
| `donut-voxel-comparison.png` | Matched-view STL → VTI → STL comparison at three voxel sizes and one smoothed result. |
| `rustacean-flat-happy.svg` | Retained upstream Ferris artwork. |

## CAE Ferris mark

`logo.svg` is the source of truth. It contains vector paths and shapes, with no
embedded raster image or font. The README displays it at 320 pixels wide. Its
`viewBox="0 35 1200 925"` crops the illustration without a wordmark.

The five file cards represent a cube, triangular mesh, gear, mechanical part,
and result graph. Their centers sit 28 degrees apart on a circle centered at
`(620, 650)` with radius `500`. The six arrow shafts use that same circle.

SVG paint order controls the overlaps:

1. The left paper stack covers the start of the incoming arrow.
2. The middle arrows and first four file cards sit above the left stack.
3. Ferris sits behind the right paper stack.
4. The outgoing arrow sits above the right stack. The graph card covers the
   arrow at its start.
5. The laptop covers Ferris's lower body.

The paper stacks and laptop are vector paths traced from generated artwork.
Keep the groups and their order when editing the mark. Check the complete
illustration and a 320-pixel preview on light and dark backgrounds.

## Conversion flowchart

`conversion-flow.svg` groups formats by the roles used under `src/formats/`.
The arrows show reader and writer availability for each group. OP2 is readable
and writable with a companion mesh; PCH is read-only. The diagram describes
supported projections, so it does not promise lossless transfer. Keep its
accessible title and description current when adapter capabilities change.
Review it at full size and at the README's typical displayed width.
The README uses a relative image URL so each branch shows its own diagram.

## Donut voxel comparison

The input is the authored, watertight `examples/donut.stl`, a torus with major
radius 2.45 and tube radius 0.78 in unspecified units. The image shows this
source STL and STL outputs from VTI occupancy grids with cell sizes 0.75, 0.50,
and 0.25. The last panel smooths the 0.25 output for ten passes. Oblique and
top views use the same camera and scale across all panels. Captions report
occupied voxel counts and enclosed-volume differences relative to the source.
The README gives the conversion commands and explains the limits of those
measurements. The image illustrates geometry occupancy, not solver-result
interpolation.

## Ferris source and license

`rustacean-flat-happy.svg` is the unmodified Happy Ferris SVG from
[rustacean.net](https://rustacean.net/assets/rustacean-flat-happy.svg), retrieved
October 4, 2026. The site identifies Karen Rustad Tölva as the creator and
publishes Ferris under CC0. `logo.svg` and `conversion-flow.svg` embed Ferris
vector paths. Keep the upstream file for provenance; edit the composed paths
in the relevant SVG.
