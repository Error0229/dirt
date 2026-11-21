// The analysis of a motion trace. Pure functions over plain data, so that
// `node --test tools/motion` checks them without a browser.
//
// A trace is a list of frames, `{ t, parts: { name: [sample, ...] } }`,
// where a sample is what the page's probe measured of one element:
// `{ x, y, w, h, o, visible, interactive }` (its box on screen in CSS
// pixels, its opacity as drawn, whether it is drawn, whether it takes the
// pointer). A part may match several elements; each is a track of its own,
// `name[i]`, unless the part only ever matched one.

export const CHANNELS = ['x', 'y', 'w', 'h', 'o', 'scroll'];

/** The tracks of a trace: key → [{ t, s }] with `s` null where the element was not there. */
export function tracksOf(frames) {
  const most = new Map();
  for (const frame of frames) {
    for (const [name, samples] of Object.entries(frame.parts || {})) {
      most.set(name, Math.max(most.get(name) || 0, samples.length));
    }
  }
  const tracks = new Map();
  for (const [name, count] of most) {
    for (let i = 0; i < Math.max(count, 1); i++) {
      const key = count > 1 ? `${name}[${i}]` : name;
      tracks.set(key, frames.map(frame => ({ t: frame.t, s: (frame.parts?.[name] || [])[i] || null })));
    }
  }
  return tracks;
}

const shownOf = s => !!(s && s.visible);

/** A sample's value on a channel; an element not drawn is opacity 0. */
export function valueOf(s, channel) {
  if (!s) return channel === 'o' ? 0 : null;
  if (channel === 'o') return s.visible ? s.o : 0;
  if (channel === 'scroll') return s.scroll ?? null;
  // A box of no size is an element taken out of the layout (display: none),
  // not one that moved to the corner.
  if (!s.visible && s.w === 0 && s.h === 0) return null;
  return s[channel];
}

/**
 * Pops: a change between two frames much bigger than the frames around
 * it, or an element that appears or goes in one frame while well
 * opaque. Steady fast movement is not a pop; a jump in the middle of it,
 * or a part that is suddenly somewhere else, is.
 */
export function findPops(frames, { px = 24, opacity = 0.3, ratio = 3 } = {}) {
  const pops = [];
  for (const [key, track] of tracksOf(frames)) {
    for (let i = 1; i < track.length; i++) {
      const a = track[i - 1].s, b = track[i].s;
      const wasShown = shownOf(a), isShown = shownOf(b);
      if (wasShown !== isShown) {
        const o = isShown ? b.o : a.o;
        // Moving into or out of sight (past the edge of what clips it) is
        // not a pop; being switched on or off where it can be seen is.
        const hidden = isShown ? a : b;
        const crossedEdge = !!hidden && hidden.w * hidden.h > 0 && hidden.seen === 0;
        if (o >= opacity && !crossedEdge) {
          pops.push({ key, t: track[i].t, from: track[i - 1].t, kind: isShown ? 'appears' : 'vanishes', detail: `${isShown ? 'appears' : 'vanishes'} at opacity ${round(o, 2)} in one frame` });
        }
        continue;
      }
      if (!isShown) continue;
      for (const channel of CHANNELS) {
        const limit = channel === 'o' ? opacity : channel === 'scroll' ? Infinity : px;
        const d = Math.abs(valueOf(b, channel) - valueOf(a, channel));
        if (d < limit) continue;
        const around = [];
        if (i >= 2 && shownOf(track[i - 2].s)) around.push(Math.abs(valueOf(a, channel) - valueOf(track[i - 2].s, channel)));
        if (i + 1 < track.length && shownOf(track[i + 1].s)) around.push(Math.abs(valueOf(track[i + 1].s, channel) - valueOf(b, channel)));
        const steady = around.length ? Math.max(...around) : 0;
        if (d >= ratio * steady) {
          pops.push({ key, t: track[i].t, from: track[i - 1].t, kind: 'jump', channel, detail: `${channel} jumps ${round(d, 1)}${channel === 'o' ? '' : ' px'} in one frame (${round(valueOf(a, channel), 1)} → ${round(valueOf(b, channel), 1)}); the frames around move ${round(steady, 1)}` });
        }
      }
    }
  }
  return pops.sort((p, q) => p.t - q.t);
}

