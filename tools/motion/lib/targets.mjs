// Where a scenario runs: a test copy of the desktop app, the Android app
// on the emulator (or, when its owner asks, a phone), the mockup in a
// headless browser. Each launcher returns a DevTools session with the
// page and a `stop` that undoes everything it started.

import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import { pathToFileURL } from 'node:url';
import { Cdp, waitForTarget, sleep } from './cdp.mjs';

const run = promisify(execFile);

/** A free TCP port on this machine at or above `from`, skipping `avoid`. */
export async function freePort(from, avoid = []) {
  for (let port = from; port < from + 200; port++) {
    if (avoid.includes(port)) continue;
    const free = await new Promise(resolve => {
      const server = net.createServer();
      server.once('error', () => resolve(false));
      server.listen(port, '127.0.0.1', () => server.close(() => resolve(true)));
    });
    if (free) return port;
  }
  throw new Error(`no free port from ${from}`);
}

// Ports other tools on this machine use for their own web views: never
// these (docs/ui/motion-check.md).
const TAKEN = [9222, 9333, 9377, 9471, 9563, 9564, 9565];

/** Removes a folder, trying again while a process that just ended lets go of it. */
export async function removeFolder(folder) {
  for (let tries = 0; tries < 20; tries++) {
    try { fs.rmSync(folder, { recursive: true, force: true }); return true; } catch { await sleep(500); }
  }
  return false;
}

// ---- The desktop app ---------------------------------------------------

/** The name the app's single-instance lock is held under (dirt-desktop's main.rs). */
const LOCK = 'dirt-desktop-single-instance';

/**
 * Copies the desktop app's folder and renames the single-instance lock in
 * the copy, so that the copy starts beside a Dirt that is already running.
 * The copy is a test build in a temporary folder; nothing in the repository
 * or the original folder is changed. The new name has the same length, so
 * nothing else in the program moves.
 */
function copyBesideRunning(exe, into, withVoice) {
  const from = path.dirname(exe);
  fs.cpSync(from, into, {
    recursive: true,
    filter: source => withVoice || path.relative(from, source) !== path.join('assets', 'voice'),
  });
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
export async function launchDesktop({ exe, port, besideRunning = false, voice = false, log = () => {} }) {
  if (!exe || !fs.existsSync(exe)) throw new Error(`no desktop app at ${exe}; build it with dx build --platform desktop --package dirt-desktop`);
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'dirt-motion-'));
  const data = path.join(home, 'data');
  fs.mkdirSync(data, { recursive: true });
  let program = exe;
  if (besideRunning) {
    log(`copying the app to ${home} and renaming its single-instance lock in the copy`);
    program = copyBesideRunning(exe, path.join(home, 'app'), voice);
  }
  port = port || await freePort(9581, TAKEN);
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
    DIRT_TRAY_AT: '-32000,-32000',
    DIRT_ACTIONS_FROM_STDIN: '1',
    DIRT_OPENED_TO_FILE: path.join(data, 'opened.txt'),
    DIRT_SIGN_IN_TO_FILE: path.join(data, 'sign-in.txt'),
    DIRT_EXPORT_TO: path.join(data, 'export'),
    // Never a real server: the store stays on this device.
    DIRT_API_BASE_URL: 'http://127.0.0.1:9',
  };
  const child = spawn(program, [], { env, stdio: ['pipe', 'ignore', 'pipe'], windowsHide: true });
  let stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-4000); });
  let exited = null;
  child.on('exit', code => { exited = code; });
  log(`desktop app started, PID ${child.pid}, DevTools port ${port}, data ${data}`);
  let target;
  try {
    target = await waitForTarget(port, t => t.type === 'page' && !t.url.startsWith('devtools'), { timeoutMs: 45000 });
  } catch (error) {
    const why = exited !== null
      ? `the app ended at once (code ${exited}). If another Dirt is running, its single-instance lock stops a second one: pass --beside-running. ${stderr.trim().split('\n').slice(-3).join(' ')}`
      : error.message;
    await stopChild(child);
    await removeFolder(home);
    throw new Error(why);
  }
  const cdp = await Cdp.connect(target.webSocketDebuggerUrl);
  return {
    cdp,
    platform: 'desktop',
    pointer: 'mouse',
    describe: `desktop app ${program} (PID ${child.pid}), off the screen, DevTools port ${port}`,
    async stop() {
      cdp.close();
      try { child.stdin.write('quit\n'); } catch { /* gone */ }
      await stopChild(child);
      await removeFolder(home);
    },
  };
}

