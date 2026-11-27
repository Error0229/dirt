// Where a scenario runs: a test copy of the desktop app, the Android app
// on the emulator (or, when its owner asks, a phone), the mockup in a
// headless browser. Each launcher returns a DevTools session with the
// page and a `stop` that undoes everything it started.
//
// Everything a launcher starts is also put down in `cleanups`, so that a
// run stopped by Ctrl C, a signal or an error still ends every process it
// started (by its process ID), removes every port forward and temporary
// folder, and gives the page back its own timers and animation rate.

import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import { pathToFileURL } from 'node:url';
import { Cdp, waitForTarget, sleep } from './cdp.mjs';

const run = promisify(execFile);

// ---- Cleaning up, whatever happens ----------------------------------------

const cleanups = new Map();
let nextCleanup = 1;

/** Puts down something to undo; returns what takes it back off once it is undone. */
export function onCleanup(what, undo) {
  const id = nextCleanup++;
  cleanups.set(id, { what, undo });
  return () => cleanups.delete(id);
}

/** Undoes everything still put down, the last first. */
export async function runCleanups(log = () => {}) {
  const left = [...cleanups.entries()].reverse();
  cleanups.clear();
  for (const [, { what, undo }] of left) {
    try { await undo(); log(`cleaned up: ${what}`); } catch (error) { log(`could not clean up ${what}: ${error.message}`); }
  }
}

/** What a target gives back to its page before it lets go: its own timers and the animation rate. */
async function restorePage(cdp) {
  await cdp.evaluate('window.__motion ? window.__motion.thaw() : false', { timeoutMs: 3000 }).catch(() => {});
  await cdp.send('Animation.setPlaybackRate', { playbackRate: 1 }, 3000).catch(() => {});
}

// ---- Ports ------------------------------------------------------------------

function answers(port) {
  return new Promise(resolve => {
    const socket = net.connect({ port, host: '127.0.0.1' });
    socket.once('connect', () => { socket.destroy(); resolve(true); });
    socket.once('error', () => resolve(false));
    socket.setTimeout(500, () => { socket.destroy(); resolve(false); });
  });
}

function canListen(port) {
  return new Promise(resolve => {
    const server = net.createServer();
    server.once('error', () => resolve(false));
    server.listen(port, '127.0.0.1', () => server.close(() => resolve(true)));
  });
}

/** Whether nothing on this machine uses the port. */
export async function portIsFree(port) {
  return !(await answers(port)) && canListen(port);
}

// Ports other tools on this machine use for their own web views: never
// these (docs/ui/motion-check.md).
const TAKEN = [9222, 9333, 9377, 9471, 9563, 9564, 9565];

/**
 * The port asked for, if it is free, or else the first free one at or
 * above `from`. A port asked for that something else uses is refused: the
 * tool would drive whatever web view answers there.
 */
export async function choosePort(asked, from) {
  if (asked) {
    if (TAKEN.includes(asked)) throw new Error(`port ${asked} is kept for other tools on this machine; choose another`);
    if (!(await portIsFree(asked))) throw new Error(`port ${asked} is in use; the tool would drive whatever answers there. Choose a free one, or leave --port out`);
    return asked;
  }
  for (let port = from; port < from + 200; port++) {
    if (!TAKEN.includes(port) && await portIsFree(port)) return port;
  }
  throw new Error(`no free port from ${from}`);
}

/** Removes a folder, trying again while a process that just ended lets go of it. */
export async function removeFolder(folder) {
  for (let tries = 0; tries < 20; tries++) {
    try { fs.rmSync(folder, { recursive: true, force: true }); return true; } catch { await sleep(500); }
  }
  return false;
}

/** Ends a child process by its ID and waits for it, never by its name. */
async function stopChild(child, gently) {
  if (gently) await gently().catch(() => {});
  for (let i = 0; i < 20 && child.exitCode === null && child.signalCode === null; i++) await sleep(150);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill();
    for (let i = 0; i < 20 && child.exitCode === null && child.signalCode === null; i++) await sleep(150);
  }
}

// ---- The desktop app ---------------------------------------------------