const area = s => Math.max(0, s.w) * Math.max(0, s.h);
function overlap(a, b) {
  const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}

/** How far `inner` reaches past `outer` on its worst side, in px. */
export function beyond(inner, outer) {
  return Math.max(outer.x - inner.x, outer.y - inner.y, (inner.x + inner.w) - (outer.x + outer.w), (inner.y + inner.h) - (outer.y + outer.h), 0);
}

/** Where a part (every element of it) reaches outside its container while drawn. */
export function findOutside(frames, pairs, { tolerance = 1.5, viewport } = {}) {
  const found = [];
  for (const [part, container] of pairs) {
    for (const frame of frames) {
      const outer = container === 'viewport'
        ? (viewport ? { x: 0, y: 0, w: viewport.width, h: viewport.height } : null)
        : (frame.parts?.[container] || [])[0];
      if (!outer || (container !== 'viewport' && !outer.visible)) continue;
      (frame.parts?.[part] || []).forEach((s, i) => {
        if (!shownOf(s) || s.o < 0.05) return;
        const by = beyond(s, outer);
        if (by > tolerance) found.push({ part, index: i, container, t: frame.t, by: round(by, 1) });
      });
    }
  }
  return collapse(found, f => `${f.part}|${f.container}`);
}

/** Where two parts that should not cover each other do, both drawn. */
export function findOverlaps(frames, pairs, { minArea = 16, opacity = 0.05 } = {}) {
  const found = [];
  for (const [a, b] of pairs) {
    for (const frame of frames) {
      for (const sa of frame.parts?.[a] || []) {
        for (const sb of frame.parts?.[b] || []) {
          if (!shownOf(sa) || !shownOf(sb) || sa.o < opacity || sb.o < opacity) continue;
          const shared = overlap(sa, sb);
          if (shared >= minArea) found.push({ a, b, t: frame.t, area: Math.round(shared), share: round(shared / Math.min(area(sa), area(sb)), 2) });
        }
      }
    }
  }
  return collapse(found, f => `${f.a}|${f.b}`);
}

/**
 * Parts that should have left by the end of a movement: still drawn, or
 * gone from sight but still taking the pointer. Also when each was last
 * seen, which shows a part that lingers after the rest has gone.
 */
export function findLingering(frames, parts) {
  const out = [];
  const last = frames[frames.length - 1];
  if (!last) return out;
  for (const part of parts) {
    const samples = last.parts?.[part] || [];
    let lastShown = null;
    for (const frame of frames) if ((frame.parts?.[part] || []).some(s => shownOf(s) && s.o >= 0.02)) lastShown = frame.t;
    const shown = samples.filter(s => shownOf(s) && s.o >= 0.02);
    const live = samples.filter(s => s.interactive && !(shownOf(s) && s.o >= 0.02));
    if (shown.length) out.push({ part, t: last.t, kind: 'still drawn', detail: `${shown.length} of ${samples.length} still drawn at the end (opacity ${shown.map(s => round(s.o, 2)).join(', ')})`, lastShown });
    if (live.length) out.push({ part, t: last.t, kind: 'still takes the pointer', detail: `${live.length} not drawn but still take the pointer at the end`, lastShown });
    if (!shown.length && !live.length) out.push({ part, t: lastShown, kind: 'left', detail: lastShown === null ? 'never drawn' : `last drawn at ${lastShown} ms`, ok: true, lastShown });
  }
  return out;
}

/** Parts that should be there, drawn and live, at the end of a movement. */
export function findMissing(frames, parts) {
  const last = frames[frames.length - 1];
  if (!last) return [];
  return parts.filter(part => !(last.parts?.[part] || []).some(s => shownOf(s) && s.o > 0.9 && s.interactive))
    .map(part => ({ part, t: last.t, detail: 'not drawn, opaque and live at the end' }));
}

