"""Regenerates src/assets/map-fill.svg (and tools/world-fill.svg for
reference) from Natural Earth's public-domain 1:110m admin-0 country
boundaries. Source data: ne_110m_admin_0_countries.geojson, fetched from
https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_admin_0_countries.geojson
(Natural Earth data is public domain, no attribution required).

Run from anywhere: `python tools/build_map.py`
"""
import json
import math
import os

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "ne_110m_admin_0_countries.geojson")
OUT_WORLD = os.path.join(HERE, "world-fill.svg")
OUT_REGION = os.path.join(HERE, "..", "src", "assets", "map-fill.svg")
# Northern/Western Europe crop, aspect-matched to the map pane (~1.12), in
# projected (Mercator) units — see project() below, recomputed by eye the
# same way as REGION_VIEWBOX originally was (render world-fill.svg full
# size, find the four server capitals, pick a centered crop).
REGION_VIEWBOX = (-98.6, -315.1, 280, 248)

SCALE = 1000 / (2 * math.pi)  # world: ~1000 wide


def project(lon, lat):
    # Web Mercator, not equirectangular — plain linear lat/lon scaling
    # stretched Scandinavia noticeably wide/flat at this latitude (~60N)
    # compared to the Mercator maps everyone's used to seeing.
    x = math.radians(lon) * SCALE
    lat_rad = math.radians(max(min(lat, 85.05), -85.05))
    y = -math.asinh(math.tan(lat_rad)) * SCALE
    return x, y


def ring_to_path(ring):
    parts = []
    for i, (lon, lat) in enumerate(ring):
        x, y = project(lon, lat)
        parts.append(f"{'M' if i == 0 else 'L'}{x:.2f} {y:.2f}")
    parts.append("Z")
    return "".join(parts)


def geometry_to_path(geom):
    gtype = geom["type"]
    coords = geom["coordinates"]
    d = []
    if gtype == "Polygon":
        for ring in coords:
            d.append(ring_to_path(ring))
    elif gtype == "MultiPolygon":
        for poly in coords:
            for ring in poly:
                d.append(ring_to_path(ring))
    return "".join(d)


data = json.load(open(SRC, encoding="utf-8"))

paths = []
for feat in data["features"]:
    props = feat["properties"]
    iso = (props.get("ISO_A2") or props.get("ISO_A2_EH") or "").lower()
    name = props.get("NAME", "")
    d = geometry_to_path(feat["geometry"])
    if not d:
        continue
    safe_name = name.replace('"', "")
    paths.append(f'<path class="land" data-iso="{iso}" data-name="{safe_name}" d="{d}"/>')

svg = (
    '<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="1000" viewBox="-500 -500 1000 1000">\n'
    '<style>.land{fill:#2a2440;stroke:#0d0c15;stroke-width:0.6;stroke-linejoin:round;}</style>\n'
    + "\n".join(paths)
    + "\n</svg>\n"
)

with open(OUT_WORLD, "w", encoding="utf-8") as f:
    f.write(svg)

vx, vy, vw, vh = REGION_VIEWBOX
svg_region = (
    # Explicit width/height (matching the viewBox aspect exactly) so the
    # intrinsic ratio used by `background-size/mask-size: cover` is
    # unambiguous, instead of relying on renderers to infer it from
    # viewBox alone.
    f'<svg xmlns="http://www.w3.org/2000/svg" width="{vw}" height="{vh}" viewBox="{vx} {vy} {vw} {vh}">\n'
    '<style>.land{fill:#2a2440;stroke:#0d0c15;stroke-width:0.6;stroke-linejoin:round;}</style>\n'
    + "\n".join(paths)
    + "\n</svg>\n"
)
with open(OUT_REGION, "w", encoding="utf-8") as f:
    f.write(svg_region)

print("wrote", OUT_WORLD, len(svg), "bytes,", len(paths), "country paths")
print("wrote", OUT_REGION, len(svg_region), "bytes")
