// Makes every icon the apps ship from one drawing, `assets/brand/cairn.svg`
// (the "Cairn": a falling clay grain over two layers of sand on a paper
// tile). Run it again after changing the drawing, and commit what it writes:
//
//     node tools/icons/make-icons.mjs
//
// It writes:
//
// - the Windows icon, `crates/dirt-desktop/icons/dirt.ico` (16 to 256 px),
//   which `dx` puts in the program and the installer (Dioxus.toml, [bundle]),
//   and which the window takes as its own (dioxus-desktop reads the
//   program's icon);
// - Android's launcher icons, in the native module's resources
//   (`crates/dirt-mobile/android/native/src/main/res`): the tile as a PNG
//   at each density for Android 7 (`mipmap-*/dirt_launcher.png`, and a
//   round one), and for Android 8 and later an adaptive icon, whose
//   foreground and monochrome layers are vectors made from the drawing's
//   own paths, so they are sharp at any size, on a background of the
//   paper's colour.
//
// The pictures are drawn by a headless Edge or Chrome (the one the motion
// check uses, `tools/motion/lib/targets.mjs`): nothing to install. The
// drawing is read with a few patterns, not a full SVG parser; it is ours
// and holds only filled paths and one circle.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { launchHeadless, runCleanups } from '../motion/lib/targets.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const source = path.join(root, 'assets', 'brand', 'cairn.svg');
const res = path.join(root, 'crates', 'dirt-mobile', 'android', 'native', 'src', 'main', 'res');
const ico = path.join(root, 'crates', 'dirt-desktop', 'icons', 'dirt.ico');

const MADE = 'Made by tools/icons/make-icons.mjs from assets/brand/cairn.svg: change the drawing and run it again, not this file.';

// ---- The drawing ------------------------------------------------------------

const svg = fs.readFileSync(source, 'utf8');
const attribute = (element, name) => element.match(new RegExp(`\\b${name}="([^"]*)"`))?.[1];
const shapes = [...svg.matchAll(/<(path|circle)\b[^>]*>/g)].map(([element, kind]) => {
  const fill = attribute(element, 'fill');
  if (kind === 'circle') {
    const [cx, cy, r] = ['cx', 'cy', 'r'].map(name => Number(attribute(element, name)));
    // A circle as a path, which an Android vector can hold.
    return { fill, d: `M${cx - r},${cy}a${r},${r} 0 1,0 ${2 * r},0a${r},${r} 0 1,0 ${-2 * r},0Z` };
  }
  return { fill, d: attribute(element, 'd') };
});
const size = Number(svg.match(/viewBox="0 0 (\d+) \1"/)?.[1]);
if (!size || shapes.length < 2) throw new Error(`${source} is not the square drawing this script reads`);
// The first shape is the paper tile; the rest are the cairn on it.
const [tile, ...cairn] = shapes;
const paper = tile.fill;
const colours = [...new Set(cairn.map(shape => shape.fill))];
const colourName = fill => ['dirt_launcher_clay', 'dirt_launcher_dark'][colours.indexOf(fill)] ?? `dirt_launcher_${colours.indexOf(fill)}`;

const cairnSvg = (transform = '') => `<g transform="${transform}">${cairn.map(shape => `<path fill="${shape.fill}" d="${shape.d}"/>`).join('')}</g>`;
const asSvg = body => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}">${body}</svg>`;

// ---- Drawing it in the browser ---------------------------------------------

