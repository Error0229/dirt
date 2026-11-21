// report.md: what a run measured and found, each finding with the
// millisecond and the sheet that shows it.

import path from 'node:path';
import { frameIntervals, round, when } from './analyse.mjs';

const ms = when;
const rel = (file, dir) => path.relative(dir, file).replace(/\\/g, '/');

/**
 * Rows that differ only in which element of a part they are (`railButton[0]`
 * to `railButton[5]` moving as one) become one row naming them all.
 */
export function mergeRows(rows) {
  const groups = new Map();
  for (const row of rows) {
    const { key, ...rest } = row;
    const part = key.replace(/\[\d+\]$/, '');
    const id = `${part}|${JSON.stringify(rest)}`;
    if (!groups.has(id)) groups.set(id, { ...row, keys: [] });
    groups.get(id).keys.push(key);
  }
  return [...groups.values()].map(group => {
    if (group.keys.length === 1) return { ...group, key: group.keys[0] };
    const indices = group.keys.map(k => (k.match(/\[(\d+)\]$/) || [])[1]).filter(Boolean);
    return { ...group, key: `${group.keys[0].replace(/\[\d+\]$/, '')}[${indices.join(',')}]` };
  });
}

function table(head, rows) {
  if (!rows.length) return '';
  return `| ${head.join(' | ')} |\n|${head.map(() => '---').join('|')}|\n${rows.map(r => `| ${r.join(' | ')} |`).join('\n')}\n`;
}

/** Whether real time can be judged here, from the frames the page was drawn at while idle. */
export function trustOf(stamps) {
  const timing = frameIntervals(stamps);
  if (!timing.gaps) return { ok: false, timing, why: 'the page drew no frames while idle' };
  if (timing.medianMs > 20) return { ok: false, timing, why: `the page drew a frame every ${timing.medianMs} ms while idle (${timing.fps} a second): it is being held back` };
  return { ok: true, timing };
}

