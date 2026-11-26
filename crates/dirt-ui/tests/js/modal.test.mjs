// node --test crates/dirt-ui/tests/js/modal.test.mjs
//
// dirt-ui's script (src/js/dirt-ui.js) with its style sheet, in a real
// headless Edge or Chrome: a panel or sheet that opens again after the
// page has collected its last opening's animations still takes the
// pointer. Skipped where neither browser is installed.
//
// Settings' sections rise as it opens, each by a Web Animation. Closed,
// the panel is inert and takes no pointer, and its sections inherit that.
// Once the page had collected the animations of the last opening, a
// section that rose again was given the style it had while closed: no
// pointer and inert, although the panel around it took both again. Every
// button in Settings did nothing but the close, which does not rise. It
// stayed so until Settings was closed and opened again. The animations
// are collected some while after they end, so a person who opens Settings
// now and then met it every time; opened and closed in a row, one opening
// in twenty or so.

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
// The shape `dirt_ui::motion::script_tokens` gives; the values do not matter.
const TOKENS = {
  springs: Object.fromEntries(['snap', 'glide', 'pop'].map(name => [name, { linear: 'ease-out', fallback: 'ease-out', ms: 480 }])),
  standard: 'ease', enter: 'ease-out', exit: 'ease-in', reducedMs: 120,
};

// A panel and a sheet as `dirt_ui::components::panel` draws them, closed,
// each with three sections that rise and a button in each.
const sections = kind => [0, 1, 2].map(n => `<section data-dui-rise><button id="${kind}-${n}">${kind} ${n}</button></section>`).join('');
const BODY = `
  <div class="dui-scrim"></div>
  <div class="dui-panel" data-dui-modal="panel" inert tabindex="-1">
    <button class="dui-panel-close" id="panel-close">Close</button>
    <div class="dui-panel-body">${sections('panel')}</div>
  </div>
  <div class="dui-scrim"></div>
  <div class="dui-sheet" data-dui-modal="sheet" inert tabindex="-1">
    <div class="dui-sheet-head" data-dui-sheet-grab><button id="sheet-close">Close</button></div>
    <div class="dui-sheet-body">${sections('sheet')}</div>
  </div>`;

// What Rust does to open or close one: the class and `inert` together.
const set = (kind, open) => `(() => {
  const modal = document.querySelector('[data-dui-modal="${kind}"]');
  modal.classList.toggle('on', ${open});
  modal.toggleAttribute('inert', ${!open});
  return true;
})()`;

// Each button of a section: its pointer-events, whether a press at its
// centre lands on it, and whether its section was left a style attribute
// it did not have.
const pressable = kind => `[0, 1, 2].map(n => {
  const button = document.getElementById('${kind}-' + n);
  const box = button.getBoundingClientRect();
  const at = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  const section = button.closest('section');
  return { pointer: getComputedStyle(section).pointerEvents, pressed: at === button, styled: section.hasAttribute('style') };
})`;

test('a panel or sheet opened again takes the pointer in every section', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await launchHeadless({ width: 900, height: 700 });
  try {
    const { cdp } = browser;
    await browser.goto('about:blank');
    await cdp.evaluate(`(() => {
      const style = document.createElement('style');
      style.textContent = ${JSON.stringify(STYLE)};
      document.head.appendChild(style);
      document.body.innerHTML = ${JSON.stringify(BODY)};
      return true;
    })()`);
    await cdp.evaluate(SCRIPT);
    await cdp.evaluate(`DirtUi.install({ tokens: ${JSON.stringify(TOKENS)}, platform: 'desktop' }); true`);
    // The script finds the panels when the page's observer has run.
    await sleep(100);

    for (const kind of ['panel', 'sheet']) {
      await t.test(kind, async () => {
        for (let opening = 1; opening <= 3; opening++) {
          await cdp.evaluate(set(kind, true));
          await sleep(900);
          const seen = await cdp.evaluate(pressable(kind));
          assert.deepEqual(seen, [0, 1, 2].map(() => ({ pointer: 'auto', pressed: true, styled: false })), `${kind}, opening ${opening}`);
          await cdp.evaluate(set(kind, false));
          await sleep(600);
          // What the page does some while after: its last opening's
          // animations, ended, are collected.
          await cdp.send('HeapProfiler.collectGarbage');
        }
      });
    }
  } finally {
    await browser.stop();
  }
});
