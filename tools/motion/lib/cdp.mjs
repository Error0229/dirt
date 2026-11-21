// A small client for the Chrome DevTools Protocol, over Node's own
// WebSocket (Node 22 and later): nothing to install.

/** Lists the pages a DevTools port offers. */
export async function listTargets(port, host = '127.0.0.1') {
  const response = await fetch(`http://${host}:${port}/json/list`);
  if (!response.ok) throw new Error(`DevTools on port ${port} answered ${response.status}`);
  return response.json();
}

/** Waits until the port offers a page that `pick` accepts. */
export async function waitForTarget(port, pick = t => t.type === 'page', { timeoutMs = 30000, host } = {}) {
  const until = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < until) {
    try {
      const target = (await listTargets(port, host)).find(pick);
      if (target) return target;
    } catch (error) { last = error; }
    await sleep(250);
  }
  throw new Error(`no page on DevTools port ${port} after ${timeoutMs} ms${last ? `: ${last.message}` : ''}`);
}

export const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

/** One session with one page. */
export class Cdp {
  static async connect(webSocketUrl) {
    const socket = new WebSocket(webSocketUrl);
    await new Promise((resolve, reject) => {
      socket.addEventListener('open', resolve, { once: true });
      socket.addEventListener('error', () => reject(new Error(`could not reach ${webSocketUrl}`)), { once: true });
    });
    return new Cdp(socket);
  }

  constructor(socket) {
    this.socket = socket;
    this.next = 1;
    this.waiting = new Map();
    this.listeners = new Map();
    socket.addEventListener('message', event => this.#heard(JSON.parse(event.data)));
    socket.addEventListener('close', () => {
      for (const { reject } of this.waiting.values()) reject(new Error('DevTools connection closed'));
      this.waiting.clear();
    });
  }

  #heard(message) {
    if (message.id) {
      const waiting = this.waiting.get(message.id);
      if (!waiting) return;
      this.waiting.delete(message.id);
      if (message.error) waiting.reject(new Error(`${waiting.method}: ${message.error.message}`));
      else waiting.resolve(message.result);
      return;
    }
    for (const listener of this.listeners.get(message.method) || []) listener(message.params);
  }

  send(method, params = {}, timeoutMs = 60000) {
    const id = this.next++;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiting.delete(id);
        reject(new Error(`${method}: no answer in ${timeoutMs} ms`));
      }, timeoutMs);
      this.waiting.set(id, {
        method,
        resolve: value => { clearTimeout(timer); resolve(value); },
        reject: error => { clearTimeout(timer); reject(error); },
      });
    });
  }

  on(method, listener) {
    if (!this.listeners.has(method)) this.listeners.set(method, new Set());
    this.listeners.get(method).add(listener);
    return () => this.listeners.get(method).delete(listener);
  }

  /** Runs `expression` in the page and returns its value. */
  async evaluate(expression, { awaitPromise = true, timeoutMs } = {}) {
    const result = await this.send('Runtime.evaluate', { expression, awaitPromise, returnByValue: true }, timeoutMs);
    if (result.exceptionDetails) {
      const details = result.exceptionDetails;
      throw new Error(`in the page: ${details.exception?.description || details.text}`);
    }
    return result.result.value;
  }

  close() { try { this.socket.close(); } catch { /* already closed */ } }
}
