// node --test tools/motion/test/analyse.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import {
  tracksOf, valueOf, findPops, findOutside, findOverlaps, findLingering, findMissing, beyond,
  movingWindow, progressCurve, sampleCurve, curveDistance, compareMirror, compareWithMockup,
  frameIntervals, activeSpans, traceTiming, parseFramestats, parseShowinfo, indexedParts, when, contradictedInRealTime, findStops, findBlankSlot, findLanding, findDoubled,
} from '../lib/analyse.mjs';
import { mergeRows, trustOf } from '../lib/report.mjs';
import { columnsFor } from '../lib/sheets.mjs';
import { pick, partsFor, writes } from '../lib/capture.mjs';

const box = (x, y, w, h, o = 1, more = {}) => ({ x, y, w, h, o, seen: 1, visible: o > 0.01, interactive: o > 0.01, ...more });
const frame = (t, parts) => ({ t, parts });
// A part moving along `values` on one channel, a frame every 16.7 ms.
const along = (values, make) => values.map((v, i) => frame(Math.round(i * 16.7), { a: [make(v)] }));

test('tracks: one per element when a part matches several, plain name when one', () => {
  const tracks = tracksOf([frame(0, { one: [box(0, 0, 1, 1)], many: [box(0, 0, 1, 1), box(1, 1, 1, 1)] })]);
  assert.deepEqual([...tracks.keys()].sort(), ['many[0]', 'many[1]', 'one']);
});

test('valueOf: an element not drawn is opacity 0, and a box of no size has no place', () => {
  assert.equal(valueOf(null, 'o'), 0);
  assert.equal(valueOf(box(5, 5, 10, 10, 0), 'o'), 0);
  assert.equal(valueOf({ x: 0, y: 0, w: 0, h: 0, o: 0, visible: false }, 'x'), null);
  assert.equal(valueOf(box(5, 6, 10, 10), 'y'), 6);
});

test('pops: a cut at the input, against the frame before it, is one', () => {
  const frames = [{ t: -1, before: true, parts: { a: [box(0, 0, 10, 10, 0)] } }, frame(0, { a: [box(0, 0, 10, 10, 1)] }), frame(17, { a: [box(0, 0, 10, 10, 1)] })];
  const [pop] = findPops(frames);
  assert.equal(pop.atInput, true);
  assert.equal(pop.t, 0);
  assert.equal(when(-1), 'before the input');
});

test('lingering: a part drawn only before the input went at once', () => {
  const frames = [{ t: -1, before: true, parts: { a: [box(0, 0, 10, 10)] } }, frame(0, { a: [box(0, 0, 10, 10, 0)] })];
  const [left] = findLingering(frames, ['a']);
  assert.equal(left.cut, true);
  assert.match(left.detail, /only before the input/);
});

test('pops: steady fast movement is not one', () => {
  const frames = along([0, 30, 60, 90, 120, 150], v => box(v, 0, 10, 10));
  assert.deepEqual(findPops(frames), []);
});

test('pops: a jump in the middle of slow movement is one, at its frame', () => {
  const frames = along([0, 2, 4, 80, 82, 84], v => box(v, 0, 10, 10));
  const pops = findPops(frames);
  assert.equal(pops.length, 1);
  assert.equal(pops[0].channel, 'x');
  assert.equal(pops[0].t, 50);
});

test('pops: appearing at full opacity in one frame is one; fading in is not', () => {
  const sudden = [frame(0, { a: [box(0, 0, 10, 10, 0)] }), frame(17, { a: [box(0, 0, 10, 10, 1)] })];
  assert.equal(findPops(sudden)[0].kind, 'appears');
  const fading = along([0, 0.1, 0.2, 0.3, 0.4], o => box(0, 0, 10, 10, o));
  assert.deepEqual(findPops(fading), []);
});

test('pops: an element that is inserted at full opacity pops; one that scrolls into sight does not', () => {
  const inserted = [frame(0, { a: [] }), frame(17, { a: [box(0, 0, 10, 10)] })];
  assert.equal(findPops(inserted).length, 1);
  const scrolledIn = [frame(0, { a: [box(0, 0, 10, 10, 1, { seen: 0, visible: false })] }), frame(17, { a: [box(0, 0, 10, 10)] })];
  assert.deepEqual(findPops(scrolledIn), []);
});