const browser = await launchHeadless({ width: 800, height: 800 });
try {
  await browser.goto('about:blank');

  // The cairn's box, for placing it in the round icon and the adaptive one.
  const box = await browser.cdp.evaluate(`(() => {
    const holder = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    holder.innerHTML = ${JSON.stringify(cairnSvg())};
    document.body.append(holder);
    const b = holder.firstChild.getBBox();
    holder.remove();
    return { x: b.x, y: b.y, width: b.width, height: b.height };
  })()`);
  const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  // How far the cairn reaches from its centre: half its box's diagonal, a
  // little more than the drawing itself, so what is placed by it fits.
  const reach = Math.hypot(box.width, box.height) / 2;

  /** The drawing `svgText` as `px` square: a PNG, and its RGBA pixels. */
  const draw = (svgText, px) => browser.cdp.evaluate(`(async () => {
    const image = new Image(${px}, ${px});
    image.src = 'data:image/svg+xml;base64,' + ${JSON.stringify(Buffer.from(svgText).toString('base64'))};
    await image.decode();
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = ${px};
    const context = canvas.getContext('2d');
    context.imageSmoothingQuality = 'high';
    context.drawImage(image, 0, 0, ${px}, ${px});
    const rgba = context.getImageData(0, 0, ${px}, ${px}).data;
    let binary = '';
    for (let i = 0; i < rgba.length; i += 0x8000) binary += String.fromCharCode(...rgba.subarray(i, i + 0x8000));
    return { png: canvas.toDataURL('image/png').split(',')[1], rgba: btoa(binary) };
  })()`).then(({ png, rgba }) => ({ png: Buffer.from(png, 'base64'), rgba: Buffer.from(rgba, 'base64') }));

  const write = (file, data) => {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, data);
    console.log(`wrote ${path.relative(root, file)}`);
  };

  // The tile as drawn, and a round one: the paper as a circle as wide as
  // the tile, the cairn a little smaller in it so that it keeps its margin.
  const square = svg;
  const half = size / 2;
  const roundScale = 0.92;
  const round = asSvg(`<circle cx="${half}" cy="${half}" r="${half - 16}" fill="${paper}"/>${cairnSvg(
    `translate(${half} ${half}) scale(${roundScale}) translate(${-centre.x} ${-centre.y})`)}`);

  // ---- Windows: one .ico, 16 to 256 px ----
  // Each size drawn from the vector, not shrunk from a large picture. Up to
  // 64 px as bitmaps, which every part of Windows reads; larger as PNG.
  const entries = [];
  for (const px of [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]) {
    const { png, rgba } = await draw(square, px);
    entries.push({ px, data: px >= 96 ? png : bitmap(rgba, px) });
  }
  write(ico, icoFile(entries));

  // ---- Android 7: the tile and the round one at each density ----
  const densities = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 };
  for (const [density, scale] of Object.entries(densities)) {
    const px = Math.round(48 * scale);
    write(path.join(res, `mipmap-${density}`, 'dirt_launcher.png'), (await draw(square, px)).png);
    write(path.join(res, `mipmap-${density}`, 'dirt_launcher_round.png'), (await draw(round, px)).png);
  }

  // ---- Android 8 and later: the adaptive icon ----
  // 108 dp square, of which the launcher shows at least the middle 66 dp
  // circle. The cairn's box is fitted inside that circle, less 1 dp.
  const canvas = 108;
  const safe = 66 / 2 - 1;
  const scale = safe / reach;
  const group = `android:scaleX="${round4(scale)}" android:scaleY="${round4(scale)}" android:translateX="${round4(canvas / 2 - centre.x * scale)}" android:translateY="${round4(canvas / 2 - centre.y * scale)}"`;
  const vector = fill => `<?xml version="1.0" encoding="utf-8"?>
<!-- ${MADE} -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="${canvas}dp" android:height="${canvas}dp"
    android:viewportWidth="${canvas}" android:viewportHeight="${canvas}">
    <group ${group}>
${cairn.map(shape => `        <path android:fillColor="${fill(shape)}"
            android:pathData="${shape.d}" />`).join('\n')}
    </group>
</vector>
`;
  write(path.join(res, 'drawable', 'dirt_launcher_foreground.xml'), vector(shape => `@color/${colourName(shape.fill)}`));
  // Tinted by the launcher with the phone's own colours (Android 13's
  // themed icons): only its shape counts.
  write(path.join(res, 'drawable', 'dirt_launcher_monochrome.xml'), vector(() => '#FFFFFFFF'));
  const adaptive = `<?xml version="1.0" encoding="utf-8"?>
<!-- ${MADE} -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/dirt_launcher_paper" />
    <foreground android:drawable="@drawable/dirt_launcher_foreground" />
    <monochrome android:drawable="@drawable/dirt_launcher_monochrome" />
</adaptive-icon>
`;
  write(path.join(res, 'mipmap-anydpi-v26', 'dirt_launcher.xml'), adaptive);
  write(path.join(res, 'mipmap-anydpi-v26', 'dirt_launcher_round.xml'), adaptive);
  // The icon's own colours, from the drawing. They are the brand's, not the
  // interface's tokens (dirt_colours.xml), and do not follow the theme.
  write(path.join(res, 'values', 'dirt_launcher.xml'), `<?xml version="1.0" encoding="utf-8"?>
<!-- ${MADE} -->
<resources>
    <color name="dirt_launcher_paper">${argb(paper)}</color>
${colours.map(fill => `    <color name="${colourName(fill)}">${argb(fill)}</color>`).join('\n')}
</resources>
`);
} finally {
  await browser.stop();
  await runCleanups();
}

// ---- Files ------------------------------------------------------------------

function round4(value) { return Number(value.toFixed(4)); }

function argb(hex) { return `#FF${hex.replace('#', '').toUpperCase()}`; }

/** An icon's bitmap entry: a 32-bit DIB, bottom row first, with its mask. */
function bitmap(rgba, px) {
  const maskRow = Math.ceil(px / 32) * 4;
  const out = Buffer.alloc(40 + px * px * 4 + maskRow * px);
  out.writeUInt32LE(40, 0);
  out.writeInt32LE(px, 4);
  out.writeInt32LE(px * 2, 8); // the colours and the mask, one above the other
  out.writeUInt16LE(1, 12);
  out.writeUInt16LE(32, 14);
  out.writeUInt32LE(px * px * 4 + maskRow * px, 20);
  for (let y = 0; y < px; y++) {
    const from = (px - 1 - y) * px * 4;
    for (let x = 0; x < px; x++) {
      const [r, g, b, a] = rgba.subarray(from + x * 4, from + x * 4 + 4);
      const at = 40 + (y * px + x) * 4;
      out[at] = b; out[at + 1] = g; out[at + 2] = r; out[at + 3] = a;
      if (a === 0) {
        const maskAt = 40 + px * px * 4 + y * maskRow + (x >> 3);
        out[maskAt] |= 0x80 >> (x & 7);
      }
    }
  }
  return out;
}

/** An .ico file holding `entries`, each `{ px, data }`. */
function icoFile(entries) {
  const header = Buffer.alloc(6 + 16 * entries.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // an icon
  header.writeUInt16LE(entries.length, 4);
  let offset = header.length;
  entries.forEach(({ px, data }, i) => {
    const at = 6 + 16 * i;
    header[at] = px >= 256 ? 0 : px;
    header[at + 1] = px >= 256 ? 0 : px;
    header.writeUInt16LE(1, at + 4);
    header.writeUInt16LE(32, at + 6);
    header.writeUInt32LE(data.length, at + 8);
    header.writeUInt32LE(offset, at + 12);
    offset += data.length;
  });
  return Buffer.concat([header, ...entries.map(entry => entry.data)]);
}
