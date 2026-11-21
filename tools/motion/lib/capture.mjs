// The three ways a movement is captured: stepped frames, real-time frames
// and the motion trace that goes with each.

import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { sleep } from './cdp.mjs';
import { installSource } from './page.mjs';
import { adb, onCleanup } from './targets.mjs';
import { parseFramestats, parseShowinfo } from './analyse.mjs';

/** The selector a scenario gives for this platform, or null. */
export function pick(value, platform) {
  if (value === undefined || value === null) return null;
  if (typeof value === 'string') return value;
  if (platform in value) return value[platform];
  if (platform.startsWith('mockup-') && 'mockup' in value) return value.mockup;
  if ('default' in value) return value.default;
  return null;
}

/** The parts of a scenario as name → selector on this platform. */
export function partsFor(scenario, platform) {
  const parts = {};
  for (const [name, value] of Object.entries(scenario.parts || {})) {
    const selector = pick(value, platform);
    if (selector) parts[name] = selector;
  }
  return parts;
}

/**
 * Waits until the page has loaded, the app has drawn into its root, and the
 * window's size has held for a moment. The desktop's web view is found as
 * soon as it exists: once, the page script was installed into the first
 * document, which the app then replaced, and the size read was the
 * window's before the app placed it (961 × 1032 for 1100 × 720).
 */
async function settled(cdp, timeoutMs = 20000) {
  const ask = `(() => { const root = document.getElementById('main');
    return { ready: document.readyState === 'complete' && !!document.body && (!root || root.children.length > 0),
             size: innerWidth + 'x' + innerHeight }; })()`;
  const end = Date.now() + timeoutMs;
  let last = null, since = 0;
  while (Date.now() < end) {
    const now = await cdp.evaluate(ask).catch(() => null);
    if (now && now.ready) {
      if (last === now.size) {
        if (Date.now() - since >= 600) return;
      } else {
        last = now.size;
        since = Date.now();
      }
    } else {
      last = null;
    }
    await sleep(100);
  }
}

export async function prepare(cdp) {
  await cdp.send('Page.enable');
  await cdp.send('Runtime.enable');
  await cdp.send('Animation.enable');
  await cdp.send('Animation.setPlaybackRate', { playbackRate: 1 });
  // The page behaves as if it had the keyboard, without the window taking
  // it from the person at the computer.
  await cdp.send('Emulation.setFocusEmulationEnabled', { enabled: true }).catch(() => {});
  await settled(cdp);
  await cdp.evaluate(installSource);
  return cdp.evaluate(`({ width: innerWidth, height: innerHeight, dpr: devicePixelRatio, reduced: matchMedia('(prefers-reduced-motion: reduce)').matches, agent: navigator.userAgent })`);
}

// ---- Input ---------------------------------------------------------------

const KEYS = {
  Escape: { code: 'Escape', keyCode: 27 },
  Enter: { code: 'Enter', keyCode: 13, text: '\r' },
  Tab: { code: 'Tab', keyCode: 9 },
  ' ': { code: 'Space', keyCode: 32, text: ' ' },
  ArrowLeft: { code: 'ArrowLeft', keyCode: 37 },
  ArrowRight: { code: 'ArrowRight', keyCode: 39 },
  ArrowUp: { code: 'ArrowUp', keyCode: 38 },
  ArrowDown: { code: 'ArrowDown', keyCode: 40 },
  Backspace: { code: 'Backspace', keyCode: 8 },
};

async function centre(cdp, selector, index = 0) {
  // What a step presses may still be on its way (the page just started, or
  // the app has not answered the step before): it is waited for, a little.
  for (let tries = 0; tries < 25; tries++) {
    const at = await locate(cdp, selector, index);
    if (at) return at;
    await sleep(200);
  }
  throw new Error(`nothing drawn matches ${selector}`);
}

function locate(cdp, selector, index) {
  return cdp.evaluate(`(() => {
    const all = [...document.querySelectorAll(${JSON.stringify(selector)})].filter(e => { const b = e.getBoundingClientRect(); return b.width > 0 && b.height > 0; });
    const e = all[${index < 0 ? `all.length + ${index}` : index}];
    if (!e) return null;
    const b = e.getBoundingClientRect();
    return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
  })()`);
}