/** The name the app's single-instance lock is held under (dirt-desktop's main.rs). */
const LOCK = 'dirt-desktop-single-instance';
/** The most the copy may take: the app's folder, without the voice model, is about 100 MB. */
const MOST_TO_COPY = 600 * 1024 * 1024;

/**
 * Whether the program is a development build, which honours the stand-in
 * hooks (DIRT_WINDOW_AT and the rest). A release build ignores them all: its
 * window would open on the person's screen, its hotkey would be real and
 * signing in would open their browser. A release build is told by its path
 * and by what it lacks: a development build checks arithmetic for overflow
 * and carries the message it panics with; a release build does not.
 */
export function looksLikeDevelopmentBuild(exe) {
  if (/[\\/]release[\\/]/i.test(path.resolve(exe))) return false;
  return fs.readFileSync(exe).includes(Buffer.from('attempt to add with overflow'));
}

function sizeOf(item) {
  const stat = fs.statSync(item);
  if (!stat.isDirectory()) return stat.size;
  return fs.readdirSync(item).reduce((sum, name) => sum + sizeOf(path.join(item, name)), 0);
}

/**
 * Copies what the app needs to run (the program, the libraries beside it
 * and its `assets` folder, the voice model only if asked) and renames the
 * single-instance lock in the copy, so that the copy starts beside a Dirt
 * that is running and never holds the lock a Dirt the person starts would
 * need. The new name has the same length, so nothing else in the program
 * moves. Nothing in the repository or the original folder is changed.
 */
function copyApp(exe, into, withVoice) {
  const from = path.dirname(exe);
  const voice = path.join(from, 'assets', 'voice');
  const wanted = [exe, ...fs.readdirSync(from).filter(name => /\.(dll|so|dylib)$/i.test(name)).map(name => path.join(from, name))];
  const assets = path.join(from, 'assets');
  if (fs.existsSync(assets)) wanted.push(assets);
  const size = wanted.reduce((sum, item) => sum + sizeOf(item), 0) - (!withVoice && fs.existsSync(voice) ? sizeOf(voice) : 0);
  if (size > MOST_TO_COPY) {
    throw new Error(`copying the app would take ${Math.round(size / 1048576)} MB (the disk is nearly full); point --exe at the dx-built app, not a folder like target/debug`);
  }
  fs.mkdirSync(into, { recursive: true });
  for (const item of wanted) {
    fs.cpSync(item, path.join(into, path.basename(item)), { recursive: true, filter: source => withVoice || !source.startsWith(voice) });
  }
  const copy = path.join(into, path.basename(exe));
  const bytes = fs.readFileSync(copy);
  const name = Buffer.from(LOCK);
  const at = bytes.indexOf(name);
  if (at < 0 || bytes.indexOf(name, at + 1) >= 0) throw new Error(`the lock's name is not in ${exe} exactly once; build a copy by hand`);
  Buffer.from('dirt-desktop-motion-instance').copy(bytes, at);
  fs.writeFileSync(copy, bytes);
  return copy;
}

/**
 * Starts a test copy of the desktop app: its own data folder, its own web
 * view data folder and DevTools port, off every screen, with every
 * development stand-in set so that it touches nothing of the person's.
 */