test('outside: how far a part reaches past its container, as one run of frames', () => {
  assert.equal(beyond({ x: -5, y: 0, w: 10, h: 10 }, { x: 0, y: 0, w: 100, h: 100 }), 5);
  const frames = [0, 17, 33, 50].map((t, i) => frame(t, { panel: [box(0, 0, 100, 100)], child: [box(i < 3 ? 95 : 50, 0, 10, 10)] }));
  const runs = findOutside(frames, [['child', 'panel']]);
  assert.equal(runs.length, 1);
  assert.equal(runs[0].from, 0);
  assert.equal(runs[0].until, 33);
  assert.equal(runs[0].worst, 5);
});

test('outside: the viewport as the container', () => {
  const frames = [frame(0, { a: [box(-30, 0, 50, 50)] })];
  assert.equal(findOutside(frames, [['a', 'viewport']], { viewport: { width: 400, height: 400 } })[0].worst, 30);
});

test('overlaps: what the page says is on top wins over the boxes', () => {
  const frames = [
    { t: 0, parts: { a: [box(0, 0, 50, 50)], b: [box(25, 25, 50, 50)] }, overlaps: { 'a|b': 0 } },
    { t: 17, parts: { a: [box(0, 0, 50, 50)], b: [box(25, 25, 50, 50)] }, overlaps: { 'a|b': 0.5 } },
    { t: 33, parts: { a: [box(0, 0, 50, 50)], b: [box(25, 25, 50, 50)] }, overlaps: { 'a|b': null } },
  ];
  const runs = findOverlaps(frames, [['a', 'b']]);
  assert.equal(runs.length, 2);
  assert.equal(runs.find(r => r.stacking).from, 17);
  assert.equal(runs.find(r => !r.stacking).from, 33);
});

test('overlaps: only while both are drawn', () => {
  const frames = [
    frame(0, { a: [box(0, 0, 50, 50)], b: [box(25, 25, 50, 50)] }),
    frame(17, { a: [box(0, 0, 50, 50)], b: [box(25, 25, 50, 50, 0)] }),
  ];
  const runs = findOverlaps(frames, [['a', 'b']]);
  assert.equal(runs.length, 1);
  assert.equal(runs[0].frames, 1);
  assert.equal(runs[0].worst, 0.25);
});

test('lingering: still drawn, or gone but still taking the pointer, at the end', () => {
  const end = [frame(0, { p: [box(0, 0, 10, 10)] }), frame(17, { p: [box(0, 0, 10, 10, 0.5)] })];
  assert.equal(findLingering(end, ['p'])[0].kind, 'still drawn');
  const live = [frame(0, { p: [box(0, 0, 10, 10)] }), frame(17, { p: [{ ...box(0, 0, 10, 10, 0), interactive: true }] })];
  assert.equal(findLingering(live, ['p'])[0].kind, 'still takes the pointer');
  const gone = [frame(0, { p: [box(0, 0, 10, 10)] }), frame(17, { p: [box(0, 0, 10, 10, 0)] })];
  const left = findLingering(gone, ['p'])[0];
  assert.equal(left.ok, true);
  assert.equal(left.lastShown, 0);
});

test('missing: a part that should have arrived and is not opaque and live', () => {
  const frames = [frame(0, { p: [box(0, 0, 10, 10, 0.5)] })];
  assert.equal(findMissing(frames, ['p']).length, 1);
  assert.equal(findMissing([frame(0, { p: [box(0, 0, 10, 10)] })], ['p']).length, 0);
});

test('movingWindow and progressCurve: when and how a channel moves', () => {
  const track = tracksOf(along([0, 0, 10, 50, 90, 100, 100], v => box(v, 0, 10, 10))).get('a');
  const window = movingWindow(track, 'x');
  assert.equal(window.from, 17);
  assert.equal(window.to, 84);
  const curve = progressCurve(track, 'x');
  assert.equal(curve.points[0].p, 0);
  assert.equal(curve.points[curve.points.length - 1].p, 1);
  assert.equal(progressCurve(tracksOf(along([5, 5, 5], v => box(v, 0, 1, 1))).get('a'), 'x'), null);
});

