// The analysis of a motion trace. Pure functions over plain data, so that
// `node --test tools/motion/test/analyse.test.mjs` checks them without a
// browser.
//
// A trace is a list of frames, `{ t, parts: { name: [sample, ...] },
// overlaps: { 'a|b': share } }`, where a sample is what the page's probe
// measured of one element: `{ x, y, w, h, o, seen, visible, interactive }`
// (its box on screen in CSS pixels, its opacity as drawn, how much of it is
// not clipped, whether it is drawn, whether it takes the pointer). A part
// may match several elements; each is a track of its own, `name[i]`, unless
// the part only ever matched one. The frame before the input has `before`
// and t = -1.

export const CHANNELS = ['x', 'y', 'w', 'h', 'o', 'scroll'];

/** Less movement than this is no movement: a channel moving less is not compared. */
export const LEAST = { x: 3, y: 3, w: 3, h: 3, o: 0.05, scroll: 3 };

/** The parts that match more than one element in any of the traces. */
export function indexedParts(...traces) {
  const names = new Set();
  for (const frames of traces) {
    for (const frame of frames) {
      for (const [name, samples] of Object.entries(frame.parts || {})) if (samples.length > 1) names.add(name);
    }
  }
  return names;
}

/**
 * The tracks of a trace: key → [{ t, s }] with `s` null where the element
 * was not there. `indexed` names the parts keyed `name[i]` even when this
 * trace has one of them, so that two traces' keys line up.
 */
export function tracksOf(frames, indexed = indexedParts(frames)) {
  const most = new Map();
  for (const frame of frames) {
    for (const [name, samples] of Object.entries(frame.parts || {})) {
      most.set(name, Math.max(most.get(name) || 0, samples.length));
    }
  }
  const tracks = new Map();
  for (const [name, count] of most) {
    for (let i = 0; i < Math.max(count, 1); i++) {
      const key = indexed.has(name) ? `${name}[${i}]` : name;
      tracks.set(key, frames.map(frame => ({ t: frame.t, s: (frame.parts?.[name] || [])[i] || null, before: !!frame.before })));
    }
  }
  return tracks;
}

const shownOf = s => !!(s && s.visible);
const drawnOf = s => shownOf(s) && s.o >= 0.02;

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

/** A time for people: the frame before the input is "before the input". */
export const when = t => (t === null || t === undefined ? '–' : t < 0 ? 'before the input' : `${round(t, 1)} ms`);

/**
 * Pops: a change between two frames much bigger than the frames around
 * it, or an element that appears or goes in one frame while well opaque.
 * Steady fast movement is not a pop; a jump in the middle of it, or a part
 * that is suddenly somewhere else, is. The frame before the input counts,
 * so a part the input itself cuts in or out is a pop at the first frame.
 */