async function waitFor(cdp, selector, gone, ms = 8000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const present = await cdp.evaluate(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return false; const b = e.getBoundingClientRect(); return b.width > 0 && b.height > 0 && getComputedStyle(e).visibility === 'visible'; })()`);
    if (present !== !!gone) return;
    await sleep(100);
  }
  throw new Error(`waited ${ms} ms for ${selector} to ${gone ? 'go' : 'show'}`);
}

/**
 * Sends one step of a scenario's input. Pointers and keys go through
 * DevTools' Input domain, as a hand would; nothing is clicked from inside
 * the page.
 */
export async function sendStep(target, step) {
  const { cdp, platform } = target;
  const sel = value => pick(value, platform);
  if (step.platforms && !step.platforms.includes(platform) && !(platform.startsWith('mockup-') && step.platforms.includes('mockup'))) return;
  if (step.click || step.tap) {
    const selector = sel(step.click || step.tap);
    if (!selector) return;
    const { x, y } = await centre(target.cdp, selector, step.index || 0);
    if (target.pointer === 'touch' || step.tap) {
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] });
      await sleep(step.holdMs || 40);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    } else {
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
      await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 });
      await sleep(step.holdMs || 30);
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 0, clickCount: 1 });
      // A hand would leave the pointer where it pressed, which keeps a
      // :hover alive on whatever opens under it (the preview's file name is
      // shown on hover). It is moved off the page unless the step says stay.
      if (!step.stay) await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: -20, y: -20 });
    }
  } else if (step.hover) {
    const selector = sel(step.hover);
    if (!selector) return;
    const { x, y } = await centre(cdp, selector, step.index || 0);
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
  } else if (step.key) {
    const key = KEYS[step.key] || { code: step.key, keyCode: step.key.toUpperCase().charCodeAt(0) };
    const modifiers = (step.ctrl ? 2 : 0) | (step.shift ? 8 : 0) | (step.alt ? 1 : 0);
    const base = { key: step.key, code: key.code, windowsVirtualKeyCode: key.keyCode, nativeVirtualKeyCode: key.keyCode, modifiers };
    await cdp.send('Input.dispatchKeyEvent', { type: key.text ? 'keyDown' : 'rawKeyDown', ...base, ...(key.text ? { text: key.text } : {}) });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
  } else if (step.type !== undefined) {
    await cdp.send('Input.insertText', { text: step.type });
  } else if (step.wheel) {
    const selector = sel(step.wheel.on || step.wheel);
    const { x, y } = await centre(cdp, selector);
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY: step.wheel.dy || 300 });
  } else if (step.wait !== undefined) {
    await sleep(step.wait);
  } else if (step.waitFor) {
    const selector = sel(step.waitFor);
    if (selector) await waitFor(cdp, selector, false, step.ms);
  } else if (step.waitGone) {
    const selector = sel(step.waitGone);
    if (selector) await waitFor(cdp, selector, true, step.ms);
  } else if (step.pastePicture) {
    const selector = sel(step.pastePicture);
    await cdp.evaluate(`__motion.pastePicture(${JSON.stringify(selector)}, ${JSON.stringify(step.name || 'motion.png')})`);
  } else if (step.androidKey) {
    if (platform === 'android') await adb(target.serial, 'shell', 'input', 'keyevent', String(step.androidKey));
  } else if (step.eval) {
    const js = sel(step.eval);
    if (js) await cdp.evaluate(js);
  } else {
    throw new Error(`a step the tool does not know: ${JSON.stringify(step)}`);
  }
  if (step.then) await sleep(step.then);
}

/**
 * Whether a step changes the person's data: typing, a pasted file, or a
 * step the scenario marks `writes` (a press that archives, deletes or
 * makes a note). On a real phone those are refused unless allowed: its
 * notes are the person's and they sync.
 */
export function writes(step) {
  return step.type !== undefined || !!step.pastePicture || !!step.writes;
}

export async function sendSteps(target, steps = []) {
  for (const step of steps) {
    await sendStep(target, step);
    // A press that may need another (Android's page bar takes the first
    // touch while typing to come back): pressed again until it is gone.
    const until = pick(step.repeatUntilGone, target.platform);
    for (let tries = 0; until && tries < 3; tries++) {
      await sleep(700);
      const present = await target.cdp.evaluate(`!!document.querySelector(${JSON.stringify(until)})`);
      if (!present) break;
      await sendStep(target, step);
    }
  }
}

// ---- Screenshots -----------------------------------------------------------

/** The part of the page a movement's frames show, in CSS pixels. */
export async function cropFor(target, scenario, viewport) {
  const selector = pick(scenario.crop, target.platform);
  let box = { x: 0, y: 0, width: viewport.width, height: viewport.height };
  if (selector) {
    const found = await target.cdp.evaluate(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return null; const b = e.getBoundingClientRect(); return { x: b.left, y: b.top, width: b.width, height: b.height }; })()`);
    if (found) box = found;
  }
  return {
    x: Math.max(0, Math.floor(box.x)), y: Math.max(0, Math.floor(box.y)),
    width: Math.ceil(Math.min(box.width, viewport.width - Math.max(0, box.x))),
    height: Math.ceil(Math.min(box.height, viewport.height - Math.max(0, box.y))),
  };
}

