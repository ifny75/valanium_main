from PIL import Image, ImageDraw
import os
import math

SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
OUT = os.path.dirname(os.path.abspath(__file__))
CANVAS = 512

# Rounded-triangle "V" badge (replaces the old rounded-square logo),
# matching the design supplied by the user: dark indigo at the bottom
# point fading to bright violet/magenta at the top-right, a soft white
# glow behind a bold V glyph.
TRIANGLE = [(72, 88), (440, 88), (256, 460)]
STROKE_W = 64  # rounds the triangle's corners, same trick as the SVG source

GRAD_STOPS = [
    (0.00, (13, 6, 48)),
    (0.45, (58, 20, 112)),
    (0.75, (122, 31, 214)),
    (1.00, (180, 50, 255)),
]


def lerp(a, b, t):
    return a + (b - a) * t


def gradient_color(t):
    t = max(0.0, min(1.0, t))
    for (t0, c0), (t1, c1) in zip(GRAD_STOPS, GRAD_STOPS[1:]):
        if t0 <= t <= t1:
            local_t = 0 if t1 == t0 else (t - t0) / (t1 - t0)
            return tuple(round(lerp(c0[i], c1[i], local_t)) for i in range(3))
    return GRAD_STOPS[-1][1]


def build_master():
    # Diagonal linear gradient background (bottom-left -> top-right, like
    # the SVG's x1=0% y1=100% -> x2=100% y2=0%).
    grad = Image.new("RGB", (CANVAS, CANVAS))
    px = grad.load()
    diag = (CANVAS - 1) * 2
    for y in range(CANVAS):
        for x in range(CANVAS):
            t = (x + (CANVAS - 1 - y)) / diag
            px[x, y] = gradient_color(t)

    # Soft white glow, centered above the V.
    glow = Image.new("L", (CANVAS, CANVAS), 0)
    gpx = glow.load()
    cx, cy, rx, ry = 256, 215, 130, 120
    for y in range(max(0, cy - ry - 10), min(CANVAS, cy + ry + 10)):
        for x in range(max(0, cx - rx - 10), min(CANVAS, cx + rx + 10)):
            d = math.hypot((x - cx) / rx, (y - cy) / ry)
            if d < 1:
                gpx[x, y] = round(140 * (1 - d))
    glow_rgb = Image.merge("RGB", [Image.new("L", (CANVAS, CANVAS), 255)] * 3)
    grad = Image.composite(glow_rgb, grad, glow)

    # Triangle mask with rounded corners (fill + thick round-joined stroke).
    mask = Image.new("L", (CANVAS, CANVAS), 0)
    mdraw = ImageDraw.Draw(mask)
    mdraw.polygon(TRIANGLE, fill=255)
    closed = TRIANGLE + [TRIANGLE[0]]
    mdraw.line(closed, fill=255, width=STROKE_W, joint="curve")
    # joint="curve" doesn't round the seam where the closed line starts/ends
    # at the same point — cover it (and guarantee every vertex is rounded)
    # with an explicit circle.
    r = STROKE_W / 2
    for vx, vy in TRIANGLE:
        mdraw.ellipse([vx - r, vy - r, vx + r, vy + r], fill=255)

    out = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    out.paste(grad, (0, 0), mask)

    # V glyph: a thick, round-jointed chevron in off-white.
    odraw = ImageDraw.Draw(out)
    v_points = [(175, 150), (256, 320), (337, 150)]
    odraw.line(v_points, fill=(245, 240, 255, 255), width=34, joint="curve")
    for p in v_points:
        r = 17
        odraw.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=(245, 240, 255, 255))

    return out


master = build_master()

icons = {size: master.resize((size, size), Image.LANCZOS) for size in SIZES}

for size, img in icons.items():
    img.save(os.path.join(OUT, f"{size}x{size}.png"))

icons[128].resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, "128x128@2x.png"))
icons[512].save(os.path.join(OUT, "icon.png"))

ico_sizes = [16, 24, 32, 48, 64, 128, 256]
icons[256].save(
    os.path.join(OUT, "icon.ico"),
    sizes=[(s, s) for s in ico_sizes],
)

print("done")