export function writeReport({ scenario, platform, describe, viewport, trust, movements, dir, started, mockupDescribe, notes, summary = [] }) {
  let out = `# Motion check: ${scenario.title || scenario.name}, ${platform}\n\n`;
  out += `- Scenario: \`${scenario.file}\`${scenario.issue ? ` (#${scenario.issue})` : ''}\n`;
  out += `- Ran on: ${describe}\n`;
  if (mockupDescribe) out += `- Against: ${mockupDescribe}\n`;
  out += `- When: ${started}\n`;
  out += `- Page: ${viewport.width} × ${viewport.height} CSS px at ${viewport.dpr}×${viewport.reduced ? ', reduced motion ON (movements are cut short)' : ''}\n`;
  out += `- Idle frame rate: about ${trust.timing.medianMs ? Math.round(1000 / trust.timing.medianMs) : '–'} frames a second (median gap ${trust.timing.medianMs ?? '–'} ms). `;
  out += trust.ok ? 'Real time can be judged.\n' : `**Real time cannot be judged here:** ${trust.why}. The stepped frames still hold.\n`;
  if (platform === 'android') out += '- Android frame rates here are the emulator\'s, not a phone\'s.\n';
  for (const note of notes || []) out += `- ${note}\n`;
  out += '\n';

  // What was found, most telling first, before the tables.
  const ranks = { 1: 'cut or left behind', 2: 'wrong against the design or the mockup', 3: 'different, worth a look', 4: 'about the capture' };
  out += '## Summary\n\n';
  if (!summary.length) out += 'Nothing the checks look for.\n\n';
  else {
    for (const [rank, label] of Object.entries(ranks)) {
      const lines = summary.filter(l => String(l.rank) === rank);
      if (!lines.length) continue;
      out += `**${label[0].toUpperCase()}${label.slice(1)}**\n\n${lines.slice(0, 14).map(l => `- ${l.text}`).join('\n')}${lines.length > 14 ? `\n- …and ${lines.length - 14} more in the sections below` : ''}\n\n`;
    }
  }

  for (const m of movements) {
    out += `## ${m.name}\n\n${m.description ? `${m.description}\n\n` : ''}`;
    if (m.stepped) {
      const s = m.stepped;
      out += `### Stepped, every ${round(s.stepMs, 2)} ms\n\n`;
      out += `- ${s.frames.length} frames${s.window ? `; something moves from ${ms(s.window.from)} to ${ms(s.window.to)}` : '; nothing moved'}.\n`;
      for (const sheet of s.sheets || []) out += `- Sheet: [${rel(sheet, dir)}](${rel(sheet, dir)})\n`;
      if (s.video) out += `- Video at 60 frames a second: ${rel(s.video, dir)}\n`;
      for (const note of s.notes || []) out += `- ${note}\n`;
      if (s.seen?.length) {
        out += `\nAnimations the clock saw (start on the clock, what, on what, delay + duration, easing):\n\n`;
        out += table(['start', 'what', 'on', 'delay + duration', 'easing'], s.seen.slice(0, 40).map(a => [ms(a.born), a.what, `\`${a.on}\``, `${round(a.delay || 0, 0)} + ${typeof a.duration === 'number' ? round(a.duration, 0) : a.duration}`, `\`${String(a.easing || '').slice(0, 48)}${String(a.easing || '').length > 48 ? '…' : ''}\``]));
        if (s.seen.length > 40) out += `\n…and ${s.seen.length - 40} more.\n`;
      }
      out += '\n';
    }
    if (m.realtime) {
      const r = m.realtime;
      out += `### In real time\n\n`;
      if (r.composited?.gaps) {
        out += `- Composited frames from the input to ${ms(r.until)}, judged ${r.composited.judged}: ${r.composited.fps} a second; gaps median ${r.composited.medianMs} ms, 95th percentile ${r.composited.p95Ms} ms, longest ${r.composited.maxMs} ms; about ${r.composited.missed} frames not drawn.\n`;
        if (r.firstChange !== null && r.firstChange !== undefined) out += `- The first change the probe saw came ${ms(r.firstChange)} after the input (the app's answer; not counted as frames missed).\n`;
        if (r.composited.longGaps.length) out += `- Long gaps (more than 1.5 frames): ${r.composited.longGaps.slice(0, 12).map(g => `${g.gap} ms before ${g.at} ms`).join(', ')}${r.composited.longGaps.length > 12 ? ', …' : ''}.\n`;
      } else out += '- DevTools kept no composited frames for this movement.\n';
      if (r.raf?.gaps) out += `- Animation frames the page ran (the probe): ${r.raf.frames}, gaps median ${r.raf.medianMs} ms, longest ${r.raf.maxMs} ms.\n`;
      if (r.timing) {
        out += `- Long tasks on the page's main thread (over 50 ms): ${r.timing.longTasks.length ? r.timing.longTasks.map(t => `${t.ms} ms at ${t.at} ms`).join(', ') : 'none'}.\n`;
        const states = Object.entries(r.timing.frameStates || {});
        if (states.length) out += `- Chromium's frame reports: ${states.map(([k, v]) => `${k} ${v}`).join(', ')}.\n`;
      }
      if (r.android) {
        const f = r.android.framestats;
        if (f?.summary?.total !== undefined) out += `- \`dumpsys gfxinfo\` over the movement: ${f.summary.total} frames, ${f.summary.janky ?? '–'} janky; 50th ${f.summary.p50 ?? '–'} ms, 90th ${f.summary.p90 ?? '–'} ms, 95th ${f.summary.p95 ?? '–'} ms, 99th ${f.summary.p99 ?? '–'} ms.\n`;
        if (f?.frames?.length) {
          const took = f.frames.map(x => x.tookMs).sort((a, b) => a - b);
          out += `- Its frame table: ${took.length} frames, vsync to done median ${took[took.length >> 1]} ms, longest ${took[took.length - 1]} ms, ${took.filter(x => x > 16.7).length} over 16.7 ms.\n`;
        }
        if (r.android.screenTiming?.gaps) out += `- The screen as recorded by the device: ${r.android.screenTiming.frames} frames, gaps median ${r.android.screenTiming.medianMs} ms, longest ${r.android.screenTiming.maxMs} ms, ${r.android.screenTiming.fps} a second while it changed.\n`;
      }
      for (const sheet of r.sheets || []) out += `- Sheet: [${rel(sheet, dir)}](${rel(sheet, dir)})\n`;
      out += '\n';
    }
    const f = m.findings;
    if (f) {
      out += '### Findings\n\n';
      const lines = [];
      for (const p of f.pops) lines.push(`- **Pop**, \`${p.key}\` at ${ms(p.t)}: ${p.detail}.`);
      for (const o of f.outside) lines.push(`- **Outside its container**: \`${o.part}\` reaches up to ${o.worst} px past \`${o.container}\`, from ${ms(o.from)} to ${ms(o.until)} (${o.frames} frames).`);
      for (const o of f.overlaps) lines.push(o.stacking
        ? `- **Seen over one another**: \`${o.a}\` and \`${o.b}\`, at up to ${Math.round(o.worst * 100)}% of the points where they meet, from ${ms(o.from)} to ${ms(o.until)}.`
        : `- **Boxed over one another**: \`${o.a}\` and \`${o.b}\` (up to ${Math.round(o.worst * 100)}% of the smaller) from ${ms(o.from)} to ${ms(o.until)}; what is on top could not be told there.`);
      for (const l of f.lingering) if (!l.ok) lines.push(`- **Should have left**, \`${l.part}\`: ${l.detail}${l.lastShown !== null ? `; last drawn ${l.lastShown < 0 ? 'before the input' : `at ${ms(l.lastShown)}`}` : ''}.`);
      for (const l of f.lingering) if (l.ok) lines.push(`- ${l.cut ? '**Cut at the input**' : 'Left as it should'}: \`${l.part}\`, ${l.detail}.`);
      for (const a of f.missing) lines.push(`- **Not there at the end**: \`${a.part}\` is ${a.detail}.`);
      for (const p of f.realtimePops || []) lines.push(`- In real time, a **pop** of \`${p.key}\` at ${ms(p.t)}: ${p.detail}.`);
      out += lines.length ? `${lines.join('\n')}\n\n` : 'Nothing the checks look for.\n\n';
    }
    if (m.mirror) {
      out += `### ${m.name} against ${m.mirrorOf}, which it is meant to mirror\n\n`;
      out += `Meant to mirror: ${m.mirrorBecause}. For each part and channel the scenario names: how long each took; where ${m.name} last showed it against where ${m.mirrorOf} first showed it (for opacity, where each began and ended)${m.mirrorCurve ? `; and how far apart the curves are, ${m.name} played backwards, in shares of the furthest each goes (0 the same, 1 at most)` : ''}.\n\n`;
      out += table(['part', 'channel', `${m.mirrorOf} ms`, `${m.name} ms`, `${m.mirrorOf} first shown at`, `${m.name} last shown at`, 'apart', ...(m.mirrorCurve ? ['curve distance (max / mean, worst at)'] : []), 'note'],
        mergeRows(m.mirror).map(r => [`\`${r.key}\``, r.channel, r.openMs ?? '–', r.closeMs ?? '–', r.openFrom ?? '–', r.closeTo ?? '–', r.away ?? '–', ...(m.mirrorCurve ? [r.distance ? `${r.distance.max} / ${r.distance.mean}, ${r.distance.worstAt}` : '–'] : []), r.note || (r.endsAway ? `does not end where ${m.mirrorOf} began` : '')]));
      if (m.mirrorSheet) out += `\nSheet: [${rel(m.mirrorSheet, dir)}](${rel(m.mirrorSheet, dir)})\n`;
      out += '\n';
    }
    if (m.mockup) {
      const live = m.mockupIn === 'real time';
      out += `### Against the mockup${live ? ', in real time' : ''}\n\nThe mockup holds for durations and curves. Times are from ${live ? 'the input' : 'the first stepped frame'} in each.\n\n`;
      out += table(['part', 'channel', 'mockup ms', 'app ms', 'mockup starts', 'app starts', 'curve distance (max / mean)', 'note'],
        mergeRows(m.mockup).map(r => [`\`${r.key}\``, r.channel, r.mockupMs, r.appMs, ms(r.mockupStart), ms(r.appStart), r.distance ? `${r.distance.max} / ${r.distance.mean}` : '–', r.note || '']));
      out += `\nCurve distance is in shares of the furthest each moves: 0 the same curve, 1 at most. Channels that move under 3 px (opacity 0.05) in both are left out.\n`;
      if (m.mockupSheet) out += `\nSheet: [${rel(m.mockupSheet, dir)}](${rel(m.mockupSheet, dir)})\n`;
      out += '\n';
    }
  }
  return out;
}
