// node --test crates/dirt-ui/tests/js/preview.test.mjs
//
// The file preview's gestures (interface design 6.16, A83), in dirt-ui's
// script with its style sheet, in a real headless Edge or Chrome: the
// maths it zooms, moves and steps by (`DirtUi.previewMaths`), and the
// preview itself zoomed by a double click, Ctrl and the wheel and a pinch,
// stepped by → and by a swipe, with Rust's part (a new item on the stage)
// done by hand. Skipped where neither browser is installed.

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
// closed. The pictures are drawn in the page.
const BODY = `
  <div class="dui-preview" data-dui-modal="preview" data-dui-id="7" tabindex="-1" inert>
    <div class="dui-pv-scrim"></div>
    <div class="dui-pv-stage" data-dui-pv-stage>
      <div class="dui-pv-item" data-file="a"><img class="dui-pv-picture" data-dui-zoom alt="a" draggable="false"></div>
      <div class="dui-pv-ghost" aria-hidden="true" inert></div>
      <div class="dui-pv-peek" data-side="prev" data-file="b" aria-hidden="true"><img class="dui-pv-picture dui-pv-standin" alt=""></div>
      <div class="dui-pv-peek" data-side="next" data-file="b" aria-hidden="true"><img class="dui-pv-picture dui-pv-standin" alt=""></div>
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
  window.pictures = { a: picture(800, 500, '#6b8f71'), b: picture(400, 640, '#8a6f9e') };
  document.querySelector('.dui-pv-item img').src = pictures.a;
  document.querySelectorAll('.dui-pv-peek img').forEach(img => { img.src = pictures.b; });
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

// The picture's view as drawn, once its movement has ended.
const VIEW = `(() => {
  const img = document.querySelector('[data-dui-pv-stage] > .dui-pv-item img');
  const t = getComputedStyle(img).transform;
  const m = t === 'none' ? new DOMMatrix() : new DOMMatrix(t);
  return { s: m.a, x: m.e, y: m.f, animations: img.getAnimations().length };
})()`;

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

test('the desktop: a double click and Ctrl with the wheel zoom about the pointer; → slides the next file in', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('desktop');
  try {
    const { cdp } = browser;
    const box = await cdp.evaluate(`(() => { const b = document.querySelector('.dui-pv-item img').getBoundingClientRect(); return { left: b.left, top: b.top, width: b.width, height: b.height }; })()`);
    const at = { x: box.left + box.width / 2 + 60, y: box.top + box.height / 2 - 40 };

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
      const zoomed = await cdp.evaluate(`document.querySelector('.dui-pv-item img').classList.contains('zoom')`);
      assert.equal(zoomed, true);
    });

    await t.test('Ctrl and the wheel zoom on, up to 4×', async () => {
      for (let turn = 0; turn < 8; turn++) {
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...at, deltaX: 0, deltaY: -200, modifiers: 2 });
      }
      await sleep(100);
      const view = await cdp.evaluate(VIEW);
      near(view.s, 4, 1e-6, 'scale');
    });

    await t.test('the wheel alone moves a zoomed picture, as far as its edge', async () => {
      const before = await cdp.evaluate(VIEW);
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...at, deltaX: 0, deltaY: 100000 });
      await sleep(100);
      const after = await cdp.evaluate(VIEW);
      assert.ok(after.y < before.y, 'moved up');
      const edge = await cdp.evaluate(`document.querySelector('.dui-pv-item img').getBoundingClientRect().bottom`);
      near(edge, 700, 0.5, 'its bottom edge on the window\'s');
    });

    await t.test('a double click on a zoomed picture brings it back to fit', async () => {
      for (const clickCount of [1, 2]) {
        await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...at, button: 'left', buttons: 1, clickCount });
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...at, button: 'left', buttons: 0, clickCount });
      }
      await sleep(450);
      const view = await cdp.evaluate(VIEW);
      assert.deepEqual({ s: view.s, x: view.x, y: view.y }, { s: 1, x: 0, y: 0 });
    });

    await t.test('→ tells Rust, and the file shown slides out as the next slides in', async () => {
      await cdp.evaluate(`document.querySelector('[data-dui-modal="preview"]').focus(); told.length = 0; true`);
      await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'ArrowRight', code: 'ArrowRight', windowsVirtualKeyCode: 39 });
      await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'ArrowRight', code: 'ArrowRight', windowsVirtualKeyCode: 39 });
      await sleep(60);
      const moving = await cdp.evaluate(`(() => ({
        told: told.map(m => m.kind),
        peek: !!document.querySelector('.dui-pv-ghost > .dui-pv-peek.on[data-file="b"]'),
        leaving: document.querySelector('[data-dui-pv-stage] > .dui-pv-item').getAnimations().length,
      }))()`);
      assert.deepEqual(moving, { told: ['next'], peek: true, leaving: 1 });
      // Rust's new item: the next file's stand-in, then its picture.
      await cdp.evaluate(`(() => {
        const stage = document.querySelector('[data-dui-pv-stage]');
        const item = document.createElement('div');
        item.className = 'dui-pv-item'; item.dataset.file = 'b';
        item.innerHTML = '<img class="dui-pv-picture dui-pv-standin" alt="">';
        item.firstChild.src = pictures.b;
        stage.querySelector(':scope > .dui-pv-item').replaceWith(item);
        // Rust draws the peeks again for the new neighbours.
        stage.querySelectorAll(':scope > .dui-pv-peek').forEach(peek => { peek.dataset.file = 'a'; peek.querySelector('img').src = pictures.a; });
        return true;
      })()`);
      await sleep(80);
      const midway = await cdp.evaluate(`(() => ({
        ghost: [...document.querySelectorAll('.dui-pv-ghost > .dui-pv-item')].map(e => e.dataset.file),
        stands: [...document.querySelectorAll('.dui-pv-ghost > .dui-pv-peek')].map(e => e.dataset.file),
        coming: document.querySelector('[data-dui-pv-stage] > .dui-pv-item').getAnimations().length > 0,
        x: new DOMMatrix(getComputedStyle(document.querySelector('[data-dui-pv-stage] > .dui-pv-item')).transform).e,
      }))()`);
      assert.deepEqual(midway.ghost, ['a']);
      // What slid in stays the file stepped to, or is gone once it came.
      assert.ok(midway.stands.every(file => file === 'b'), `stand-ins: ${midway.stands}`);
      assert.equal(midway.coming, true);
      assert.ok(midway.x > 0, `the next file comes from the right (${midway.x})`);
      await cdp.evaluate(`(() => {
        const item = document.createElement('div');
        item.className = 'dui-pv-item'; item.dataset.file = 'b';
        item.innerHTML = '<img class="dui-pv-picture" data-dui-zoom alt="b">';
        item.firstChild.src = pictures.b;
        document.querySelector('[data-dui-pv-stage] > .dui-pv-item').replaceWith(item);
        return true;
      })()`);
      await sleep(900);
      const landed = await cdp.evaluate(`(() => {
        const item = document.querySelector('[data-dui-pv-stage] > .dui-pv-item');
        return {
          file: item.dataset.file,
          transform: getComputedStyle(item).transform,
          waiting: item.classList.contains('waiting'),
          ghosts: document.querySelector('.dui-pv-ghost').children.length,
          peeks: document.querySelectorAll('.dui-pv-peek.on').length,
          zoom: getComputedStyle(item.querySelector('img')).transform,
        };
      })()`);
      assert.deepEqual(landed, { file: 'b', transform: 'none', waiting: false, ghosts: 0, peeks: 0, zoom: 'none' });
    });
  } finally {
    await browser.stop();
  }
});

test('Android: a pinch follows the fingers and settles within 4×; a swipe steps or comes back', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('android');
  try {
    const { cdp } = browser;
    const mid = await cdp.evaluate(`(() => { const b = document.querySelector('.dui-pv-item img').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
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
        transform: getComputedStyle(document.querySelector('[data-dui-pv-stage] > .dui-pv-item')).transform,
        peeks: document.querySelectorAll('.dui-pv-peek.on').length,
      }))()`);
      assert.deepEqual(r, { told: [], transform: 'none', peeks: 0 });
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
  document.querySelector('[data-dui-pv-stage] > .dui-pv-item').replaceWith(item);
  return true;
})()`;
const press = async (cdp, key, code) => {
  await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key, code: key, windowsVirtualKeyCode: code });
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code: key, windowsVirtualKeyCode: code });
};
const ITEM_X = `new DOMMatrix(getComputedStyle(document.querySelector('[data-dui-pv-stage] > .dui-pv-item')).transform).e`;

