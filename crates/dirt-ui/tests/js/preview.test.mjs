// node --test crates/dirt-ui/tests/js/preview.test.mjs
//
// The file preview's gestures (interface design 6.16, A83), in dirt-ui's
// script with its style sheet, in a real headless Edge or Chrome: the
// maths it zooms, moves and steps by (`DirtUi.previewMaths`), and the
// preview itself zoomed by a double click, the wheel and a pinch, stepped
// by → and by a swipe, with Rust's part (a new item on the stage) done by
// hand. Skipped where neither browser is installed.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { findBrowser, launchHeadless } from '../../../../tools/motion/lib/targets.mjs';
import { sleep } from '../../../../tools/motion/lib/cdp.mjs';

let browserFound = true;
try { findBrowser(); } catch { browserFound = false; }

const here = new URL('.', import.meta.url);
const SCRIPT = fs.readFileSync(new URL('../../src/js/dirt-ui.js', here), 'utf8');
const STYLE = fs.readFileSync(new URL('../../src/style/components.css', here), 'utf8');
// The shape `dirt_ui::motion::script_tokens` gives; short, so the tests are.
const TOKENS = {
  springs: Object.fromEntries(['snap', 'glide', 'pop'].map(name => [name, { linear: 'ease-out', fallback: 'ease-out', ms: 300 }])),
  standard: 'ease', enter: 'ease-out', exit: 'ease-in', reducedMs: 90,
};

// The preview as `dirt_ui::files::preview` draws it with two pictures,
// closed: the file in the track, the ghost layer beside it, and the deck
// of what stands in for each file. The pictures are drawn in the page.
const BODY = `
  <div class="dui-preview" data-dui-modal="preview" data-dui-id="7" tabindex="-1" inert>
    <div class="dui-pv-scrim"></div>
    <div class="dui-pv-stage" data-dui-pv-stage>
      <div class="dui-pv-track">
        <div class="dui-pv-item" data-file="a"><img class="dui-pv-picture" data-dui-zoom alt="a" draggable="false"></div>
        <div class="dui-pv-ghost" aria-hidden="true" inert></div>
      </div>
      <div class="dui-pv-deck" aria-hidden="true" data-edge="256">
        <div class="dui-pv-stand" data-file="a"><img class="dui-pv-picture dui-pv-standin" alt=""></div>
        <div class="dui-pv-stand" data-file="b"><img class="dui-pv-picture dui-pv-standin" alt=""></div>
      </div>
    </div>
    <div class="dui-pv-dots"><i class="on"></i><i></i></div>
  </div>`;

const SETUP = `(async () => {
  const picture = (w, h, colour) => {
    const canvas = document.createElement('canvas');
    canvas.width = w; canvas.height = h;
    const g = canvas.getContext('2d');
    g.fillStyle = colour; g.fillRect(0, 0, w, h);
    g.fillStyle = '#fff'; g.fillRect(w / 8, h / 8, w / 3, h / 2);
    return canvas.toDataURL('image/png');
  };
  window.pictures = {
    a: picture(800, 500, '#6b8f71'), b: picture(400, 640, '#8a6f9e'), c: picture(700, 700, '#4f6d8a'),
    // Thumbnails, as \`dirt_core::files::thumbnail\` makes them: 256 a side
    // at most.
    ta: picture(256, 160, '#6b8f71'), tb: picture(160, 256, '#8a6f9e'), tc: picture(256, 256, '#4f6d8a'),
    small: picture(200, 130, '#b07a4f'),
    // A photograph's size: six million pixels.
    large: picture(3000, 2000, '#4f6d8a'),
  };
  document.querySelector('.dui-pv-item img').src = pictures.a;
  for (const card of document.querySelectorAll('.dui-pv-deck > .dui-pv-stand')) card.querySelector('img').src = pictures['t' + card.dataset.file];
  await Promise.all([...document.images].map(img => img.decode()));
  window.told = [];
  DirtUi.connect(message => told.push(message));
  return true;
})()`;

// What Rust does to open or close it.
const set = open => `(() => {
  const modal = document.querySelector('[data-dui-modal="preview"]');
  modal.classList.toggle('on', ${open});
  modal.toggleAttribute('inert', ${!open});
  return true;
})()`;

const ITEM = `document.querySelector('[data-dui-pv-stage] > .dui-pv-track > .dui-pv-item')`;
const TRACK = `document.querySelector('[data-dui-pv-stage] > .dui-pv-track')`;
// The picture's view as drawn, once its movement has ended.
const VIEW = `(() => {
  const img = ${ITEM}.querySelector('img');
  const t = getComputedStyle(img).transform;
  const m = t === 'none' ? new DOMMatrix() : new DOMMatrix(t);
  return { s: m.a, x: m.e, y: m.f, animations: img.getAnimations().length };
})()`;
// How far the track is drawn moved.
const TRACK_X = `new DOMMatrix(getComputedStyle(${TRACK}).transform).e`;

async function page(platform) {
  const browser = await launchHeadless({ width: 1000, height: 700 });
  const { cdp } = browser;
  await browser.goto('about:blank');
  if (platform === 'android') await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
  await cdp.evaluate(`(() => {
    const style = document.createElement('style');
    style.textContent = ${JSON.stringify(STYLE)};
    document.head.appendChild(style);
    document.documentElement.classList.add(${JSON.stringify(platform)});
    document.body.innerHTML = ${JSON.stringify(BODY)};
    return true;
  })()`);
  await cdp.evaluate(SCRIPT);
  await cdp.evaluate(`DirtUi.install({ tokens: ${JSON.stringify(TOKENS)}, platform: ${JSON.stringify(platform)} }); true`);
  await cdp.evaluate(SETUP, { awaitPromise: true });
  await sleep(100);
  await cdp.evaluate(set(true));
  await sleep(500);
  return browser;
}

const near = (a, b, within, what) => assert.ok(Math.abs(a - b) <= within, `${what}: ${a} is not within ${within} of ${b}`);