test('sampleCurve reads between points; curveDistance is 0 for the same curve and its exact reverse', () => {
  const ease = [0, 0.25, 0.5, 0.75, 1].map(s => ({ s, p: s * s }));
  assert.equal(sampleCurve(ease, 0.5), 0.25);
  assert.equal(curveDistance(ease, ease).max, 0);
  // Played backwards: b(s) = 1 - a(1 - s).
  const reversed = ease.map(({ s }) => ({ s, p: 1 - (1 - s) * (1 - s) }));
  assert.equal(curveDistance(ease, reversed, { reversed: true }).max, 0);
  assert.ok(curveDistance(ease, ease, { reversed: true }).max > 0.2);
});

test('compareMirror: a closing that is the opening reversed scores 0 and ends where the opening began', () => {
  const open = along([0, 10, 40, 90, 100], v => box(v, 0, 10, 10));
  const close = along([100, 90, 40, 10, 0], v => box(v, 0, 10, 10));
  const [row] = compareMirror(open, close, { parts: { a: ['x'] }, curve: true });
  assert.equal(row.channel, 'x');
  // Frame times are whole milliseconds, so not quite even: near 0, not 0.
  assert.ok(row.distance.max < 0.05);
  assert.equal(row.away, 0);
  assert.equal(row.endsAway, false);
});

test('compareMirror: a closing that fades where it is is last seen far from where the opening was first seen', () => {
  // Opening: from a small box at 300 to a big one at 100, drawn throughout.
  const open = along([300, 200, 100], v => box(v, v, 400 - v, 400 - v));
  // Closing: fades out in place, then (invisible) snaps back.
  const close = [frame(-1, { a: [box(100, 100, 300, 300)] }), ...along([1, 0.5, 0], o => box(110, 110, 290, 290, o)), frame(60, { a: [box(100, 100, 300, 300, 0)] })];
  close[0].before = true;
  const rows = compareMirror(open, close, { parts: { a: ['x', 'w'] }, curve: false });
  const x = rows.find(r => r.channel === 'x');
  assert.equal(x.openFrom, 300);
  assert.equal(x.closeTo, 110);
  assert.equal(x.endsAway, true);
  assert.equal(x.distance, undefined);
});

test('compareMirror: only the parts and channels the scenario names', () => {
  const open = along([0, 50, 100], v => box(v, v, 10, 10));
  const close = along([100, 50, 0], v => box(v, v, 10, 10));
  const rows = compareMirror(open, close, { parts: { a: ['y'] } });
  assert.deepEqual(rows.map(r => r.channel), ['y']);
});

test('compareMirror and compareWithMockup line up parts with more elements in one trace than the other', () => {
  const one = [frame(0, { b: [box(0, 0, 10, 10)] }), frame(17, { b: [box(50, 0, 10, 10)] })];
  const two = [frame(0, { b: [box(0, 0, 10, 10), box(0, 20, 10, 10)] }), frame(17, { b: [box(50, 0, 10, 10), box(50, 20, 10, 10)] })];
  assert.deepEqual([...indexedParts(one, two)], ['b']);
  const rows = compareWithMockup(one, two);
  assert.ok(rows.some(r => r.key === 'b[0]' && r.distance));
  assert.ok(rows.some(r => r.key === 'b[1]' && r.note === 'the part is not in the app'));
});

test('progressCurve: a part that goes out and comes back is measured by how far it went; less than a few px is no movement', () => {
  const track = tracksOf(along([500, 600, 700, 600, 501], v => box(0, v, 10, 10))).get('a');
  const curve = progressCurve(track, 'y');
  assert.equal(Math.max(...curve.points.map(p => p.p)), 1);
  assert.ok(curve.points.every(p => Math.abs(p.p) <= 1));
  assert.equal(progressCurve(tracksOf(along([10, 11, 12, 11], v => box(0, v, 10, 10))).get('a'), 'y'), null);
});

test('curveDistance is at most 1', () => {
  const a = [{ s: 0, p: 0 }, { s: 1, p: 1 }];
  const b = [{ s: 0, p: 0 }, { s: 0.5, p: -3 }, { s: 1, p: 1 }];
  assert.ok(curveDistance(a, b).max <= 1);
});

test('compareWithMockup: durations, start times and parts one side lacks', () => {
  const mockup = along([0, 0, 50, 100, 100], v => box(v, 0, 10, 10));
  const app = along([0, 100, 100, 100, 100], v => box(v, 0, 10, 10));
  const [row] = compareWithMockup(app, mockup);
  assert.equal(row.mockupMs, 33);
  assert.equal(row.appMs, 17);
  assert.equal(row.mockupStart, 17);
  assert.equal(row.appStart, 0);
  const lacking = compareWithMockup([frame(0, {})], mockup);
  assert.equal(lacking[0].note, 'the part is not in the app');
});

