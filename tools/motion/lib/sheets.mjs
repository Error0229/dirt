// Contact sheets: a movement's frames laid out in a grid, each labelled
// with its millisecond and anything the analysis found at it, drawn as a
// page in the headless browser and saved as one image. An image an agent
// can open with its file-reading tool and read.

import fs from 'node:fs';

import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { sleep } from './cdp.mjs';

export const PER_SHEET = 24;

const esc = text => String(text).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

function page(title, subtitle, body, width) {
  return `<!doctype html><meta charset="utf-8"><style>
  *{box-sizing:border-box}body{margin:0;background:#15120f;color:#ede8e1;font:13px/1.35 Consolas,'Cascadia Mono',monospace;width:${width}px;padding:14px}
  h1{font:600 16px/1.3 system-ui,sans-serif;margin:0 0 2px}p.sub{margin:0 0 12px;color:#a59d92;font-size:12px}
  .grid{display:grid;gap:8px}.cell{background:#221e1a;border:2px solid #221e1a;border-radius:6px;padding:4px}
  .cell.flag{border-color:#e5484d}.cell.note{border-color:#d9a441}
  .cell img{display:block;width:100%;height:auto;border-radius:3px}
  .crop{position:relative;overflow:hidden;border-radius:3px}.crop img{position:absolute;max-width:none;border-radius:0}
  .cap{display:flex;justify-content:space-between;gap:6px;padding:3px 2px 0;font-size:13px}
  .t{font-weight:700;color:#fff}.k{color:#a59d92}.f{color:#ff8a8f;font-size:11.5px;padding:1px 2px 0;white-space:normal}
  .n{color:#e7c27a;font-size:11.5px;padding:1px 2px 0}
  .row-label{grid-column:1/-1;font:600 12px system-ui,sans-serif;color:#cfc7bc;margin:4px 0 -2px}
  </style><h1>${esc(title)}</h1><p class="sub">${esc(subtitle)}</p>${body}`;
}

function cell(frame) {
  const flags = frame.flags || [];
  const notes = frame.notes || [];
  const label = frame.label ?? (frame.before ? 'before' : `${frame.t} ms`);
  const src = pathToFileURL(frame.file).href;
  // A frame of the whole page (DevTools' trace keeps no other) shown cut to
  // the movement's crop: `view` is the crop and the page's size, in CSS px.
  const picture = frame.view && (frame.view.crop.width < frame.view.width - 1 || frame.view.crop.height < frame.view.height - 1)
    ? (({ crop: c, width, height }) => `<div class="crop" style="aspect-ratio:${c.width}/${c.height}"><img src="${src}" style="width:${(width / c.width) * 100}%;left:${(-c.x / c.width) * 100}%;top:${(-c.y / c.height) * 100}%"></div>`)(frame.view)
    : `<img src="${src}">`;
  return `<div class="cell${flags.length ? ' flag' : notes.length ? ' note' : ''}">${picture}
    <div class="cap"><span class="t">${esc(label)}</span><span class="k">${esc(frame.kind || '')}</span></div>
    ${flags.map(f => `<div class="f">${esc(f)}</div>`).join('')}${notes.map(n => `<div class="n">${esc(n)}</div>`).join('')}</div>`;
}

/** Columns for frames of this shape, so a sheet stays about 1600 px or less each way. */
export function columnsFor(aspect, count = PER_SHEET) {
  if (aspect >= 1.2) return Math.min(4, count);
  if (aspect >= 0.8) return Math.min(5, count);
  return Math.min(8, count);
}

async function draw(browser, html, width, out) {
  const file = `${out}.html`;
  fs.writeFileSync(file, html);
  await browser.cdp.send('Emulation.setDeviceMetricsOverride', { width, height: 800, deviceScaleFactor: 1, mobile: false });
  await browser.goto(pathToFileURL(file).href);
  await browser.cdp.evaluate('Promise.all([...document.images].map(i => i.complete ? 0 : new Promise(r => { i.onload = i.onerror = r; })))');
  const height = await browser.cdp.evaluate('Math.ceil(document.documentElement.scrollHeight)');
  await browser.cdp.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  await sleep(100);
  const shot = await browser.cdp.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true });
  fs.writeFileSync(out, Buffer.from(shot.data, 'base64'));
  fs.rmSync(file);
  return out;
}