// Rust's part, by hand: a new item on the stage, as `dirt_ui::files::preview`
// draws one. `kind` is 'standin' (the thumbnail while the file is read),
// 'picture', or 'sound' (a sound's card).
const putItem = (file, kind, src = 'null') => `(() => {
  const item = document.createElement('div');
  item.className = 'dui-pv-item'; item.dataset.file = ${JSON.stringify(file)};
  if (${JSON.stringify(kind)} === 'sound') {
    item.innerHTML = '<div class="dui-pv-audio" data-dui-audio="data:audio/wav;base64,"></div>';
  } else {
    const img = document.createElement('img');
    img.className = ${JSON.stringify(kind)} === 'standin' ? 'dui-pv-picture dui-pv-standin' : 'dui-pv-picture';
    if (${JSON.stringify(kind)} === 'picture') img.dataset.duiZoom = '';
    img.alt = '';
    img.src = ${src};
    item.appendChild(img);
  }
  ${ITEM}.replaceWith(item);
  return true;
})()`;
// The deck Rust draws for these files, in this order.
const deckOf = files => `(() => {
  const deck = document.querySelector('.dui-pv-deck');
  deck.replaceChildren(...${JSON.stringify(files)}.map(file => {
    const card = document.createElement('div');
    card.className = 'dui-pv-stand'; card.dataset.file = file;
    card.innerHTML = '<img class="dui-pv-picture dui-pv-standin" alt="">';
    card.firstChild.src = pictures['t' + file];
    return card;
  }));
  return true;
})()`;
const press = async (cdp, key, code) => {
  await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key, code: key, windowsVirtualKeyCode: code });
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code: key, windowsVirtualKeyCode: code });
};

// Every frame, where each thing drawn in the track is, by its file, from
// the stage's left edge: the file, unless it waits undrawn, and what the
// ghost layer keeps.
const WATCH = `(() => {
  window.seen = [];
  let n = 0;
  const stage = document.querySelector('[data-dui-pv-stage]');
  const tick = () => {
    const left = stage.getBoundingClientRect().left;
    const track = stage.querySelector(':scope > .dui-pv-track');
    const item = track.querySelector(':scope > .dui-pv-item');
    const drawn = [...(item && getComputedStyle(item).visibility !== 'hidden' ? [item] : []), ...track.querySelectorAll(':scope > .dui-pv-ghost > *')];
    const frame = drawn.map(e => {
      if (!e.duiSeen) e.duiSeen = ++n;
      return { id: e.duiSeen, file: e.dataset.file, at: e.duiAt || 0, x: Math.round(e.getBoundingClientRect().left - left), o: Number(getComputedStyle(e).opacity) };
    });
    frame.track = new DOMMatrix(getComputedStyle(track).transform).e;
    seen.push({ track: frame.track, drawn: frame });
    if (window.watching) requestAnimationFrame(tick);
  };
  window.watching = true;
  requestAnimationFrame(tick);
  return true;
})()`;
// What moved wrongly in the frames watched: a thing that jumped (a file
// drawn back at its place for a frame), or one that appeared in sight
// where nothing of its file was along the track, in that frame or the one
// before, and where it would not have been out of sight the frame before
// (a copy put beside the file, a whole step away).
function jumps(frames, width) {
  const found = [];
  for (let k = 1; k < frames.length; k++) {
    const { drawn, track } = frames[k], earlier = frames[k - 1];
    const before = new Map(earlier.drawn.map(e => [e.id, e]));
    for (const e of drawn) {
      const was = before.get(e.id);
      if (was) {
        if (Math.abs(e.x - was.x) > 300) found.push(`frame ${k}: ${e.file} (#${e.id}) jumped from ${was.x} to ${e.x}`);
        continue;
      }
      if (Math.abs(e.x) >= width || Math.abs(e.at + earlier.track) >= width) continue;
      const beside = [...earlier.drawn, ...drawn.filter(other => other.id !== e.id)];
      if (!beside.some(other => other.file === e.file && Math.abs(other.at - e.at) <= 1)) found.push(`frame ${k}: ${e.file} (#${e.id}) appeared at ${e.x} (track ${track})`);
    }
  }
  return found;
}
async function watched(cdp) {
  await cdp.evaluate('window.watching = false; true');
  await sleep(40);
  return cdp.evaluate('seen');
}