/** Runs of findings in consecutive frames, kept as one with their first and last time. */
function collapse(found, keyOf) {
  const runs = [];
  const open = new Map();
  for (const f of found.sort((p, q) => p.t - q.t)) {
    const key = keyOf(f);
    const run = open.get(key);
    if (run && f.t - run.until <= 40) {
      run.until = f.t;
      run.frames++;
      run.worst = Math.max(run.worst, f.by ?? f.share ?? 0);
    } else {
      const fresh = { ...f, from: f.t, until: f.t, frames: 1, worst: f.by ?? f.share ?? 0 };
      runs.push(fresh);
      open.set(key, fresh);
    }
  }
  return runs;
}

// ---- When a part moves, and how ------------------------------------------------

/** The first and last frame at which a track's channel changes: when it moves. */
export function movingWindow(track, channel, epsilon = channel === 'o' ? 0.01 : 0.5) {
  const values = track.map(p => valueOf(p.s, channel));
  let first = -1, lastIndex = -1;
  for (let i = 1; i < values.length; i++) {
    if (values[i] === null || values[i - 1] === null) continue;
    if (Math.abs(values[i] - values[i - 1]) > epsilon) {
      if (first < 0) first = i - 1;
      lastIndex = i;
    }
  }
  if (first < 0) return null;
  return { from: track[first].t, to: track[lastIndex].t, start: values[first], end: values[lastIndex], fromIndex: first, toIndex: lastIndex };
}

/**
 * A channel as progress from its start to its end, 0 to 1, against its
 * own time, 0 to 1. Null when it does not move.
 */
export function progressCurve(track, channel) {
  const window = movingWindow(track, channel);
  if (!window || window.to === window.from) return null;
  const span = window.end - window.start;
  if (Math.abs(span) < (channel === 'o' ? 0.02 : 1)) return null;
  const points = [];
  for (let i = window.fromIndex; i <= window.toIndex; i++) {
    const v = valueOf(track[i].s, channel);
    if (v === null) continue;
    points.push({ s: (track[i].t - window.from) / (window.to - window.from), p: (v - window.start) / span });
  }
  return { ...window, span, points };
}

/** A curve's value at `s`, read between its points. */
export function sampleCurve(points, s) {
  if (!points.length) return null;
  if (s <= points[0].s) return points[0].p;
  for (let i = 1; i < points.length; i++) {
    if (s <= points[i].s) {
      const a = points[i - 1], b = points[i];
      const k = b.s === a.s ? 0 : (s - a.s) / (b.s - a.s);
      return a.p + k * (b.p - a.p);
    }
  }
  return points[points.length - 1].p;
}

/**
 * How far apart two curves are, as the largest and the mean difference in
 * progress at the same moment of their own time. `reversed` compares `b`
 * played backwards: a closing that is its opening reversed has
 * b(1 - s) = 1 - a(s) on the opening's axis.
 */
export function curveDistance(a, b, { reversed = false, samples = 41 } = {}) {
  let max = 0, sum = 0, worstAt = 0;
  for (let i = 0; i < samples; i++) {
    const s = i / (samples - 1);
    const pa = sampleCurve(a, s);
    const pb = reversed ? 1 - sampleCurve(b, 1 - s) : sampleCurve(b, s);
    const d = Math.abs(pa - pb);
    sum += d;
    if (d > max) { max = d; worstAt = s; }
  }
  return { max: round(max, 3), mean: round(sum / samples, 3), worstAt: round(worstAt, 2) };
}

/**
 * Opening against closing reversed, part by part and channel by channel:
 * how long each took, whether closing ends where opening began, and how
 * far the closing's curve is from the opening's played backwards.
 */
