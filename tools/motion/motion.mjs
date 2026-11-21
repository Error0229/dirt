#!/usr/bin/env node
// The motion check (docs/ui/motion-check.md): a movement of the real app,
// frame by frame, from start to end, as images an agent can read and as
// numbers.
//
//   node tools/motion/motion.mjs run <scenario> --platform desktop|android|mockup-desktop|mockup-android [options]
//   node tools/motion/motion.mjs list
//
// Node 22 or later, nothing to install. ffmpeg on the PATH for videos and
// Android's screen recording; Edge or Chrome for the mockup and the sheets.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { launchDesktop, launchAndroid, launchMockup, launchHeadless, runCleanups, onCleanup, dirtIsRunning } from './lib/targets.mjs';
import { prepare, sendSteps, partsFor, cropFor, scaleFor, captureStepped, captureRealtime, measureFrameRate, pick, writes } from './lib/capture.mjs';
import {
  findPops, findOutside, findOverlaps, findLingering, findMissing, compareMirror, compareWithMockup,
  frameIntervals, activeSpans, traceTiming, tracksOf, movingWindow, CHANNELS, round, when, contradictedInRealTime,
} from './lib/analyse.mjs';
import { contactSheets, sideBySide, video, nearest, removeFrames } from './lib/sheets.mjs';
import { writeReport, trustOf } from './lib/report.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '..', '..');

function parseArgs(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg.startsWith('--')) {
      const [key, inline] = arg.slice(2).split('=');
      const next = argv[i + 1];
      if (inline !== undefined) args[key] = inline;
      else if (next !== undefined && !next.startsWith('--')) { args[key] = next; i++; } else args[key] = true;
    } else args._.push(arg);
  }
  return args;
}

const USAGE = `node tools/motion/motion.mjs run <scenario> --platform <platform> [options]
node tools/motion/motion.mjs list

platform: desktop, android, mockup-desktop, mockup-android

options:
  --mockup             also run the scenario on the mockup and compare (desktop, android)
  --layers stepped,realtime   which captures to make (both by default)
  --only <movement,...>       run only these movements (and the ones they mirror)
  --step-ms <ms>       the stepped clock's step (default 1000/60)
  --realtime-probe off  measure no parts on each frame in real time (to see that
                       the probe costs nothing); frame timing and screenshots stay
  --keep-frames        keep every frame and the DevTools trace, not only the sheets
  --video              also write a 60 fps video of each stepped movement (ffmpeg)
  --out <folder>       where run folders go (default: $CARGO_TARGET_DIR/motion, or target/motion)
  desktop:  --exe <path>        a development build of the app (default:
                                <target>/dx/dirt-desktop/debug/windows/app/dirt-desktop.exe);
                                a release build is refused, since it ignores every stand-in
            --in-place          run the app where it is, not a copy whose single-instance
                                lock has another name (a Dirt started meanwhile then exits)
            --voice             copy the voice model too
            --port <n>          its DevTools port, if free (default: the first free from 9581)
  android:  --serial <s>        the device (default: the running emulator)
            --real-phone        allow a device that is not an emulator (its owner's choice; needs --serial)
            --allow-writes      on a phone, allow the steps that type or change notes
            --package <name>    (default com.example.DirtMobile)
            --port <n>          the local port forwarded to its web view, if free`;

function targetFolder() {
  return process.env.CARGO_TARGET_DIR ? path.resolve(process.env.CARGO_TARGET_DIR) : path.join(repo, 'target');
}

function loadScenario(name) {
  const candidates = [name, path.join(here, 'scenarios', name), path.join(here, 'scenarios', `${name}.json`)];
  const file = candidates.find(candidate => fs.existsSync(candidate) && fs.statSync(candidate).isFile());
  if (!file) throw new Error(`no scenario ${name} (see node tools/motion/motion.mjs list)`);
  const scenario = JSON.parse(fs.readFileSync(file, 'utf8'));
  scenario.file = path.relative(repo, path.resolve(file)).replace(/\\/g, '/');
  return scenario;
}