test('the gesture maths', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await launchHeadless({ width: 400, height: 300 });
  try {
    const { cdp } = browser;
    await browser.goto('about:blank');
    await cdp.evaluate(SCRIPT);
    const run = body => cdp.evaluate(`(() => { const m = DirtUi.previewMaths; ${body} })()`);

    await t.test('a rubber band gives less the further it is pulled, never its room', async () => {
      const r = await run('return [0, 10, 100, 400, 4000, 1e6, -100].map(d => m.rubber(d, 400));');
      assert.equal(r[0], 0);
      assert.ok(r[1] > 0 && r[1] < 10);
      // Each further pull is drawn shorter than it is pulled.
      for (let k = 1; k < 5; k++) assert.ok(r[k + 1] > r[k] && r[k + 1] - r[k] < [90, 300, 3600, 996000][k - 1]);
      assert.ok(r[5] < 400);
      assert.equal(r[6], -r[2]);
    });

    await t.test('a scale pinched past its limits gives, and within them is itself', async () => {
      const r = await run('return [m.softScale(2), m.softScale(4), m.softScale(6), m.softScale(40), m.softScale(0.8), m.softScale(0.01)];');
      assert.equal(r[0], 2);
      assert.equal(r[1], 4);
      assert.ok(r[2] > 4 && r[2] < 6 && r[3] > r[2] && r[3] < 6);
      assert.ok(r[4] < 1 && r[4] > 0.8 && r[5] > 0.55);
    });

    await t.test('zooming about a point keeps that point where it is', async () => {
      const r = await run(`
        const c = { x: 500, y: 350 };
        const view = { s: 1.5, x: 30, y: -20 };
        const to = m.zoomAbout(view, 3, 640, 260, c);
        // The point of the picture under (640, 260), before and after.
        const local = v => ({ x: (640 - c.x - v.x) / v.s, y: (260 - c.y - v.y) / v.s });
        return { before: local(view), after: local(to), s: to.s };`);
      assert.equal(r.s, 3);
      near(r.after.x, r.before.x, 1e-9, 'x');
      near(r.after.y, r.before.y, 1e-9, 'y');
    });

    await t.test('a picture larger than the window moves until its edge meets the window', async () => {
      const [lo, hi] = await run('return m.panRange(500, 2000, 0, 1000);');
      assert.equal(lo, -500);
      assert.equal(hi, 500);
    });

    await t.test('a picture no larger than the window stays where it fits, inside the window', async () => {
      const r = await run('return [m.panRange(500, 800, 0, 1000), m.panRange(600, 880, 0, 1000)];');
      assert.deepEqual(r[0], [0, 0]);
      // Where it fits, its right edge would be past the window by 40 px.
      assert.deepEqual(r[1], [-40, -40]);
    });

    await t.test('at rest a view is within the limits, about the point named, and fits at 1×', async () => {
      const r = await run(`
        const g = { x: 500, y: 350, w: 600, h: 400, left: 0, top: 0, right: 1000, bottom: 700 };
        return {
          small: m.rest({ s: 0.6, x: 40, y: 30 }, g),
          large: m.rest({ s: 7, x: 0, y: 0 }, g, 500, 350),
          far: m.rest({ s: 2, x: 5000, y: -5000 }, g),
        };`);
      assert.deepEqual(r.small, { s: 1, x: 0, y: 0 });
      assert.deepEqual(r.large, { s: 4, x: 0, y: 0 });
      // 2×: 1200 × 800 in a 1000 × 700 window, about a middle at 500, 350.
      assert.deepEqual(r.far, { s: 2, x: 100, y: -50 });
    });

    await t.test('held past its edge it gives, and within its range it follows', async () => {
      const r = await run(`
        const g = { x: 500, y: 350, w: 600, h: 400, left: 0, top: 0, right: 1000, bottom: 700 };
        return [m.held({ s: 2, x: 60, y: 0 }, g), m.held({ s: 2, x: 300, y: 0 }, g)];`);
      assert.deepEqual(r[0], { s: 2, x: 60, y: 0 });
      assert.ok(r[1].x > 100 && r[1].x < 300, `pulled 200 px past, drawn at ${r[1].x}`);
    });

    await t.test('a fling coasts, and stops at the edge', async () => {
      const r = await run('return [m.coast(0, 1, [-500, 500]), m.coast(0, -4, [-500, 500]), m.coast(10, 0, [-500, 500])];');
      assert.deepEqual(r, [250, -500, 10]);
    });

    await t.test('a swipe steps when flung or past a third, and comes back otherwise', async () => {
      const r = await run(`return {
        fling: m.stepOf(-60, -0.9, 400, true),
        flingRight: m.stepOf(60, 0.9, 400, true),
        flungBack: m.stepOf(-200, 0.9, 400, true),
        tooShortFling: m.stepOf(-20, -1.2, 400, true),
        slowFar: m.stepOf(-150, -0.05, 400, true),
        slowShort: m.stepOf(-120, -0.05, 400, true),
        alone: m.stepOf(-300, -2, 400, false),
      };`);
      assert.deepEqual(r, { fling: 1, flingRight: -1, flungBack: 0, tooShortFling: 0, slowFar: 1, slowShort: 0, alone: 0 });
    });

    await t.test('a finger held still before it lets go has no speed', async () => {
      const r = await run(`
        const samples = [0, 16, 32, 48, 64, 80].map(t => ({ t, x: t * 2, y: -t }));
        return [m.speed(samples, 85), m.speed(samples, 200), m.speed([], 10)];`);
      assert.deepEqual(r[0], { x: 2, y: -1 });
      assert.deepEqual(r[1], { x: 0, y: 0 });
      assert.deepEqual(r[2], { x: 0, y: 0 });
    });
  } finally {
    await browser.stop();
  }
});