export async function launchDesktop({ exe, port, inPlace = false, voice = false, log = () => {} }) {
  if (!exe || !fs.existsSync(exe)) throw new Error(`no desktop app at ${exe}; build it with dx build --platform desktop --package dirt-desktop`);
  if (!looksLikeDevelopmentBuild(exe)) {
    throw new Error(`${exe} looks like a release build. A release build ignores every stand-in (DIRT_WINDOW_AT, DIRT_ACTIONS_FROM_STDIN, DIRT_OPENED_TO_FILE and the rest): its window would open on the screen, its hotkey would be real and signing in would open the browser. Use a development build`);
  }
  port = await choosePort(port, 9581);
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'dirt-motion-'));
  let undoHome = onCleanup(`temporary folder ${home}`, () => removeFolder(home));
  const data = path.join(home, 'data');
  fs.mkdirSync(data, { recursive: true });
  let program = exe;
  if (inPlace) {
    log('WARNING: --in-place: while this run lasts the test copy holds the single-instance lock, so a Dirt started meanwhile exits at once, without a word');
  } else {
    log(`copying the app to ${home} and renaming its single-instance lock in the copy`);
    program = copyApp(exe, path.join(home, 'app'), voice);
  }
  const env = {
    ...process.env,
    DIRT_DATA_DIR: data,
    WEBVIEW2_USER_DATA_FOLDER: path.join(home, 'webview'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: [
      `--remote-debugging-port=${port}`,
      // A window off the screen is otherwise taken for hidden, and its
      // frames are slowed or stopped.
      '--disable-features=CalculateNativeWinOcclusion',
      '--disable-renderer-backgrounding',
      '--disable-background-timer-throttling',
      '--disable-backgrounding-occluded-windows',
    ].join(' '),
    DIRT_WINDOW_AT: '-32000,-32000',
    // Where quick capture's flight lands; the tray icon itself is real.
    DIRT_TRAY_AT: '-32000,-32000',
    DIRT_ACTIONS_FROM_STDIN: '1',
    DIRT_OPENED_TO_FILE: path.join(data, 'opened.txt'),
    DIRT_SIGN_IN_TO_FILE: path.join(data, 'sign-in.txt'),
    DIRT_EXPORT_TO: path.join(data, 'export'),
    // Never a real server: the store stays on this device.
    DIRT_API_BASE_URL: 'http://127.0.0.1:9',
  };
  const child = spawn(program, [], { env, stdio: ['pipe', 'ignore', 'pipe'], windowsHide: true });
  const quit = async () => { try { child.stdin.write('quit\n'); } catch { /* gone */ } };
  const undoChild = onCleanup(`desktop app, PID ${child.pid}`, () => stopChild(child, quit));
  let stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-4000); });
  log(`desktop app started, PID ${child.pid}, DevTools port ${port}, data ${data}`);
  let cdp, undoPage;
  try {
    const target = await waitForTarget(port, t => t.type === 'page' && !t.url.startsWith('devtools'), { timeoutMs: 45000 }).catch(error => {
      throw new Error(child.exitCode !== null
        ? `the app ended at once (code ${child.exitCode}). With --in-place, another Dirt's single-instance lock stops it. ${stderr.trim().split('\n').slice(-3).join(' ')}`
        : error.message);
    });
    cdp = await Cdp.connect(target.webSocketDebuggerUrl);
    undoPage = onCleanup('the test app page\'s timers and animation rate', () => restorePage(cdp));
  } catch (error) {
    undoChild(); await stopChild(child, quit);
    undoHome(); await removeFolder(home);
    throw error;
  }
  return {
    cdp,
    platform: 'desktop',
    pointer: 'mouse',
    describe: `a test copy of the desktop app (PID ${child.pid}), off the screen, DevTools port ${port}`,
    async stop() {
      undoPage(); await restorePage(cdp); cdp.close();
      undoChild(); await stopChild(child, quit);
      undoHome(); await removeFolder(home);
    },
  };
}

/** Whether a Dirt desktop app is running on this machine (Windows only; elsewhere, unknown). */
export async function dirtIsRunning() {
  if (process.platform !== 'win32') return null;
  const { stdout } = await run('tasklist', ['/FI', 'IMAGENAME eq dirt-desktop.exe', '/NH'], { windowsHide: true }).catch(() => ({ stdout: '' }));
  return /dirt-desktop\.exe/i.test(stdout);
}

// ---- Android -------------------------------------------------------------

export const ANDROID_PACKAGE = 'dev.catjam.dirt';
/** The app's main screen (Dioxus's activity); `--package` names another app with the same. */
export const ANDROID_ACTIVITY = 'dev.dioxus.main.MainActivity';

export function adb(serial, ...args) {
  return run('adb', ['-s', serial, ...args], { maxBuffer: 64 * 1024 * 1024, windowsHide: true }).then(result => result.stdout);
}

