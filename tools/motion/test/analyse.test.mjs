// node --test tools/motion

import test from 'node:test';
import assert from 'node:assert/strict';
import {
  tracksOf, valueOf, findPops, findOutside, findOverlaps, findLingering, findMissing, beyond,
  movingWindow, progressCurve, sampleCurve, curveDistance, compareReversed, compareWithMockup,
  frameIntervals, traceTiming, parseFramestats, parseShowinfo,
} from '../lib/analyse.mjs';
import { mergeRows, trustOf } from '../lib/report.mjs';
import { columnsFor } from '../lib/sheets.mjs';
import { pick, partsFor } from '../lib/capture.mjs';

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

test('compareReversed: a closing that is the opening reversed scores 0 and ends where it began', () => {
  const open = along([0, 10, 40, 90, 100], v => box(v, 0, 10, 10));
  const close = along([100, 90, 40, 10, 0], v => box(v, 0, 10, 10));
  const [row] = compareReversed(open, close);
  assert.equal(row.channel, 'x');
  // Frame times are whole milliseconds, so not quite even: near 0, not 0.
  assert.ok(row.distance.max < 0.05);
  assert.equal(row.endsWhereOpenBegan, 0);
});

test('compareReversed: a closing that only fades where it is shows the opening\'s path missing', () => {
  const open = along([0, 50, 100], v => box(v, 0, 10, 10));
  const close = along([1, 0.5, 0], o => box(100, 0, 10, 10, o));
  const rows = compareReversed(open, close);
  assert.equal(rows.find(r => r.channel === 'x').note, 'moves when opening, not when closing');
  assert.equal(rows.find(r => r.channel === 'o').note, 'moves when closing, not when opening');
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