test('the desktop: a double click and the wheel zoom about the pointer; → slides the next file in', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('desktop');
  try {
    const { cdp } = browser;
    const box = await cdp.evaluate(`(() => { const b = ${ITEM}.querySelector('img').getBoundingClientRect(); return { left: b.left, top: b.top, width: b.width, height: b.height }; })()`);
    const at = { x: box.left + box.width / 2 + 60, y: box.top + box.height / 2 - 40 };
    const wheel = (deltaY, more = {}) => cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...at, deltaX: 0, deltaY, ...more });

    await t.test('a double click zooms to 2× about the point clicked, and back', async () => {
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...at });
      for (const clickCount of [1, 2]) {
        await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...at, button: 'left', buttons: 1, clickCount });
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...at, button: 'left', buttons: 0, clickCount });
      }
      await sleep(450);
      const view = await cdp.evaluate(VIEW);
      near(view.s, 2, 1e-6, 'scale');
      // The point clicked is 60, -40 from the middle; at 2× it is drawn
      // 120, -80 from it unless moved back by as much.
      near(view.x, -60, 0.5, 'x');
      near(view.y, 40, 0.5, 'y');
      assert.equal(view.animations, 0);
      const zoomed = await cdp.evaluate(`${ITEM}.querySelector('img').classList.contains('zoom')`);
      assert.equal(zoomed, true);
      for (const clickCount of [1, 2]) {
        await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...at, button: 'left', buttons: 1, clickCount });
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...at, button: 'left', buttons: 0, clickCount });
      }
      await sleep(450);
      const back = await cdp.evaluate(VIEW);
      assert.deepEqual({ s: back.s, x: back.x, y: back.y }, { s: 1, x: 0, y: 0 });
    });

    await t.test('the wheel alone zooms about the pointer, up to 4×, and back down to fit', async () => {
      // The point of the picture under the pointer, before and after.
      const under = view => ({ x: (at.x - (box.left + box.width / 2) - view.x) / view.s, y: (at.y - (box.top + box.height / 2) - view.y) / view.s });
      await wheel(-100);
      await sleep(50);
      const one = await cdp.evaluate(VIEW);
      near(one.s, Math.exp(0.25), 1e-4, 'one turn zooms in by about a quarter');
      // Once the picture is larger than the window both ways, the point
      // under the pointer stays under it.
      for (let turn = 0; turn < 2; turn++) await wheel(-100);
      await sleep(50);
      const three = await cdp.evaluate(VIEW);
      near(three.s, Math.exp(0.75), 1e-4, `three turns (${JSON.stringify([one, three])})`);
      near(under(three).x, under(one).x, 0.5, 'the point under the pointer stays under it (x)');
      near(under(three).y, under(one).y, 0.5, 'the point under the pointer stays under it (y)');
      for (let turn = 0; turn < 8; turn++) await wheel(-100);
      await sleep(50);
      near((await cdp.evaluate(VIEW)).s, 4, 1e-6, 'no further than 4×');
      for (let turn = 0; turn < 12; turn++) await wheel(100);
      await sleep(50);
      const fit = await cdp.evaluate(VIEW);
      assert.deepEqual({ s: fit.s, x: fit.x, y: fit.y }, { s: 1, x: 0, y: 0 });
    });

    await t.test('Ctrl and the wheel (a touchpad\'s pinch) zoom the same way', async () => {
      for (let turn = 0; turn < 3; turn++) await wheel(-100, { modifiers: 2 });
      await sleep(50);
      const view = await cdp.evaluate(VIEW);
      near(view.s, Math.exp(0.75), 1e-4, 'scale');
    });

    await t.test('the wheel does nothing else: turned sideways it neither moves nor zooms the picture', async () => {
      const before = await cdp.evaluate(VIEW);
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...at, deltaX: 300, deltaY: 0 });
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...at, deltaX: 300, deltaY: 0, modifiers: 8 });
      await sleep(50);
      assert.deepEqual(await cdp.evaluate(VIEW), before);
      for (let turn = 0; turn < 6; turn++) await wheel(100);
      await sleep(50);
      assert.equal((await cdp.evaluate(VIEW)).s, 1);
    });

    await t.test('→ tells Rust, and the track alone moves: the file shown out, a copy of the next one\'s thumbnail in', async () => {
      await cdp.evaluate(`document.querySelector('[data-dui-modal="preview"]').focus(); told.length = 0; true`);
      await cdp.evaluate(WATCH);
      await press(cdp, 'ArrowRight', 39);
      await sleep(30);
      const moving = await cdp.evaluate(`(() => {
        const stand = document.querySelector('.dui-pv-ghost > .dui-pv-stand[data-file="b"]');
        return {
          told: told.map(m => m.kind),
          stand: !!stand,
          at: stand && stand.duiAt === innerWidth + 16,
          track: ${TRACK}.getAnimations().length,
          others: [...${TRACK}.querySelectorAll('*')].filter(e => e.getAnimations().length).length,
        };
      })()`);
      assert.deepEqual(moving, { told: ['next'], stand: true, at: true, track: 1, others: 0 });
      // Rust's new item: the next file's stand-in, then its picture.
      await cdp.evaluate(putItem('b', 'standin', 'pictures.tb'));
      await sleep(60);
      const midway = await cdp.evaluate(`(() => ({
        ghost: [...document.querySelectorAll('.dui-pv-ghost > .dui-pv-item')].map(e => e.dataset.file + '@' + e.duiAt),
        x: ${TRACK_X},
        others: [...${TRACK}.querySelectorAll('*')].filter(e => e.getAnimations().length).length,
      }))()`);
      // What was shown is kept where it was along the track, which carries
      // it out.
      assert.deepEqual(midway.ghost, ['a@0']);
      assert.ok(midway.x < -5, `the track goes to the left (${midway.x})`);
      assert.equal(midway.others, 0, 'nothing in the track moves by itself');
      await cdp.evaluate(putItem('b', 'picture', 'pictures.b'));
      await sleep(900);
      const frames = await watched(cdp);
      assert.deepEqual(jumps(frames, 1000), []);
      const landed = await cdp.evaluate(`(() => {
        const item = ${ITEM};
        return {
          file: item.dataset.file,
          track: getComputedStyle(${TRACK}).transform,
          transform: getComputedStyle(item).transform,
          waiting: item.classList.contains('waiting'),
          ghosts: document.querySelector('.dui-pv-ghost').children.length,
          zoom: getComputedStyle(item.querySelector('img')).transform,
        };
      })()`);
      assert.deepEqual(landed, { file: 'b', track: 'none', transform: 'none', waiting: false, ghosts: 0, zoom: 'none' });
    });
  } finally {
    await browser.stop();
  }
});

