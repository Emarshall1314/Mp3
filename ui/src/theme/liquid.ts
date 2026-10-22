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