async function stopChild(child) {
  for (let i = 0; i < 20 && child.exitCode === null; i++) await sleep(150);
  if (child.exitCode === null) {
    // By its process ID, never by name: the person's own Dirt has the same name.
    try { process.kill(child.pid); } catch { /* gone */ }
    for (let i = 0; i < 20 && child.exitCode === null; i++) await sleep(150);
  }
}

// ---- Android -------------------------------------------------------------

export const ANDROID_PACKAGE = 'com.example.DirtMobile';

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
  const emulator = devices.find(s => s.startsWith('emulator-'));
  if (!emulator) throw new Error('no emulator is running; start one (docs/ui/motion-check.md)');
  return emulator;
}

/**
 * Reaches the Android app's web view: starts the app if it is not running,
 * finds its DevTools socket and forwards a local port to it.
 */
export async function launchAndroid({ serial, realPhone = false, port, packageName = ANDROID_PACKAGE, log = () => {} }) {
  serial = await pickDevice(serial, realPhone);
  let pid = (await adb(serial, 'shell', 'pidof', packageName).catch(() => '')).trim();
  if (!pid) {
    log(`starting ${packageName} on ${serial}`);
    await adb(serial, 'shell', 'monkey', '-p', packageName, '-c', 'android.intent.category.LAUNCHER', '1');
    for (let i = 0; i < 40 && !pid; i++) { await sleep(500); pid = (await adb(serial, 'shell', 'pidof', packageName).catch(() => '')).trim(); }
    if (!pid) throw new Error(`${packageName} did not start on ${serial}`);
  }
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
  port = port || await freePort(9591, TAKEN);
  await adb(serial, 'forward', `tcp:${port}`, `localabstract:${socket}`);
  log(`forwarded port ${port} to ${socket} on ${serial}`);
  const target = await waitForTarget(port, t => t.type === 'page', { timeoutMs: 20000 });
  const cdp = await Cdp.connect(target.webSocketDebuggerUrl);
  return {
    cdp,
    platform: 'android',
    pointer: 'touch',
    serial,
    packageName,
    describe: `${packageName} (PID ${pid}) on ${serial}, DevTools through port ${port}`,
    async stop() {
      cdp.close();
      await adb(serial, 'forward', '--remove', `tcp:${port}`).catch(() => {});
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
    '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/microsoft-edge',
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
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'dirt-motion-browser-'));
  port = port || await freePort(9601, TAKEN);
  const child = spawn(browser, [
    '--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`,
    `--window-size=${width},${height}`, '--force-device-scale-factor=1', '--hide-scrollbars',
    '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--disable-sync',
    '--disable-background-networking', '--disable-component-update', '--mute-audio',
    'about:blank',
  ], { stdio: 'ignore', windowsHide: true });
  const target = await waitForTarget(port, t => t.type === 'page', { timeoutMs: 20000 });
  const cdp = await Cdp.connect(target.webSocketDebuggerUrl);
  await cdp.send('Page.enable');
  await cdp.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  log(`headless browser PID ${child.pid}, port ${port}`);
  return {
    cdp,
    async goto(url) {
      const loaded = new Promise(resolve => { const off = cdp.on('Page.loadEventFired', () => { off(); resolve(); }); });
      await cdp.send('Page.navigate', { url });
      await Promise.race([loaded, sleep(15000)]);
    },
    async stop() {
      cdp.close();
      try { process.kill(child.pid); } catch { /* gone */ }
      await sleep(300);
      await removeFolder(profile);
    },
  };
}

/** The mockup in a headless browser, showing the desktop's window or the phone. */
export async function launchMockup({ as = 'desktop', mockup, log }) {
  const browser = await launchHeadless({ width: 1600, height: 1000, log });
  await browser.goto(pathToFileURL(mockup).href);
  // The mockup's stage shows both devices; one is enough.
  await browser.cdp.evaluate(`(() => { const s = document.getElementById('stage'); if (s) s.dataset.view = ${JSON.stringify(as === 'android' ? 'm' : 'd')}; return true; })()`);
  await sleep(400);
  return {
    cdp: browser.cdp,
    platform: `mockup-${as}`,
    pointer: as === 'android' ? 'touch' : 'mouse',
    describe: `docs/ui/mockup.html in headless ${path.basename(findBrowser())}, as the ${as}`,
    stop: () => browser.stop(),
  };
}