test('the desktop: the slide between pictures (the owner\'s check of #336)', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('desktop');
  try {
    const { cdp } = browser;
    await cdp.evaluate(`document.querySelector('[data-dui-modal="preview"]').focus(); true`);
    const step = async (key, code, file, kind, src, after = 20) => {
      await press(cdp, key, code);
      await sleep(after);
      await cdp.evaluate(putItem(file, 'standin', `pictures.t${file}`));
      if (kind) { await sleep(20); await cdp.evaluate(putItem(file, kind, src)); }
    };

    await t.test('a large picture is not drawn while the track moves, and takes its thumbnail\'s place once it rests', async () => {
      await cdp.evaluate(WATCH);
      await press(cdp, 'ArrowRight', 39);
      await sleep(20);
      await cdp.evaluate(putItem('b', 'picture', 'pictures.large'));
      const during = [];
      for (let k = 0; k < 6; k++) {
        await sleep(30);
        during.push(await cdp.evaluate(`({ x: ${TRACK_X}, waiting: ${ITEM}.classList.contains('waiting') })`));
      }
      const moving = during.filter(d => Math.abs(d.x) > 0.5);
      assert.ok(moving.length > 2, `the track moved: ${JSON.stringify(during)}`);
      assert.ok(moving.every(d => d.waiting), `undrawn while it moves: ${JSON.stringify(during)}`);
      await sleep(700);
      const rest = await cdp.evaluate(`({ waiting: ${ITEM}.classList.contains('waiting'), ghosts: document.querySelector('.dui-pv-ghost').children.length, box: ${ITEM}.querySelector('img').getBoundingClientRect().width })`);
      assert.equal(rest.waiting, false);
      assert.equal(rest.ghosts, 0);
      // Drawn as large as fits, as its thumbnail stood in.
      assert.ok(rest.box > 800, `drawn as large as fits (${rest.box})`);
      assert.deepEqual(jumps(await watched(cdp), 1000), []);
      await step('ArrowLeft', 37, 'a', 'picture', 'pictures.a');
      await sleep(700);
    });

    await t.test('a picture Rust leaves to the script to ask for is asked for once the track rests', async () => {
      await press(cdp, 'ArrowRight', 39);
      await sleep(20);
      await cdp.evaluate(`(() => {
        const item = document.createElement('div');
        item.className = 'dui-pv-item'; item.dataset.file = 'b';
        item.innerHTML = '<img class="dui-pv-picture" data-dui-zoom alt="">';
        item.firstChild.dataset.duiSrc = pictures.large;
        ${ITEM}.replaceWith(item);
        return true;
      })()`);
      const during = [];
      for (let k = 0; k < 5; k++) {
        await sleep(30);
        during.push(await cdp.evaluate(`({ x: ${TRACK_X}, asked: ${ITEM}.querySelector('img').hasAttribute('src') })`));
      }
      const moving = during.filter(d => Math.abs(d.x) > 0.5);
      assert.ok(moving.length > 2 && moving.every(d => !d.asked), `not asked for while the track moves: ${JSON.stringify(during)}`);
      await sleep(800);
      const rest = await cdp.evaluate(`({ asked: ${ITEM}.querySelector('img').getAttribute('src') === pictures.large, waiting: ${ITEM}.classList.contains('waiting'), ghosts: document.querySelector('.dui-pv-ghost').children.length })`);
      assert.deepEqual(rest, { asked: true, waiting: false, ghosts: 0 });
      await step('ArrowLeft', 37, 'a', 'picture', 'pictures.a');
      await sleep(700);
    });

    await t.test('a small picture slides in drawn once it is decoded, before the track rests', async () => {
      await press(cdp, 'ArrowRight', 39);
      await sleep(20);
      await cdp.evaluate(putItem('b', 'picture', 'pictures.b'));
      let drawnMoving = false;
      for (let k = 0; k < 8 && !drawnMoving; k++) {
        await sleep(20);
        const now = await cdp.evaluate(`({ x: ${TRACK_X}, waiting: ${ITEM}.classList.contains('waiting') })`);
        drawnMoving = !now.waiting && Math.abs(now.x) > 0.5;
      }
      assert.ok(drawnMoving, 'drawn while the track still moved');
      await sleep(600);
      await step('ArrowLeft', 37, 'a', 'picture', 'pictures.a');
      await sleep(700);
    });

    await t.test('steps one after another, each before the last has landed: nothing is drawn back at its place', async () => {
      await cdp.evaluate(deckOf(['a', 'b', 'c']));
      await cdp.evaluate(`told.length = 0; true`);
      await cdp.evaluate(WATCH);
      await step('ArrowRight', 39, 'b', 'picture', 'pictures.b', 30);
      await sleep(40);
      await step('ArrowRight', 39, 'c', null, null, 30);
      await sleep(40);
      await press(cdp, 'ArrowRight', 39);
      await sleep(20);
      await cdp.evaluate(putItem('c', 'picture', 'pictures.c'));
      await sleep(30);
      await cdp.evaluate(putItem('a', 'standin', 'pictures.ta'));
      await sleep(30);
      await cdp.evaluate(putItem('a', 'picture', 'pictures.a'));
      await sleep(900);
      const frames = await watched(cdp);
      assert.deepEqual(jumps(frames, 1000), []);
      // Each file went one way only: to the left.
      const byId = new Map();
      for (const frame of frames) for (const e of frame.drawn) byId.set(e.id, [...(byId.get(e.id) || []), e.x]);
      for (const [id, xs] of byId) {
        for (let k = 1; k < xs.length; k++) assert.ok(xs[k] <= xs[k - 1] + 1, `#${id} went back: ${xs.join(' ')}`);
      }
      const rest = await cdp.evaluate(`({ told: told.map(m => m.kind), file: ${ITEM}.dataset.file, waiting: ${ITEM}.classList.contains('waiting'), x: ${TRACK_X}, ghosts: document.querySelector('.dui-pv-ghost').children.length })`);
      assert.deepEqual(rest, { told: ['next', 'next', 'next'], file: 'a', waiting: false, x: 0, ghosts: 0 });
    });

    await t.test('a step, and back before it has landed: the file comes back from where it is drawn', async () => {
      await cdp.evaluate(WATCH);
      await press(cdp, 'ArrowRight', 39);
      await sleep(100);
      await press(cdp, 'ArrowLeft', 37);
      await sleep(20);
      await cdp.evaluate(putItem('b', 'standin', 'pictures.tb'));
      await sleep(30);
      await cdp.evaluate(putItem('a', 'standin', 'pictures.ta'));
      // While it reads the picture, the file itself is still drawn in its
      // place: its thumbnail is not put over it.
      await sleep(30);
      assert.equal(await cdp.evaluate(`${ITEM}.classList.contains('waiting')`), true);
      await cdp.evaluate(putItem('a', 'picture', 'pictures.a'));
      await sleep(900);
      assert.deepEqual(jumps(await watched(cdp), 1000), []);
      const rest = await cdp.evaluate(`({ file: ${ITEM}.dataset.file, waiting: ${ITEM}.classList.contains('waiting'), x: ${TRACK_X}, ghosts: document.querySelector('.dui-pv-ghost').children.length })`);
      assert.deepEqual(rest, { file: 'a', waiting: false, x: 0, ghosts: 0 });
    });

    await t.test('a picture smaller than a thumbnail is stood in for at its own size, and nothing moves as it comes', async () => {
      await cdp.evaluate(deckOf(['a', 'b']));
      await cdp.evaluate(putItem('a', 'standin', 'pictures.small'));
      await sleep(200);
      const stand = await cdp.evaluate(`(() => { const img = ${ITEM}.querySelector('img'); const b = img.getBoundingClientRect(); return { own: img.classList.contains('own'), w: b.width, h: b.height }; })()`);
      assert.deepEqual(stand, { own: true, w: 200, h: 130 });
      await cdp.evaluate(putItem('a', 'picture', 'pictures.small'));
      await sleep(20);
      const moving = await cdp.evaluate(`[...document.querySelectorAll('[data-dui-pv-stage] *')].flatMap(e => e.getAnimations()).filter(run => run.effect.getKeyframes().some(f => f.transform && f.transform !== 'none')).length`);
      assert.equal(moving, 0, 'no movement as the picture takes over');
      await sleep(400);
      const rest = await cdp.evaluate(`(() => { const b = ${ITEM}.querySelector('img').getBoundingClientRect(); return { w: b.width, h: b.height, ghosts: document.querySelector('.dui-pv-ghost').children.length }; })()`);
      assert.deepEqual(rest, { w: 200, h: 130, ghosts: 0 });
    });
  } finally {
    await browser.stop();
  }
});