test('frameIntervals: only the gaps while something moved count, with active spans', () => {
  const times = [0, 16.7, 33.3, 50, 400, 416.7];
  assert.equal(frameIntervals(times).missed, 20);
  const judged = frameIntervals(times, { active: [{ from: 0, to: 50 }] });
  assert.equal(judged.missed, 0);
  assert.equal(judged.judged, 'while something moved');
});

test('activeSpans: when the probe saw something change', () => {
  const samples = [0, 17, 33, 50, 300, 317].map((t, i) => frame(t, { a: [box(i < 3 ? i * 10 : 20, 0, 10, 10)] }));
  assert.deepEqual(activeSpans(samples), [{ from: 0, to: 33 }]);
});

test('frameIntervals: rate, gaps, and frames not drawn', () => {
  const times = [0, 16.7, 33.3, 50, 100, 116.7];
  const t = frameIntervals(times);
  assert.equal(t.frames, 6);
  assert.equal(t.missed, 2);
  assert.equal(t.longGaps.length, 1);
  assert.equal(t.longGaps[0].at, 100);
  assert.equal(frameIntervals([5]).gaps, 0);
});

test('traceTiming: frames, long tasks and Chromium\'s frame states from the mark on', () => {
  const events = [
    { name: 'thread_name', ph: 'M', pid: 1, tid: 2, args: { name: 'CrRendererMain' } },
    { name: 'motion:input', ts: 1000000 },
    { name: 'RunTask', ph: 'X', pid: 1, tid: 2, ts: 1010000, dur: 80000 },
    { name: 'RunTask', ph: 'X', pid: 1, tid: 9, ts: 1010000, dur: 90000 },
    { name: 'DrawFrame', ts: 1016000 }, { name: 'DrawFrame', ts: 1033000 },
    { name: 'PipelineReporter', ts: 1020000, args: { chrome_frame_reporter: { state: 'STATE_DROPPED' } } },
  ];
  const timing = traceTiming(events);
  assert.deepEqual(timing.longTasks, [{ at: 10, ms: 80 }]);
  assert.deepEqual(timing.drawn, [16, 33]);
  assert.equal(timing.frameStates.STATE_DROPPED, 1);
  assert.equal(traceTiming([]), null);
});

test('parseFramestats: the summary and each counted frame', () => {
  const text = `Total frames rendered: 30\nJanky frames: 12 (40.00%)\n50th percentile: 19ms\n90th percentile: 48ms\n95th percentile: 53ms\n99th percentile: 65ms\n---PROFILEDATA---\nFlags,IntendedVsync,Vsync,FrameCompleted,\n0,1000000000,1000000000,1020000000,\n1,1016000000,1016000000,1030000000,\n---PROFILEDATA---\n`;
  const stats = parseFramestats(text);
  assert.equal(stats.summary.total, 30);
  assert.equal(stats.summary.janky, 12);
  assert.equal(stats.summary.p90, 48);
  assert.equal(stats.frames.length, 1);
  assert.equal(stats.frames[0].tookMs, 20);
  assert.equal(parseFramestats(''), null);
});

test('parseShowinfo: each frame\'s time from ffmpeg', () => {
  assert.deepEqual(parseShowinfo('[Parsed_showinfo_1] n:   0 pts:      0 pts_time:0  \n[Parsed_showinfo_1] n:   1 pts:  1234 pts_time:0.0411 '), [0, 0.0411]);
});

test('mergeRows: elements of one part moving alike become one row', () => {
  const rows = mergeRows([
    { key: 'b[0]', channel: 'x', ms: 10 }, { key: 'b[1]', channel: 'x', ms: 10 }, { key: 'b[2]', channel: 'x', ms: 12 }, { key: 'a', channel: 'x', ms: 10 },
  ]);
  assert.deepEqual(rows.map(r => r.key), ['b[0,1]', 'b[2]', 'a']);
});

test('trustOf: real time is judged only when the page was drawn near the display\'s rate', () => {
  assert.equal(trustOf([0, 16.7, 33.4, 50.1]).ok, true);
  assert.equal(trustOf([0, 100, 200, 300]).ok, false);
  assert.equal(trustOf([0]).ok, false);
});