export function compareReversed(openFrames, closeFrames) {
  const open = tracksOf(openFrames.filter(f => !f.before));
  const close = tracksOf(closeFrames.filter(f => !f.before));
  const rows = [];
  for (const [key, openTrack] of open) {
    const closeTrack = close.get(key);
    if (!closeTrack) continue;
    for (const channel of CHANNELS) {
      const a = progressCurve(openTrack, channel);
      const b = progressCurve(closeTrack, channel);
      if (!a && !b) continue;
      const row = { key, channel, openMs: a ? round(a.to - a.from, 0) : 0, closeMs: b ? round(b.to - b.from, 0) : 0 };
      row.openFrom = a ? round(a.start, 2) : null;
      row.openTo = a ? round(a.end, 2) : null;
      row.closeFrom = b ? round(b.start, 2) : null;
      row.closeTo = b ? round(b.end, 2) : null;
      row.openStart = a ? a.from : null;
      row.closeStart = b ? b.from : null;
      if (a && b) {
        // On the opening's axis: the closing goes from its end back to its start.
        const onAxis = b.points.map(p => ({ s: p.s, p: (b.start + p.p * b.span - a.start) / a.span }));
        const flipped = onAxis.map(p => ({ s: p.s, p: 1 - p.p }));
        row.distance = curveDistance(a.points, flipped, { reversed: true });
        row.endsWhereOpenBegan = round(Math.abs(b.end - a.start), 2);
        row.startsWhereOpenEnded = round(Math.abs(b.start - a.end), 2);
      } else {
        row.distance = null;
        row.note = a ? 'moves when opening, not when closing' : 'moves when closing, not when opening';
      }
      rows.push(row);
    }
  }
  return rows;
}

/**
 * The app against the mockup, for the parts both name: how long each
 * channel moves in each, when it starts, and how far apart the curves are.
 * The mockup holds for durations and curves.
 */
export function compareWithMockup(appFrames, mockupFrames) {
  const app = tracksOf(appFrames.filter(f => !f.before));
  const mockup = tracksOf(mockupFrames.filter(f => !f.before));
  const rows = [];
  for (const [key, mockTrack] of mockup) {
    const appTrack = app.get(key);
    for (const channel of CHANNELS) {
      const m = progressCurve(mockTrack, channel);
      const a = appTrack ? progressCurve(appTrack, channel) : null;
      if (!m && !a) continue;
      const row = {
        key, channel,
        mockupMs: m ? round(m.to - m.from, 0) : 0, appMs: a ? round(a.to - a.from, 0) : 0,
        mockupStart: m ? m.from : null, appStart: a ? a.from : null,
      };
      if (m && a) row.distance = curveDistance(m.points, a.points);
      else row.note = !appTrack ? 'the part is not in the app' : m ? 'moves in the mockup, not in the app' : 'moves in the app, not in the mockup';
      rows.push(row);
    }
  }
  for (const [key] of app) {
    if (mockup.has(key)) continue;
    for (const channel of CHANNELS) {
      const a = progressCurve(app.get(key), channel);
      if (a) rows.push({ key, channel, mockupMs: 0, appMs: round(a.to - a.from, 0), appStart: a.from, mockupStart: null, note: 'not in the mockup' });
    }
  }
  return rows;
}

// ---- Frame timing ------------------------------------------------------------

/** Intervals between frame times (ms) and what they say about the rate. */
export function frameIntervals(times, { vsync = 1000 / 60 } = {}) {
  const sorted = [...times].filter(t => typeof t === 'number').sort((a, b) => a - b);
  const gaps = sorted.slice(1).map((t, i) => t - sorted[i]).filter(g => g > 0.5);
  if (!gaps.length) return { frames: sorted.length, gaps: 0 };
  const ordered = [...gaps].sort((a, b) => a - b);
  const at = q => ordered[Math.min(ordered.length - 1, Math.floor(q * ordered.length))];
  // A gap of n vsyncs is n - 1 frames that were not drawn.
  const missed = gaps.reduce((sum, g) => sum + Math.max(0, Math.round(g / vsync) - 1), 0);
  const span = sorted[sorted.length - 1] - sorted[0];
  return {
    frames: sorted.length,
    gaps: gaps.length,
    medianMs: round(at(0.5), 1),
    p95Ms: round(at(0.95), 1),
    maxMs: round(ordered[ordered.length - 1], 1),
    fps: round(gaps.length / (span / 1000), 1),
    missed,
    longGaps: gaps.map((g, i) => ({ at: round(sorted[i + 1], 1), gap: round(g, 1) })).filter(g => g.gap > 1.5 * vsync),
  };
}

