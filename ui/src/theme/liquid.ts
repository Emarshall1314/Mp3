// Liquid glass: panels that bend what is behind them at their edges, with bright rims and a
// sheen. The bending is an SVG displacement filter in `backdrop-filter`, which only Chromium
// engines apply (Android's WebView). WebKitGTK (Linux) drops a whole `backdrop-filter` that
// references an SVG filter (WebKit bug 245510), so there liquid glass keeps the rims, sheen and
// stronger saturation over an ordinary blur (D51).

export const refracts = /Chrome\//.test(navigator.userAgent);

const SVG_NS = 'http://www.w3.org/2000/svg';

/// The displacement map: neutral in the middle and ramping over the outer 12 % of each edge,
/// red for x and green for y, so content bends inwards at the rims like a thick lens. Drawn on
/// a canvas (an SVG image here isn't decoded inside filters reliably).
function displacementMap(size = 128): string {
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = size;
  const ctx = canvas.getContext('2d');
  if (!ctx) return '';
  const img = ctx.createImageData(size, size);
  const edge = 0.12;
  const ramp = (t: number) =>
    t < edge
      ? 128 + 127 * (1 - t / edge)
      : t > 1 - edge
        ? 128 - 128 * ((t - 1 + edge) / edge)
        : 128;
  for (let y = 0; y < size; y++)
    for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      img.data[i] = ramp(x / (size - 1));
      img.data[i + 1] = ramp(y / (size - 1));
      img.data[i + 2] = 128;
      img.data[i + 3] = 255;
    }
  ctx.putImageData(img, 0, 0);
  return canvas.toDataURL();
}

/// Adds the filter to the page once, where it takes effect. Sizes are fractions of each panel,
/// so one filter fits the sidebar, the player bar and menus alike.
export function ensureLiquidFilter() {
  if (!refracts || document.getElementById('cadenza-liquid')) return;
  const map = displacementMap();
  if (!map) return;
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('width', '0');
  svg.setAttribute('height', '0');
  svg.style.position = 'absolute';
  svg.innerHTML = `<filter id="cadenza-liquid" x="0" y="0" width="1" height="1" primitiveUnits="objectBoundingBox" color-interpolation-filters="sRGB">
<feImage href="${map}" x="0" y="0" width="1" height="1" preserveAspectRatio="none" result="map"/>
<feDisplacementMap in="SourceGraphic" in2="map" scale="0.06" xChannelSelector="R" yChannelSelector="G"/>
</filter>`;
  document.body.appendChild(svg);
}

/// The displacement field of a lens the shape of a rounded rectangle, `w`×`h` px with corner
/// radius `r`, as RGBA bytes for a canvas: red for x, green for y, 128 for none, 255 or 0 for
/// `LENS_REACH` × `bezel` px. Over the outer `bezel` px the glass curves away like the rim of a
/// thick lens: what's behind the rim is squeezed towards the edge (three times at the very
/// edge), so straight lines bend as they go under it. A backdrop filter can only see what is
/// behind the element itself, so the rim gathers from inside the bezel rather than beyond the
/// edge, and the mapping never folds back on itself. The middle is flat and leaves it alone.
/// The furthest the rim reaches in, as a fraction of the bezel (the peak of `1 - (1 - t)³ - t`).
export const LENS_REACH = 0.385;

export function lensField(w: number, h: number, r: number, bezel: number): Uint8ClampedArray {
  const data = new Uint8ClampedArray(w * h * 4);
  const hx = w / 2;
  const hy = h / 2;
  const rad = Math.min(r, hx, hy);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      // Signed distance to the rounded rectangle and the outward direction of its edge.
      const px = x + 0.5 - hx;
      const py = y + 0.5 - hy;
      const qx = Math.abs(px) - (hx - rad);
      const qy = Math.abs(py) - (hy - rad);
      let nx: number;
      let ny: number;
      let inside: number;
      if (qx > 0 && qy > 0) {
        const len = Math.hypot(qx, qy);
        inside = rad - len;
        nx = qx / len;
        ny = qy / len;
      } else if (qx > qy) {
        inside = rad - Math.max(qx, 0);
        nx = 1;
        ny = 0;
      } else {
        inside = rad - Math.max(qy, 0);
        nx = 0;
        ny = 1;
      }
      nx *= Math.sign(px) || 1;
      ny *= Math.sign(py) || 1;
      const t = Math.min(1, Math.max(0, inside / bezel));
      // A point `t` into the bezel shows what is `1 - (1 - t)³` in: inwards by this much.
      const amount = (1 - (1 - t) ** 3 - t) / LENS_REACH;
      const i = (y * w + x) * 4;
      data[i] = Math.round(128 - 127 * nx * amount);
      data[i + 1] = Math.round(128 - 127 * ny * amount);
      data[i + 2] = 128;
      data[i + 3] = 255;
    }
  return data;
}

const lenses = new Map<string, string>();

/// The id of an SVG filter that refracts like a glass lens of this size and corner radius,
/// added to the page the first time it is asked for. One displacement pass: bending red, green
/// and blue separately (a colour fringe) took three, and the dock's glass is redrawn on every
/// frame of a scroll, so it cost smoothness on the phone. Chromium only (see the top of this
/// file); elsewhere it returns null.
export function lensFilter(w: number, h: number, r: number): string | null {
  if (!refracts) return null;
  w = Math.round(w);
  h = Math.round(h);
  r = Math.round(r);
  if (w < 4 || h < 4) return null;
  const key = `${w}x${h}r${r}`;
  const known = lenses.get(key);
  if (known) return known;
  const bezel = Math.max(8, Math.min(22, Math.min(w, h) * 0.32));
  const canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;
  const image = ctx.createImageData(w, h);
  image.data.set(lensField(w, h, r, bezel));
  ctx.putImageData(image, 0, 0);
  const id = `cadenza-lens-${key}`;
  // feDisplacementMap moves by `scale` × (value − ½), so a full value moves LENS_REACH × bezel.
  const scale = 2 * LENS_REACH * bezel;
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('width', '0');
  svg.setAttribute('height', '0');
  svg.style.position = 'absolute';
  svg.innerHTML = `<filter id="${id}" x="0" y="0" width="${w}" height="${h}" filterUnits="userSpaceOnUse" primitiveUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feImage href="${canvas.toDataURL()}" x="0" y="0" width="${w}" height="${h}" preserveAspectRatio="none" result="map"/>
<feDisplacementMap in="SourceGraphic" in2="map" scale="${scale.toFixed(1)}" xChannelSelector="R" yChannelSelector="G"/>
</filter>`;
  document.body.appendChild(svg);
  lenses.set(key, id);
  return id;
}
