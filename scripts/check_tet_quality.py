#!/usr/bin/env python3
"""Check a real Gmsh surface-to-Tet4-to-STL conversion against a unit cube."""

import argparse
from collections import Counter
from pathlib import Path
import struct
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def run(binary: Path, *args: object) -> None:
    command = [str(binary), 'convert', *(str(arg) for arg in args), '--accept-all']
    result = subprocess.run(command, capture_output=True, text=True)
    assert result.returncode == 0, (command, result.stdout, result.stderr)


def tetra_volume(points: list[tuple[float, float, float]], ids: list[int]) -> float:
    a, b, c, d = (points[index] for index in ids)
    ab = [b[i] - a[i] for i in range(3)]
    ac = [c[i] - a[i] for i in range(3)]
    ad = [d[i] - a[i] for i in range(3)]
    return abs(ab[0] * (ac[1] * ad[2] - ac[2] * ad[1])
               - ab[1] * (ac[0] * ad[2] - ac[2] * ad[0])
               + ab[2] * (ac[0] * ad[1] - ac[1] * ad[0])) / 6


def stl_metrics(path: Path) -> tuple[float, Counter]:
    data = path.read_bytes()
    triangles = struct.unpack_from('<I', data, 80)[0]
    assert len(data) == 84 + triangles * 50 and triangles > 0
    volume = 0.0
    edges = Counter()
    for index in range(triangles):
        offset = 84 + index * 50 + 12
        a, b, c = [struct.unpack_from('<fff', data, offset + corner * 12)
                   for corner in range(3)]
        volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
                   + a[1] * (b[2] * c[0] - b[0] * c[2])
                   + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6
        for u, v in [(a, b), (b, c), (c, a)]:
            edges[tuple(sorted((u, v)))] += 1
    return volume, edges


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/caexfer')
    parser.add_argument('--gmsh', default='gmsh')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='caexfer-tet-quality-') as directory:
        folder = Path(directory)
        cube = folder / 'cube.bdf'
        cube.write_text(
            'GRID,1,,0.,0.,0.\nGRID,2,,1.,0.,0.\nGRID,3,,1.,1.,0.\n'
            'GRID,4,,0.,1.,0.\nGRID,5,,0.,0.,1.\nGRID,6,,1.,0.,1.\n'
            'GRID,7,,1.,1.,1.\nGRID,8,,0.,1.,1.\n'
            'CHEXA,1,7,1,2,3,4,5,6\n+,7,8\n'
        )
        surface = folder / 'surface.vtu'
        volume = folder / 'volume.vtu'
        shell = folder / 'shell.stl'
        run(args.binary, cube, surface, '--surface')
        run(args.binary, surface, volume, '--infill-shape', 'tet4',
            '--infill-unit-size', '0.5', '--gmsh', args.gmsh)

        piece = ET.parse(volume).find('./UnstructuredGrid/Piece')
        assert piece is not None
        coordinates = [float(value) for value in piece.find('./Points/DataArray').text.split()]
        points = [tuple(coordinates[i:i + 3]) for i in range(0, len(coordinates), 3)]
        cells = piece.find('./Cells')
        connectivity = [int(value) for value in cells.find('./DataArray[@Name="connectivity"]').text.split()]
        offsets = [int(value) for value in cells.find('./DataArray[@Name="offsets"]').text.split()]
        types = [int(value) for value in cells.find('./DataArray[@Name="types"]').text.split()]
        assert types and len(types) == len(offsets) and all(kind == 10 for kind in types)
        assert offsets[-1] == len(connectivity)
        start = 0
        total = 0.0
        for end in offsets:
            ids = connectivity[start:end]
            assert len(ids) == 4
            cell_volume = tetra_volume(points, ids)
            assert cell_volume > 0
            total += cell_volume
            start = end
        assert abs(total - 1.0) < 1e-3, f'Tet4 volume drift: {total}'

        run(args.binary, volume, shell)
        shell_volume, edges = stl_metrics(shell)
        assert edges and all(count == 2 for count in edges.values())
        assert abs(shell_volume - 1.0) < 1e-3, f'STL volume drift: {shell_volume}'
    print('Gmsh Tet4 and STL unit-cube quality checks passed')


if __name__ == '__main__':
    main()