test('columnsFor: wide frames in fewer columns than tall ones', () => {
  assert.ok(columnsFor(1.5) < columnsFor(0.5));
});

test('pick and partsFor: a selector for every platform, or per platform with the mockup\'s fallback', () => {
  assert.equal(pick('.a', 'android'), '.a');
  assert.equal(pick({ desktop: '.d', mockup: '.m' }, 'mockup-android'), '.m');
  assert.equal(pick({ desktop: '.d' }, 'android'), null);
  assert.deepEqual(partsFor({ parts: { p: { desktop: '.d' }, q: '.q' } }, 'android'), { q: '.q' });
});

test('contradictedInRealTime: a part stepped frames saw vanish that real time still draws', () => {
  const stepped = [frame(0, { a: [box(0, 0, 10, 10)] }), frame(33, { a: [box(0, 0, 10, 10, 0)] })];
  const [pop] = findPops(stepped);
  const late = [0, 100, 200, 300].map(t => frame(t, { a: [box(0, 0, 10, 10, t < 300 ? 1 : 0)] }));
  assert.equal(contradictedInRealTime(pop, stepped, late), true);
  const early = [0, 17, 33, 50].map(t => frame(t, { a: [box(0, 0, 10, 10, t < 33 ? 1 : 0)] }));
  assert.equal(contradictedInRealTime(pop, stepped, early), false);
});

test('pops: what a scroll carries in the same frame is not a jump of its own', () => {
  const frames = [
    frame(0, { list: [{ ...box(0, 0, 100, 100), scroll: 0 }], row: [box(0, 300, 100, 20)] }),
    frame(17, { list: [{ ...box(0, 0, 100, 100), scroll: 300 }], row: [box(0, 0, 100, 20)] }),
    frame(33, { list: [{ ...box(0, 0, 100, 100), scroll: 300 }], row: [box(0, 0, 100, 20)] }),
  ];
  assert.deepEqual(findPops(frames), []);
});

test('contradictedInRealTime: real time counted from the app\'s answer, not the input', () => {
  const stepped = [frame(0, { a: [box(0, 0, 10, 10)] }), frame(350, { a: [box(0, 0, 10, 10, 0)] })];
  const [pop] = findPops(stepped);
  const late = [300, 500, 700, 726].map(t => frame(t, { a: [box(0, 0, 10, 10, t < 726 ? 1 : 0)] }));
  assert.equal(contradictedInRealTime(pop, stepped, late, { answeredAt: 0 }), true);
  assert.equal(contradictedInRealTime(pop, stepped, late, { answeredAt: 380 }), false);
});

test('writes: the steps refused on a real phone without --allow-writes', () => {
  assert.equal(writes({ type: 'a thought' }), true);
  assert.equal(writes({ pastePicture: '[data-dui-typed]' }), true);
  assert.equal(writes({ tap: '.archive', writes: true }), true);
  assert.equal(writes({ tap: "button[aria-label='Settings']" }), false);
  assert.equal(writes({ key: 'Escape' }), false);
  // A character key types it; with Ctrl it is a shortcut.
  assert.equal(writes({ key: 'x' }), true);
  assert.equal(writes({ key: 'o', ctrl: true }), false);
});

test('stops: a part that halts on the way and sets off again, not one that eases in or swings past its end', () => {
  const y = v => box(0, v, 10, 10);
  // Two steps of a walk: 0 to 34, a halt, then on to 68.
  const walk = along([0, 10, 22, 30, 33, 34, 34, 34, 34, 44, 56, 64, 67, 68, 68], y);
  const [found] = findStops(walk, ['a']);
  assert.equal(found.channel, 'y');
  assert.equal(found.stops.length, 1);
  assert.equal(found.stops[0].v, 34);
  // The rail's highlight as measured in real time (#321), at uneven times.
  const rail = [[72, 141.8], [90, 193.8], [108, 158.9], [125, 150.8], [142, 146.8], [160, 161.4], [179, 186.5], [196, 194.5], [213, 198.8], [231, 201.8], [249, 203.8], [266, 224.1], [284, 230.8], [302, 234.7], [338, 238.9], [391, 241.8], [462, 243.4], [533, 243.8]]
    .map(([t, v]) => frame(t, { a: [y(v)] }));
  assert.equal(findStops(rail, ['a'])[0].stops[0].v, 203.8);
  // Straight there on an ease-out, and a spring that swings 2% past its end.
  assert.deepEqual(findStops(along([0, 30, 52, 66, 75, 80, 83, 84.5, 85], y), ['a']), []);
  assert.deepEqual(findStops(along([0, 40, 80, 96, 102, 101, 100, 100], y), ['a']), []);
  // Only the parts named.
  assert.deepEqual(findStops(walk, ['b']), []);
});