/** Frames as sheets of at most PER_SHEET each; returns the sheets' paths. */
export async function contactSheets(browser, frames, { title, subtitle, out, aspect }) {
  if (!frames.length) return [];
  const columns = columnsFor(aspect);
  const width = columns >= 8 ? 1640 : columns === 5 ? 1640 : 1680;
  const sheets = [];
  const count = Math.ceil(frames.length / PER_SHEET);
  for (let n = 0; n < count; n++) {
    const part = frames.slice(n * PER_SHEET, (n + 1) * PER_SHEET);
    const first = part.find(f => !f.before) || part[0];
    const last = part[part.length - 1];
    const body = `<div class="grid" style="grid-template-columns:repeat(${columns},1fr)">${part.map(cell).join('')}</div>`;
    const file = count > 1 ? `${out}-${n + 1}.png` : `${out}.png`;
    const range = `${first.label ?? `${first.t} ms`} to ${last.label ?? `${last.t} ms`}`;
    sheets.push(await draw(browser, page(`${title}${count > 1 ? ` (${n + 1} of ${count})` : ''}`, `${subtitle} · ${range}`, body, width), width, file));
  }
  return sheets;
}

/**
 * Two rows of frames, one above the other, column by column: opening
 * against closing played backwards, or the app against the mockup at the
 * same millisecond. `pairs` is [{ top, bottom }].
 */
export async function sideBySide(browser, pairs, { title, subtitle, out, aspect, topLabel, bottomLabel }) {
  if (!pairs.length) return null;
  const columns = aspect >= 1.2 ? 4 : aspect >= 0.8 ? 6 : 8;
  const width = 1680;
  let body = '';
  for (let at = 0; at < pairs.length; at += columns) {
    const chunk = pairs.slice(at, at + columns);
    const blank = '<div></div>';
    body += `<div class="grid" style="grid-template-columns:repeat(${columns},1fr);margin-bottom:14px">
      <div class="row-label">${esc(topLabel)}</div>${chunk.map(p => (p.top ? cell(p.top) : blank)).join('')}
      <div class="row-label">${esc(bottomLabel)}</div>${chunk.map(p => (p.bottom ? cell(p.bottom) : blank)).join('')}</div>`;
  }
  return draw(browser, page(title, subtitle, body, width), width, out);
}

/** A video of stepped frames at 60 frames a second, if ffmpeg is there. */
export function video(frames, out, fps = 60) {
  const list = `${out}.txt`;
  fs.writeFileSync(list, frames.map(f => `file '${f.file.replace(/\\/g, '/').replace(/'/g, "'\\''")}'\nduration ${1 / fps}`).join('\n'));
  return new Promise(resolve => {
    const ffmpeg = spawn('ffmpeg', ['-y', '-hide_banner', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', list,
      '-vf', 'pad=ceil(iw/2)*2:ceil(ih/2)*2', '-r', String(fps), '-pix_fmt', 'yuv420p', out], { stdio: 'ignore', windowsHide: true });
    ffmpeg.on('error', () => resolve(null));
    ffmpeg.on('exit', code => { fs.rmSync(list, { force: true }); resolve(code === 0 ? out : null); });
  });
}

/** The frame nearest to `t` among `frames` (stepped frames are at fixed times). */
export function nearest(frames, t) {
  let best = null;
  for (const frame of frames) {
    if (frame.before) continue;
    if (!best || Math.abs(frame.t - t) < Math.abs(best.t - t)) best = frame;
  }
  return best;
}

export function removeFrames(dir) {
  if (fs.existsSync(dir)) fs.rmSync(dir, { recursive: true, force: true });
}