export function findPops(frames, { px = 24, opacity = 0.3, ratio = 3 } = {}) {
  const pops = [];
  const tracks = tracksOf(frames);
  // Frames at which something scrolled: what moves with the scroll then is
  // carried by it (a smooth scroll is not stepped), not a jump of its own.
  const scrolled = new Set();
  for (const track of tracks.values()) {
    for (let i = 1; i < track.length; i++) {
      const a = valueOf(track[i - 1].s, 'scroll'), b = valueOf(track[i].s, 'scroll');
      if (a !== null && b !== null && Math.abs(b - a) >= LEAST.scroll) scrolled.add(i);
    }
  }
  for (const [key, track] of tracks) {
    for (let i = 1; i < track.length; i++) {
      const a = track[i - 1].s, b = track[i].s;
      const atInput = track[i - 1].before;
      const wasShown = shownOf(a), isShown = shownOf(b);
      if (wasShown !== isShown) {
        const o = isShown ? b.o : a.o;
        // Moving into or out of sight (past the edge of what clips it) is
        // not a pop; being switched on or off where it can be seen is.
        const hidden = isShown ? a : b;
        const crossedEdge = !!hidden && hidden.w * hidden.h > 0 && hidden.seen === 0;
        if (o >= opacity && !crossedEdge) {
          const verb = isShown ? 'appears' : 'vanishes';
          pops.push({ key, t: track[i].t, from: track[i - 1].t, atInput, kind: verb, detail: `${verb} at opacity ${round(o, 2)} in one frame${atInput ? ', as the input lands' : ''}` });
        }
        continue;
      }
      if (!isShown) continue;
      for (const channel of CHANNELS) {
        if (scrolled.has(i) && (channel === 'x' || channel === 'y')) continue;
        const limit = channel === 'o' ? opacity : channel === 'scroll' ? Infinity : px;
        const d = Math.abs(valueOf(b, channel) - valueOf(a, channel));
        if (!(d >= limit)) continue;
        const around = [];
        if (i >= 2 && shownOf(track[i - 2].s)) around.push(Math.abs(valueOf(a, channel) - valueOf(track[i - 2].s, channel)));
        if (i + 1 < track.length && shownOf(track[i + 1].s)) around.push(Math.abs(valueOf(track[i + 1].s, channel) - valueOf(b, channel)));
        const steady = around.length ? Math.max(...around) : 0;
        if (d >= ratio * steady) {
          pops.push({ key, t: track[i].t, from: track[i - 1].t, atInput, kind: 'jump', channel, detail: `${channel} jumps ${round(d, 1)}${channel === 'o' ? '' : ' px'} in one frame (${round(valueOf(a, channel), 1)} → ${round(valueOf(b, channel), 1)})${atInput ? ', as the input lands' : ''}; the frames around move ${round(steady, 1)}` });
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

/**
 * Where two parts that should not cover each other are seen over one
 * another. The page's probe asks what is drawn on top where their boxes
 * meet (`frame.overlaps`): a part under a backdrop is not seen under the
 * other. Where it could not tell, the boxes alone are used, and the
 * finding says so (`stacking: false`).
 */
export function findOverlaps(frames, pairs, { minArea = 16, opacity = 0.05 } = {}) {
  const found = [];
  for (const [a, b] of pairs) {
    const key = `${a}|${b}`;
    for (const frame of frames) {
      const seen = frame.overlaps ? frame.overlaps[key] : undefined;
      if (seen !== undefined && seen !== null) {
        const drawn = name => (frame.parts?.[name] || []).some(s => shownOf(s) && s.o >= opacity);
        if (seen > 0 && drawn(a) && drawn(b)) found.push({ a, b, t: frame.t, share: seen, stacking: true });
        continue;
      }
      for (const sa of frame.parts?.[a] || []) {
        for (const sb of frame.parts?.[b] || []) {
          if (!shownOf(sa) || !shownOf(sb) || sa.o < opacity || sb.o < opacity) continue;
          const shared = overlap(sa, sb);
          if (shared >= minArea) found.push({ a, b, t: frame.t, share: round(shared / Math.min(area(sa), area(sb)), 2), stacking: false });
        }
      }
    }
  }
  return collapse(found, f => `${f.a}|${f.b}|${f.stacking}`);
}

/**
 * Parts that should have left by the end of a movement: still drawn, or
 * gone from sight but still taking the pointer. Also when each was last
 * drawn (`lastShown`, -1 for only before the input), which shows a part
 * that lingers after the rest has gone, or one cut at the input.
 */
export function findLingering(frames, parts) {
  const out = [];
  const last = frames[frames.length - 1];
  if (!last) return out;
  for (const part of parts) {
    const samples = last.parts?.[part] || [];
    let lastShown = null;
    for (const frame of frames) if ((frame.parts?.[part] || []).some(drawnOf)) lastShown = frame.t;
    const shown = samples.filter(drawnOf);
    const live = samples.filter(s => s.interactive && !drawnOf(s));
    if (shown.length) out.push({ part, t: last.t, kind: 'still drawn', detail: `${shown.length} of ${samples.length} still drawn at the end (opacity ${shown.map(s => round(s.o, 2)).join(', ')})`, lastShown });
    if (live.length) out.push({ part, t: last.t, kind: 'still takes the pointer', detail: `${live.length} not drawn but still take the pointer at the end`, lastShown });
    if (!shown.length && !live.length) {
      const detail = lastShown === null ? 'never drawn' : lastShown < 0 ? 'drawn only before the input: it went at once' : `last drawn at ${lastShown} ms`;
      out.push({ part, t: lastShown, kind: 'left', detail, ok: true, cut: lastShown !== null && lastShown < 0, lastShown });
    }
  }
  return out;
}

/**
 * Whether real time says otherwise about a part the stepped frames saw
 * vanish: still drawn well after it, in the probe's real-time samples. The
 * app's own timers (Rust's) run on the real clock, which stepping cannot
 * hold, so what one of them takes away goes early in the stepped frames.
 * `answeredAt` is when real time first saw anything change: its times
 * count from the input, the stepped frames' from the app's answer.
 */
export function contradictedInRealTime(pop, steppedFrames, samples, { slack = 100, answeredAt = 0 } = {}) {
  if (pop.kind !== 'vanishes' || pop.atInput || !samples?.length) return false;
  const track = tracksOf(samples, indexedParts(steppedFrames, samples)).get(pop.key);
  if (!track) return false;
  let lastDrawn = null;
  for (const point of track) if (drawnOf(point.s)) lastDrawn = point.t;
  // Real time counts from the input, stepped time from the app's answer.
  return lastDrawn !== null && lastDrawn - answeredAt > pop.t + slack;
}

/**
 * Stops on the way: a part that should go straight to where it ends
 * (`straight`), on each of x, y, w and h, slows down somewhere away from
 * both its ends and then picks up speed again. A single movement, on a
 * curve or a spring, only slows once it has reached its top speed; one
 * that speeds up again was sent somewhere else on the way. A spring that
 * swings past its end and back is not one: the slow point must be away
 * from both ends. Settings' rail highlight walked through every section
 * the scroll passed (#321), halting on each. Frames may come at uneven
 * times (real time), so speed is change per ms.
 */
export function findStops(frames, parts, { slow = 0.3, again = 0.25, rise = 3, margin = 0.15 } = {}) {
  const out = [];
  const tracks = tracksOf(frames);
  for (const [key, track] of tracks) {
    if (!parts.includes(key.replace(/\[\d+\]$/, ''))) continue;
    for (const channel of ['x', 'y', 'w', 'h']) {
      const window = movingWindow(track, channel);
      if (!window) continue;
      const travel = window.end - window.start;
      if (Math.abs(travel) < LEAST[channel]) continue;
      const points = [];
      for (let i = window.fromIndex; i <= window.toIndex; i++) {
        const v = valueOf(track[i].s, channel);
        if (v !== null) points.push({ t: track[i].t, v });
      }
      const speeds = [];
      for (let i = 1; i < points.length; i++) {
        const dt = points[i].t - points[i - 1].t;
        if (dt > 0) speeds.push({ t: points[i].t, v: points[i].v, speed: Math.abs(points[i].v - points[i - 1].v) / dt });
      }
      const top = Math.max(0, ...speeds.map(s => s.speed));
      if (!top) continue;
      // Away from both ends, as a share of the whole way.
      const away = v => {
        const share = (v - window.start) / travel;
        return share > margin && share < 1 - margin;
      };
      const stops = [];
      let peaked = false, low = null;
      for (const s of speeds) {
        if (!peaked) {
          if (s.speed >= again * top) peaked = true;
          continue;
        }
        if (s.speed <= slow * top && away(s.v) && (!low || s.speed <= low.speed)) {
          low = { t: s.t, v: s.v, speed: s.speed };
        } else if (low && s.speed >= again * top && s.speed >= rise * low.speed) {
          stops.push(low);
          low = null;
        }
      }
      if (!stops.length) continue;
      const where = stops.map(s => `${round(s.v, 1)} (${when(s.t)})`).join(', ');
      out.push({ key, channel, t: stops[0].t, stops, detail: `${channel} slows ${stops.length === 1 ? 'once' : `${stops.length} times`} on its way from ${round(window.start, 1)} to ${round(window.end, 1)} and sets off again: at ${where}` });
    }
  }
  return out.sort((p, q) => p.t - q.t);
}

/**
 * A page that hands over to the card it closes onto (`handsOver`): at
 * every frame, words must be seen in the card's slot, the page's or the
 * card's own. The page's are its `words` part as drawn (the page's own
 * opacity included) where its box meets the card (none, in a frame that
 * lacks the part); the card's are the
 * card as drawn and not cut, under what of the page covers it, which is
 * the share of the card the page's box covers times the page's opacity
 * (its surface hides what is under it). The card is the `index`-th of
 * `onto` when the scenario says which it opened, or else the one the
 * page's smallest drawn box covers most, the one it shrinks onto or grows
 * out of.
 * With `line`, the page's words are only the first line of them, a band
 * that high at the top of the words part, and count while half of it or
 * more is inside the page's clip, wherever the clip is, not only over the
 * card: words left at the top of a page that shrinks onto a card lower
 * down are cut off by the clip's top edge before it lands, and the box over
 * the card is empty. (Over the card would fail the first frames of every
 * close, a short note's first line being at the top of the page, nowhere
 * near its card; where the page lands is `findLanding`'s.)
 * Android's note page had faded its words in 200 ms and then covered its
 * card, empty, until it had gone 140 ms later, when the card's words came
 * back at once (#322). Runs of frames where neither reaches `least` are
 * returned, with the worst.
 */
export function findBlankSlot(frames, spec = {}) {
  const { least = 0.35 } = spec;
  const found = slotFrames(frames, spec).filter(f => f.shown < least);
  return runsOf(found).map(run => ({
    ...run,
    detail: `the slot of \`${run.key}\` shows no words from ${when(run.from)} to ${when(run.until)} (${run.frames} frame${run.frames === 1 ? '' : 's'}): at worst, at ${when(run.worst.t)}, the page's words are drawn at ${run.worst.pageWords} and the page covers the card at ${run.worst.cover}`,
  }));
}

/**
 * A page whose words in sight are not its card's (`handsOver` with
 * `otherWords`: a long note scrolled to its end) must have let them go by
 * the time the card's can be read: frames where the card's words are drawn
 * at half strength or more and the page's other words still at `strength`
 * (0.3) or more are a finding, one after another as one run. Riding onto
 * the card's title, such a note's last lines lay over the title as a ghost,
 * at half strength as the title came through and fading only with the
 * page (#322).
 */
export function findDoubled(frames, spec = {}) {
  if (!spec.otherWords) return [];
  const { strength = 0.3 } = spec;
  const found = slotFrames(frames, spec).filter(f => f.cardWords >= 0.5 && f.pageWords >= strength)
    .map(f => ({ ...f, shown: -f.pageWords }));
  return runsOf(found).map(run => ({
    ...run,
    detail: `other words are drawn over those of \`${run.key}\` from ${when(run.from)} to ${when(run.until)} (${run.frames} frame${run.frames === 1 ? '' : 's'}): at worst, at ${when(run.worst.t)}, the page's at ${run.worst.pageWords} and the card's at ${run.worst.cardWords}`,
  }));
}

/** For each frame with the card: how strongly the page's words and the card's are drawn in the card's slot. */
function slotFrames(frames, { page, words, onto, line = null, index: given = null } = {}) {
  const live = frames.filter(f => !f.before);
  const index = given ?? coveredMost(live, page, onto);
  if (index < 0) return [];
  const out = [];
  live.forEach((frame, at) => {
    const c = (frame.parts?.[onto] || [])[index];
    if (!c || !(area(c) > 0)) return;
    const p = (frame.parts?.[page] || [])[0], w = (frame.parts?.[words] || [])[0];
    const cover = shownOf(p) ? Math.min(1, overlap(p, c) / area(c)) * p.o : 0;
    let pageWords = 0;
    if (shownOf(w) && w.seen > 0) {
      const band = line ? { x: w.x, y: w.y, w: w.w, h: line } : w;
      const there = line
        ? shownOf(p) && clipTo(band, p).h >= line / 2 && clipTo(band, p).w > 0
        : overlap(w, c) > 0;
      if (there) pageWords = w.o;
    }
    const cardWords = shownOf(c) ? c.o * Math.min(1, c.seen) * (1 - cover) : 0;
    out.push({ at, t: frame.t, shown: round(Math.max(pageWords, cardWords), 2), pageWords: round(pageWords, 2), cardWords: round(cardWords, 2), cover: round(cover, 2), key: `${onto}[${index}]` });
  });
  return out;
}

/**
 * Runs of frames one after another, however far apart they came (a slow
 * device in real time), each with its worst (the least `shown`).
 */
function runsOf(found) {
  const runs = [];
  for (const f of found) {
    const run = runs[runs.length - 1];
    if (run && f.at === run.last + 1) {
      run.last = f.at;
      run.until = f.t;
      run.frames++;
      if (f.shown < run.worst.shown) run.worst = f;
    } else runs.push({ key: f.key, t: f.t, from: f.t, until: f.t, last: f.at, frames: 1, worst: f });
  }
  return runs.map(({ last, ...run }) => run);
}

/** Where the page is smallest, the index of the card it covers most there, or -1. */
function coveredMost(live, page, onto) {
  let smallest = null;
  for (const frame of live) {
    const p = (frame.parts?.[page] || [])[0];
    if (shownOf(p) && area(p) > 0 && (!smallest || area(p) < area(smallest.p))) smallest = { frame, p };
  }
  if (!smallest) return -1;
  let index = -1, most = 0;
  (smallest.frame.parts?.[onto] || []).forEach((c, i) => {
    const shared = c && area(c) > 0 ? overlap(smallest.p, c) / area(c) : 0;
    if (shared > most) { most = shared; index = i; }
  });
  return index;
}

/**
 * Where a page that closes onto a card lands (`handsOver`): its last box
 * on the screen, at any opacity, against the card's in that frame. The
 * card is the `index`-th of `onto`, the one the scenario opened: the one
 * the page covers most would be wrong for a page that lands on the place
 * of the card's neighbour. Without `index`, it is that one all the same,
 * or the nearest. A page that lands more than
 * `tolerance` px from its card on any edge is a finding: Android's note
 * page had its target measured once, as the close began, and landed on
 * the card's old place when the card moved under it (#322).
 */
export function findLanding(frames, { page, onto, index = null, tolerance = 6 } = {}) {
  const live = frames.filter(f => !f.before);
  let last = null;
  for (const frame of live) {
    const p = (frame.parts?.[page] || [])[0];
    if (p && p.o > 0 && p.w > 0.5 && p.h > 0.5 && p.seen > 0) last = { frame, p };
  }
  if (!last) return [];
  let card = index === null ? null : (last.frame.parts?.[onto] || [])[index] || null, most = 0;
  if (!card) for (const c of last.frame.parts?.[onto] || []) {
    const shared = c && area(c) > 0 ? overlap(last.p, c) / area(c) : 0;
    if (shared > most) { most = shared; card = c; }
  }
  // Nothing covered: the nearest card.
  if (!card) {
    const centre = s => [s.x + s.w / 2, s.y + s.h / 2];
    const [px, py] = centre(last.p);
    for (const c of last.frame.parts?.[onto] || []) {
      if (!c || !(area(c) > 0)) continue;
      const [cx, cy] = centre(c);
      if (!card || Math.hypot(cx - px, cy - py) < Math.hypot(centre(card)[0] - px, centre(card)[1] - py)) card = c;
    }
  }
  if (!card) return [];
  const p = last.p;
  const off = Math.max(Math.abs(p.x - card.x), Math.abs(p.y - card.y), Math.abs(p.x + p.w - card.x - card.w), Math.abs(p.y + p.h - card.y - card.h));
  if (off <= tolerance) return [];
  return [{ t: last.frame.t, off: round(off, 1), detail: `the page lands ${round(off, 1)} px from its card: last seen at ${when(last.frame.t)} at y ${round(p.y, 1)} to ${round(p.y + p.h, 1)}, the card at y ${round(card.y, 1)} to ${round(card.y + card.h, 1)}` }];
}

/** The part of box `a` inside box `b`. */
function clipTo(a, b) {
  const x = Math.max(a.x, b.x), y = Math.max(a.y, b.y);
  return { x, y, w: Math.max(0, Math.min(a.x + a.w, b.x + b.w) - x), h: Math.max(0, Math.min(a.y + a.h, b.y + b.h) - y) };
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
 * A channel's movement as a curve, against its own time (0 to 1): how far
 * it is from where it started, as a share of the furthest it gets, so that
 * a movement that goes out and comes back is measured by how far it went,
 * not by the little between its ends. Null when it moves less than LEAST.
 */
export function progressCurve(track, channel) {
  const window = movingWindow(track, channel);
  if (!window || window.to === window.from) return null;
  let furthest = 0;
  for (let i = window.fromIndex; i <= window.toIndex; i++) {
    const v = valueOf(track[i].s, channel);
    if (v !== null && Math.abs(v - window.start) > Math.abs(furthest)) furthest = v - window.start;
  }
  if (Math.abs(furthest) < LEAST[channel]) return null;
  const points = [];
  for (let i = window.fromIndex; i <= window.toIndex; i++) {
    const v = valueOf(track[i].s, channel);
    if (v === null) continue;
    points.push({ s: (track[i].t - window.from) / (window.to - window.from), p: (v - window.start) / furthest });
  }
  return { ...window, furthest, points };
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
 * How far apart two curves are, as the largest and the mean difference at
 * the same moment of their own time, in shares of the furthest each goes:
 * 0 is the same curve, 1 (the most it reports) a whole movement apart.
 * `reversed` compares `b` played backwards: a closing that is its opening
 * reversed has b(1 - s) = 1 - a(s).
 */
export function curveDistance(a, b, { reversed = false, samples = 41 } = {}) {
  let max = 0, sum = 0, worstAt = 0;
  for (let i = 0; i < samples; i++) {
    const s = i / (samples - 1);
    const pa = sampleCurve(a, s);
    const pb = reversed ? 1 - sampleCurve(b, 1 - s) : sampleCurve(b, s);
    const d = Math.min(1, Math.abs(pa - pb));
    sum += d;
    if (d > max) { max = d; worstAt = s; }
  }
  return { max: round(max, 3), mean: round(sum / samples, 3), worstAt: round(worstAt, 2) };
}

/**
 * Where a part was first drawn (opening) or last drawn (closing), on a
 * channel. Closing, a part that fades as it lands (Android's note fading
 * into its card, #322) is too faint to count as drawn for its last few
 * pixels, though it goes on to its end: where it went is where it was last
 * on the screen at any opacity above none. Once it is at none, it has gone,
 * and where it is put then is not where it went.
 */
function seenAt(track, channel, last) {
  const order = last ? [...track].reverse() : track;
  const laidOut = s => !!s && s.o > 0 && s.seen > 0 && s.w > 0.5 && s.h > 0.5 && valueOf(s, channel) !== null;
  const point = order.find(p => !p.before && (drawnOf(p.s) || (last && laidOut(p.s))));
  return point ? valueOf(point.s, channel) : null;
}

/**
 * A closing against the opening it is meant to mirror, as the scenario
 * declares from the design (`mirrors`: its parts and channels, and whether
 * the curve is meant to be the opening's reversed or only the path). For
 * each: how long each took; where the closing was last drawn against where
 * the opening was first drawn (for opacity: where each began and ended);
 * and, when the curve is meant to mirror, how far it is from the opening's
 * played backwards.
 */
export function compareMirror(openFrames, closeFrames, spec) {
  const indexed = indexedParts(openFrames, closeFrames);
  const open = tracksOf(openFrames, indexed);
  const close = tracksOf(closeFrames, indexed);
  const rows = [];
  for (const [part, channels] of Object.entries(spec.parts || {})) {
    const keys = [...new Set([...open.keys(), ...close.keys()])].filter(k => k === part || k.startsWith(`${part}[`));
    for (const key of keys) {
      const openTrack = open.get(key), closeTrack = close.get(key);
      if (!openTrack || !closeTrack) { rows.push({ key, channel: '–', note: openTrack ? 'not there when closing' : 'not there when opening' }); continue; }
      for (const channel of channels) {
        const a = progressCurve(openTrack, channel);
        const b = progressCurve(closeTrack, channel);
        // A change from the frame before the input to the first is a cut: it takes no time.
        const took = c => (c ? round(c.to - Math.max(0, c.from), 0) : 0);
        const row = { key, channel, openMs: took(a), closeMs: took(b) };
        const cut = c => c && c.from < 0 && c.to <= 0;
        if (channel === 'o') {
          row.openFrom = valueOf(openTrack[0].s, 'o');
          row.closeTo = valueOf(closeTrack[closeTrack.length - 1].s, 'o');
        } else {
          row.openFrom = seenAt(openTrack, channel, false);
          row.closeTo = seenAt(closeTrack, channel, true);
        }
        if (row.openFrom !== null && row.closeTo !== null) {
          row.away = round(Math.abs(row.closeTo - row.openFrom), channel === 'o' ? 2 : 1);
          row.endsAway = row.away > LEAST[channel] * (channel === 'o' ? 1 : 2);
        }
        if (spec.curve && a && b) row.distance = curveDistance(a.points, b.points, { reversed: true });
        if (!a && b) row.note = 'moves when closing, not when opening';
        if (a && !b) row.note = 'moves when opening, not when closing';
        if (cut(a) || cut(b)) row.note = [cut(a) && 'cut at the input when opening', cut(b) && 'cut at the input when closing', row.note].filter(Boolean).join('; ');
        rows.push(row);
      }
    }
  }
  return rows;
}

/**
 * The app against the mockup, for the parts both name: how long each
 * channel moves in each, when it starts, and how far apart the curves are.
 * The mockup holds for durations and curves. Channels that move less than
 * LEAST in both are left out.
 */
export function compareWithMockup(appFrames, mockupFrames) {
  const indexed = indexedParts(appFrames, mockupFrames);
  const app = tracksOf(appFrames.filter(f => !f.before), indexed);
  const mockup = tracksOf(mockupFrames.filter(f => !f.before), indexed);
  const rows = [];
  for (const key of new Set([...mockup.keys(), ...app.keys()])) {
    const mockTrack = mockup.get(key), appTrack = app.get(key);
    for (const channel of CHANNELS) {
      const m = mockTrack ? progressCurve(mockTrack, channel) : null;
      const a = appTrack ? progressCurve(appTrack, channel) : null;
      if (!m && !a) continue;
      const row = {
        key, channel,
        mockupMs: m ? round(m.to - m.from, 0) : 0, appMs: a ? round(a.to - a.from, 0) : 0,
        mockupStart: m ? m.from : null, appStart: a ? a.from : null,
      };
      if (m && a) row.distance = curveDistance(m.points, a.points);
      else if (!appTrack) row.note = 'the part is not in the app';
      else if (!mockTrack) row.note = 'not in the mockup';
      else row.note = m ? 'moves in the mockup, not in the app' : 'moves in the app, not in the mockup';
      rows.push(row);
    }
  }
  return rows;
}

// ---- Frame timing ------------------------------------------------------------

function signature(frame) {
  const out = [];
  for (const name of Object.keys(frame.parts || {}).sort()) {
    for (const s of frame.parts[name]) out.push(s ? `${Math.round(s.x)},${Math.round(s.y)},${Math.round(s.w)},${Math.round(s.h)},${round(valueOf(s, 'o'), 2)},${s.scroll ?? ''}` : '-');
  }
  return out.join(';');
}

/**
 * When something moved, from the probe's samples: the spans between
 * samples that differ, joined where they are close. Outside them nothing
 * was asked of the screen, so a long gap there is not a frame missed.
 */
export function activeSpans(samples, { join = 60 } = {}) {
  const spans = [];
  for (let i = 1; i < samples.length; i++) {
    if (signature(samples[i]) === signature(samples[i - 1])) continue;
    const from = samples[i - 1].t, to = samples[i].t;
    const last = spans[spans.length - 1];
    if (last && from - last.to <= join) last.to = to;
    else spans.push({ from, to });
  }
  return spans;
}

/**
 * Intervals between frame times (ms) and what they say about the rate.
 * With `active` spans, only the gaps while something moved are judged;
 * without, every gap is, and `judged` says so.
 */
export function frameIntervals(times, { vsync = 1000 / 60, active = null } = {}) {
  const sorted = [...times].filter(t => typeof t === 'number').sort((a, b) => a - b);
  let gaps = sorted.slice(1).map((t, i) => ({ from: sorted[i], to: t, gap: t - sorted[i] })).filter(g => g.gap > 0.5);
  const judged = active ? 'while something moved' : 'over the whole span';
  if (active) gaps = gaps.filter(g => active.some(span => g.from < span.to && g.to > span.from));
  if (!gaps.length) return { frames: sorted.length, gaps: 0, judged };
  const ordered = gaps.map(g => g.gap).sort((a, b) => a - b);
  const at = q => ordered[Math.min(ordered.length - 1, Math.floor(q * ordered.length))];
  // A gap of n vsyncs is n - 1 frames that were not drawn.
  const missed = gaps.reduce((sum, g) => sum + Math.max(0, Math.round(g.gap / vsync) - 1), 0);
  const span = gaps.reduce((sum, g) => sum + g.gap, 0);
  return {
    frames: sorted.length,
    gaps: gaps.length,
    judged,
    medianMs: round(at(0.5), 1),
    p95Ms: round(at(0.95), 1),
    maxMs: round(ordered[ordered.length - 1], 1),
    fps: round(gaps.length / (span / 1000), 1),
    missed,
    longGaps: gaps.filter(g => g.gap > 1.5 * vsync).map(g => ({ at: round(g.to, 1), gap: round(g.gap, 1) })),
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