test('Android: a pinch follows the fingers and settles within 4×; a swipe steps or comes back', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('android');
  try {
    const { cdp } = browser;
    const mid = await cdp.evaluate(`(() => { const b = ${ITEM}.querySelector('img').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
    const touches = (apartPx, dx = 0) => [
      { x: mid.x + dx - apartPx / 2, y: mid.y, id: 0 },
      { x: mid.x + dx + apartPx / 2, y: mid.y, id: 1 },
    ];
    async function pinch(from, to, dx = 0) {
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: touches(from, dx) });
      let last;
      for (let k = 1; k <= 10; k++) {
        last = touches(from + ((to - from) * k) / 10, dx);
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: last });
        await sleep(16);
      }
      return last;
    }
    async function lift(last) {
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [last[0]] });
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    }

    await t.test('two fingers spreading zoom as far as they spread, about the point between them', async () => {
      const last = await pinch(100, 250, 40);
      const held = await cdp.evaluate(VIEW);
      near(held.s, 2.5, 1e-6, 'scale while held');
      await lift(last);
      await sleep(450);
      const rest = await cdp.evaluate(VIEW);
      near(rest.s, 2.5, 1e-6, 'scale at rest');
      // About the point 40 px right of the middle: moved back by 1.5 × 40.
      near(rest.x, -60, 0.5, 'x');
    });

    await t.test('pinched past 4× it gives, and settles back to 4×', async () => {
      const last = await pinch(100, 400, 40);
      const held = await cdp.evaluate(VIEW);
      assert.ok(held.s > 4, `drawn past 4× while held (${held.s})`);
      assert.ok(held.s < 10, `with a give, not all the way (${held.s})`);
      await lift(last);
      await sleep(450);
      near((await cdp.evaluate(VIEW)).s, 4, 1e-6, 'scale at rest');
    });

    await t.test('a double tap brings it back to fit', async () => {
      for (let tap = 0; tap < 2; tap++) {
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: mid.x, y: mid.y, id: 0 }] });
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
        await sleep(90);
      }
      await sleep(450);
      const view = await cdp.evaluate(VIEW);
      assert.deepEqual({ s: view.s, x: view.x, y: view.y }, { s: 1, x: 0, y: 0 });
    });

    async function swipe(dx, steps, every, hold) {
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: mid.x, y: mid.y, id: 0 }] });
      for (let k = 1; k <= steps; k++) {
        await sleep(every);
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: mid.x + (dx * k) / steps, y: mid.y, id: 0 }] });
      }
      if (hold) await sleep(hold);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    }

    await t.test('a short drag, held still, comes back and does not step', async () => {
      await cdp.evaluate('told.length = 0; true');
      await swipe(-90, 8, 20, 150);
      await sleep(500);
      const r = await cdp.evaluate(`(() => ({
        told: told.map(m => m.kind),
        transform: getComputedStyle(${TRACK}).transform,
        ghosts: document.querySelector('.dui-pv-ghost').children.length,
      }))()`);
      assert.deepEqual(r, { told: [], transform: 'none', ghosts: 0 });
    });

    await t.test('a fling to the left steps to the next file', async () => {
      await cdp.evaluate('told.length = 0; true');
      await swipe(-120, 6, 16, 0);
      await sleep(60);
      assert.deepEqual(await cdp.evaluate('told.map(m => m.kind)'), ['next']);
    });
  } finally {
    await browser.stop();
  }
});

test('the desktop: what the review of #336 found', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('desktop');
  try {
    const { cdp } = browser;
    await cdp.evaluate(`document.querySelector('[data-dui-modal="preview"]').focus(); true`);

    await t.test('a thumbnail kept for a picture not decoded yet leaves with a step, and is not left over the next file', async () => {
      // A picture whose bytes never come: the request is held.
      await cdp.send('Fetch.enable', { patterns: [{ urlPattern: '*slow.invalid*' }] });
      await cdp.evaluate(putItem('a', 'standin', 'pictures.ta'));
      await sleep(400);
      await cdp.evaluate(putItem('a', 'picture', "'http://slow.invalid/a.png'"));
      await sleep(100);
      const waiting = await cdp.evaluate(`(() => ({
        ghosts: document.querySelectorAll('.dui-pv-ghost > .dui-pv-item').length,
        waiting: ${ITEM}.classList.contains('waiting'),
      }))()`);
      assert.deepEqual(waiting, { ghosts: 1, waiting: true });
      await press(cdp, 'ArrowRight', 39);
      await sleep(60);
      await cdp.evaluate(putItem('b', 'standin', 'pictures.tb'));
      await sleep(100);
      await cdp.evaluate(putItem('b', 'picture', 'pictures.b'));
      await sleep(900);
      const after = await cdp.evaluate(`(() => ({
        ghosts: document.querySelector('.dui-pv-ghost').children.length,
        file: ${ITEM}.dataset.file,
      }))()`);
      assert.deepEqual(after, { ghosts: 0, file: 'b' });
    });

    await t.test('and one whose file is replaced by another without a step goes at once', async () => {
      await cdp.evaluate(putItem('b', 'standin', 'pictures.tb'));
      await sleep(400);
      await cdp.evaluate(putItem('b', 'picture', "'http://slow.invalid/b2.png'"));
      await sleep(100);
      await cdp.evaluate(putItem('a', 'picture', 'pictures.a'));
      await sleep(100);
      assert.equal(await cdp.evaluate(`document.querySelectorAll('.dui-pv-ghost > [data-file="b"]').length`), 0);
      await cdp.send('Fetch.disable');
      await sleep(500);
    });

    await t.test('a sound that slides out is not played or read again from the ghost layer', async () => {
      await cdp.evaluate(putItem('snd', 'sound'));
      await sleep(300);
      assert.equal(await cdp.evaluate(`!!${ITEM}.querySelector('[data-dui-audio]')._duiSound`), true);
      await press(cdp, 'ArrowRight', 39);
      await sleep(60);
      await cdp.evaluate(putItem('b', 'picture', 'pictures.b'));
      await sleep(60);
      const kept = await cdp.evaluate(`(() => {
        const card = document.querySelector('.dui-pv-ghost [data-dui-audio]');
        return { kept: !!card, sound: card ? card._duiSound : 'none' };
      })()`);
      assert.deepEqual(kept, { kept: true, sound: null });
      await sleep(900);
    });

    await t.test('a zoomed picture is kept within its edges when the window changes size', async () => {
      await cdp.evaluate(putItem('a', 'picture', 'pictures.a'));
      await sleep(400);
      const box = await cdp.evaluate(`(() => { const b = ${ITEM}.querySelector('img').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.bottom - 4 }; })()`);
      for (let turn = 0; turn < 8; turn++) await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...box, deltaX: 0, deltaY: -200 });
      await sleep(100);
      await cdp.send('Emulation.setDeviceMetricsOverride', { width: 700, height: 500, deviceScaleFactor: 1, mobile: false });
      await sleep(300);
      const edges = await cdp.evaluate(`(() => { const b = ${ITEM}.querySelector('img').getBoundingClientRect(); return { top: b.top, bottom: b.bottom, height: innerHeight }; })()`);
      assert.ok(edges.top <= 0.5 && edges.bottom >= edges.height - 0.5, `the picture covers the window top to bottom: ${JSON.stringify(edges)}`);
      await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1000, height: 700, deviceScaleFactor: 1, mobile: false });
    });
  } finally {
    await browser.stop();
  }
});

test('Android: what the review of #336 found', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('android');
  try {
    const { cdp } = browser;
    const mid = await cdp.evaluate(`(() => { const b = ${ITEM}.querySelector('img').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
    const finger = (x, y, id) => ({ x, y, id });

    await t.test('a third finger during a pinch past 4× settles it back to 4×', async () => {
      let two = [finger(mid.x - 50, mid.y, 0), finger(mid.x + 50, mid.y, 1)];
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: two });
      for (let k = 1; k <= 10; k++) {
        two = [finger(mid.x - 50 - 20 * k, mid.y, 0), finger(mid.x + 50 + 20 * k, mid.y, 1)];
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: two });
        await sleep(16);
      }
      assert.ok((await cdp.evaluate(VIEW)).s > 4, 'drawn past 4× while held');
      const three = [...two, finger(mid.x, mid.y + 120, 2)];
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: three });
      await sleep(450);
      near((await cdp.evaluate(VIEW)).s, 4, 1e-6, 'settled while the fingers are still down');
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchCancel', touchPoints: [] });
      await sleep(450);
      near((await cdp.evaluate(VIEW)).s, 4, 1e-6, 'and after they are gone');
      for (let tap = 0; tap < 2; tap++) {
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [finger(mid.x, mid.y, 0)] });
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
        await sleep(90);
      }
      await sleep(450);
      assert.equal((await cdp.evaluate(VIEW)).s, 1);
    });

    await t.test('a file taken again as it slides back follows the finger from where it is drawn', async () => {
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [finger(mid.x, mid.y, 0)] });
      for (let k = 1; k <= 8; k++) {
        await sleep(20);
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [finger(mid.x - 12 * k, mid.y, 0)] });
      }
      await sleep(150);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await sleep(80);
      const back = await cdp.evaluate(TRACK_X);
      assert.ok(back < -5 && back > -96, `sliding back (${back})`);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [finger(mid.x, mid.y, 0)] });
      for (let k = 1; k <= 4; k++) {
        await sleep(16);
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [finger(mid.x - 10 * k, mid.y, 0)] });
      }
      await sleep(50);
      const held = await cdp.evaluate(`({ x: ${TRACK_X}, next: !!document.querySelector('.dui-pv-ghost > .dui-pv-stand[data-file="b"]') })`);
      // It went on sliding back for a few ms between the two readings.
      assert.ok(held.x < back - 25 && held.x > back - 60, `held at ${held.x}, 40 px on from about ${back}`);
      assert.equal(held.next, true);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await sleep(600);
      assert.equal(await cdp.evaluate(TRACK_X), 0);
    });
  } finally {
    await browser.stop();
  }
});