/**
 * Picks the device: the emulator unless the person names a phone and says
 * it is theirs to use (`--serial X --real-phone`). A phone is never chosen
 * by default.
 */
export async function pickDevice(serial, realPhone) {
  const { stdout } = await run('adb', ['devices'], { windowsHide: true });
  const devices = stdout.split('\n').slice(1).map(line => line.trim().split(/\s+/)).filter(([s, state]) => s && state === 'device').map(([s]) => s);
  if (serial) {
    if (!devices.includes(serial)) throw new Error(`${serial} is not connected (connected: ${devices.join(', ') || 'none'})`);
    if (!serial.startsWith('emulator-') && !realPhone) throw new Error(`${serial} is not an emulator; a phone is used only with --real-phone, when its owner runs this`);
    return serial;
  }
  if (realPhone) throw new Error('--real-phone needs --serial: the phone is never picked for you');
  const emulator = devices.find(s => s.startsWith('emulator-'));
  if (!emulator) throw new Error('no emulator is running; start one (docs/ui/motion-check.md)');
  return emulator;
}

/**
 * Reaches the Android app's web view: brings the app's main screen up by
 * name (started, or brought to the front), finds its DevTools socket and
 * forwards a local port to it.
 */
export async function launchAndroid({ serial, realPhone = false, port, packageName = ANDROID_PACKAGE, log = () => {} }) {
  serial = await pickDevice(serial, realPhone);
  // The main screen, by name, whether or not the process runs: the app has
  // two launcher entries (Talk is the other), of which `monkey` picked
  // either, and its process also runs without the screen (a capture job),
  // when it has no web view to find.
  log(`bringing ${packageName}'s main screen up on ${serial}`);
  await adb(serial, 'shell', 'am', 'start', '-W', '-n', `${packageName}/${ANDROID_ACTIVITY}`);
  let pid = '';
  for (let i = 0; i < 40 && !pid; i++) { pid = (await adb(serial, 'shell', 'pidof', packageName).catch(() => '')).trim(); if (!pid) await sleep(500); }
  if (!pid) throw new Error(`${packageName} did not start on ${serial}`);
  pid = pid.split(/\s+/)[0];
  let socket = null;
  for (let i = 0; i < 40 && !socket; i++) {
    const unix = await adb(serial, 'shell', 'cat', '/proc/net/unix');
    const found = unix.match(new RegExp(`@(webview_devtools_remote_${pid})\\b`));
    if (found) socket = found[1];
    else await sleep(500);
  }
  if (!socket) {
    throw new Error(`the app's web view offers no DevTools socket (webview_devtools_remote_${pid}). `
      + 'A debug build turns it on (wry calls WebView.setWebContentsDebuggingEnabled when debug_assertions are on); a release build does not.');
  }
  port = await choosePort(port, 9591);
  await adb(serial, 'forward', `tcp:${port}`, `localabstract:${socket}`);
  const unforward = () => adb(serial, 'forward', '--remove', `tcp:${port}`).catch(() => {});
  const undoForward = onCleanup(`adb forward tcp:${port} on ${serial}`, unforward);
  log(`forwarded port ${port} to ${socket} on ${serial}`);
  let cdp, undoPage;
  try {
    const target = await waitForTarget(port, t => t.type === 'page', { timeoutMs: 20000 });
    cdp = await Cdp.connect(target.webSocketDebuggerUrl);
    undoPage = onCleanup('the Android page\'s timers and animation rate', () => restorePage(cdp));
  } catch (error) {
    undoForward(); await unforward();
    throw error;
  }
  return {
    cdp,
    platform: 'android',
    pointer: 'touch',
    serial,
    realPhone,
    packageName,
    describe: `${packageName} on ${realPhone ? 'a phone' : 'the emulator'}, DevTools through an adb forward`,
    async stop() {
      undoPage(); await restorePage(cdp); cdp.close();
      undoForward(); await unforward();
    },
  };
}

// ---- A headless browser: the mockup, and drawing the sheets -------------