/** How much a frame is shrunk so that it fits the contact sheet's cell. */
export function scaleFor(crop, viewport) {
  const portrait = crop.height > crop.width;
  const cell = portrait ? 260 : 400;
  return Math.min(1, cell / (crop.width * (viewport.dpr || 1)));
}

async function shoot(cdp, file, crop, scale) {
  const shot = await cdp.send('Page.captureScreenshot', {
    format: 'jpeg', quality: 80,
    clip: { ...crop, scale },
    optimizeForSpeed: true,
  }, 20000);
  fs.writeFileSync(file, Buffer.from(shot.data, 'base64'));
}

// ---- Stepped frames ----------------------------------------------------------

/**
 * A movement stepped: the timeline stopped, the input sent, then time
 * moved on `stepMs` at a time, with a screenshot and a probe at each step.
 */
export async function captureStepped(target, movement, { parts, pairs = [], crop, scale, dir, stepMs, viewport, log }) {
  const { cdp } = target;
  fs.mkdirSync(dir, { recursive: true });
  const maxMs = movement.ms || 1500;
  const minMs = movement.minMs || 200;
  const frames = [];
  let notes = [];
  const probe = `__motion.probe(${JSON.stringify(parts)}, ${JSON.stringify(pairs)})`;
  try {
    await cdp.send('Animation.setPlaybackRate', { playbackRate: 0 });
    await cdp.evaluate('__motion.freeze()');
    // The input, and the page's answer to it (the app's round trip to
    // Rust), with the clock held at 0. The frame before the input is kept:
    // a part cut in or out by the input itself is a pop against it.
    const before = await cdp.evaluate(probe);
    const beforeFile = path.join(dir, 'before.jpg');
    await shoot(cdp, beforeFile, crop, scale);
    frames.push({ t: -1, file: beforeFile, ...before, before: true });
    await sendSteps(target, movement.steps);
    const settleUntil = Date.now() + (movement.settleMs || 4000);
    let quietSince = Date.now();
    let changed = false;
    while (Date.now() < settleUntil) {
      const { mutations } = await cdp.evaluate('__motion.settle()');
      if (mutations) { quietSince = Date.now(); changed = true; }
      if (changed && Date.now() - quietSince > 250) break;
      if (!changed && Date.now() - quietSince > 1500) break;
      await sleep(40);
    }
    if (!changed) notes.push('The page did not change within the wait after the input: the movement may have been missed.');
    let still = 0;
    let last = null;
    for (let k = 0; ; k++) {
      const t = Math.round(k * stepMs * 1000) / 1000;
      if (t > maxMs) { notes.push(`Stopped at ${maxMs} ms, the movement's limit, with things still moving.`); break; }
      const state = await cdp.evaluate(`__motion.step(${t})`);
      // Two frames drawn, so the screenshot shows the step.
      await cdp.evaluate('new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))', { timeoutMs: 5000 }).catch(() => {});
      const measured = await cdp.evaluate(probe);
      const file = path.join(dir, `f${String(k).padStart(4, '0')}.jpg`);
      await shoot(cdp, file, crop, scale);
      frames.push({ t, file, ...measured });
      const same = last && JSON.stringify(measured) === JSON.stringify(last);
      still = same ? still + 1 : 0;
      last = measured;
      const soon = state.timers.some(ms => ms < 400);
      if (t >= minMs && state.running === 0 && !soon && still >= 3) break;
    }
    const seen = await cdp.evaluate('__motion.seen()');
    return { frames, seen, notes, stepMs, viewport };
  } finally {
    await cdp.evaluate('__motion.thaw()').catch(() => {});
    await cdp.send('Animation.setPlaybackRate', { playbackRate: 1 }).catch(() => {});
    await sleep(movement.afterMs || 900);
  }
}