const stamp = () => new Date().toISOString().replace(/[-:]/g, '').replace('T', '-').slice(0, 15);
const log = (...words) => console.log('[motion]', ...words);

function forPlatform(list, platform) {
  return (list || []).filter(item => !item.platforms || item.platforms.includes(platform) || (platform.startsWith('mockup-') && item.platforms.includes('mockup')));
}

/** A list a scenario gives for every platform, or per platform. */
function listFor(value, platform) {
  if (!value) return [];
  if (Array.isArray(value)) return value;
  return pick(value, platform) || [];
}

/** What a movement is meant to mirror on this platform, from the design, or null. */
function mirrorFor(movement, platform) {
  const spec = movement.mirrors;
  if (!spec) return null;
  const parts = spec.parts && !Object.values(spec.parts).every(Array.isArray) ? pick(spec.parts, platform) : spec.parts;
  if (!parts) return null;
  return { of: spec.of, parts, curve: !!spec.curve, because: spec.because };
}

/** Every step a run would send on this platform. */
function stepsOf(scenario, platform) {
  const steps = [...forPlatform(scenario.setup, platform), ...forPlatform(scenario.between, platform)];
  for (const movement of forPlatform(scenario.movements, platform)) steps.push(...forPlatform(movement.steps, platform), ...forPlatform(movement.after, platform));
  return steps.filter(step => !step.platforms || step.platforms.includes(platform));
}

/** Runs a scenario's movements on one target: stepped, then in real time. */
async function capture(target, scenario, { layers, dir, stepMs, only, probeLive = true }) {
  const viewport = await prepare(target.cdp);
  log(`page ${viewport.width}×${viewport.height} at ${viewport.dpr}×${viewport.reduced ? ', reduced motion' : ''}`);
  await sendSteps(target, forPlatform(scenario.setup, target.platform));
  const idle = await measureFrameRate(target.cdp, 1000);
  const parts = partsFor(scenario, target.platform);
  const pairs = ((scenario.checks || {}).apart || []).filter(([a, b]) => parts[a] && parts[b]);
  const movements = forPlatform(scenario.movements, target.platform).filter(m => !only || only.includes(m.name));
  const results = { viewport, idle, stepped: {}, realtime: {}, notes: target.notes || [] };
  const passes = layers.filter(layer => layer === 'stepped' || layer === 'realtime');
  for (const [n, layer] of passes.entries()) {
    if (n > 0) await sendSteps(target, forPlatform(scenario.between, target.platform));
    for (const movement of movements) {
      const crop = await cropFor(target, { crop: movement.crop ?? scenario.crop }, viewport);
      const scale = scaleFor(crop, viewport);
      const into = path.join(dir, 'frames', target.platform, `${layer}-${movement.name}`);
      log(`${target.platform}: ${movement.name}, ${layer}`);
      if (layer === 'stepped') {
        results.stepped[movement.name] = { ...await captureStepped(target, movement, { parts, pairs, crop, scale, dir: into, stepMs, viewport, log }), crop };
      } else {
        // Without the probe, the page does no work of ours while it plays:
        // for checking that the probe itself is not what drops frames.
        results.realtime[movement.name] = { ...await captureRealtime(target, movement, { parts: probeLive ? parts : {}, pairs: probeLive ? pairs : [], crop, scale, dir: into, viewport, log }), crop };
      }
      await sendSteps(target, forPlatform(movement.after, target.platform));
    }
  }
  return results;
}

