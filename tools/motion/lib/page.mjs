// What the tool runs inside the page: the probe that measures the parts a
// scenario names, the recorder that measures them on every frame, and the
// clock that steps a movement's time.
//
// It is sent as the source of `install` and run once; it keeps itself on
// `window.__motion`. Nothing here changes what the page does until a
// stepped capture freezes its clock, and `thaw` puts every timer back.

function install(version) {
  // A page that lives on between runs (Android's web view does) keeps what
  // an earlier run installed; a different version is put back and replaced.
  if (window.__motion) {
    if (window.__motion.version === version) return 'already';
    try { window.__motion.thaw(); window.__motion.stopRecording(); } catch { /* an older one */ }
  }
  const real = {
    setTimeout: window.setTimeout.bind(window),
    clearTimeout: window.clearTimeout.bind(window),
    setInterval: window.setInterval.bind(window),
    clearInterval: window.clearInterval.bind(window),
    now: performance.now.bind(performance),
  };
  const round = value => Math.round(value * 10) / 10;

  // ---- The probe -------------------------------------------------------
  // For each part (a name and a selector), every element that matches:
  // its box on screen (transforms included), its opacity as drawn (its
  // own times its ancestors'), whether it is drawn at all, and whether it
  // takes the pointer.
  function drawnOpacity(element) {
    let opacity = 1;
    for (let at = element; at && at.nodeType === 1; at = at.parentElement) {
      opacity *= Number(getComputedStyle(at).opacity);
      if (opacity === 0) break;
    }
    return opacity;
  }
  // A clip-path of inset() or circle() cuts what is seen of a box; Android's
  // note page grows by nothing else. The box returned is what is left.
  function clipped(box, clip) {
    const length = (text, whole) => (text.endsWith('%') ? (parseFloat(text) / 100) * whole : parseFloat(text) || 0);
    let found = clip.match(/^inset\(([^)]*?)(?:\s+round\s+[^)]*)?\)$/);
    if (found) {
      const v = found[1].trim().split(/\s+/);
      const [t, r, b, l] = [v[0], v[1] ?? v[0], v[2] ?? v[0], v[3] ?? v[1] ?? v[0]];
      const top = length(t, box.height), right = length(r, box.width), bottom = length(b, box.height), left = length(l, box.width);
      return { left: box.left + left, top: box.top + top, width: Math.max(0, box.width - left - right), height: Math.max(0, box.height - top - bottom) };
    }
    found = clip.match(/^circle\(([^ ]+)\s+at\s+([^ ]+)\s+([^ )]+)\)$/);
    if (found) {
      const radius = length(found[1], Math.hypot(box.width, box.height) / Math.SQRT2);
      const x = box.left + length(found[2], box.width), y = box.top + length(found[3], box.height);
      const left = Math.max(box.left, x - radius), top = Math.max(box.top, y - radius);
      return { left, top, width: Math.max(0, Math.min(box.right, x + radius) - left), height: Math.max(0, Math.min(box.bottom, y + radius) - top) };
    }
    return null;
  }
  function measure(element) {
    const style = getComputedStyle(element);
    const whole = element.getBoundingClientRect();
    const shape = style.clipPath !== 'none' ? clipped(whole, style.clipPath) : null;
    const box = shape ? { left: shape.left, top: shape.top, width: shape.width, height: shape.height, right: shape.left + shape.width, bottom: shape.top + shape.height } : whole;
    let displayed = element.isConnected && style.display !== 'none';
    for (let at = element.parentElement; displayed && at; at = at.parentElement) {
      if (getComputedStyle(at).display === 'none') displayed = false;
    }
    const opacity = displayed ? drawnOpacity(element) : 0;
    // How much of its box can be seen: cut by the viewport and by every
    // ancestor that clips (a list that scrolls, a panel with its overflow
    // hidden), 0 to 1.
    let cut = { left: Math.max(box.left, 0), top: Math.max(box.top, 0), right: Math.min(box.right, innerWidth), bottom: Math.min(box.bottom, innerHeight) };
    for (let at = element.parentElement; at && at !== document.documentElement; at = at.parentElement) {
      const clips = getComputedStyle(at);
      if (clips.overflowX === 'visible' && clips.overflowY === 'visible') continue;
      const edge = at.getBoundingClientRect();
      cut = { left: Math.max(cut.left, edge.left), top: Math.max(cut.top, edge.top), right: Math.min(cut.right, edge.right), bottom: Math.min(cut.bottom, edge.bottom) };
    }
    const area = box.width * box.height;
    const seen = area > 0 ? Math.max(0, cut.right - cut.left) * Math.max(0, cut.bottom - cut.top) / area : 0;
    const visible = displayed && style.visibility === 'visible' && opacity > 0.01 && box.width > 0.5 && box.height > 0.5 && seen > 0;
    const inert = !!element.closest('[inert]');
    const interactive = displayed && !inert && style.pointerEvents !== 'none' && style.visibility === 'visible';
    const sample = {
      x: round(box.left), y: round(box.top), w: round(box.width), h: round(box.height),
      o: Math.round(opacity * 1000) / 1000,
      seen: Math.round(seen * 100) / 100,
      visible, interactive,
    };
    if (style.transform !== 'none') sample.transform = style.transform;
    if (style.scale !== 'none') sample.scale = style.scale;
    if (style.translate !== 'none') sample.translate = style.translate;
    if (style.clipPath !== 'none') sample.clip = style.clipPath;
    if (style.visibility !== 'visible') sample.visibility = style.visibility;
    if (element.scrollHeight > element.clientHeight + 1 && /(auto|scroll)/.test(style.overflowY)) sample.scroll = Math.round(element.scrollTop);
    return sample;
  }
  function probe(parts) {
    const out = {};
    for (const [name, selector] of Object.entries(parts)) {
      let found;
      try { found = [...document.querySelectorAll(selector)]; } catch { found = []; }
      out[name] = found.slice(0, 24).map(measure);
    }
    return out;
  }

  // ---- The recorder: every frame, in real time --------------------------
  let recording = null;
  function record(parts) {
    recording = { parts, frames: [], startedAt: real.now(), offset: null };
    // A frame's own time keeps the gaps between frames exact, but WebView2
    // was seen to count it from another origin than performance.now()
    // (12.7 s apart), so it is moved onto that clock once, at the first frame.
    const tick = stamp => {
      if (!recording) return;
      if (recording.offset === null) recording.offset = real.now() - stamp;
      recording.frames.push({ t: stamp + recording.offset, parts: probe(recording.parts) });
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return real.now();
  }
  function stopRecording() {
    const done = recording;
    recording = null;
    return done ? done.frames : [];
  }

  // How fast the page is drawn: the gaps between animation frames over
  // `ms`, for the report, since a window off the screen may be held back.
  function frameRate(ms) {
    return new Promise(resolve => {
      const stamps = [];
      const tick = stamp => {
        stamps.push(stamp);
        if (stamp - stamps[0] < ms) requestAnimationFrame(tick);
        else resolve(stamps);
      };
      requestAnimationFrame(tick);
      real.setTimeout(() => resolve(stamps), ms + 1500);
    });
  }

  // ---- The clock: a movement's time stepped ------------------------------
  // `freeze` is called after the tool has stopped the document's timeline
  // (DevTools' Animation.setPlaybackRate(0)), so no animation moves by
  // itself. Timers with a delay are held on a clock of our own; a timer
  // with none still runs at once, being the next task rather than a time.
  // `step(t)` brings the clock to `t` ms, runs the timers due on the way,
  // each at its own time, and sets every animation's current time to how
  // long it has run on this clock. An animation is taken to start when it
  // is first seen: at a timer that made it, or at the step after the page
  // made it. Every CSS transition and animation and every Web Animation is
  // one of `document.getAnimations()`, so all of them are stepped.
  let clock = null;
  function stamp(now) {
    for (const animation of document.getAnimations()) {
      if (!clock.born.has(animation)) {
        clock.born.set(animation, now);
        clock.names.set(animation, describe(animation));
        hold(animation);
      }
    }
  }
  // Paused, so that what is drawn is the time the clock sets. With the
  // timeline stopped but the animation running, Android's WebView was seen
  // to draw an opacity animation run by the compositor at its start, while
  // the page reported it nearly done.
  function hold(animation) {
    try { if (animation.playState === 'running' || animation.pending) { animation.pause(); clock.held.add(animation); } } catch { /* cannot be paused */ }
  }
  function describe(animation) {
    const effect = animation.effect;
    const target = effect && effect.target;
    const timing = effect ? effect.getComputedTiming() : {};
    let what = animation.constructor.name;
    if (animation.transitionProperty) what += ` ${animation.transitionProperty}`;
    if (animation.animationName) what += ` ${animation.animationName}`;
    let on = '';
    if (target) on = target.tagName.toLowerCase() + (target.id ? `#${target.id}` : '') + [...target.classList].slice(0, 3).map(c => `.${c}`).join('');
    return { what, on, duration: timing.duration, delay: timing.delay, easing: effect && effect.getTiming().easing };
  }
  function freeze() {
    clock = { now: 0, timers: new Map(), seq: 1, born: new Map(), names: new Map(), held: new Set(), base: real.now(), mutations: 0 };
    for (const animation of document.getAnimations()) {
      clock.born.set(animation, -(Number(animation.currentTime) || 0));
      clock.names.set(animation, describe(animation));
      hold(animation);
    }
    clock.observer = new MutationObserver(list => { clock.mutations += list.length; });
    clock.observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true });
    const hold = (fn, ms, args, every) => {
      const id = 1e9 + clock.seq++;
      clock.timers.set(id, { at: clock.now + ms, fn, args, every, id });
      return id;
    };
    window.setTimeout = (fn, ms, ...args) => {
      ms = Number(ms) || 0;
      if (ms <= 0 || typeof fn !== 'function') return real.setTimeout(fn, ms, ...args);
      return hold(fn, ms, args, 0);
    };
    window.setInterval = (fn, ms, ...args) => {
      ms = Math.max(1, Number(ms) || 0);
      if (typeof fn !== 'function') return real.setInterval(fn, ms, ...args);
      return hold(fn, ms, args, ms);
    };
    window.clearTimeout = id => { if (!(clock && clock.timers.delete(id))) real.clearTimeout(id); };
    window.clearInterval = id => { if (!(clock && clock.timers.delete(id))) real.clearInterval(id); };
    performance.now = () => clock.base + clock.now;
    return true;
  }
  // While the page answers the input (the app's round trip to Rust), the
  // clock stays at 0; this says how long the page has been quiet.
  function settle() {
    stamp(clock.now);
    const mutations = clock.mutations;
    clock.mutations = 0;
    return { mutations, animations: document.getAnimations().length };
  }
  function step(t) {
    for (;;) {
      let due = null;
      for (const timer of clock.timers.values()) {
        if (timer.at <= t && (!due || timer.at < due.at || (timer.at === due.at && timer.id < due.id))) due = timer;
      }
      if (!due) break;
      clock.now = Math.max(clock.now, due.at);
      stamp(clock.now);
      if (due.every) due.at += due.every; else clock.timers.delete(due.id);
      try { due.fn(...due.args); } catch (error) { console.error(error); }
      stamp(clock.now);
    }
    clock.now = t;
    stamp(t);
    let running = 0;
    for (const animation of document.getAnimations()) {
      const ran = Math.max(0, t - clock.born.get(animation)) * (animation.playbackRate || 1);
      const timing = animation.effect ? animation.effect.getComputedTiming() : null;
      const end = timing ? timing.endTime : Infinity;
      try {
        if (end !== Infinity && ran >= end) {
          // Past its end: finished, as it would be, so that what waits for
          // it (the app's `finished` handlers) runs now, on the clock.
          clock.held.delete(animation);
          if (animation.playState !== 'finished') animation.finish();
        } else {
          animation.currentTime = ran;
          running += end !== Infinity ? 1 : 0;
        }
      } catch { /* an animation with no timeline, or one that cannot finish */ }
    }
    return { running, timers: [...clock.timers.values()].filter(timer => !timer.every).map(timer => Math.round(timer.at - t)) };
  }
  // Every animation the clock saw, for the report: what it animated, on
  // what, and when it started on the clock.
  function seen() {
    if (!clock) return [];
    return [...clock.born.entries()].map(([animation, born]) => ({ born: Math.round(born), ...clock.names.get(animation) }));
  }
  function thaw() {
    if (!clock) return false;
    const left = [...clock.timers.values()];
    const now = clock.now;
    window.setTimeout = real.setTimeout;
    window.clearTimeout = real.clearTimeout;
    window.setInterval = real.setInterval;
    window.clearInterval = real.clearInterval;
    performance.now = real.now;
    clock.observer.disconnect();
    for (const animation of clock.held) {
      try { if (animation.playState === 'paused') animation.play(); } catch { /* gone */ }
    }
    clock = null;
    // What was still waiting runs on the real clock, as late as it was due.
    for (const timer of left) {
      if (timer.every) real.setInterval(timer.fn, timer.every, ...timer.args);
      else real.setTimeout(timer.fn, Math.max(0, timer.at - now), ...timer.args);
    }
    return true;
  }

  // ---- Input that the scenario cannot send as a hand would ---------------
  // A picture pasted into the page, as a picture copied from another
  // program is: the app's own paste handler takes it (design 6.16).
  async function pastePicture(selector, name, colour) {
    const canvas = document.createElement('canvas');
    canvas.width = 640; canvas.height = 420;
    const g = canvas.getContext('2d');
    g.fillStyle = colour || '#6b8f71'; g.fillRect(0, 0, 640, 420);
    g.fillStyle = '#f4efe6'; g.fillRect(40, 40, 260, 340);
    g.fillStyle = '#2a2622'; g.font = 'bold 48px sans-serif'; g.fillText('motion', 330, 230);
    const blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
    const data = new DataTransfer();
    data.items.add(new File([blob], name || 'motion.png', { type: 'image/png' }));
    const target = document.querySelector(selector) || document.activeElement || document.body;
    target.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    return true;
  }

  window.__motion = { version, probe, record, stopRecording, frameRate, freeze, settle, step, seen, thaw, pastePicture, realNow: real.now };
  return 'installed';
}

// Changed whenever what runs in the page changes, so that a page that kept
// an older one takes this one.
const VERSION = 3;
export const installSource = `(${install.toString()})(${VERSION})`;