test('the desktop: what the review of #336 found', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('desktop');
  try {
    const { cdp } = browser;
    await cdp.evaluate(`document.querySelector('[data-dui-modal="preview"]').focus(); true`);

    await t.test('a thumbnail kept for a picture not decoded yet leaves with a step, and is not left over the next file', async () => {
      // A picture whose bytes never come: the request is held.
      await cdp.send('Fetch.enable', { patterns: [{ urlPattern: '*slow.invalid*' }] });
      await cdp.evaluate(putItem('a', 'standin', 'pictures.a'));
      await sleep(400);
      await cdp.evaluate(putItem('a', 'picture', "'http://slow.invalid/a.png'"));
      await sleep(100);
      const waiting = await cdp.evaluate(`(() => ({
        ghosts: document.querySelectorAll('.dui-pv-ghost > .dui-pv-item').length,
        waiting: document.querySelector('[data-dui-pv-stage] > .dui-pv-item').classList.contains('waiting'),
      }))()`);
      assert.deepEqual(waiting, { ghosts: 1, waiting: true });
      await press(cdp, 'ArrowRight', 39);
      await sleep(60);
      await cdp.evaluate(putItem('b', 'standin', 'pictures.b'));
      await sleep(100);
      await cdp.evaluate(putItem('b', 'picture', 'pictures.b'));
      await sleep(900);
      const after = await cdp.evaluate(`(() => ({
        ghosts: document.querySelector('.dui-pv-ghost').children.length,
        file: document.querySelector('[data-dui-pv-stage] > .dui-pv-item').dataset.file,
      }))()`);
      assert.deepEqual(after, { ghosts: 0, file: 'b' });
    });

    await t.test('and one whose picture is replaced without a step goes at once', async () => {
      await cdp.evaluate(putItem('a', 'standin', 'pictures.a'));
      await sleep(400);
      await cdp.evaluate(putItem('a', 'picture', "'http://slow.invalid/a2.png'"));
      await sleep(100);
      await cdp.evaluate(putItem('b', 'picture', 'pictures.b'));
      await sleep(100);
      assert.equal(await cdp.evaluate(`document.querySelectorAll('.dui-pv-ghost > .dui-pv-item[data-file="a"]').length`), 0);
      await cdp.send('Fetch.disable');
      await sleep(500);
    });

    await t.test('a sound that slides out is not played or read again from the ghost layer', async () => {
      await cdp.evaluate(putItem('snd', 'sound'));
      await sleep(300);
      assert.equal(await cdp.evaluate(`!!document.querySelector('[data-dui-pv-stage] > .dui-pv-item [data-dui-audio]')._duiSound`), true);
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

    const box = await cdp.evaluate(`(() => { const b = document.querySelector('[data-dui-pv-stage] > .dui-pv-item img').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
    const doubleClick = async () => {
      for (const clickCount of [1, 2]) {
        await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...box, button: 'left', buttons: 1, clickCount });
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...box, button: 'left', buttons: 0, clickCount });
      }
    };

    await t.test('the wheel does not stop a zoom on its way back to fit', async () => {
      await doubleClick();
      await sleep(450);
      near((await cdp.evaluate(VIEW)).s, 2, 1e-6, 'zoomed');
      await doubleClick();
      await sleep(60);
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...box, deltaX: 0, deltaY: 40 });
      await sleep(450);
      const view = await cdp.evaluate(VIEW);
      assert.deepEqual({ s: view.s, x: view.x, y: view.y }, { s: 1, x: 0, y: 0 });
    });

    await t.test('a zoomed picture is kept within its edges when the window changes size', async () => {
      for (let turn = 0; turn < 8; turn++) await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...box, deltaX: 0, deltaY: -200, modifiers: 2 });
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...box, deltaX: 0, deltaY: 100000 });
      await sleep(100);
      await cdp.send('Emulation.setDeviceMetricsOverride', { width: 700, height: 500, deviceScaleFactor: 1, mobile: false });
      await sleep(300);
      const edges = await cdp.evaluate(`(() => { const b = document.querySelector('[data-dui-pv-stage] > .dui-pv-item img').getBoundingClientRect(); return { top: b.top, bottom: b.bottom, height: innerHeight }; })()`);
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
    const mid = await cdp.evaluate(`(() => { const b = document.querySelector('.dui-pv-item img').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
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
      const back = await cdp.evaluate(ITEM_X);
      assert.ok(back < -5 && back > -96, `sliding back (${back})`);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [finger(mid.x, mid.y, 0)] });
      for (let k = 1; k <= 4; k++) {
        await sleep(16);
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [finger(mid.x - 10 * k, mid.y, 0)] });
      }
      await sleep(50);
      const held = await cdp.evaluate(`({ x: ${ITEM_X}, peek: document.querySelector('.dui-pv-peek[data-side="next"]').classList.contains('on') })`);
      // It went on sliding back for a few ms between the two readings.
      assert.ok(held.x < back - 25 && held.x > back - 60, `held at ${held.x}, 40 px on from about ${back}`);
      assert.equal(held.peek, true);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await sleep(600);
      assert.equal(await cdp.evaluate(ITEM_X), 0);
    });
  } finally {
    await browser.stop();
  }
});

