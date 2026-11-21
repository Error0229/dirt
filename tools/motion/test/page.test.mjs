// node --test tools/motion/test/page.test.mjs
//
// What the tool runs inside the page (lib/page.mjs), in a real headless
// Edge or Chrome: the stepped clock and the probe. Skipped where neither
// browser is installed.

import test from 'node:test';
import assert from 'node:assert/strict';
import { findBrowser, launchHeadless } from '../lib/targets.mjs';
import { installSource } from '../lib/page.mjs';

let browserFound = true;
try { findBrowser(); } catch { browserFound = false; }

const PAGE = `<!doctype html><meta charset="utf-8"><style>
  body{margin:0}
  #running{width:50px;height:50px;background:red}
  #scroller{position:absolute;left:0;top:100px;width:100px;height:100px;overflow:hidden}
  #fixed{position:fixed;left:300px;top:300px;width:40px;height:40px;background:blue}
  #inner{position:absolute;left:150px;top:0;width:40px;height:40px;background:green}
  #shape{position:absolute;left:0;top:400px;width:200px;height:100px;clip-path:inset(0px 100px 0px 0px)}
  #shaped{width:200px;height:100px;background:teal}
  #calc{position:absolute;left:250px;top:400px;width:200px;height:100px;clip-path:inset(0px calc(100% - 50px) 0px 0px round 8px)}
  #calced{width:200px;height:100px;background:olive}
  .box{position:absolute;width:100px;height:100px}
  #a{left:500px;top:0;background:#aaa}#b{left:550px;top:50px;background:#bbb}
  #c{left:500px;top:300px;background:#ccc}#d{left:550px;top:350px;background:#ddd}
  #cover{position:absolute;left:480px;top:280px;width:200px;height:200px;background:#000;z-index:5}
</style>
<div id="running"></div>
<div id="scroller"><div id="fixed"></div><div id="inner"></div></div>
<div id="shape"><div id="shaped"></div></div>
<div id="calc"><div id="calced"></div></div>
<div class="box" id="a"></div><div class="box" id="b"></div>
<div class="box" id="c"></div><div class="box" id="d"></div><div id="cover"></div>
<script>window.started = document.getElementById('running').animate([{ opacity: 0 }, { opacity: 1 }], { duration: 1000 });</script>`;

test('the clock and the probe in a real page', { skip: !browserFound && 'no Edge or Chrome here' }, async t => {
  const browser = await launchHeadless({ width: 800, height: 600 });
  try {
    const { cdp } = browser;
    await browser.goto(`data:text/html,${encodeURIComponent(PAGE)}`);
    await cdp.send('Animation.enable');
    assert.equal(await cdp.evaluate(installSource), 'installed');
    const native = await cdp.evaluate('String(window.setTimeout)');

    await t.test('freezes with an animation already running, and steps it', async () => {
      await cdp.send('Animation.setPlaybackRate', { playbackRate: 0 });
      assert.equal(await cdp.evaluate('__motion.freeze()'), true);
      const before = await cdp.evaluate('started.currentTime');
      await cdp.evaluate('window.fired = false; setTimeout(() => { window.fired = true; }, 100); true');
      await cdp.evaluate('__motion.step(50)');
      assert.equal(await cdp.evaluate('window.fired'), false);
      await cdp.evaluate('__motion.step(120)');
      assert.equal(await cdp.evaluate('window.fired'), true, 'a timer runs as the clock passes it');
      const now = await cdp.evaluate('started.currentTime');
      assert.ok(Math.abs(now - (before + 120)) < 1, `the running animation moved 120 ms on the clock (${before} → ${now})`);
      assert.equal(await cdp.evaluate('started.playState'), 'paused');
    });

    await t.test('thaw gives the page its own timers back, and the animation goes on', async () => {
      assert.equal(await cdp.evaluate('__motion.thaw()'), true);
      await cdp.send('Animation.setPlaybackRate', { playbackRate: 1 });
      assert.equal(await cdp.evaluate('String(window.setTimeout)'), native);
      assert.equal(await cdp.evaluate('started.playState'), 'running');
      // A thaw with nothing frozen still puts the timers back.
      assert.equal(await cdp.evaluate('__motion.thaw()'), false);
    });

    await t.test('a fixed element is not cut by a scrolling ancestor; an absolute one is', async () => {
      const { parts } = await cdp.evaluate(`__motion.probe({ fixed: '#fixed', inner: '#inner' })`);
      assert.equal(parts.fixed[0].seen, 1);
      assert.equal(parts.inner[0].seen, 0);
    });

    await t.test('an ancestor\'s clip-path cuts what is seen', async () => {
      const { parts } = await cdp.evaluate(`__motion.probe({ shaped: '#shaped' })`);
      assert.equal(parts.shaped[0].seen, 0.5);
    });

    await t.test('a clip whose edges are calc() of the box and pixels cuts too', async () => {
      const { parts } = await cdp.evaluate(`__motion.probe({ calced: '#calced' })`);
      assert.equal(parts.calced[0].seen, 0.25);
    });

    await t.test('two parts are seen over one another unless something covers both', async () => {
      const { overlaps } = await cdp.evaluate(`__motion.probe({ a: '#a', b: '#b', c: '#c', d: '#d' }, [['a', 'b'], ['c', 'd']])`);
      assert.ok(overlaps['a|b'] > 0);
      assert.equal(overlaps['c|d'], 0);
    });
  } finally {
    await browser.stop();
  }
});