// ---- Real time -----------------------------------------------------------------

const TRACE_CATEGORIES = [
  'devtools.timeline', 'disabled-by-default-devtools.timeline', 'disabled-by-default-devtools.timeline.frame',
  'disabled-by-default-devtools.screenshot', 'blink.user_timing', 'toplevel', 'benchmark', 'cc',
];

async function readStream(cdp, handle) {
  let text = '';
  for (;;) {
    const chunk = await cdp.send('IO.read', { handle, size: 1 << 20 });
    text += chunk.base64Encoded ? Buffer.from(chunk.data, 'base64').toString('utf8') : chunk.data;
    if (chunk.eof) break;
  }
  await cdp.send('IO.close', { handle });
  return text;
}

/**
 * A movement in real time: a DevTools trace with a screenshot of every
 * composited frame, and the probe on every animation frame. On Android,
 * also the screen recorded by the device and its frame statistics.
 */
export async function captureRealtime(target, movement, { parts, pairs = [], crop, scale, dir, viewport, log }) {
  const { cdp } = target;
  fs.mkdirSync(dir, { recursive: true });
  const ms = movement.ms || 1500;
  const android = target.platform === 'android';
  const result = { samples: [], events: [], frames: [], android: null };
  let recording = null, tracing = false, probing = false, ok = false, wallInput = 0;
  try {
    if (android) {
      await adb(target.serial, 'shell', 'dumpsys', 'gfxinfo', target.packageName, 'reset').catch(() => {});
      recording = await startRecording(target, ms);
    }
    const complete = new Promise(resolve => { const off = cdp.on('Tracing.tracingComplete', params => { off(); resolve(params); }); });
    await cdp.send('Tracing.start', {
      transferMode: 'ReturnAsStream',
      traceConfig: { includedCategories: TRACE_CATEGORIES, recordMode: 'recordContinuously' },
    });
    tracing = true;
    await sleep(150);
    result.startedAt = await cdp.evaluate(`__motion.record(${JSON.stringify(parts)}, ${JSON.stringify(pairs)})`);
    probing = true;
    await sleep(100);
    const inputAt = await cdp.evaluate(`(performance.mark('motion:input'), performance.now())`);
    result.inputAt = inputAt;
    wallInput = Date.now();
    await sendSteps(target, movement.steps);
    await sleep(ms);
    const samples = await cdp.evaluate('__motion.stopRecording()');
    probing = false;
    result.samples = samples.map(s => ({ ...s, t: Math.round((s.t - inputAt) * 10) / 10 }));
    await cdp.send('Tracing.end');
    tracing = false;
    const { stream } = await complete;
    const raw = await readStream(cdp, stream);
    const parsed = JSON.parse(raw);
    result.events = Array.isArray(parsed) ? parsed : parsed.traceEvents || [];

    // The composited frames DevTools kept, with their times from the input.
    const mark = result.events.find(e => e.name === 'motion:input');
    const zero = mark ? mark.ts : null;
    const shots = result.events.filter(e => e.name === 'Screenshot' && e.args && e.args.snapshot);
    if (zero !== null) {
      let n = 0;
      for (const shot of shots) {
        const t = Math.round((shot.ts - zero) / 100) / 10;
        if (t < -50 || t > ms) continue;
        const file = path.join(dir, `r${String(n++).padStart(4, '0')}.jpg`);
        fs.writeFileSync(file, Buffer.from(shot.args.snapshot, 'base64'));
        result.frames.push({ t, file });
      }
    }
    result.zero = zero;
    ok = true;
  } finally {
    if (probing) await cdp.evaluate('__motion.stopRecording()').catch(() => {});
    if (tracing) await cdp.send('Tracing.end').catch(() => {});
    if (recording) {
      const video = path.join(dir, 'screen.mp4');
      // Stopped, pulled only if the run got that far, and always deleted
      // from the device.
      await recording.stop();
      if (ok) {
        const framestats = await adb(target.serial, 'shell', 'dumpsys', 'gfxinfo', target.packageName, 'framestats').catch(() => '');
        await adb(target.serial, 'pull', recording.remote, video).catch(() => {});
        const screen = fs.existsSync(video) ? await videoFrames(video, path.join(dir, 'screen'), wallInput) : null;
        result.android = { framestats: parseFramestats(framestats), screen };
      }
      await recording.remove();
      if (fs.existsSync(video)) fs.rmSync(video);
    }
  }
  await sleep(movement.afterMs || 900);
  return result;
}