// A page shrinking from the screen onto the second of two cards at y 200,
// its words fading by `words(t)` and the page itself by `page(t)`.
function closing({ words, page }) {
  const frames = [{ t: -1, before: true, parts: { page: [box(0, 0, 400, 800)], text: [box(0, 60, 400, 740)], card: [box(10, 100, 380, 80), box(10, 200, 380, 80)] } }];
  for (let t = 0; t <= 340; t += 17) {
    const k = Math.min(1, t / 200);
    const shape = { x: 10 * k, y: 200 * k, w: 400 - 20 * k, h: 800 - 720 * k };
    const p = page(t), w = words(t) * p;
    frames.push(frame(t, {
      page: [box(shape.x, shape.y, shape.w, shape.h, p)],
      text: [box(0, 60, 400, 740, w, { seen: 0.5, visible: w > 0.01 })],
      card: [box(10, 100, 380, 80), box(10, 200, 380, 80)],
    }));
  }
  frames.push(frame(357, { page: [box(0, 0, 0, 0, 0, { visible: false })], text: [box(0, 0, 0, 0, 0, { visible: false })], card: [box(10, 100, 380, 80), box(10, 200, 380, 80)] }));
  return frames;
}

test('blank slot: words faded early, then the page covers its card empty until it goes (#322)', () => {
  const frames = closing({ words: t => Math.max(0, 1 - t / 200), page: () => 1 });
  const [run, more] = findBlankSlot(frames, { page: 'page', words: 'text', onto: 'card' });
  assert.equal(more, undefined);
  assert.equal(run.key, 'card[1]');
  assert.ok(run.from > 100 && run.from < 150, `from ${run.from}`);
  assert.equal(run.until, 340);
  assert.equal(run.worst.cover, 1);
  assert.equal(run.worst.pageWords, 0);
});

test('blank slot: a page that fades as a whole into its card hands over at every frame', () => {
  const frames = closing({ words: () => 1, page: t => (t < 140 ? 1 : Math.max(0, 1 - (t - 140) / 200)) });
  assert.deepEqual(findBlankSlot(frames, { page: 'page', words: 'text', onto: 'card' }), []);
});

test('blank slot: a page beside the list (the desktop) covers no card, and has none to hand over', () => {
  const frames = [frame(0, { page: [box(400, 0, 400, 800)], text: [box(400, 0, 400, 800, 0)], card: [box(0, 0, 380, 80)] })];
  assert.deepEqual(findBlankSlot(frames, { page: 'page', words: 'text', onto: 'card' }), []);
});

test('blank slot: blank frames one after another are one run, however far apart they came', () => {
  const slow = closing({ words: t => Math.max(0, 1 - t / 100), page: () => 1 }).filter((f, i) => f.before || i % 5 === 0 || f.t > 340);
  const runs = findBlankSlot(slow, { page: 'page', words: 'text', onto: 'card' });
  assert.equal(runs.length, 1);
  assert.ok(runs[0].frames >= 2);
});

test('compareMirror: a closing that fades as it lands is last seen where it was last laid out', () => {
  const open = [frame(0, { a: [box(0, 100, 100, 50)] }), frame(17, { a: [box(0, 50, 100, 100)] }), frame(34, { a: [box(0, 0, 100, 200)] })];
  const close = [frame(0, { a: [box(0, 0, 100, 200)] }), frame(17, { a: [box(0, 60, 100, 90, 0.01, { visible: false })] }), frame(34, { a: [box(0, 100, 100, 50, 0.001, { visible: false })] }), frame(51, { a: [box(0, 0, 100, 200, 0, { visible: false })] })];
  const [y] = compareMirror(open, close, { parts: { a: ['y'] } });
  assert.equal(y.closeTo, 100);
  assert.equal(y.endsAway, false);
});