async function launch(platform, args) {
  const target = targetFolder();
  const port = args.port ? Number(args.port) : undefined;
  if (platform === 'desktop') {
    const inPlace = !!args['in-place'];
    if (inPlace && await dirtIsRunning()) throw new Error('--in-place, but a Dirt is running: its single-instance lock would stop the test app. Leave --in-place out to run a copy');
    return launchDesktop({
      exe: args.exe || path.join(target, 'dx', 'dirt-desktop', 'debug', 'windows', 'app', process.platform === 'win32' ? 'dirt-desktop.exe' : 'dirt-desktop'),
      inPlace, voice: !!args.voice, port, log,
    });
  }
  if (platform === 'android') return launchAndroid({ serial: args.serial, realPhone: !!args['real-phone'], packageName: args.package, port, log });
  if (platform === 'mockup-desktop' || platform === 'mockup-android') return launchMockup({ as: platform.slice(7), mockup: path.join(repo, 'docs', 'ui', 'mockup.html'), log });
  throw new Error(`no platform ${platform}`);
}

/** The time span over which anything in a trace moves. */
function windowOf(frames) {
  let from = Infinity, to = -Infinity;
  for (const track of tracksOf(frames.filter(f => !f.before)).values()) {
    for (const channel of CHANNELS) {
      const w = movingWindow(track, channel);
      if (w) { from = Math.min(from, w.from); to = Math.max(to, w.to); }
    }
  }
  return from === Infinity ? null : { from, to };
}

function flag(frames, t, text, kind = 'flags') {
  const frame = frames.find(f => f.t === t) || nearest(frames, t);
  if (frame) (frame[kind] = frame[kind] || []).push(text);
}

const sheetName = (sheets, t, frames) => {
  if (!sheets?.length) return '';
  if (sheets.length === 1) return path.basename(sheets[0]);
  const index = Math.max(0, frames.findIndex(f => f.t >= t));
  return path.basename(sheets[Math.min(sheets.length - 1, Math.floor(index / 24))]);
};

/**
 * The findings of a run, most telling first, each in one line with its
 * movement, its millisecond and the sheet that shows it.
 */