/**
 * The device records its screen. The shell prints its own process ID and
 * becomes screenrecord (exec), so the recording is stopped by that ID,
 * never by the program's name. Its file on the device is deleted however
 * the run ends.
 */
async function startRecording(target, ms) {
  const remote = `/sdcard/dirt-motion-${Date.now()}.mp4`;
  const limit = Math.ceil(ms / 1000) + 3;
  const child = spawn('adb', ['-s', target.serial, 'shell', `echo $$; exec screenrecord --bit-rate 8000000 --time-limit ${limit} ${remote}`], { stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true });
  const pid = await new Promise(resolve => {
    let out = '';
    child.stdout.on('data', chunk => { out += chunk; const line = out.split('\n')[0].trim(); if (/^\d+$/.test(line)) resolve(line); });
    child.on('exit', () => resolve(null));
    setTimeout(() => resolve(null), 5000);
  });
  const remove = () => adb(target.serial, 'shell', 'rm', '-f', remote).catch(() => {});
  const stop = async () => {
    if (pid) await adb(target.serial, 'shell', 'kill', '-INT', pid).catch(() => {});
    await new Promise(resolve => { if (child.exitCode !== null) resolve(); else child.on('exit', resolve); setTimeout(resolve, 8000); });
    if (child.exitCode === null) child.kill();
    await sleep(500); // the device finishes writing the file
  };
  const undo = onCleanup(`screen recording ${remote} on ${target.serial}`, async () => { await stop(); await remove(); });
  await sleep(900); // screenrecord takes a moment to begin
  return { remote, stop, remove: async () => { undo(); await remove(); } };
}

/**
 * Each frame of a screen recording, with its time in the video. The device
 * writes a frame only when the screen changes, so the gaps between them
 * are the gaps between the frames a person saw.
 */
function videoFrames(video, dir, wallInput) {
  fs.mkdirSync(dir, { recursive: true });
  return new Promise(resolve => {
    const ffmpeg = spawn('ffmpeg', ['-hide_banner', '-i', video, '-vf', 'scale=260:-2,showinfo', '-fps_mode', 'passthrough', '-q:v', '4', path.join(dir, 's%04d.jpg')], { stdio: ['ignore', 'ignore', 'pipe'], windowsHide: true });
    let log = '';
    ffmpeg.stderr.on('data', chunk => { log += chunk; });
    ffmpeg.on('exit', () => {
      const times = parseShowinfo(log);
      const files = fs.readdirSync(dir).filter(f => f.endsWith('.jpg')).sort();
      resolve({ frames: files.map((f, i) => ({ t: times[i] !== undefined ? Math.round(times[i] * 1000) : null, file: path.join(dir, f) })), wallInput });
    });
  });
}

export async function measureFrameRate(cdp, ms = 1000) {
  const stamps = await cdp.evaluate(`__motion.frameRate(${ms})`, { timeoutMs: ms + 5000 });
  return stamps;
}