test('blank slot: with `line`, words left at the top of the page are cut off by the clip before it lands on the card', () => {
  // The page fades from 140 ms, but its first line, at y 60 to 90, stays
  // where it is while the clip's top edge passes it on its way to 200.
  const frames = closing({ words: () => 1, page: t => (t < 140 ? 1 : Math.max(0, 1 - (t - 140) / 200)) });
  assert.deepEqual(findBlankSlot(frames, { page: 'page', words: 'text', onto: 'card' }), []);
  const [run] = findBlankSlot(frames, { page: 'page', words: 'text', onto: 'card', line: 30 });
  assert.ok(run, 'a blank run');
  assert.ok(run.from < 140, `from ${run.from}`);
});

test('blank slot: with `line`, words that ride with the clip onto the card hand over', () => {
  const frames = closing({ words: () => 1, page: t => (t < 140 ? 1 : Math.max(0, 1 - (t - 140) / 200)) });
  for (const f of frames) {
    const p = f.parts.page[0], w = f.parts.text[0];
    if (p.visible && w.visible) w.y = p.y + 4;
  }
  assert.deepEqual(findBlankSlot(frames, { page: 'page', words: 'text', onto: 'card', line: 30 }), []);
});

test('blank slot: a frame with the card but no words part counts as no page words, and does not throw', () => {
  for (const line of [null, 30]) {
    const frames = closing({ words: () => 1, page: () => 1 });
    frames[5].parts.text = [];
    const runs = findBlankSlot(frames, { page: 'page', words: 'text', onto: 'card', line });
    const hit = runs.find(r => r.from <= frames[5].t && r.until >= frames[5].t);
    assert.ok(hit, `line ${line}: the frame without words is blank`);
    assert.equal(hit.worst.pageWords, 0);
  }
});

test('landing: a page that ends on its card lands; one that ends on the card\'s old place does not', () => {
  const on = closing({ words: () => 1, page: () => 1 });
  assert.deepEqual(findLanding(on, { page: 'page', onto: 'card' }), []);
  // The card moves 80 px down while the page closes onto where it was.
  const moved = closing({ words: () => 1, page: () => 1 });
  for (const f of moved) if (f.t >= 100) f.parts.card = [box(10, 180, 380, 80), box(10, 280, 380, 80)];
  const [miss] = findLanding(moved, { page: 'page', onto: 'card', index: 1 });
  assert.equal(miss.off, 80);
  // Without the index, the card it covers most is taken: here the wrong one.
  assert.equal(findLanding(moved, { page: 'page', onto: 'card' })[0].off, 20);
});

test('doubled: other words riding over the card\'s linger as a ghost once the card can be read; fading early they do not', () => {
  // The page fades from 140 ms over 200 ms on the standard curve,
  // cubic-bezier(.2, 0, 0, 1), as the app's does.
  const standard = x => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let lo = 0, hi = 1, u = 0.5;
    for (let i = 0; i < 40; i++) {
      u = (lo + hi) / 2;
      const at = 3 * 0.2 * u * (1 - u) ** 2 + u ** 3;
      if (at < x) lo = u; else hi = u;
    }
    return 3 * u * u * (1 - u) + u ** 3;
  };
  const fade = t => 1 - standard((t - 140) / 200);
  // Riding: the page's words stay as strong as the page.
  const riding = closing({ words: () => 1, page: fade });
  for (const f of riding) { const p = f.parts.page[0], w = f.parts.text[0]; if (p.visible && w.visible) w.y = p.y + 4; }
  const [run] = findDoubled(riding, { page: 'page', words: 'text', onto: 'card', index: 1, otherWords: true });
  assert.ok(run, 'a ghost');
  assert.ok(run.worst.pageWords >= 0.4, `the ghost at ${run.worst.pageWords}`);
  // Not said unless the scenario says the words differ.
  assert.deepEqual(findDoubled(riding, { page: 'page', words: 'text', onto: 'card', index: 1 }), []);
  // Fading on their own from 140 ms in 80 ms (linear).
  const early = closing({ words: t => Math.max(0, Math.min(1, 1 - (t - 140) / 80)), page: fade });
  assert.deepEqual(findDoubled(early, { page: 'page', words: 'text', onto: 'card', index: 1, otherWords: true }), []);
  assert.deepEqual(findBlankSlot(early, { page: 'page', words: 'text', onto: 'card', index: 1 }), []);
});