// A picture smaller than the stage, whose thumbnail stands in at the
// stage's size: as it is decoded it grows from the thumbnail's box down
// to its own (`takeOver`, 280 ms).
const SMALL = `(() => {
  const canvas = document.createElement('canvas');
  canvas.width = 200; canvas.height = 130;
  const g = canvas.getContext('2d');
  g.fillStyle = '#b07a4f'; g.fillRect(0, 0, 200, 130);
  window.pictures.small = canvas.toDataURL('image/png');
  return true;
})()`;
async function growing(cdp, file) {
  await cdp.evaluate(putItem(file, 'standin', 'pictures.small'));
  await sleep(300);
  await cdp.evaluate(putItem(file, 'picture', 'pictures.small'));
  // Decoded, and growing: well inside its 280 ms.
  for (let tries = 0; tries < 50; tries++) {
    const running = await cdp.evaluate(`document.querySelector('[data-dui-pv-stage] > .dui-pv-item img').getAnimations().length`);
    if (running) return;
    await sleep(5);
  }
  throw new Error('the picture did not grow from its thumbnail');
}
const SLIDE = (side) => `({
  x: ${ITEM_X},
  peeks: document.querySelectorAll('[data-dui-pv-stage] > .dui-pv-peek.on').length,
  told: told.map(m => m.kind),
})`;