/** What a DevTools trace says about frames and long tasks, from the input's mark on. */
export function traceTiming(events, { mark = 'motion:input', untilMs = Infinity } = {}) {
  const zero = (events.find(e => e.name === mark) || {}).ts;
  if (zero === undefined) return null;
  const within = e => e.ts >= zero - 50000 && (e.ts - zero) / 1000 <= untilMs;
  const ms = e => round((e.ts - zero) / 1000, 1);
  const mainThreads = new Set(events.filter(e => e.name === 'thread_name' && e.args?.name === 'CrRendererMain').map(e => `${e.pid}:${e.tid}`));
  const longTasks = events.filter(e => e.ph === 'X' && e.name === 'RunTask' && e.dur > 50000 && within(e) && (!mainThreads.size || mainThreads.has(`${e.pid}:${e.tid}`)))
    .map(e => ({ at: ms(e), ms: round(e.dur / 1000, 1) }));
  const draws = events.filter(e => (e.name === 'DrawFrame' || e.name === 'Screenshot') && within(e));
  const drawName = draws.some(e => e.name === 'DrawFrame') ? 'DrawFrame' : 'Screenshot';
  const drawn = draws.filter(e => e.name === drawName).map(ms);
  const dropped = events.filter(e => e.name === 'DroppedFrame' && within(e)).map(ms);
  const reports = events.filter(e => e.name === 'PipelineReporter' && within(e) && e.args?.chrome_frame_reporter);
  const states = {};
  for (const r of reports) {
    const state = r.args.chrome_frame_reporter.state || 'unknown';
    states[state] = (states[state] || 0) + 1;
  }
  return { drawnFrom: drawName, drawn, dropped, longTasks, frameStates: states };
}

// ---- Android -------------------------------------------------------------------

/**
 * `dumpsys gfxinfo <package> framestats`: the summary lines and, for each
 * frame in its table, how long it took from its vsync to being done.
 */
export function parseFramestats(text) {
  if (!text) return null;
  const summary = {};
  for (const [label, key] of [['Total frames rendered', 'total'], ['Janky frames', 'janky'], ['50th percentile', 'p50'], ['90th percentile', 'p90'], ['95th percentile', 'p95'], ['99th percentile', 'p99']]) {
    const found = text.match(new RegExp(`${label}: ([0-9.]+)`));
    if (found) summary[key] = Number(found[1]);
  }
  const frames = [];
  const block = text.split('---PROFILEDATA---')[1];
  if (block) {
    const lines = block.trim().split('\n').map(l => l.trim()).filter(Boolean);
    const header = lines[0].split(',');
    const col = name => header.indexOf(name);
    const [flags, intended, completed] = [col('Flags'), col('IntendedVsync'), col('FrameCompleted')];
    for (const line of lines.slice(1)) {
      const cells = line.split(',');
      if (cells.length < header.length - 1) continue;
      if (Number(cells[flags]) !== 0) continue; // a frame Android marks as not to count
      const start = Number(cells[intended]), end = Number(cells[completed]);
      if (!start || !end) continue;
      frames.push({ vsyncMs: start / 1e6, tookMs: round((end - start) / 1e6, 1) });
    }
  }
  return { summary, frames };
}

/** The presentation times (s) of the frames ffmpeg's showinfo filter printed. */
export function parseShowinfo(text) {
  const times = [];
  for (const found of text.matchAll(/pts_time:\s*([0-9.]+)/g)) times.push(Number(found[1]));
  return times;
}

export function round(value, places = 1) {
  if (value === null || value === undefined || Number.isNaN(value)) return value;
  const k = 10 ** places;
  return Math.round(value * k) / k;
}