function summarise(movements, { platform, trust }) {
  const lines = [];
  const add = (rank, text) => lines.push({ rank, text });
  for (const m of movements) {
    const f = m.findings;
    const frames = m.stepped?.frames || [];
    const sheetAt = t => sheetName(m.stepped?.sheets, t, frames);
    if (f) {
      for (const p of f.pops) add(p.contradicted ? 4 : p.atInput ? 1 : 2, `**${m.name}**: \`${p.key}\` ${p.detail} (${when(p.t)}, ${sheetAt(p.t)})`);
      for (const l of f.lingering) if (!l.ok) add(1, `**${m.name}**: \`${l.part}\` should have left: ${l.detail} (${sheetAt(l.t)})`);
      for (const o of f.overlaps) add(o.stacking ? 2 : 3, `**${m.name}**: \`${o.a}\` and \`${o.b}\` are ${o.stacking ? 'seen over one another' : 'boxed over one another (what is on top could not be told)'} from ${when(o.from)} to ${when(o.until)} (${sheetAt(o.from)})`);
      for (const o of f.outside) add(2, `**${m.name}**: \`${o.part}\` reaches ${o.worst} px past \`${o.container}\` from ${when(o.from)} to ${when(o.until)} (${sheetAt(o.from)})`);
      for (const a of f.missing) add(1, `**${m.name}**: \`${a.part}\` is ${a.detail}`);
    }
    for (const r of m.mirror || []) {
      if (r.note) add(2, `**${m.name}** against **${m.mirrorOf}**: \`${r.key}\` ${r.channel} ${r.note} (meant to mirror: ${m.mirrorBecause})`);
      else if (r.endsAway) add(2, `**${m.name}** against **${m.mirrorOf}**: \`${r.key}\` is last seen at ${r.channel} ${r.closeTo}, ${r.away}${r.channel === 'o' ? '' : ' px'} from where **${m.mirrorOf}** first showed it (${r.openFrom}) (meant to mirror: ${m.mirrorBecause})`);
      if (r.distance && r.distance.max > 0.3) add(3, `**${m.name}** against **${m.mirrorOf}** played backwards: \`${r.key}\` ${r.channel} curves ${r.distance.max} apart (worst at ${Math.round(r.distance.worstAt * 100)}% of the way)`);
    }
    // A part whose stepped trace an app timer cut short is compared on that
    // short trace: said, but as a note on the capture.
    const cutShort = new Set((f?.pops || []).filter(p => p.contradicted).map(p => p.key));
    for (const r of m.mockup || []) {
      if (cutShort.has(r.key) && m.mockupIn !== 'real time') {
        add(4, `**${m.name}** against the mockup: \`${r.key}\` ${r.channel} is compared on a stepped trace an app timer cut short (see above); judge it from the real-time frames`);
        continue;
      }
      if (r.note && r.note !== 'not in the mockup') add(2,`**${m.name}** against the mockup: \`${r.key}\` ${r.channel} ${r.note}${r.mockupMs ? ` (mockup: ${r.mockupMs} ms from ${when(r.mockupStart)})` : ''}`);
      else if (r.distance) {
        const longer = Math.abs(r.appMs - r.mockupMs) > Math.max(60, 0.3 * r.mockupMs);
        if (longer || r.distance.max > 0.3) add(3, `**${m.name}** against the mockup: \`${r.key}\` ${r.channel} takes ${r.appMs} ms (mockup ${r.mockupMs} ms), curves ${r.distance.max} apart`);
      }
    }
    const rt = m.realtime;
    if (rt?.composited?.missed > 1 && trust.ok) {
      add(3, `**${m.name}** in real time: ${rt.composited.fps} frames a second, about ${rt.composited.missed} not drawn, judged ${rt.composited.judged}${platform === 'android' ? ' (the emulator\'s, not a phone\'s)' : ''}`);
    }
    for (const note of m.stepped?.notes || []) add(4, `**${m.name}**: ${note}`);
  }
  // Rows of one part and one kind are said once, with how many there are.
  const seen = new Map();
  for (const line of lines) {
    const key = `${line.rank}|${line.text.replace(/\[\d+(,\d+)*\]/g, '[…]').replace(/[\d.]+/g, '#')}`;
    if (seen.has(key)) seen.get(key).more++;
    else seen.set(key, { ...line, more: 0 });
  }
  return [...seen.values()].sort((a, b) => a.rank - b.rank).map(l => ({ rank: l.rank, text: l.more ? `${l.text}, and ${l.more} more like it` : l.text }));
}

/** A real phone's notes are the person's and sync: steps that write are refused unless allowed. */
function guardPhone(scenario, args) {
  if (!args['real-phone']) return;
  const writing = stepsOf(scenario, 'android').filter(writes);
  if (writing.length && !args['allow-writes']) {
    throw new Error(`on a real phone, ${scenario.name} would write into the phone's notes, which sync: ${writing.slice(0, 4).map(s => JSON.stringify(s)).join(', ')}${writing.length > 4 ? ', …' : ''}. Give --allow-writes only if the phone's owner agrees`);
  }
}