test('the desktop: what the second review of #336 found', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('desktop');
  try {
    const { cdp } = browser;
    await cdp.evaluate(SMALL);

    await t.test('a double click while a picture grows from its thumbnail zooms to 2×', async () => {
      await growing(cdp, 'c');
      const at = await cdp.evaluate(`(() => { const b = document.querySelector('[data-dui-pv-stage] > .dui-pv-item').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
      for (const clickCount of [1, 2]) {
        await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...at, button: 'left', buttons: 1, clickCount });
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...at, button: 'left', buttons: 0, clickCount });
      }
      await sleep(500);
      const view = await cdp.evaluate(VIEW);
      near(view.s, 2, 1e-6, 'scale');
      assert.equal(view.animations, 0);
    });
  } finally {
    await browser.stop();
  }
});

test('Android: what the second review of #336 found', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await page('android');
  try {
    const { cdp } = browser;
    await cdp.evaluate(SMALL);
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

    await t.test('a tap while a picture grows from its thumbnail leaves it to grow, and it rests at fit', async () => {
      await growing(cdp, 'c');
      await down(finger(mid.x, mid.y));
      await up();
      await sleep(500);
      const view = await cdp.evaluate(VIEW);
      assert.deepEqual({ s: view.s, x: view.x, y: view.y, animations: view.animations }, { s: 1, x: 0, y: 0, animations: 0 });
    });

    await t.test('a pinch while it grows takes it from where it is drawn and settles within the limits', async () => {
      await sleep(400);
      await growing(cdp, 'd');
      await down(finger(mid.x - 50, mid.y, 0), finger(mid.x + 50, mid.y, 1));
      for (let k = 1; k <= 5; k++) {
        await move(finger(mid.x - 50 - 5 * k, mid.y, 0), finger(mid.x + 50 + 5 * k, mid.y, 1));
        await sleep(16);
      }
      const held = await cdp.evaluate(VIEW);
      assert.ok(held.s > 1 && held.animations === 0, `held where it was drawn, with nothing moving it (${JSON.stringify(held)})`);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [finger(mid.x - 75, mid.y, 0)] });
      await up();
      await sleep(500);
      const rest = await cdp.evaluate(VIEW);
      assert.ok(rest.s >= 1 && rest.s <= 4 && rest.animations === 0, `at rest within 1× and 4× (${JSON.stringify(rest)})`);
      // Back to fit for what follows.
      for (let tap = 0; tap < 2; tap++) { await down(finger(mid.x, mid.y)); await up(); await sleep(90); }
      await sleep(500);
      assert.equal((await cdp.evaluate(VIEW)).s, 1);
    });

    await t.test('a second flick takes the file still coming in from a step from where it is drawn', async () => {
      await cdp.evaluate(putItem('a', 'picture', 'pictures.a'));
      await sleep(400);
      await cdp.evaluate('told.length = 0; true');
      await drag(-120, 0, 6, 16, 0);
      await sleep(30);
      await cdp.evaluate(putItem('b', 'standin', 'pictures.b'));
      await sleep(40);
      await down(finger(mid.x, mid.y));
      for (let k = 1; k <= 2; k++) { await sleep(16); await move(finger(mid.x - 10 * k, mid.y)); }
      await sleep(30);
      const held = await cdp.evaluate(SLIDE());
      assert.deepEqual(held.told, ['next']);
      assert.ok(held.x > 60, `held where it was coming in from the right, not snapped to rest (${held.x})`);
      // Flung on to the left: it comes the rest of its way in.
      for (let k = 3; k <= 8; k++) { await sleep(16); await move(finger(mid.x - 30 * k, mid.y)); }
      await up();
      await sleep(600);
      const rest = await cdp.evaluate(SLIDE());
      assert.deepEqual(rest, { x: 0, peeks: 0, told: ['next'] });
    });

    // A file taken again as it slides back, by a finger that then does
    // something else than drag it sideways.
    async function slidingBack() {
      await drag(-90, 0, 8, 20, 150);
      await sleep(60);
      const x = await cdp.evaluate(ITEM_X);
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
      assert.deepEqual(await cdp.evaluate(SLIDE()), { x: 0, peeks: 0, told: [] });
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
      assert.deepEqual({ x: rest.x, peeks: rest.peeks }, { x: 0, peeks: 0 });
      for (let tap = 0; tap < 2; tap++) { await down(finger(mid.x, mid.y)); await up(); await sleep(90); }
      await sleep(500);
    });

    await t.test('taken as it slides back on text that scrolls, and scrolled, it goes back in place', async () => {
      await cdp.evaluate(`(() => {
        const item = document.createElement('div');
        item.className = 'dui-pv-item'; item.dataset.file = 'txt';
        item.innerHTML = '<div class="dui-scroll" data-dui-scroll style="width:100%;height:100%;overflow:auto"><pre style="height:3000px;margin:0">text</pre></div>';
        document.querySelector('[data-dui-pv-stage] > .dui-pv-item').replaceWith(item);
        return true;
      })()`);
      await sleep(400);
      await cdp.evaluate('told.length = 0; true');
      await slidingBack();
      for (let k = 1; k <= 6; k++) { await sleep(16); await move(finger(mid.x, mid.y - 25 * k)); }
      await up();
      await sleep(600);
      assert.deepEqual(await cdp.evaluate(SLIDE()), { x: 0, peeks: 0, told: [] });
    });
  } finally {
    await browser.stop();
  }
});