export function findBrowser() {
  const candidates = [
    process.env.DIRT_MOTION_BROWSER,
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
    '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/microsoft-edge',
  ].filter(Boolean);
  const found = candidates.find(candidate => fs.existsSync(candidate));
  if (!found) throw new Error('no Edge or Chrome found; set DIRT_MOTION_BROWSER to one');
  return found;
}

/**
 * A headless Edge or Chrome with a profile of its own in a temporary
 * folder, never the person's.
 */
export async function launchHeadless({ width = 1600, height = 1000, port, log = () => {} } = {}) {
  const browser = findBrowser();
  port = await choosePort(port, 9601);
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'dirt-motion-browser-'));
  const undoProfile = onCleanup(`browser profile ${profile}`, () => removeFolder(profile));
  const child = spawn(browser, [
    '--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`,
    `--window-size=${width},${height}`, '--force-device-scale-factor=1', '--hide-scrollbars',
    '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--disable-sync',
    '--disable-background-networking', '--disable-component-update', '--mute-audio',
    // Linux runners keep unprivileged namespaces from Chrome's sandbox; it
    // only ever opens the repository's own pages and the tool's sheets.
    ...(process.platform === 'linux' ? ['--no-sandbox'] : []),
    'about:blank',
  ], { stdio: 'ignore', windowsHide: true });
  const undoChild = onCleanup(`headless browser, PID ${child.pid}`, () => stopChild(child));
  let cdp;
  try {
    const target = await waitForTarget(port, t => t.type === 'page', { timeoutMs: 20000 });
    cdp = await Cdp.connect(target.webSocketDebuggerUrl);
    await cdp.send('Page.enable');
    await cdp.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  } catch (error) {
    if (cdp) cdp.close();
    undoChild(); await stopChild(child);
    undoProfile(); await removeFolder(profile);
    throw error;
  }
  log(`headless browser PID ${child.pid}, port ${port}`);
  return {
    cdp,
    async goto(url) {
      const loaded = new Promise(resolve => { const off = cdp.on('Page.loadEventFired', () => { off(); resolve(); }); });
      await cdp.send('Page.navigate', { url });
      await Promise.race([loaded, sleep(15000)]);
    },
    async stop() {
      // Edge on Windows hands the browser to a process of its own, and the
      // one started ends at once: ending that leaves the browser running,
      // its profile held. So the browser is asked to close itself.
      await cdp.send('Browser.close', {}, 5000).catch(() => {});
      cdp.close();
      undoChild(); await stopChild(child);
      undoProfile(); await removeFolder(profile);
    },
  };
}

/** The mockup in a headless browser, showing the desktop's window or the phone. */
export async function launchMockup({ as = 'desktop', mockup, log }) {
  const browser = await launchHeadless({ width: 1600, height: 1000, log });
  let fonts;
  try {
    await browser.goto(pathToFileURL(mockup).href);
    // The mockup's stage shows both devices; one is enough.
    await browser.cdp.evaluate(`(() => { const s = document.getElementById('stage'); if (s) s.dataset.view = ${JSON.stringify(as === 'android' ? 'm' : 'd')}; return true; })()`);
    // Its fonts come from Google Fonts; without them it draws in another
    // font, and boxes the size of text differ a little.
    fonts = await browser.cdp.evaluate(`document.fonts.ready.then(() => ({ inter: document.fonts.check('16px Inter'), mono: document.fonts.check('16px "JetBrains Mono"') }))`, { timeoutMs: 10000 }).catch(() => ({ inter: false, mono: false }));
    await sleep(400);
  } catch (error) {
    await browser.stop();
    throw error;
  }
  const missing = [!fonts.inter && 'Inter', !fonts.mono && 'JetBrains Mono'].filter(Boolean);
  return {
    cdp: browser.cdp,
    platform: `mockup-${as}`,
    pointer: as === 'android' ? 'touch' : 'mouse',
    describe: `docs/ui/mockup.html in headless ${path.basename(findBrowser())}, as the ${as}`,
    notes: missing.length ? [`The mockup's fonts (${missing.join(', ')}) did not load from Google Fonts: it was drawn in a fallback font, so its text boxes differ a little.`] : [],
    stop: () => browser.stop(),
  };
}