// Where things are, as the tests below read them: the track, what the
// ghost layer keeps (beside the file, or for it), and what Rust was told.
const SLIDE = () => `({
  x: ${TRACK_X},
  ghosts: document.querySelector('.dui-pv-ghost').children.length,
  told: told.map(m => m.kind),
})`;

test('Android: what the second review of #336 found, and dragging what stands in for a picture', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('android');
  try {
    const { cdp } = browser;
    const mid = await cdp.evaluate(`(() => { const b = document.querySelector('[data-dui-pv-stage]').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
    const finger = (x, y, id = 0) => ({ x, y, id });
    const down = (...points) => cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: points });
    const move = (...points) => cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: points });
    const up = () => cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    async function drag(dx, dy, steps, every, hold) {
      await down(finger(mid.x, mid.y));
      for (let k = 1; k <= steps; k++) {
        await sleep(every);
        await move(finger(mid.x + (dx * k) / steps, mid.y + (dy * k) / steps));
      }
      if (hold) await sleep(hold);
      await up();
    }
    const leftOf = file => `(() => {
      const stage = document.querySelector('[data-dui-pv-stage]').getBoundingClientRect();
      const track = ${TRACK};
      const item = track.querySelector(':scope > .dui-pv-item');
      const all = [...(item && getComputedStyle(item).visibility !== 'hidden' ? [item] : []), ...track.querySelectorAll(':scope > .dui-pv-ghost > *')];
      return all.filter(e => e.dataset.file === ${JSON.stringify(file)}).map(e => Math.round(e.getBoundingClientRect().left - stage.left));
    })()`;

    await t.test('what stands in for a picture, dragged as the picture comes, stays under the finger with it', async () => {
      await cdp.evaluate(putItem('a', 'standin', 'pictures.ta'));
      await sleep(300);
      await down(finger(mid.x, mid.y));
      for (let k = 1; k <= 6; k++) { await sleep(16); await move(finger(mid.x - 10 * k, mid.y)); }
      await cdp.evaluate(putItem('a', 'picture', 'pictures.a'));
      await sleep(60);
      const held = await cdp.evaluate(leftOf('a'));
      assert.ok(held.length && held.every(x => x <= -40 && x >= -62), `drawn where the finger has it, not in the middle: ${held}`);
      await up();
      await sleep(600);
      assert.deepEqual(await cdp.evaluate(SLIDE()), { x: 0, ghosts: 0, told: [] });
    });

    await t.test('a second flick takes the file still coming in from a step from where it is drawn', async () => {
      await cdp.evaluate(putItem('a', 'picture', 'pictures.a'));
      await sleep(400);
      await cdp.evaluate('told.length = 0; true');
      await drag(-120, 0, 6, 16, 0);
      await sleep(30);
      await cdp.evaluate(putItem('b', 'standin', 'pictures.tb'));
      await sleep(40);
      await down(finger(mid.x, mid.y));
      for (let k = 1; k <= 2; k++) { await sleep(16); await move(finger(mid.x - 10 * k, mid.y)); }
      await sleep(30);
      const held = await cdp.evaluate(leftOf('b'));
      assert.deepEqual(await cdp.evaluate('told.map(m => m.kind)'), ['next']);
      assert.ok(held.length && held.every(x => x > 60), `held where it was coming in from the right, not snapped to rest (${held})`);
      // Flung on to the left: it comes the rest of its way in.
      for (let k = 3; k <= 8; k++) { await sleep(16); await move(finger(mid.x - 30 * k, mid.y)); }
      await up();
      await sleep(600);
      assert.deepEqual(await cdp.evaluate(SLIDE()), { x: 0, ghosts: 0, told: ['next'] });
      assert.deepEqual(await cdp.evaluate(leftOf('b')), [0]);
    });

    // A file taken again as it slides back, by a finger that then does
    // something else than drag it sideways.
    async function slidingBack() {
      await drag(-90, 0, 8, 20, 150);
      await sleep(60);
      const x = await cdp.evaluate(TRACK_X);
      assert.ok(x < -5, `sliding back (${x})`);
      await down(finger(mid.x, mid.y));
    }

    await t.test('taken as it slides back and then moved down, it goes back in place and nothing closes', async () => {
      await cdp.evaluate(putItem('b', 'picture', 'pictures.b'));
      await sleep(400);
      await cdp.evaluate('told.length = 0; true');
      await slidingBack();
      for (let k = 1; k <= 6; k++) { await sleep(16); await move(finger(mid.x, mid.y + 25 * k)); }
      await up();
      await sleep(600);
      assert.deepEqual(await cdp.evaluate(SLIDE()), { x: 0, ghosts: 0, told: [] });
    });

    await t.test('taken as it slides back and then a second finger comes, it goes back in place', async () => {
      await slidingBack();
      await down(finger(mid.x, mid.y, 0), finger(mid.x + 80, mid.y, 1));
      await sleep(40);
      await move(finger(mid.x - 5, mid.y, 0), finger(mid.x + 85, mid.y, 1));
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [finger(mid.x - 5, mid.y, 0)] });
      await up();
      await sleep(600);
      const rest = await cdp.evaluate(SLIDE());
      assert.deepEqual({ x: rest.x, ghosts: rest.ghosts }, { x: 0, ghosts: 0 });
      for (let tap = 0; tap < 2; tap++) { await down(finger(mid.x, mid.y)); await up(); await sleep(90); }
      await sleep(500);
    });

    await t.test('taken as it slides back on text that scrolls, and scrolled, it goes back in place', async () => {
      await cdp.evaluate(`(() => {
        const item = document.createElement('div');
        item.className = 'dui-pv-item'; item.dataset.file = 'txt';
        item.innerHTML = '<div class="dui-scroll" data-dui-scroll style="width:100%;height:100%;overflow:auto"><pre style="height:3000px;margin:0">text</pre></div>';
        ${ITEM}.replaceWith(item);
        return true;
      })()`);
      await sleep(400);
      await cdp.evaluate('told.length = 0; true');
      await slidingBack();
      for (let k = 1; k <= 6; k++) { await sleep(16); await move(finger(mid.x, mid.y - 25 * k)); }
      await up();
      await sleep(600);
      assert.deepEqual(await cdp.evaluate(SLIDE()), { x: 0, ghosts: 0, told: [] });
    });
  } finally {
    await browser.stop();
  }
});