async function runScenario(args) {
  const scenario = loadScenario(args._[1]);
  const platform = args.platform || 'desktop';
  const layers = String(args.layers || 'stepped,realtime').split(',');
  const only = args.only ? String(args.only).split(',') : null;
  const stepMs = Number(args['step-ms'] || 1000 / 60);
  const started = new Date().toString();
  if (platform === 'android') guardPhone(scenario, args);
  const dir = path.join(path.resolve(args.out || path.join(targetFolder(), 'motion')), `${scenario.name}-${platform}-${stamp()}`);
  fs.mkdirSync(dir, { recursive: true });
  // A run that fails or is stopped leaves no half-made folder behind.
  let undoDir = onCleanup(`unfinished run folder ${dir}`, () => fs.rmSync(dir, { recursive: true, force: true }));
  log(`run folder ${dir}`);
  if (only) {
    // A movement compared with another needs that one too.
    for (const m of scenario.movements) if (only.includes(m.name) && m.mirrors?.of && !only.includes(m.mirrors.of)) only.push(m.mirrors.of);
  }

  const target = await launch(platform, args);
  let results;
  const describe = target.describe;
  try {
    results = await capture(target, scenario, { layers, dir, stepMs, only, probeLive: args['realtime-probe'] !== 'off' });
  } finally {
    await target.stop();
  }

  let mockup = null, mockupDescribe = null;
  if (args.mockup && !platform.startsWith('mockup-')) {
    const as = platform === 'android' ? 'mockup-android' : 'mockup-desktop';
    const other = await launch(as, args);
    mockupDescribe = other.describe;
    try {
      // Real time too, for a movement compared in real time (a smooth
      // scroll, which no clock of ours steps).
      const needsRealtime = scenario.movements.some(m => m.compareIn === 'realtime');
      mockup = await capture(other, scenario, { layers: needsRealtime ? ['stepped', 'realtime'] : ['stepped'], dir, stepMs, only });
    } finally {
      await other.stop();
    }
  }

  // ---- Analysis, sheets, report -------------------------------------------
  const browser = await launchHeadless({ log });
  const checks = scenario.checks || {};
  const movements = [];
  const notes = [...results.notes, ...(mockup ? mockup.notes : [])];
  try {
    for (const movement of forPlatform(scenario.movements, platform).filter(m => !only || only.includes(m.name))) {
      const out = { name: movement.name, description: movement.description };
      const stepped = results.stepped[movement.name];
      const viewport = results.viewport;
      const aspect = stepped ? stepped.crop.width / stepped.crop.height : viewport.width / viewport.height;
      if (stepped) {
        const frames = stepped.frames;
        const findings = {
          // The frame before the input is kept: a cut at the input is a pop.
          pops: findPops(frames, checks.pops),
          outside: findOutside(frames, checks.within || [], { viewport }),
          overlaps: findOverlaps(frames, checks.apart || []),
          lingering: findLingering(frames, listFor(movement.leaves, platform)),
          missing: findMissing(frames, listFor(movement.arrives, platform)),
        };
        for (const p of findings.pops) flag(frames, p.t, `pop: ${p.key} ${p.channel || p.kind}${p.atInput ? ' at the input' : ''}`);
        for (const o of findings.outside) flag(frames, o.from, `${o.part} outside ${o.container} by ${o.worst}px until ${o.until}`);
        for (const o of findings.overlaps) flag(frames, o.from, `${o.a} over ${o.b} until ${o.until}${o.stacking ? '' : ' (boxes)'}`);
        for (const l of findings.lingering) if (!l.ok) flag(frames, l.t, `${l.part}: ${l.kind}`);
        for (const a of stepped.seen || []) if (a.born > 0) flag(frames, a.born, `starts: ${a.what.replace('CSSTransition ', '').replace('Animation', 'anim')} on ${a.on.split('.').slice(0, 2).join('.')}`, 'notes');
        out.findings = findings;
        const scrolled = [...tracksOf(frames).values()].some(track => movingWindow(track, 'scroll'));
        if (scrolled) stepped.notes.push('Something scrolled while this was stepped. A native smooth scroll runs on the real clock, not on the stepped one, so what moves with it is not stepped faithfully here: read the real-time layer for it.');
        out.stepped = {
          frames, stepMs: stepped.stepMs, notes: stepped.notes, seen: stepped.seen, window: windowOf(frames),
          sheets: await contactSheets(browser, frames, {
            title: `${scenario.name} · ${movement.name} · ${platform} · stepped`,
            subtitle: `every ${round(stepped.stepMs, 2)} ms of the movement's own time, whatever the machine's speed; red: a finding, gold: an animation starts`,
            out: path.join(dir, `stepped-${movement.name}`), aspect,
          }),
        };
        if (args.video) out.stepped.video = await video(frames.filter(f => !f.before), path.join(dir, `stepped-${movement.name}.mp4`));
      }
      const realtime = results.realtime[movement.name];
      if (realtime) {
        const steppedWindow = out.stepped?.window;
        // Judged while something moves: the probe's samples say when. A
        // blinking caret draws a frame every half second long after, and
        // nothing is drawn while the app has not answered yet; neither is a
        // frame missed. Without the probe, the whole span is judged.
        const samples = realtime.samples.filter(s => s.t >= 0);
        const spans = samples.length ? activeSpans(samples) : null;
        const liveWindow = spans?.length ? { from: spans[0].from, to: spans[spans.length - 1].to } : null;
        const moved = Math.max(steppedWindow ? steppedWindow.to : 0, liveWindow ? liveWindow.to : 0);
        const until = Math.round(Math.min(movement.ms || 1500, moved ? moved + 100 : movement.ms || 1500));
        const view = { crop: realtime.crop, width: viewport.width, height: viewport.height };
        const shown = realtime.frames.filter(f => f.t <= until).map(f => Object.assign(f, { view }));
        const active = spans && spans.length ? spans : null;
        let previous = null;
        for (const frame of shown) {
          frame.kind = previous === null ? '' : `+${round(frame.t - previous, 1)}`;
          const counted = !active || active.some(span => previous < span.to && frame.t > span.from);
          if (previous !== null && frame.t - previous > 25 && counted) frame.flags = [`gap ${round(frame.t - previous, 1)} ms`];
          previous = frame.t;
        }
        const r = {
          until,
          firstChange: liveWindow ? liveWindow.from : null,
          composited: frameIntervals(shown.map(f => f.t), { active }),
          raf: frameIntervals(samples.filter(s => s.t <= until).map(s => s.t)),
          timing: traceTiming(realtime.events, { untilMs: until }),
          sheets: await contactSheets(browser, shown, {
            title: `${scenario.name} · ${movement.name} · ${platform} · real time`,
            subtitle: `frames as composited, ms from the input; the small number is the gap since the frame before; red: a gap over 25 ms ${active ? 'while something moved' : '(no probe: every gap)'}`,
            out: path.join(dir, `realtime-${movement.name}`), aspect,
          }),
        };
        if (realtime.android) {
          r.android = { framestats: realtime.android.framestats };
          const screen = realtime.android.screen;
          if (screen?.frames.length) {
            r.android.screenTiming = frameIntervals(screen.frames.map(f => f.t));
            let before = null;
            const labelled = screen.frames.map(f => { const kind = before === null ? '' : `+${f.t - before}`; const flags = before !== null && f.t - before > 25 ? [`gap ${f.t - before} ms`] : []; before = f.t; return { ...f, label: `${f.t} ms`, kind, flags }; });
            r.sheets.push(...await contactSheets(browser, labelled, {
              title: `${scenario.name} · ${movement.name} · emulator screen recording`,
              subtitle: 'every frame the device wrote; it writes one only when the screen changes. Times are the video\'s, from when recording began, not from the input',
              out: path.join(dir, `screen-${movement.name}`), aspect: 0.45,
            }));
          }
        }
        if (out.findings) {
          out.findings.realtimePops = findPops(samples.filter(s => s.t <= until), { ...checks.pops, px: (checks.pops?.px || 24) * 2 });
          // A part the stepped frames saw vanish that real time still draws
          // went early because a timer on the app's real clock took it.
          for (const pop of out.findings.pops) {
            if (contradictedInRealTime(pop, out.stepped.frames, samples, { answeredAt: liveWindow ? liveWindow.from : 0 })) {
              pop.contradicted = true;
              pop.detail += '; real time still draws it well after, so a timer on the app’s own clock (which stepping cannot hold) took it early here';
            }
          }
        }
        out.realtime = r;
      }
      movements.push(out);
    }

    // A closing against the opening the design means it to mirror.
    for (const out of movements) {
      const movement = scenario.movements.find(m => m.name === out.name);
      const spec = mirrorFor(movement, platform);
      if (!spec) continue;
      const open = movements.find(m => m.name === spec.of);
      if (!open?.stepped || !out.stepped) continue;
      out.mirrorOf = spec.of;
      out.mirrorBecause = spec.because;
      out.mirrorCurve = spec.curve;
      out.mirror = compareMirror(open.stepped.frames, out.stepped.frames, spec);
      const a = open.stepped.window, b = out.stepped.window;
      if (a && b) {
        const columns = 12;
        const pairs = [];
        for (let i = 0; i < columns; i++) {
          const s = i / (columns - 1);
          const top = nearest(open.stepped.frames, a.from + s * (a.to - a.from));
          const bottom = nearest(out.stepped.frames, b.to - s * (b.to - b.from));
          pairs.push({ top: top && { ...top, label: `${top.t} ms`, flags: [], notes: [] }, bottom: bottom && { ...bottom, label: `${bottom.t} ms`, flags: [], notes: [] } });
        }
        out.mirrorSheet = await sideBySide(browser, pairs, {
          title: `${scenario.name} · ${spec.of} against ${out.name} played backwards · ${platform}`,
          subtitle: `columns at the same fraction of each movement: ${spec.of} ${a.from}–${a.to} ms forwards, ${out.name} ${b.from}–${b.to} ms backwards. Meant to mirror: ${spec.because}`,
          out: path.join(dir, `mirror-${out.name}.png`), aspect: results.stepped[spec.of] ? results.stepped[spec.of].crop.width / results.stepped[spec.of].crop.height : 1.5,
          topLabel: `${spec.of}, forwards`, bottomLabel: `${out.name}, backwards`,
        });
      }
    }

    // The app against the mockup.
    if (mockup) {
      for (const out of movements) {
        const movement = scenario.movements.find(m => m.name === out.name);
        const live = movement?.compareIn === 'realtime';
        const mine = live ? results.realtime[out.name] : results.stepped[out.name];
        const theirs = live ? mockup.realtime[out.name] : mockup.stepped[out.name];
        if (!theirs || !mine) { if (!theirs) notes.push(`The mockup has no "${out.name}" movement in this scenario.`); continue; }
        const myTrace = live ? mine.samples.filter(s => s.t >= 0) : mine.frames;
        const theirTrace = live ? theirs.samples.filter(s => s.t >= 0) : theirs.frames;
        out.mockupIn = live ? 'real time' : 'stepped';
        out.mockup = compareWithMockup(myTrace, theirTrace);
        const a = windowOf(myTrace), b = windowOf(theirTrace);
        const end = Math.max(a ? a.to : 0, b ? b.to : 0);
        if (end > 0 && mine.frames.length && theirs.frames.length) {
          const columns = 12;
          const pairs = [];
          for (let i = 0; i < columns; i++) {
            const t = Math.round((i / (columns - 1)) * end);
            const top = nearest(mine.frames, t), bottom = nearest(theirs.frames, t);
            const cell = (frame, crop, size) => frame && { ...frame, label: `${round(frame.t, 1)} ms`, flags: [], notes: [], kind: '', view: live ? { crop, width: size.width, height: size.height } : undefined };
            pairs.push({ top: cell(top, mine.crop, results.viewport), bottom: cell(bottom, theirs.crop, mockup.viewport) });
          }
          out.mockupSheet = await sideBySide(browser, pairs, {
            title: `${scenario.name} · ${out.name} · ${platform} against the mockup${live ? ', in real time' : ''}`,
            subtitle: live
              ? 'frames as composited at about the same millisecond from the input in each; the mockup holds for durations and curves'
              : 'the same millisecond of each, from the first stepped frame; the mockup holds for durations and curves',
            out: path.join(dir, `mockup-${out.name}.png`), aspect: mine.crop.width / mine.crop.height,
            topLabel: 'the app', bottomLabel: 'the mockup',
          });
        }
      }
    }
  } finally {
    await browser.stop();
  }

  const trust = trustOf(results.idle);
  if (args['realtime-probe'] === 'off') notes.push('The real-time probe was off (--realtime-probe off): frame timing and screenshots only, no numbers per part in real time, so every gap between frames is judged, not only those while something moved.');
  else if (platform === 'android' && layers.includes('realtime')) notes.push('The probe measured every part on every frame in real time, which costs a slow device frames (on the emulator, a third or more of them). Judge the frame rate from a run with --realtime-probe off.');
  if (trust.ok) {
    // Real time while it moved, too: an off-screen window may be throttled only while busy.
    for (const m of movements) {
      if (m.realtime?.raf?.medianMs > 20) notes.push(`While "${m.name}" played in real time the page ran an animation frame every ${m.realtime.raf.medianMs} ms: real time is suspect for it.`);
    }
  }
  const summary = summarise(movements, { platform, trust });
  const report = writeReport({ scenario, platform, describe, viewport: results.viewport, trust, movements, dir, started, mockupDescribe, notes, summary });
  fs.writeFileSync(path.join(dir, 'report.md'), report);
  const trace = {
    scenario: scenario.file, platform, started, describe, viewport: results.viewport,
    idleFrameTimes: results.idle,
    summary,
    movements: movements.map(m => ({
      name: m.name,
      stepped: m.stepped && { stepMs: m.stepped.stepMs, window: m.stepped.window, seen: m.stepped.seen, frames: m.stepped.frames.map(f => ({ t: f.t, before: f.before || undefined, parts: f.parts, overlaps: f.overlaps })) },
      realtime: m.realtime && { composited: m.realtime.composited, raf: m.realtime.raf, timing: m.realtime.timing, android: m.realtime.android, samples: results.realtime[m.name].samples },
      findings: m.findings, mirror: m.mirror, mockup: m.mockup,
    })),
    mockup: mockup && { movements: Object.fromEntries(Object.entries(mockup.stepped).map(([name, s]) => [name, { seen: s.seen, frames: s.frames.map(f => ({ t: f.t, before: f.before || undefined, parts: f.parts })) }])) },
  };
  fs.writeFileSync(path.join(dir, 'trace.json'), JSON.stringify(trace));
  if (args['keep-frames']) {
    for (const [name, r] of Object.entries(results.realtime)) fs.writeFileSync(path.join(dir, 'frames', `devtools-trace-${name}.json`), JSON.stringify({ traceEvents: r.events }));
  } else {
    removeFrames(path.join(dir, 'frames'));
  }
  undoDir();
  log(`done: ${path.join(dir, 'report.md')}`);
  return dir;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const command = args._[0];
  if (command === 'run' && args._[1]) {
    await runScenario(args);
  } else if (command === 'list') {
    for (const file of fs.readdirSync(path.join(here, 'scenarios')).filter(f => f.endsWith('.json'))) {
      const s = JSON.parse(fs.readFileSync(path.join(here, 'scenarios', file), 'utf8'));
      console.log(`${s.name.padEnd(22)} ${(s.platforms || []).join(', ').padEnd(44)} ${s.title || ''}`);
    }
  } else {
    console.log(USAGE);
    process.exitCode = command ? 1 : 0;
  }
}

// Stopped by Ctrl C, a signal or an error: every process this run started
// is ended by its ID, every forward and temporary folder removed, and the
// page given back its own timers and animation rate.
let ending = false;
async function end(code, why) {
  if (ending) return;
  ending = true;
  if (why) console.error(`[motion] ${why}`);
  await runCleanups(log);
  process.exit(code);
}
process.on('SIGINT', () => end(130, 'interrupted; cleaning up'));
process.on('SIGTERM', () => end(143, 'stopped; cleaning up'));
process.on('uncaughtException', error => end(1, error.stack || error.message));
process.on('unhandledRejection', error => end(1, (error && error.stack) || String(error)));

main().then(() => end(process.exitCode || 0)).catch(error => end(1, error.stack || error.message));
