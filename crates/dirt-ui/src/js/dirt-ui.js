// The one script both apps run (interface design, sections 5.7 and 10).
//
// It does what a style sheet cannot and what must not wait for Rust:
//
// - the list's animation, which watches the page and plays the difference
//   from the positions it last saw;
// - everything that follows a finger or a pointer (a swiped card, a
//   dragged sheet, the scrollbar's thumb, the slider), from first touch to
//   release, telling Rust only how it ended;
// - motion that depends on where things are (a panel out of its button,
//   the circle a new theme spreads in, a toast opening);
// - tooltips, and focus going into a panel or sheet and back out of it;
// - the app's own menu on a right click, arrow keys through a group, the
//   one highlight of a list, and more rows as a list nears its end;
// - on the desktop: controls shown by the pointer's nearness, the page
//   emptied as a thought is done, the window moved by its top row, and
//   focus while writing;
// - the file preview's keys, gestures and movement, a sound's card, a
//   PDF's pages drawn by the bundled PDF.js, files dragged over the window
//   and a picture pasted into the page.
//
// Rust renders elements with `data-dui-*` attributes; the script finds
// them as they appear. Nothing here keeps the app's state.
(function () {
  'use strict';
  if (window.DirtUi) return;

  const root = document.documentElement;
  const $$ = (selector, from = document) => [...from.querySelectorAll(selector)];
  const motionQuery = matchMedia('(prefers-reduced-motion: reduce)');
  // Read when an animation starts, so that changing the setting takes
  // effect at once (section 5.6).
  const reduced = () => root.dataset.motion === 'reduced' || motionQuery.matches;
  // Asked of the web view itself: no engine is named.
  const knowsLinear = !!(window.CSS && CSS.supports && CSS.supports('transition-timing-function', 'linear(0, 1)'));

  let tokens = null;
  let platform = 'desktop';
  const touchUi = () => platform === 'android';

  function spring(name) {
    if (reduced()) return { easing: 'ease', ms: tokens.reducedMs };
    const one = tokens.springs[name];
    return { easing: knowsLinear ? one.linear : one.fallback, ms: one.ms };
  }

  // Runs `then` when an animation has finished. A page that is not being
  // drawn (the window hidden, the phone's screen off) finishes nothing
  // until it is drawn again, so a timer stands behind the animation.
  function after(animation, ms, then) {
    let done = false;
    const once = () => { if (!done) { done = true; then(); } };
    animation.finished.catch(() => {}).then(once);
    setTimeout(once, ms + 400);
  }

  // Reduced motion keeps the fade and drops the movement.
  function animate(element, frames, options) {
    if (!element) return { finished: Promise.resolve(), cancel() {} };
    if (reduced()) {
      const fades = frames.map(frame => ({ opacity: frame.opacity === undefined ? 1 : frame.opacity }));
      const fill = options.fill === 'forwards' || options.fill === 'both' ? 'forwards' : 'none';
      return element.animate(fades, { duration: Math.min(options.duration || 0, 120), fill });
    }
    return element.animate(frames, options);
  }

  // ---- Telling Rust ---------------------------------------------------
  // One channel, opened by the root component in the same eval that
  // installs this script, so nothing is told before it. A message is how
  // something ended, never how it is going.
  let post = null;
  const stats = { moves: 0, told: 0 };
  function showStats() {
    $$('[data-dui-stat]').forEach(element => { element.textContent = String(stats[element.dataset.duiStat]); });
  }
  function tell(element, kind, more) {
    const id = element && element.dataset ? element.dataset.duiId : null;
    if (!id) return;
    const message = Object.assign({ id, kind }, more || {});
    stats.told += 1;
    showStats();
    if (post) post(message);
  }
  function connect(send) {
    post = send;
  }

  // ---- Where the last press was ---------------------------------------
  let lastPress = [innerWidth / 2, 80];
  document.addEventListener('pointerdown', event => { lastPress = [event.clientX, event.clientY]; }, true);
  document.addEventListener('keydown', event => {
    if (event.key !== 'Enter' && event.key !== ' ' && !event.key.startsWith('Arrow')) return;
    const at = document.activeElement;
    if (!at || at === document.body) return;
    const box = at.getBoundingClientRect();
    lastPress = [box.left + box.width / 2, box.top + box.height / 2];
  }, true);

  // ---- Theme, accent, text size (sections 4.1, 4.1.1, 5.5) --------------
  let looks = null;
  function setLooks(to) {
    if (to.theme === 'system') delete root.dataset.theme; else root.dataset.theme = to.theme;
    if (to.accent === 'clay') delete root.dataset.accent; else root.dataset.accent = to.accent;
    root.style.setProperty('--note-size', to.size + 'px');
    if (to.reduced) root.dataset.motion = 'reduced'; else delete root.dataset.motion;
    root.classList.toggle('desktop', to.platform === 'desktop');
    root.classList.toggle('android', to.platform === 'android');
    platform = to.platform;
    tellBars();
  }
  // Android draws the bars above and below the page itself. The activity
  // offers `DirtAndroid.bars` to be told whether the page is dark now and
  // the colour of its background, so that the bars match it.
  const darkQuery = matchMedia('(prefers-color-scheme: dark)');
  // An element with `data-dui-bars` that covers the page (an old screen
  // with colours of its own) gives the bars its background while it is
  // there.
  const hex = colour => {
    const parts = (colour.match(/[\d.]+/g) || []).slice(0, 3).map(Number);
    if (parts.length < 3) return null;
    return '#' + parts.map(part => Math.round(part).toString(16).padStart(2, '0')).join('');
  };
  function tellBars() {
    if (!window.DirtAndroid || !DirtAndroid.bars) return;
    let dark = root.dataset.theme === 'dark' || (root.dataset.theme !== 'light' && darkQuery.matches);
    let background = getComputedStyle(root).getPropertyValue('--bg').trim();
    const cover = $$('[data-dui-bars]').filter(element => element.isConnected).pop();
    const own = cover ? hex(getComputedStyle(cover).backgroundColor) : null;
    if (own) {
      background = own;
      const [r, g, b] = own.slice(1).match(/../g).map(part => parseInt(part, 16));
      dark = 0.299 * r + 0.587 * g + 0.114 * b < 128;
    }
    try { DirtAndroid.bars(dark, background); } catch (error) { /* the activity went away */ }
  }
  darkQuery.addEventListener('change', tellBars);
  function crossFade(change) {
    root.classList.add('dui-theme-fade');
    change();
    setTimeout(() => root.classList.remove('dui-theme-fade'), 360);
  }
  // The new look spreads in a circle from the point pressed. Where the web
  // view cannot, and with reduced motion, every colour cross-fades.
  function reveal(change) {
    if (!document.startViewTransition || reduced()) return crossFade(change);
    const [x, y] = lastPress;
    const radius = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
    const glide = spring('glide');
    let transition;
    try { transition = document.startViewTransition(change); } catch (error) { return crossFade(change); }
    transition.ready.then(() => root.animate(
      { clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${radius}px at ${x}px ${y}px)`] },
      { duration: 620, easing: glide.easing, pseudoElement: '::view-transition-new(root)' },
    )).catch(() => {});
  }
  function apply(to) {
    const now = to.theme + '/' + to.accent;
    const change = () => setLooks(to);
    if (looks === null || looks === now) change(); else reveal(change);
    looks = now;
  }

  // ---- Tooltips, and long presses on Android (section 4.5) --------------
  const tip = document.createElement('div');
  tip.className = 'dui-tip';
  tip.setAttribute('aria-hidden', 'true');
  let tipFor = null, tipTimer = 0, tipSeenAt = 0, swallowClick = false;
  function showTip(element) {
    if (!element.isConnected) return;
    if (!tip.isConnected) document.body.append(tip);
    tip.textContent = element.dataset.tip;
    const keys = element.dataset.kbd;
    if (keys && !touchUi()) {
      const chips = document.createElement('span');
      chips.className = 'dui-tip-keys';
      keys.split(' ').forEach(key => {
        const chip = document.createElement('kbd');
        chip.textContent = key;
        chips.append(chip);
      });
      tip.append(chips);
    }
    const box = element.getBoundingClientRect(), mine = tip.getBoundingClientRect();
    let top = box.top - mine.height - 8;
    if (top < 8) top = box.bottom + 8;
    const left = Math.max(8, Math.min(innerWidth - mine.width - 8, box.left + box.width / 2 - mine.width / 2));
    tip.style.top = top + 'px';
    tip.style.left = left + 'px';
    tip.classList.add('on');
    tipSeenAt = Date.now();
  }
  function hideTip() {
    clearTimeout(tipTimer);
    if (tip.classList.contains('on')) tipSeenAt = Date.now();
    tip.classList.remove('on');
    tipFor = null;
  }
  function tipSoon(element) {
    if (element === tipFor) return;
    hideTip();
    if (!element || !element.dataset.tip) return;
    tipFor = element;
    // At once if another closed a moment ago, to sweep along a toolbar.
    const warm = Date.now() - tipSeenAt < 500;
    tipTimer = setTimeout(() => showTip(element), warm ? 0 : 480);
  }
  document.addEventListener('pointerover', event => {
    if (event.pointerType === 'touch') return;
    tipSoon(event.target.closest ? event.target.closest('[data-tip]') : null);
  });
  document.addEventListener('focusin', event => {
    const element = event.target.closest ? event.target.closest('[data-tip]') : null;
    if (element && element.matches(':focus-visible')) tipSoon(element);
  });
  document.addEventListener('focusout', hideTip);
  document.addEventListener('scroll', hideTip, true);
  // A long press shows what a hover would, and is not a tap. A web view
  // says when a press has become a long one by asking for its menu, and
  // then takes the pointer away; the timer is for one that does not ask.
  let byTouch = false, pressed = false;
  function longPress(element) {
    if (pressed) return;
    pressed = true;
    clearTimeout(tipTimer);
    tipFor = element;
    showTip(element);
    swallowClick = true;
    // It stays long enough to be read, whenever the finger lifts.
    tipTimer = setTimeout(hideTip, 1500);
  }
  document.addEventListener('pointerdown', event => {
    hideTip();
    byTouch = event.pointerType === 'touch';
    pressed = false;
    if (!byTouch) return;
    const element = event.target.closest ? event.target.closest('[data-tip]') : null;
    if (!element) return;
    tipFor = element;
    tipTimer = setTimeout(() => longPress(element), 500);
  }, true);
  const endPress = () => {
    if (!pressed) clearTimeout(tipTimer);
    setTimeout(() => { swallowClick = false; }, 80);
  };
  document.addEventListener('pointerup', endPress, true);
  document.addEventListener('pointercancel', endPress, true);
  document.addEventListener('click', event => {
    if (!swallowClick) return;
    swallowClick = false;
    event.preventDefault();
    event.stopPropagation();
  }, true);
  document.addEventListener('contextmenu', event => {
    const element = event.target.closest ? event.target.closest('[data-tip]') : null;
    if (!element || !(byTouch || touchUi())) return;
    event.preventDefault();
    if (byTouch) longPress(element);
  });

  // ---- The list's animation (section 5.5, "The list narrows or reorders")
  // A list is an element with `data-dui-list`; its rows have `data-key`.
  // The script remembers where each row was laid out. When the page under
  // a list changes, rows that stayed slide from where they were to where
  // they are (snap), new ones settle in, and ones that left are drawn
  // leaving. Rust only changes the page.
  const lists = new Map();
  function rowsOf(list) {
    return $$('[data-key]', list).filter(row => row.closest('[data-dui-list]') === list && !row.closest('.dui-ghost'));
  }
  // Where a row is laid out inside its list. Read from the layout, not
  // from the screen, so a row in mid-flight reads as where it will land.
  function placeOf(row, list) {
    let x = 0, y = 0, at = row;
    while (at && at !== list) {
      x += at.offsetLeft;
      y += at.offsetTop;
      at = at.offsetParent;
      if (at && !list.contains(at)) break;
    }
    return { x, y, w: row.offsetWidth, h: row.offsetHeight, row };
  }
  const shown = element => element.getClientRects().length > 0;
  // The part of a list a person can see, top and bottom, in the list's own
  // measure, with a little to spare. A row that neither was there nor is
  // there now is not animated: a list may hold hundreds.
  function viewOf(list) {
    const box = list.getBoundingClientRect();
    const top = Math.max(list.scrollTop, list.scrollTop - box.top);
    const bottom = Math.min(list.scrollTop + list.clientHeight, list.scrollTop - box.top + innerHeight);
    return [top - 120, bottom + 120];
  }
  const within = (place, view) => place.y + place.h > view[0] && place.y < view[1];
  function remember(list) {
    const state = lists.get(list);
    if (!state || !shown(list)) return;
    const seen = new Map();
    rowsOf(list).forEach(row => seen.set(row.dataset.key, placeOf(row, list)));
    state.seen = seen;
    state.ready = true;
    placeSelection(list, true);
  }
  // The one highlight of a list (section 5.5, "Selecting a note"): an
  // element `.dui-sel-ind` among the list's children, which Rust renders
  // and the script moves. It slides from the row that was chosen to the
  // one that is, stretching to its height (snap); nothing fades. `quiet`
  // puts it there without the slide, for a list that only changed size.
  function placeSelection(list, quiet) {
    const mark = [...list.children].find(child => child.classList.contains('dui-sel-ind'));
    if (!mark) return;
    const row = rowsOf(list).find(candidate => candidate.classList.contains('sel'));
    // Touching the class only when it changes: the list's observer hears
    // every write to it, even one that leaves it as it was.
    if (!row || !shown(row)) { if (mark.classList.contains('on')) mark.classList.remove('on'); return; }
    const place = placeOf(row, list);
    const fresh = !mark.classList.contains('on');
    const jump = quiet || fresh || reduced();
    if (jump) mark.style.transition = 'none';
    Object.assign(mark.style, { top: place.y + 'px', left: place.x + 'px', width: place.w + 'px', height: place.h + 'px' });
    if (fresh) mark.classList.add('on');
    if (jump) {
      void mark.offsetHeight;
      mark.style.transition = '';
    }
  }
  // Where a row in flight is on screen, as an offset from its layout.
  function flightOf(row) {
    const run = row._duiMove;
    if (!run || run.playState !== 'running') return [0, 0];
    const matrix = new DOMMatrixReadOnly(getComputedStyle(row).transform);
    run.cancel();
    return [matrix.m41, matrix.m42];
  }
  function leave(list, was, how) {
    const row = was.row;
    if (how === 'none' || row.dataset.duiGone !== undefined) return false;
    const ghost = row.cloneNode(true);
    ghost.classList.add('dui-ghost');
    ghost.removeAttribute('data-key');
    ghost.removeAttribute('id');
    $$('[data-key],[id],[data-dui-id]', ghost).forEach(inner => {
      inner.removeAttribute('data-key'); inner.removeAttribute('id'); inner.removeAttribute('data-dui-id');
    });
    ghost.setAttribute('aria-hidden', 'true');
    ghost.inert = true;
    Object.assign(ghost.style, { left: was.x + 'px', top: was.y + 'px', width: was.w + 'px', height: was.h + 'px' });
    list.append(ghost);
    const moving = !reduced();
    const tear = how === 'tear' && moving;
    let going, ms;
    if (tear) {
      // The row tears away to the left (170 ms, exit).
      ms = 170;
      going = ghost.animate([{ transform: 'none', opacity: 1 }, { transform: 'translateX(-28px) scale(.96)', opacity: 0 }],
        { duration: ms, easing: tokens.exit, fill: 'forwards' });
    } else if ((how === 'fold' || how === 'sink') && moving) {
      // An archived row folds shut where it was, while the rows below
      // rise into its place (snap); its grain sinks into the pile.
      if (how === 'sink') sink(list, was);
      const snap = spring('snap');
      ms = Math.min(snap.ms, 320);
      ghost.style.transformOrigin = '50% 0';
      ghost.style.overflow = 'hidden';
      going = ghost.animate([{ transform: 'none', opacity: 1 }, { transform: 'scaleY(.2)', opacity: 0 }],
        { duration: ms, easing: tokens.exit, fill: 'forwards' });
    } else if (how === 'shrink' && moving) {
      // A token shrinks to nothing sideways while fading (200 ms, exit),
      // and the field closes up.
      ms = 200;
      ghost.style.transformOrigin = '0 50%';
      going = ghost.animate([{ transform: 'none', opacity: 1 }, { transform: 'scaleX(0)', opacity: 0 }],
        { duration: ms, easing: tokens.exit, fill: 'forwards' });
    } else {
      ms = moving ? 120 : tokens.reducedMs;
      going = ghost.animate([{ opacity: 1 }, { opacity: 0 }], { duration: ms, easing: tokens.standard, fill: 'forwards' });
    }
    after(going, ms, () => ghost.remove());
    return tear;
  }
  function play(list) {
    const state = lists.get(list);
    if (!state) return;
    if (!shown(list)) { state.ready = false; return; }
    // Nobody sees a page that is not being drawn: take the places down and
    // play nothing.
    if (!state.ready || document.hidden) return remember(list);
    const seen = state.seen, next = new Map();
    const rows = rowsOf(list);
    rows.forEach(row => next.set(row.dataset.key, placeOf(row, list)));
    const quiet = reduced();
    const how = list.dataset.duiExit || 'tear';
    const view = viewOf(list);
    // What a leaving row disturbs follows it, a moment later.
    let torn = false;
    seen.forEach((was, key) => { if (!next.has(key) && within(was, view)) torn = leave(list, was, how) || torn; });
    const snap = spring('snap');
    const follow = torn ? 110 : 0;
    let fresh = 0;
    rows.forEach(row => {
      const now = next.get(row.dataset.key), was = seen.get(row.dataset.key);
      if (!was) {
        // A new thought's row waits for its grain (`settleRows`).
        if (row.dataset.duiSettle !== undefined) return;
        const first = !seen.size && list.dataset.duiEnterFirst === undefined;
        if (first || row.dataset.duiEnter === 'none' || !within(now, view)) return;
        if (quiet) { row.animate([{ opacity: 0 }, { opacity: 1 }], { duration: tokens.reducedMs }); return; }
        if (row.dataset.duiEnter === 'grow') {
          // A file's tile grows out of the point last pressed (the
          // paperclip) or where it was dropped, and bounces into place
          // (pop); several follow one another, 70 ms apart.
          const pop = spring('pop'), box = row.getBoundingClientRect();
          const dx = lastPress[0] - (box.left + box.width / 2), dy = lastPress[1] - (box.top + box.height / 2);
          row.animate([{ opacity: 0, transform: `translate(${dx}px, ${dy}px) scale(.3)` }, { opacity: 1, transform: 'none' }],
            { duration: pop.ms, easing: pop.easing, delay: Math.min(fresh++, 6) * 70, fill: 'backwards' });
          return;
        }
        if (row.dataset.duiEnter === 'pop') {
          // Undo: the row drops back into its place with a bounce.
          const pop = spring('pop');
          row.animate([{ opacity: 0, transform: 'scale(.9) translateY(-6px)' }, { opacity: 1, transform: 'none' }],
            { duration: pop.ms, easing: pop.easing });
          return;
        }
        // New rows settle in from above, one after another; past the
        // sixth the rest arrive together.
        row.animate([{ opacity: 0, transform: 'translateY(-8px) scale(.98)' }, { opacity: 1, transform: 'none' }],
          { duration: snap.ms, easing: snap.easing, delay: follow + Math.min(fresh++, 6) * 24, fill: 'backwards' });
        return;
      }
      let dx = was.x - now.x, dy = was.y - now.y;
      // Laid out where it was: a row in flight keeps flying.
      if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return;
      const [fx, fy] = flightOf(row);
      if (quiet || !(within(was, view) || within(now, view))) return;
      dx += fx; dy += fy;
      row._duiMove = row.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }],
        { duration: snap.ms, easing: snap.easing, delay: follow, fill: 'backwards' });
    });
    state.seen = next;
    placeSelection(list, false);
  }
  // A window made narrower, a font arriving or a text size chosen moves
  // rows with no change to the page. Those are not played: the positions
  // are only taken down again.
  const resized = new Set();
  let resizeFrame = 0;
  const sizes = new ResizeObserver(entries => {
    entries.forEach(entry => {
      const list = entry.target.closest('[data-dui-list]');
      if (list) resized.add(list);
    });
    cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(() => { resized.forEach(remember); resized.clear(); });
  });
  function watchList(list) {
    if (lists.has(list)) return;
    const state = { seen: new Map(), ready: false, known: new Set() };
    // The observer's callback runs after the page has changed and before
    // it is drawn, which is when the difference can be measured.
    state.observer = new MutationObserver(() => {
      play(list);
      settleRows(list);
      const rows = new Set(rowsOf(list));
      rows.forEach(row => { if (!state.known.has(row)) sizes.observe(row); });
      state.known.forEach(row => { if (!rows.has(row)) sizes.unobserve(row); });
      state.known = rows;
    });
    state.observer.observe(list, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['class', 'hidden', 'data-key', 'data-dui-settle'] });
    lists.set(list, state);
    sizes.observe(list);
    rowsOf(list).forEach(row => { state.known.add(row); sizes.observe(row); });
    remember(list);
    settleRows(list);
  }
  function forgetList(list) {
    const state = lists.get(list);
    if (!state) return;
    state.observer.disconnect();
    sizes.unobserve(list);
    state.known.forEach(row => sizes.unobserve(row));
    lists.delete(list);
  }

  // How long each frame took while something moved, for the gallery and
  // for anyone who doubts it.
  function frames(ms) {
    return new Promise(resolve => {
      const times = [];
      let last = 0, start = 0;
      const step = now => {
        if (!start) start = now; else times.push(now - last);
        last = now;
        if (now - start < ms) return requestAnimationFrame(step);
        const sorted = times.slice().sort((a, b) => a - b);
        const at = share => sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * share))] : 0;
        const median = at(0.5);
        const round = value => Math.round(value * 10) / 10;
        resolve({
          frames: times.length,
          ms: round(now - start),
          median: round(median),
          p95: round(at(0.95)),
          longest: round(sorted.length ? sorted[sorted.length - 1] : 0),
          // A frame that took more than half as long again as most did.
          late: times.filter(time => time > median * 1.5).length,
          hz: median ? Math.round(1000 / median) : 0,
        });
      };
      requestAnimationFrame(step);
    });
  }

  // ---- Swiping a card (sections 5.5 and 8.2) -----------------------------
  // The card is the element with `data-dui-swipe`. It follows the finger
  // exactly, here, with no word to Rust; let go past 96 px it flies off and
  // Rust is told once.
  const SWIPE_PAST = 96;
  let swipe = null, quietClick = false;
  document.addEventListener('pointerdown', event => {
    if (event.button) return;
    const card = event.target.closest ? event.target.closest('[data-dui-swipe]') : null;
    if (!card || card.dataset.duiGone !== undefined) return;
    swipe = { card, x: event.clientX, y: event.clientY, id: event.pointerId, mode: null, dx: 0 };
  }, true);
  window.addEventListener('pointermove', event => {
    if (!swipe || event.pointerId !== swipe.id) return;
    const dx = event.clientX - swipe.x, dy = event.clientY - swipe.y;
    if (!swipe.mode) {
      if (Math.abs(dx) > 8 && Math.abs(dx) > Math.abs(dy) && dx < 0) {
        swipe.mode = 'swipe';
        try { swipe.card.setPointerCapture(swipe.id); } catch (error) { /* the pointer has gone */ }
        root.classList.add('dui-noselect');
      } else if (Math.abs(dy) > 8 || Math.abs(dx) > 8) swipe.mode = 'none';
    }
    if (swipe.mode !== 'swipe') return;
    // A swipe is not a long press: the tooltip that was coming does not.
    if (tipFor) hideTip();
    stats.moves += 1;
    showStats();
    swipe.dx = Math.min(0, dx);
    swipe.card.style.transition = 'none';
    swipe.card.style.transform = `translateX(${swipe.dx}px)`;
    const holder = swipe.card.parentElement;
    if (holder) holder.classList.toggle('armed', swipe.dx < -SWIPE_PAST);
  }, { passive: true });
  function endSwipe(event) {
    if (!swipe || (event && event.pointerId !== swipe.id)) return;
    const done = swipe;
    swipe = null;
    root.classList.remove('dui-noselect');
    if (done.mode !== 'swipe') return;
    quietClick = true;
    setTimeout(() => { quietClick = false; }, 60);
    const card = done.card, holder = card.parentElement;
    if (done.dx < -SWIPE_PAST && !(event && event.type === 'pointercancel')) {
      card.dataset.duiGone = '';
      const ms = reduced() ? tokens.reducedMs : 180;
      const off = card.animate([{ transform: `translateX(${done.dx}px)` }, { transform: 'translateX(-110%)' }],
        { duration: ms, easing: tokens.exit, fill: 'forwards' });
      after(off, ms, () => {
        // Where it is drawn now, written on it: the copy of it that the
        // list draws leaving is made from what is written, and would
        // otherwise come back to where the finger let go.
        card.style.transform = 'translateX(-110%)';
        tell(card, 'swipe');
        // If Rust keeps the card after all, it comes back.
        setTimeout(() => {
          if (!card.isConnected) return;
          delete card.dataset.duiGone;
          off.cancel();
          card.style.transition = '';
          card.style.transform = '';
          if (holder) holder.classList.remove('armed');
        }, 700);
      });
    } else {
      // Let go before the mark: it springs back (pop), by the style sheet.
      card.style.transition = '';
      card.style.transform = '';
      if (holder) holder.classList.remove('armed');
    }
  }
  window.addEventListener('pointerup', endSwipe);
  window.addEventListener('pointercancel', endSwipe);
  document.addEventListener('click', event => {
    if (!quietClick) return;
    quietClick = false;
    event.preventDefault();
    event.stopPropagation();
  }, true);

  // ---- Pulling a list down to sync (sections 5.5 and 8.2) ----------------
  // The element with `data-dui-pull` is the content of a list that
  // scrolls. Pulled down while the list is at its top, it follows the
  // finger with resistance; let go past 62 px it holds at 58 px while the
  // arrow spins, and Rust is told once. It holds for as long as the
  // element is marked `data-busy`, and then settles back (400 ms, enter).
  //
  // Touch events, not pointer events: the list scrolls natively, and a web
  // view takes a vertical drag for its own once it begins, cancelling the
  // pointer. A touch can still be held back from scrolling by its first
  // move, which is when a pull is told from a scroll.
  const PULL_PAST = 62, PULL_HOLD = 58;
  const pullOf = distance => 96 * (1 - Math.exp(-Math.max(0, distance) / 130));
  let pull = null;
  function scrollerOf(element) {
    return element.closest('[data-dui-scroll]') || element.parentElement;
  }
  function pullParts(area) {
    const arrow = area.querySelector(':scope > .dui-ptr');
    return { arrow, icon: arrow ? arrow.querySelector('.dui-i') : null };
  }
  function settlePull(area) {
    const { arrow, icon } = pullParts(area);
    const ms = reduced() ? tokens.reducedMs : 400;
    area.style.transition = `transform ${ms}ms ${tokens.enter}`;
    area.style.transform = '';
    if (arrow) {
      arrow.style.transition = `opacity ${ms}ms ${tokens.enter}`;
      arrow.style.opacity = '';
      arrow.classList.remove('armed', 'spin');
    }
    if (icon) icon.style.transform = '';
    setTimeout(() => { area.style.transition = ''; if (arrow) arrow.style.transition = ''; delete area.dataset.duiHeld; }, ms + 40);
  }
  function holdPull(area) {
    const { arrow, icon } = pullParts(area);
    area.dataset.duiHeld = '';
    area.style.transition = `transform ${spring('snap').ms}ms ${spring('snap').easing}`;
    area.style.transform = `translateY(${PULL_HOLD}px)`;
    if (arrow) { arrow.classList.add('spin'); arrow.style.opacity = '1'; }
    if (icon) icon.style.transform = '';
    tell(area, 'pull');
    // Long enough to be seen, and for Rust to say that sync has begun;
    // then for as long as it runs, up to a limit.
    const began = Date.now();
    const check = () => {
      if (!area.isConnected) return;
      const waited = Date.now() - began;
      if (waited < 600 || (area.dataset.busy !== undefined && waited < 15000)) { setTimeout(check, 120); return; }
      settlePull(area);
    };
    setTimeout(check, 120);
  }
  document.addEventListener('touchstart', event => {
    // A second finger, or a touch that began before the last one ended,
    // ends a pull in progress: it settles back, as one let go short of the
    // mark does, and is never left half pulled.
    if (pull) {
      const was = pull;
      pull = null;
      if (was.mode === 'pull') {
        root.classList.remove('dui-noselect');
        settlePull(was.area);
      }
    }
    if (event.touches.length !== 1 || !event.target.closest) return;
    const area = event.target.closest('[data-dui-pull]');
    if (!area || area.dataset.duiHeld !== undefined) return;
    const touch = event.touches[0];
    pull = { area, x: touch.clientX, y: touch.clientY, mode: null, d: 0 };
  }, { passive: true, capture: true });
  document.addEventListener('touchmove', event => {
    if (!pull) return;
    const touch = event.touches[0];
    const dx = touch.clientX - pull.x, dy = touch.clientY - pull.y;
    if (!pull.mode) {
      if (Math.abs(dx) < 6 && Math.abs(dy) < 6) return;
      const top = scrollerOf(pull.area).scrollTop <= 0;
      pull.mode = dy > 0 && Math.abs(dy) > Math.abs(dx) && top ? 'pull' : 'none';
      if (pull.mode === 'pull') { hideTip(); root.classList.add('dui-noselect'); }
    }
    if (pull.mode !== 'pull') return;
    // The list does not scroll or stretch: the pull is the movement.
    if (event.cancelable) event.preventDefault();
    stats.moves += 1;
    showStats();
    pull.d = pullOf(dy);
    const { arrow, icon } = pullParts(pull.area);
    pull.area.style.transition = 'none';
    pull.area.style.transform = `translateY(${pull.d}px)`;
    if (arrow) {
      arrow.style.transition = 'none';
      arrow.style.opacity = String(Math.min(1, pull.d / 48));
      arrow.classList.toggle('armed', pull.d > PULL_PAST);
    }
    if (icon && !reduced()) icon.style.transform = `rotate(${pull.d * 3.5}deg)`;
  }, { passive: false, capture: true });
  function endPull(event) {
    if (!pull) return;
    const done = pull;
    pull = null;
    if (done.mode !== 'pull') return;
    root.classList.remove('dui-noselect');
    // A touch that was a pull is not also a tap on what it began on.
    quietClick = true;
    setTimeout(() => { quietClick = false; }, 60);
    if (done.d > PULL_PAST && event.type !== 'touchcancel') holdPull(done.area);
    else settlePull(done.area);
  }
  document.addEventListener('touchend', endPull, true);
  document.addEventListener('touchcancel', endPull, true);

  // ---- A page that grows out of what opened it (sections 5.5 and 10) -----
  // An element `[data-dui-grow]` is a page over the screen, in the page
  // always. When it gains the class `on` it grows out of the element whose
  // ID is in its `data-dui-from` (a card: 440 ms, enter, its text rising
  // 14 px behind it; the bar it was asked for from, `data-dui-from-kind`
  // "bar": the glide curve over 560 ms). When it loses `on` it shrinks
  // back into the element `data-dui-from` names then, wherever that is now
  // (340 ms, or 320 ms into the bar, standard). Where that element is not
  // on screen it fades. The movement is a clip-path from the element's
  // rectangle to the screen.
  const grows = new Map();
  function clipOf(element, round) {
    const box = element.getBoundingClientRect();
    const radius = round === undefined ? parseFloat(getComputedStyle(element).borderTopLeftRadius) || 16 : round;
    return `inset(${box.top}px ${innerWidth - box.right}px ${innerHeight - box.bottom}px ${box.left}px round ${radius}px)`;
  }
  function onScreen(element) {
    if (!element || !element.isConnected || !shown(element)) return false;
    const box = element.getBoundingClientRect();
    return box.bottom > 0 && box.top < innerHeight && box.width > 0 && box.height > 0;
  }
  function growOpened(page, state) {
    state.runs.forEach(run => run.cancel());
    state.runs = [];
    page.classList.remove('leaving');
    const from = document.getElementById(page.dataset.duiFrom || '');
    const body = page.querySelector('[data-dui-grow-body]'), bar = page.querySelector('[data-dui-grow-bar]');
    if (reduced() || !onScreen(from)) {
      state.runs.push(page.animate([{ opacity: 0 }, { opacity: 1 }], { duration: reduced() ? tokens.reducedMs : 200, easing: tokens.standard }));
      return;
    }
    const whole = 'inset(0px 0px 0px 0px round 0px)';
    if (page.dataset.duiFromKind === 'bar') {
      const glide = spring('glide');
      state.runs.push(page.animate([{ clipPath: clipOf(from) }, { clipPath: whole }], { duration: 560, easing: glide.easing }));
      if (bar) state.runs.push(bar.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, delay: 180, fill: 'backwards' }));
      return;
    }
    state.runs.push(page.animate([{ clipPath: clipOf(from) }, { clipPath: whole }], { duration: 440, easing: tokens.enter }));
    if (body) state.runs.push(body.animate([{ opacity: 0, transform: 'translateY(14px)' }, { opacity: 1, transform: 'none' }],
      { duration: 380, delay: 60, easing: tokens.enter, fill: 'backwards' }));
    if (bar) state.runs.push(bar.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 260, delay: 120, fill: 'backwards' }));
  }
  function growClosed(page, state) {
    state.runs.forEach(run => run.cancel());
    state.runs = [];
    // Drawn until it has gone; the style sheet hides a page without `on`.
    page.classList.add('leaving');
    const to = document.getElementById(page.dataset.duiFrom || '');
    const body = page.querySelector('[data-dui-grow-body]');
    let going, ms;
    const grain = page.dataset.duiFromKind === 'grain';
    if (reduced() || (!grain && !onScreen(to))) {
      ms = reduced() ? tokens.reducedMs : 180;
      going = page.animate([{ opacity: 1 }, { opacity: 0 }], { duration: ms, easing: tokens.standard, fill: 'forwards' });
    } else if (grain) {
      // A new page with words closes to a 7 px circle where its words
      // began (300 ms), and its grain is tossed from there onto its card
      // (`settle`).
      const field = page.querySelector('textarea');
      const box = field ? field.getBoundingClientRect() : null;
      const size = field ? parseFloat(getComputedStyle(field).fontSize) || 16 : 16;
      const x = box ? box.left + 4 : innerWidth / 2, y = box ? box.top + 6 + size * 0.8 : innerHeight / 2;
      const far = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
      ms = 300;
      going = page.animate([{ clipPath: `circle(${far}px at ${x}px ${y}px)` }, { clipPath: `circle(7px at ${x}px ${y}px)` }],
        { duration: ms, easing: 'cubic-bezier(.5,0,.75,.2)', fill: 'forwards' });
      gathering = { at: performance.now(), x, y };
    } else {
      ms = page.dataset.duiFromKind === 'bar' ? 320 : 340;
      going = page.animate([{ clipPath: 'inset(0px 0px 0px 0px round 0px)' }, { clipPath: clipOf(to) }], { duration: ms, easing: tokens.standard, fill: 'forwards' });
      if (body && page.dataset.duiFromKind !== 'bar') state.runs.push(body.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 200, easing: tokens.standard, fill: 'forwards' }));
    }
    state.runs.push(going);
    after(going, ms, () => {
      if (page.classList.contains('on')) return;
      page.classList.remove('leaving');
      state.runs.forEach(run => run.cancel());
      state.runs = [];
    });
  }
  function watchGrow(page) {
    if (grows.has(page)) return;
    const state = { open: page.classList.contains('on'), runs: [] };
    state.observer = new MutationObserver(() => {
      const open = page.classList.contains('on');
      if (open === state.open) return;
      state.open = open;
      if (open) growOpened(page, state); else growClosed(page, state);
    });
    state.observer.observe(page, { attributes: true, attributeFilter: ['class'] });
    grows.set(page, state);
  }
  function forgetGrow(page) {
    const state = grows.get(page);
    if (!state) return;
    state.observer.disconnect();
    grows.delete(page);
  }

  // ---- Android's Back (section 8.2) ----------------------------------------
  // The activity asks the page first. A panel or sheet that is open closes;
  // otherwise the last element in the page with `data-dui-back` that is
  // drawn is told, and closes whatever it is. With none, the page has
  // nothing to close and says so: the activity then does what Back does.
  function back() {
    const top = openModals().pop();
    if (top) { tell(top, 'dismiss'); return true; }
    const takers = $$('[data-dui-back]').filter(element => element.dataset.duiId && shown(element) && !element.closest('[inert]'));
    const taker = takers.pop();
    if (!taker) return false;
    hideTip();
    tell(taker, 'back');
    return true;
  }

  // ---- Panels and sheets (sections 5.5, 6.9, 12) -------------------------
  const FOCUSABLE = 'button:not([disabled]),[href],input:not([disabled]),select,textarea,[tabindex]:not([tabindex="-1"])';
  const focusables = within => $$(FOCUSABLE, within).filter(element => shown(element) && !element.closest('[inert]'));
  const modals = new Map();
  const openModals = () => [...modals.keys()].filter(modal => modal.classList.contains('on') && modal.isConnected);
  function centreOf(element) {
    const box = element.getBoundingClientRect();
    return [box.left + box.width / 2, box.top + box.height / 2];
  }
  function opened(modal, state) {
    const at = document.activeElement;
    state.opener = at && at !== document.body ? at : null;
    state.from = state.opener && shown(state.opener) ? centreOf(state.opener) : lastPress.slice();
    const preview = modal.dataset.duiModal === 'preview';
    setTimeout(() => {
      if (!modal.classList.contains('on')) return;
      // The preview takes the focus itself, so that Space closes it and
      // presses none of its buttons.
      const first = preview ? modal : focusables(modal)[0] || modal;
      first.focus({ preventScroll: true });
    }, 60);
    if (preview) { previewOpened(modal, state); return; }
    const scroller = modal.querySelector('[data-dui-scroll]');
    if (scroller && railFor(scroller)) setTimeout(() => followRail(scroller), 80);
    const rises = $$('[data-dui-rise]', modal);
    if (modal.dataset.duiModal === 'panel') {
      // It grows out of what opened it: from that point and 6% of its
      // size, its corners from round to 14 px (480 ms, snap).
      const snap = spring('snap');
      if (!reduced()) {
        const [cx, cy] = centreOf(modal);
        if (state.run) state.run.cancel();
        state.run = modal.animate([
          { transform: `translate(${state.from[0] - cx}px, ${state.from[1] - cy}px) scale(.06)`, borderRadius: '80px' },
          { transform: 'none', borderRadius: '14px' },
        ], { duration: 480, easing: snap.easing });
        rises.forEach((rise, index) => rise.animate([{ opacity: 0, transform: 'translateY(14px)' }, { opacity: 1, transform: 'none' }],
          { duration: snap.ms, easing: snap.easing, delay: 140 + index * 40, fill: 'backwards' }));
      }
    } else if (!reduced()) {
      const glide = spring('glide');
      rises.forEach((rise, index) => rise.animate([{ opacity: 0, transform: 'translateY(28px)' }, { opacity: 1, transform: 'none' }],
        { duration: glide.ms, easing: glide.easing, delay: 80 + index * 45, fill: 'backwards' }));
    }
  }
  function closed(modal, state) {
    // A sheet let go past the mark was held where the finger left it.
    modal.classList.remove('drag');
    modal.style.transform = '';
    if (modal.dataset.duiModal === 'preview') previewClosed(modal, state);
    else if (modal.dataset.duiModal === 'panel' && !reduced()) {
      // It shrinks back into what opened it (240 ms, exit).
      const from = state.opener && state.opener.isConnected && shown(state.opener) ? centreOf(state.opener) : state.from;
      const [cx, cy] = centreOf(modal);
      if (state.run) state.run.cancel();
      const run = modal.animate([{ transform: 'none' }, { transform: `translate(${from[0] - cx}px, ${from[1] - cy}px) scale(.08)` }],
        { duration: 240, easing: tokens.exit, fill: 'forwards' });
      state.run = run;
      run.finished.catch(() => {}).then(() => setTimeout(() => { if (state.run === run) run.cancel(); }, 80));
    }
    // Focus goes back to what opened it, unless the person has already
    // put it somewhere else.
    const opener = state.opener, at = document.activeElement;
    state.opener = null;
    if (opener && opener.isConnected && (!at || at === document.body || modal.contains(at))) {
      opener.focus({ preventScroll: true });
    }
  }
  function watchModal(modal) {
    if (modals.has(modal)) return;
    const state = { open: modal.classList.contains('on'), opener: null, from: lastPress.slice(), run: null };
    state.observer = new MutationObserver(() => {
      const open = modal.classList.contains('on');
      if (open === state.open) return;
      state.open = open;
      if (open) opened(modal, state); else closed(modal, state);
    });
    state.observer.observe(modal, { attributes: true, attributeFilter: ['class'] });
    if (modal.dataset.duiModal === 'preview') {
      const stage = stageOf(modal);
      state.items = new MutationObserver(() => previewItemChanged(modal, state));
      if (stage) state.items.observe(stage, { childList: true });
      state.item = modal.querySelector('.dui-pv-item');
    }
    modals.set(modal, state);
    if (state.open) opened(modal, state);
  }
  function forgetModal(modal) {
    const state = modals.get(modal);
    if (!state) return;
    state.observer.disconnect();
    if (state.items) state.items.disconnect();
    modals.delete(modal);
    if (state.open && state.opener && state.opener.isConnected) state.opener.focus({ preventScroll: true });
  }
  // Tab stays inside the panel or sheet on top; Esc closes it.
  document.addEventListener('keydown', event => {
    if (event.isComposing) return;
    const top = openModals().pop();
    if (!top) return;
    if (top.dataset.duiModal === 'preview' && previewKey(event, top)) return;
    if (event.key === 'Escape') {
      // A confirmation inside it takes Esc first.
      if (document.activeElement && document.activeElement.closest('.dui-confirm')) return;
      event.preventDefault();
      tell(top, 'dismiss');
      return;
    }
    if (event.key !== 'Tab') return;
    const all = focusables(top);
    if (!all.length) { event.preventDefault(); return; }
    const first = all[0], last = all[all.length - 1], at = document.activeElement;
    if (!top.contains(at)) { event.preventDefault(); first.focus(); }
    else if (event.shiftKey && at === first) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && at === last) { event.preventDefault(); first.focus(); }
  });
  // Dragging a sheet down by its top edge closes it.
  const SHEET_PAST = 110;
  let sheetDrag = null;
  document.addEventListener('pointerdown', event => {
    if (event.button || !event.target.closest) return;
    const grab = event.target.closest('[data-dui-sheet-grab]');
    if (!grab || event.target.closest('button')) return;
    const sheet = grab.closest('[data-dui-modal="sheet"]');
    if (!sheet || !sheet.classList.contains('on')) return;
    sheetDrag = { sheet, y: event.clientY, dy: 0, id: event.pointerId };
    sheet.classList.add('drag');
    try { grab.setPointerCapture(event.pointerId); } catch (error) { /* the pointer has gone */ }
  }, true);
  window.addEventListener('pointermove', event => {
    if (!sheetDrag || event.pointerId !== sheetDrag.id) return;
    sheetDrag.dy = Math.max(0, event.clientY - sheetDrag.y);
    stats.moves += 1;
    showStats();
    sheetDrag.sheet.style.transform = `translateY(${sheetDrag.dy}px)`;
  }, { passive: true });
  function endSheet(event) {
    if (!sheetDrag || (event && event.pointerId !== sheetDrag.id)) return;
    const { sheet, dy } = sheetDrag;
    sheetDrag = null;
    if (dy > SHEET_PAST && event.type !== 'pointercancel') {
      // Held where it was let go until Rust closes it, so that it goes on
      // down from there. If Rust does not, it comes back up.
      tell(sheet, 'dismiss');
      setTimeout(() => { sheet.classList.remove('drag'); sheet.style.transform = ''; }, 400);
    } else {
      sheet.classList.remove('drag');
      sheet.style.transform = '';
    }
  }
  window.addEventListener('pointerup', endSheet);
  window.addEventListener('pointercancel', endSheet);

  // ---- Settings: the rail, a section brought into sight, the code -------
  // The rail beside the page follows the scroll: its highlight is on the
  // section at the top, or the last one once the page is at its end. A
  // press on the rail, or Rust asking (`DirtUi.jump`), brings a section
  // into sight.
  const sectionsOf = scroller => $$('[data-dui-section]', scroller);
  // Where a section starts in the page it scrolls in. Measured by offsets,
  // not by boxes on screen: as Settings opens it grows from a point, and
  // its boxes are a small fraction of their size.
  function topIn(element, scroller) {
    let top = 0;
    for (let at = element; at && at !== scroller; at = at.offsetParent) top += at.offsetTop;
    return top;
  }
  function railFor(scroller) {
    const modal = scroller.closest('[data-dui-modal]');
    return modal ? modal.querySelector('[data-dui-rail]') : null;
  }
  function markRail(scroller, name) {
    const rail = railFor(scroller);
    if (!rail) return;
    const buttons = $$('[data-dui-jump]', rail);
    const index = Math.max(0, buttons.findIndex(button => button.dataset.duiJump === name));
    buttons.forEach((button, at) => button.setAttribute('aria-current', at === index ? 'true' : 'false'));
    const bar = rail.querySelector('.dui-rail-b');
    if (bar) bar.style.setProperty('--i', index);
  }
  function followRail(scroller) {
    const sections = sectionsOf(scroller);
    if (!sections.length) return;
    let current = sections[0];
    sections.forEach(section => { if (topIn(section, scroller) - scroller.scrollTop - 40 <= 0) current = section; });
    if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4) current = sections[sections.length - 1];
    markRail(scroller, current.dataset.duiSection);
  }
  document.addEventListener('scroll', event => {
    const scroller = event.target;
    if (scroller && scroller.matches && scroller.matches('[data-dui-scroll]') && railFor(scroller)) followRail(scroller);
  }, true);
  // Asked as Settings opens, it may come before the panel or sheet is
  // open; it waits for it, a little.
  function jump(name, instant, tries = 20) {
    const section = $$(`[data-dui-section="${name}"]`).filter(element => !element.closest('[inert]')).pop();
    if (!section) {
      if (tries > 0) setTimeout(() => jump(name, instant, tries - 1), 25);
      return;
    }
    const scroller = section.closest('[data-dui-scroll]');
    if (!scroller) return;
    const top = topIn(section, scroller) - 8;
    scroller.scrollTo({ top, behavior: instant || reduced() ? 'auto' : 'smooth' });
    markRail(scroller, name);
  }
  // "They match": the seven pictures gather to the middle, from the
  // outside in, shrinking (300 ms, exit), and the tick takes their place.
  // Rust answers the exchange as they arrive.
  document.addEventListener('click', event => {
    const button = event.target.closest && event.target.closest('[data-dui-gather]');
    if (!button || reduced()) return;
    const pair = button.closest('.dui-pair');
    const items = pair ? $$('.dui-sas-i', pair) : [];
    if (!items.length) return;
    const [cx, cy] = centreOf(items[0].parentElement);
    const middle = (items.length - 1) / 2;
    items.forEach((item, index) => {
      const [x, y] = centreOf(item);
      item.animate([{ transform: 'none', opacity: 1 }, { transform: `translate(${cx - x}px, ${cy - y}px) scale(.3)`, opacity: 0 }],
        { duration: 300, delay: Math.abs(index - middle) * 28, easing: tokens.exit, fill: 'forwards' });
    });
  }, true);

  // ---- Confirmation in place (sections 6.12, 12) -------------------------
  // The row that asks takes focus as it appears (on Cancel, the safe
  // answer), keeps Tab, takes Esc as Cancel, and gives focus back to the
  // row it replaced.
  function confirmShown(confirm) {
    const cancel = confirm.querySelector('[data-dui-cancel]') || focusables(confirm)[0];
    if (cancel) cancel.focus({ preventScroll: true });
  }
  function confirmGone(host) {
    if (!host.isConnected) return;
    const at = document.activeElement;
    if (at && at !== document.body && at.isConnected) return;
    const back = focusables(host)[0];
    if (back) back.focus({ preventScroll: true });
  }
  document.addEventListener('keydown', event => {
    if (event.isComposing || !document.activeElement) return;
    const confirm = document.activeElement.closest('.dui-confirm');
    if (!confirm) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      const cancel = confirm.querySelector('[data-dui-cancel]');
      if (cancel) cancel.click();
    } else if (event.key === 'Tab') {
      const all = focusables(confirm);
      if (!all.length) return;
      const first = all[0], last = all[all.length - 1], at = document.activeElement;
      if (event.shiftKey && at === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && at === last) { event.preventDefault(); first.focus(); }
    }
  }, true);

  // ---- Toasts (sections 5.5, 6.7, 12) ------------------------------------
  // Rust renders the toast; the script opens it, drains its bar, waits
  // while the pointer or the keyboard is on it, closes it, and then tells
  // Rust it has gone. The bar is the timer.
  const PILL = 'inset(0 calc(50% - 22px) round 22px)', OPEN = 'inset(0 0 round 12px)';
  function closeToast(toast, then) {
    if (toast._duiClosing) return;
    toast._duiClosing = true;
    const ms = reduced() ? tokens.reducedMs : 300;
    const going = reduced()
      ? toast.animate([{ opacity: 1 }, { opacity: 0 }], { duration: ms, fill: 'forwards' })
      // It closes back to a pill and drops away (300 ms, exit).
      : toast.animate([
        { clipPath: OPEN, opacity: 1, transform: 'none' },
        { clipPath: PILL, opacity: 1, transform: 'none', offset: 0.55 },
        { clipPath: PILL, opacity: 0, transform: 'translateY(12px) scale(.9)' },
      ], { duration: ms, easing: tokens.exit, fill: 'forwards' });
    after(going, ms, then);
  }
  function toastShown(toast) {
    if (reduced()) toast.animate([{ opacity: 0 }, { opacity: 1 }], { duration: tokens.reducedMs });
    else {
      // A 44 px pill rises 16 px (enter), then opens sideways into the
      // message (snap); the words come in after it, 50 ms apart.
      const snap = spring('snap');
      toast.animate([
        { clipPath: PILL, transform: 'translateY(16px)', opacity: 0, easing: tokens.enter },
        { clipPath: PILL, transform: 'none', opacity: 1, offset: 0.34, easing: snap.easing },
        { clipPath: OPEN, transform: 'none', opacity: 1 },
      ], { duration: 560 });
      $$('.dui-toast-words, .dui-toast-act, kbd', toast).forEach((part, index) => part.animate(
        [{ opacity: 0, transform: 'translateX(-4px)' }, { opacity: 1, transform: 'none' }],
        { duration: 220, delay: 230 + index * 50, easing: tokens.enter, fill: 'backwards' }));
    }
    const bar = toast.querySelector('.dui-toast-bar');
    const ms = Number(toast.dataset.ms) || 0;
    if (!bar || !ms) return;
    const drain = bar.animate([{ width: '100%' }, { width: '0%' }], { duration: ms, easing: 'linear', fill: 'forwards' });
    toast._duiDrain = drain;
    // Held while a mouse or pen moves onto it and stays, or the keyboard is
    // in it (#312). A finger is no pointer that rests: Android keeps :hover
    // on what was last touched, and focus stays on a button tapped, so a
    // toast touched, or one whose Settings was tapped, waited for ever.
    let pointer = false;
    const keyboard = () => !!toast.querySelector(':focus-visible');
    const go = () => { if (!pointer && !keyboard()) drain.play(); };
    toast.addEventListener('pointermove', event => {
      if (event.pointerType === 'touch' || pointer) return;
      pointer = true;
      drain.pause();
    });
    toast.addEventListener('pointerleave', () => { pointer = false; go(); });
    toast.addEventListener('focusin', () => { if (keyboard()) drain.pause(); });
    toast.addEventListener('focusout', () => setTimeout(go, 0));
    // The window put away or left with the pointer still on the toast
    // tells it nothing: it lets go then.
    toast._duiLetGo = () => { pointer = false; go(); };
    drain.finished.catch(() => {}).then(() => {
      if (!toast.isConnected) return;
      closeToast(toast, () => { toast._duiTold = true; tell(toast, 'expired'); });
    });
  }
  // A toast Rust takes away while it shows (a new one came, or Undo was
  // pressed) closes as a copy of itself, so the new one opens as the old
  // one closes.
  function toastGone(toast, layer) {
    if (toast._duiDrain) toast._duiDrain.cancel();
    if (toast._duiTold || toast._duiClosing || !layer || !layer.isConnected) return;
    const ghost = toast.cloneNode(true);
    ghost.classList.add('dui-toast-ghost');
    ghost.removeAttribute('data-dui-id');
    ghost.setAttribute('aria-hidden', 'true');
    ghost.inert = true;
    $$('.dui-toast-bar', ghost).forEach(bar => bar.remove());
    layer.append(ghost);
    closeToast(ghost, () => ghost.remove());
  }
  const letGoOfToasts = () => $$('.dui-toast').forEach(toast => toast._duiLetGo && toast._duiLetGo());
  window.addEventListener('blur', letGoOfToasts);
  document.addEventListener('visibilitychange', letGoOfToasts);
  // Ctrl Z, outside a text, presses Undo on the toast that offers it.
  document.addEventListener('keydown', event => {
    if (event.isComposing || event.key.toLowerCase() !== 'z' || !(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey) return;
    const at = document.activeElement;
    if (at && (at.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(at.tagName))) return;
    const undo = document.querySelector('.dui-toast:not(.dui-toast-ghost) [data-dui-undo]');
    if (!undo) return;
    event.preventDefault();
    undo.click();
  });

  // ---- Segmented control: the thumb stretches as it sets off (5.5) -------
  document.addEventListener('click', event => {
    const segment = event.target.closest ? event.target.closest('.dui-seg button') : null;
    if (!segment || reduced()) return;
    const thumb = segment.parentElement.querySelector('.dui-seg-thumb');
    if (thumb) thumb.animate([{ scale: '1 1' }, { scale: '1.16 .9', offset: 0.35 }, { scale: '1 1' }], { duration: tokens.springs.snap.ms, easing: 'ease-out' });
  }, true);

  // ---- Slider: what it sets follows the thumb here, not through Rust -----
  document.addEventListener('input', event => {
    const slider = event.target;
    if (!slider.dataset || !slider.dataset.duiLive) return;
    stats.moves += 1;
    showStats();
    root.style.setProperty(slider.dataset.duiLive, slider.value + (slider.dataset.duiUnit || ''));
  });

  // ---- Scrollbars (section 6.15) -----------------------------------------
  // The content scrolls natively; only the bar is drawn. The bar is the
  // element after the scrolling one, which Rust renders.
  const scrolls = new Map();
  function watchScroll(area) {
    if (scrolls.has(area)) return;
    const bar = area.nextElementSibling;
    if (!bar || !bar.classList.contains('dui-sb')) return;
    const thumb = bar.firstElementChild;
    let hideTimer = 0, drag = null;
    function layout() {
      const view = area.clientHeight, all = area.scrollHeight;
      if (all <= view + 2 || !view) { bar.hidden = true; return; }
      bar.hidden = false;
      const track = view - 8;
      const length = Math.max(touchUi() ? 36 : 28, track * view / all);
      thumb.style.height = length + 'px';
      thumb.style.transform = `translateY(${(track - length) * area.scrollTop / (all - view)}px)`;
    }
    function show() {
      layout();
      bar.classList.add('on');
      clearTimeout(hideTimer);
      // It goes 0.9 s after the last scroll, unless the pointer is on it.
      hideTimer = setTimeout(() => { if (!drag && !bar.matches(':hover')) bar.classList.remove('on'); }, 900);
    }
    area.addEventListener('scroll', show, { passive: true });
    // Coming within 28 px of the edge shows it, so that it can be taken.
    area.addEventListener('pointermove', event => {
      if (event.pointerType === 'touch') return;
      if (area.getBoundingClientRect().right - event.clientX < 28) show();
    }, { passive: true });
    bar.addEventListener('pointerleave', () => { if (!drag) show(); });
    thumb.addEventListener('pointerdown', event => {
      if (event.pointerType === 'touch') return;
      event.preventDefault();
      event.stopPropagation();
      thumb.setPointerCapture(event.pointerId);
      drag = { y: event.clientY, top: area.scrollTop };
      bar.classList.add('drag');
    });
    thumb.addEventListener('pointermove', event => {
      if (!drag) return;
      const view = area.clientHeight, all = area.scrollHeight, room = view - 8 - thumb.offsetHeight;
      area.scrollTop = drag.top + (event.clientY - drag.y) * (all - view) / Math.max(1, room);
    });
    const endDrag = () => { if (!drag) return; drag = null; bar.classList.remove('drag'); show(); };
    thumb.addEventListener('pointerup', endDrag);
    thumb.addEventListener('pointercancel', endDrag);
    // A click on the track moves 90% of a page towards it.
    bar.addEventListener('pointerdown', event => {
      if (event.target !== bar || event.pointerType === 'touch') return;
      const box = thumb.getBoundingClientRect();
      area.scrollBy({ top: (event.clientY < box.top ? -1 : 1) * area.clientHeight * 0.9, behavior: reduced() ? 'auto' : 'smooth' });
    });
    const resize = new ResizeObserver(layout);
    resize.observe(area);
    const change = new MutationObserver(layout);
    change.observe(area, { childList: true, subtree: true, characterData: true });
    if (area.tagName === 'TEXTAREA') area.addEventListener('input', layout);
    scrolls.set(area, { resize, change });
    layout();
  }
  function forgetScroll(area) {
    const state = scrolls.get(area);
    if (!state) return;
    state.resize.disconnect();
    state.change.disconnect();
    scrolls.delete(area);
  }

  // ---- Text fields the app does not hold (dirt_ui::dom) -----------------
  // A field of the new interface keeps its own text: the app hears every
  // change and never writes the text back while the person types. These
  // are what the app asks for when it must touch a field itself.
  const isField = element => !!element && (element.tagName === 'TEXTAREA'
    || (element.tagName === 'INPUT' && /^(text|search|url|email)$/.test(element.type)));
  function placeCaret(element, caret) {
    if (!isField(element)) return;
    const end = element.value.length;
    if (caret === 'end') element.setSelectionRange(end, end);
    else if (caret === 'all') element.select();
  }
  // The first of `selectors` that is drawn gets the keyboard's focus.
  function focus(selectors, caret) {
    for (const selector of selectors) {
      const element = $$(selector).find(shown);
      if (!element) continue;
      element.focus({ preventScroll: caret === 'kept' });
      placeCaret(element, caret);
      if (caret === 'end' && element.tagName === 'TEXTAREA') element.scrollTop = element.scrollHeight;
      return true;
    }
    return false;
  }
  // Rust's changes to a field that held `expected` ([start, end, text],
  // first to last), made to what it holds now: typed meanwhile, it differs
  // in one stretch, and a change that falls there is left for Rust to try
  // again. The caret keeps its place in the text around the changes.
  function mergeInto(selector, expected, changes) {
    const element = document.querySelector(selector);
    if (!element) return null;
    const actual = element.value;
    let same = 0;
    while (same < expected.length && same < actual.length && expected[same] === actual[same]) same += 1;
    let tail = 0;
    while (tail < expected.length - same && tail < actual.length - same
      && expected[expected.length - 1 - tail] === actual[actual.length - 1 - tail]) tail += 1;
    const typedEnd = expected.length - tail, shift = actual.length - expected.length;
    const focused = document.activeElement === element;
    let caret = focused ? [element.selectionStart, element.selectionEnd] : null;
    let value = actual, all = true;
    for (const [start, end, text] of changes.slice().reverse()) {
      let from, to;
      if (end <= same && !(start === same && same < typedEnd)) { from = start; to = end; }
      else if (start >= typedEnd) { from = start + shift; to = end + shift; }
      else { all = false; continue; }
      value = value.slice(0, from) + text + value.slice(to);
      if (caret) caret = caret.map(at => (at >= to ? at + text.length - (to - from) : at > from ? from + text.length : at));
    }
    element.value = value;
    if (caret) element.setSelectionRange(caret[0], caret[1]);
    tellText(element, false, false);
    return [value, all];
  }
  function setValue(selector, text) {
    const element = document.querySelector(selector);
    if (!element || element.value === text) return;
    const keep = document.activeElement === element ? [element.selectionStart, element.selectionEnd] : null;
    element.value = text;
    if (keep) element.setSelectionRange(Math.min(keep[0], text.length), Math.min(keep[1], text.length));
    tellText(element, false, false);
  }

  // ---- A note's text, told in order however long (dirt_ui::typing) --------
  // A note's field (`data-dui-typed`) has no `oninput` of Dioxus's: an
  // event goes to Rust with all it carries in one request header, which
  // fails past a few hundred kilobytes. What changed is told instead,
  // through the hidden carrier beside it (`data-dui-carrier`), whose
  // `input` Dioxus delivers in order with the keys and the presses: where,
  // how many units went, and what came, a long paste in pieces. A change
  // the app made itself (`setValue`, `mergeInto`, the page emptied by Esc)
  // is told too, so Rust's copy stays the field's, but not as typed.
  // A piece is kept under 16 KiB of JSON as UTF-8. Dioxus base64s each
  // event with `String.fromCharCode.apply`, one argument per byte of the
  // event's JSON, in which the piece's is escaped once more, and
  // JavaScriptCore (macOS, Linux) takes at most 65,536 arguments. So it is
  // the bytes that are counted, not the characters: a run of quotes,
  // backslashes or control characters takes two to six times the room.
  const PIECE_BYTES = 16 * 1024, PIECE_UNITS = 8192;
  const encoder = new TextEncoder();
  const isHigh = unit => unit >= 0xD800 && unit <= 0xDBFF;
  const isLow = unit => unit >= 0xDC00 && unit <= 0xDFFF;
  // Half of a pair with no other half becomes U+FFFD, one unit for one, as
  // the old way through `TextEncoder` made it: JSON writes it as an escape
  // that Rust cannot read.
  const wellFormed = text => (typeof text.toWellFormed === 'function'
    ? text.toWellFormed()
    : text.replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, '�'));
  function tellText(field, typed, whole) {
    if (!field || !field.dataset || field.dataset.duiTyped === undefined) return;
    const carrier = document.querySelector('[data-dui-carrier="' + field.dataset.duiTyped + '"]');
    if (!carrier) return;
    const now = wellFormed(field.value);
    const fresh = whole || typeof field.duiBase !== 'string';
    const base = fresh ? '' : field.duiBase;
    if (!fresh && base === now) return;
    const most = Math.min(base.length, now.length);
    let start = 0;
    while (start < most && base.charCodeAt(start) === now.charCodeAt(start)) start += 1;
    let end = 0;
    while (end < most - start
      && base.charCodeAt(base.length - 1 - end) === now.charCodeAt(now.length - 1 - end)) end += 1;
    // A character written as two units is never cut in two.
    if (start > 0 && isHigh(now.charCodeAt(start - 1))) start -= 1;
    if (end > 0 && isLow(now.charCodeAt(now.length - end))) end -= 1;
    const came = now.slice(start, now.length - end);
    let at = start, gone = base.length - start - end, from = 0, first = true;
    do {
      let units = Math.min(came.length - from, PIECE_UNITS), to, json;
      for (;;) {
        to = from + units;
        if (to < came.length && isHigh(came.charCodeAt(to - 1))) to += to - 1 > from ? -1 : 1;
        json = JSON.stringify({ r: fresh && first, a: at, d: gone, i: came.slice(from, to), m: to < came.length, t: typed, n: now.length });
        if (units <= 1 || encoder.encode(json).length <= PIECE_BYTES) break;
        units >>= 1;
      }
      carrier.value = json;
      carrier.dispatchEvent(new Event('input', { bubbles: true }));
      at += to - from;
      gone = 0;
      from = to;
      first = false;
    } while (from < came.length);
    carrier.value = '';
    field.duiBase = now;
  }
  // Dioxus listens for each kind of event once, on the app's root, and an
  // `input` or `change` that reaches it from any text field is sent with
  // the field's whole text, whoever listens: the note's field's own are
  // stopped at the field, after its own listeners have heard them.
  const stopHere = event => event.stopPropagation();
  function keepToItself(field) {
    if (field.duiKept) return;
    field.duiKept = true;
    field.addEventListener('input', stopHere);
    field.addEventListener('change', stopHere);
  }
  document.addEventListener('focusin', event => {
    const field = event.target;
    if (field && field.dataset && field.dataset.duiTyped !== undefined) keepToItself(field);
  }, true);
  document.addEventListener('input', event => {
    const field = event.target;
    if (!field || !field.dataset || field.dataset.duiTyped === undefined) return;
    keepToItself(field);
    field.duiAgain = 0;
    tellText(field, true, false);
  }, true);
  // Rust's copy did not agree with the field: the whole text, as typed. At
  // most three times for each change the person makes, so that a text the
  // road cannot carry is never sent round and round.
  function typedAgain(id) {
    const field = document.querySelector('[data-dui-typed="' + id + '"]');
    if (!field || (field.duiAgain || 0) >= 3) return;
    field.duiAgain = (field.duiAgain || 0) + 1;
    tellText(field, true, true);
  }
  // Text added at the end of a field, where the caret is not: the tags a
  // new page gets. The field then says so as if it had been typed, so the
  // app hears the whole text, with whatever was typed meanwhile.
  function appendText(selector, text) {
    const element = document.querySelector(selector);
    if (!element || !text) return;
    const keep = [element.selectionStart, element.selectionEnd];
    element.value += text;
    element.setSelectionRange(keep[0], keep[1]);
    element.dispatchEvent(new Event('input', { bubbles: true }));
  }
  // What goes into a field as if it had been typed there: kept by the
  // field's own undo, and heard by the app.
  function insertText(element, text) {
    if (element.dataset.duiOneline !== undefined) text = text.replace(/\s*[\r\n]+\s*/g, ' ');
    element.focus({ preventScroll: true });
    let done = false;
    try { done = document.execCommand('insertText', false, text); } catch (error) { done = false; }
    if (!done) {
      element.setRangeText(text, element.selectionStart, element.selectionEnd, 'end');
      element.dispatchEvent(new Event('input', { bubbles: true }));
    }
  }
  // Find never grows: a line break pasted into it becomes a space.
  document.addEventListener('paste', event => {
    const field = event.target;
    if (!field || !field.dataset || field.dataset.duiOneline === undefined || !event.clipboardData) return;
    const text = event.clipboardData.getData('text/plain');
    if (!/[\r\n]/.test(text)) return;
    event.preventDefault();
    insertText(field, text);
  }, true);

  // ---- Where the keyboard goes when nothing has it -----------------------
  // A button that is pressed and then taken away (a row archived, a view
  // closed) leaves the focus on the page's body, where a screen's keys are
  // not heard. An element with `data-dui-home` takes it instead. While a
  // panel or sheet is open the focus stays in it: a confirmation that has
  // gone gives it back to its row first, and otherwise the panel takes it.
  const lost = () => {
    const at = document.activeElement;
    return !at || at === document.body || at === root;
  };
  document.addEventListener('focusout', () => {
    setTimeout(() => {
      if (!lost()) return;
      const top = openModals().pop();
      if (top) {
        setTimeout(() => { if (lost()) (focusables(top)[0] || top).focus({ preventScroll: true }); }, 0);
        return;
      }
      const home = document.querySelector('[data-dui-home]');
      if (home && shown(home)) home.focus({ preventScroll: true });
    }, 0);
  }, true);

  // ---- Arrow keys through a group (sections 6.3, 7.4, 12) ------------------
  // Inside an element with `data-dui-arrows`, Up and Down move between its
  // buttons. Up from the first goes to the element the attribute names,
  // if it names one.
  document.addEventListener('keydown', event => {
    if (event.isComposing || event.ctrlKey || event.altKey || event.metaKey) return;
    if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return;
    const at = document.activeElement;
    const group = at && at.closest ? at.closest('[data-dui-arrows]') : null;
    if (!group || isField(at)) return;
    const stops = $$('button:not([disabled])', group).filter(shown);
    if (!stops.length) return;
    const index = stops.indexOf(at);
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Home') return stops[0].focus();
    if (event.key === 'End') return stops[stops.length - 1].focus();
    if (event.key === 'ArrowDown') return stops[Math.min(stops.length - 1, index + 1)].focus();
    if (index > 0) return stops[index - 1].focus();
    const above = group.dataset.duiArrows && document.querySelector(group.dataset.duiArrows);
    if (above) above.focus();
  }, true);

  // ---- The app's own menu (section 7.4) ------------------------------------
  // Rust renders one element `[data-dui-menu]` with Copy, Paste, Select
  // all and the screen's own entries. The script shows it on a right click
  // or the menu key, with what applies there.
  let menuFor = null;
  const menuOf = () => document.querySelector('[data-dui-menu]');
  function closeMenu(giveBack) {
    const menu = menuOf();
    if (!menu || !menu.classList.contains('on')) return;
    menu.classList.remove('on');
    const field = menuFor;
    if (giveBack && field && field.isConnected) field.focus({ preventScroll: true });
  }
  function openMenu(event) {
    const menu = menuOf();
    if (!menu || touchUi()) return;
    const target = event.target && event.target.closest ? event.target : document.activeElement;
    if (!target.closest('.dui-root') || target.closest('[data-dui-menu]')) return;
    const field = isField(target) ? target : null;
    const picked = field ? field.selectionEnd > field.selectionStart : String(getSelection() || '').length > 0;
    const offers = { copy: picked, paste: !!field && !field.readOnly, all: !!field };
    $$('[data-dui-do]', menu).forEach(item => { item.hidden = !offers[item.dataset.duiDo]; });
    const own = $$('[role="menuitem"]:not([data-dui-do])', menu);
    if (!own.length && !offers.copy && !offers.paste && !offers.all) return;
    event.preventDefault();
    menuFor = field || (document.activeElement !== document.body ? document.activeElement : null);
    menu.classList.add('on');
    // By the pointer; for the menu key, by the field.
    let [x, y] = [event.clientX, event.clientY];
    if (!x && !y) {
      const box = (field || target).getBoundingClientRect();
      [x, y] = [box.left + 16, box.top + 24];
    }
    const box = menu.getBoundingClientRect();
    x = Math.max(8, Math.min(innerWidth - box.width - 8, x));
    y = y + box.height + 8 > innerHeight ? Math.max(8, y - box.height) : y;
    Object.assign(menu.style, { left: x + 'px', top: y + 'px' });
    if (!reduced()) {
      const snap = spring('snap');
      menu.animate([{ opacity: 0, transform: 'scale(.96)' }, { opacity: 1, transform: 'none' }], { duration: snap.ms, easing: snap.easing });
    }
    const first = $$('[role="menuitem"]', menu).find(item => !item.hidden);
    if (first) first.focus({ preventScroll: true });
  }
  document.addEventListener('contextmenu', openMenu);
  document.addEventListener('pointerdown', event => {
    const menu = menuOf();
    if (menu && menu.classList.contains('on') && !menu.contains(event.target)) closeMenu(false);
  }, true);
  document.addEventListener('keydown', event => {
    const menu = menuOf();
    if (!menu || !menu.classList.contains('on')) return;
    if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault();
      event.stopPropagation();
      closeMenu(true);
    }
  }, true);
  window.addEventListener('blur', () => closeMenu(false));
  document.addEventListener('click', event => {
    const item = event.target.closest ? event.target.closest('[data-dui-menu] [role="menuitem"]') : null;
    if (!item) return;
    const menu = item.closest('[data-dui-menu]');
    const field = menuFor;
    closeMenu(true);
    const what = item.dataset.duiDo;
    if (!what) return;
    if (what === 'copy') {
      if (field) field.focus({ preventScroll: true });
      document.execCommand('copy');
    } else if (what === 'all') {
      if (field) { field.focus({ preventScroll: true }); field.select(); }
    } else if (what === 'paste') {
      tell(menu, 'paste');
    }
  });
  // The clipboard's text, read by the app, put where the caret was.
  function paste(text) {
    const field = menuFor && menuFor.isConnected && isField(menuFor) ? menuFor : null;
    if (field && text) insertText(field, text);
  }

  // ---- More rows as a list nears its end (section 3) -----------------------
  // A mark `[data-dui-more]` at the end of a list says once, as it comes
  // within a screen of being seen, that there is room for more.
  const mores = new Map();
  function watchMore(mark) {
    if (mores.has(mark) || !window.IntersectionObserver) return;
    const area = mark.closest('[data-dui-scroll]');
    const seen = new IntersectionObserver(entries => {
      if (!entries.some(entry => entry.isIntersecting)) return;
      seen.disconnect();
      mores.delete(mark);
      tell(mark, 'more');
    }, { root: area, rootMargin: '0px 0px 600px 0px' });
    seen.observe(mark);
    mores.set(mark, seen);
  }
  function forgetMore(mark) {
    const seen = mores.get(mark);
    if (seen) seen.disconnect();
    mores.delete(mark);
  }

  // ---- Controls shown by nearness (sections 6.4, 7.7) ---------------------
  // While a note is written, a control comes back only when the pointer
  // goes to where it lives. An element with `data-dui-near="64"` is marked
  // `data-near` while the pointer is within that many pixels of it; the
  // style sheet says what that shows. Only a pointer that moves counts: a
  // list sliding away under a still pointer brings nothing back. An
  // element that also has `data-dui-reach` tells Rust once when the
  // pointer comes onto it.
  //
  // A touch on a touch screen shows them all, until the next key.
  let pointerAt = null, nearFrame = 0, touchShown = false;
  const nears = () => $$('[data-dui-near]');
  function setNear(element, near) {
    if (near === (element.dataset.near !== undefined)) return;
    if (near) element.dataset.near = ''; else delete element.dataset.near;
  }
  function measureNear() {
    nearFrame = 0;
    if (!pointerAt) return;
    const [x, y] = pointerAt;
    nears().forEach(element => {
      if (!shown(element)) { setNear(element, false); element._duiReached = false; return; }
      const box = element.getBoundingClientRect();
      const dx = Math.max(box.left - x, 0, x - box.right), dy = Math.max(box.top - y, 0, y - box.bottom);
      setNear(element, Math.hypot(dx, dy) <= (Number(element.dataset.duiNear) || 0));
      if (element.dataset.duiReach === undefined) return;
      const on = dx === 0 && dy === 0;
      if (on && !element._duiReached) tell(element, 'reach');
      element._duiReached = on;
    });
  }
  function forgetNear() {
    pointerAt = null;
    nears().forEach(element => { setNear(element, false); element._duiReached = false; });
  }
  document.addEventListener('pointermove', event => {
    if (event.pointerType === 'touch') return;
    const was = pointerAt;
    pointerAt = [event.clientX, event.clientY];
    if (!was || (was[0] === pointerAt[0] && was[1] === pointerAt[1])) return;
    if (!nearFrame) nearFrame = requestAnimationFrame(measureNear);
  }, { passive: true });
  document.addEventListener('pointerleave', forgetNear);
  window.addEventListener('blur', forgetNear);
  document.addEventListener('pointerdown', event => {
    if (event.pointerType !== 'touch') return;
    touchShown = true;
    nears().forEach(element => setNear(element, true));
  }, true);
  document.addEventListener('keydown', () => {
    if (!touchShown) return;
    touchShown = false;
    nears().forEach(element => setNear(element, false));
  }, true);

  // ---- A press that leaves the cursor where it is (section 7.7) -----------
  // Inside `data-dui-keep-focus` (the rail's grains), a press does not take
  // the keyboard's focus from the note being written. The click still
  // happens.
  document.addEventListener('mousedown', event => {
    if (event.target.closest && event.target.closest('[data-dui-keep-focus]')) event.preventDefault();
  }, true);

  // ---- The page: done is at once (sections 6.1, 7.4) ----------------------
  // Esc, Ctrl Enter and Ctrl N on the page (`data-dui-page`) put the thought
  // on the pile and give a blank page. The attribute names the command key,
  // `ctrl` or `meta` (Cmd on a Mac). The rule is the app's own, key for key
  // (the desktop's `keys::empties_page`): Esc with no modifier, or the
  // command key with Enter or N and nothing else. Were the two to differ,
  // the next character would be saved over the note, or the old text would
  // become a second note. The field is emptied here, as the
  // key is pressed, before Rust has heard of it: whatever is typed next is
  // the next thought's, however quickly it comes. Rust keeps what the field
  // held, from what it had heard before the key.
  //
  // It listens before the page's own handlers: Dioxus marks a key the app
  // takes as handled before a listener after it would hear it. The app's
  // own menu takes Esc first while it is open; it is listened to before
  // this.
  document.addEventListener('keydown', event => {
    const field = event.target;
    if (event.defaultPrevented || event.isComposing || !field || !field.matches || !field.matches('[data-dui-page]')) return;
    const mac = field.dataset.duiPage === 'meta';
    const command = mac ? event.metaKey : event.ctrlKey;
    const other = mac ? event.ctrlKey : event.metaKey;
    if (event.altKey || event.shiftKey || other) return;
    const done = command
      ? event.key === 'Enter' || event.key.toLowerCase() === 'n'
      : event.key === 'Escape';
    if (!done) return;
    shootPage();
    field.value = '';
    field.setSelectionRange(0, 0);
    tellText(field, false, false);
  }, true);

  // ---- The window's top row (section 7.6) ---------------------------------
  // Where the app draws the window without the system's title bar, any
  // empty part of its top rows (`data-dui-drag`, the height in pixels)
  // moves it: a press there tells Rust, which hands the move to the system
  // until the button is let go. A double-click maximises or restores.
  const NOT_DRAG = 'button,a,input,textarea,select,label,[role="listbox"],[role="option"],[contenteditable],'
    + '[data-dui-modal],[data-dui-menu],.dui-find,.dui-taglist,.dui-toast,.dui-sb,[data-dui-resize],[data-dui-no-drag]';
  // An edge the app draws for resizing (`data-dui-resize`), where the web
  // view lies over the system's own border: a press there hands the
  // resizing to the system, from that edge.
  document.addEventListener('pointerdown', event => {
    if (event.button !== 0 || event.pointerType !== 'mouse' || !event.target.closest) return;
    const edge = event.target.closest('[data-dui-resize]');
    if (!edge) return;
    event.preventDefault();
    tell(edge, 'resize');
  }, true);
  let lastDragPress = null;
  document.addEventListener('pointerdown', event => {
    if (event.button !== 0 || event.pointerType !== 'mouse' || !event.target.closest) return;
    const area = event.target.closest('[data-dui-drag]');
    if (!area || event.target.closest(NOT_DRAG)) return;
    const box = area.getBoundingClientRect();
    if (event.clientY - box.top > (Number(area.dataset.duiDrag) || 0)) return;
    event.preventDefault();
    // A pointer's press carries no count of clicks, and the press that
    // would is not sent once this one is taken: a second press soon after
    // the first and in the same place is the double-click. The same place
    // on the screen: after a short drag the window has moved with the
    // pointer, and a quick second grab is in the same place in the page.
    const now = Date.now(), last = lastDragPress;
    const again = last && now - last.at < 500 && Math.abs(event.screenX - last.x) < 5 && Math.abs(event.screenY - last.y) < 5;
    lastDragPress = again ? null : { at: now, x: event.screenX, y: event.screenY };
    tell(area, again ? 'maximize' : 'drag');
  }, true);

  // ---- A new thought settles onto the pile (sections 5.5 and 6.17) --------
  // When a new thought is let go, Rust marks its row in the list
  // `data-dui-settle`, with the shade of its grain. The row is held out of
  // sight while the thought's words gather into the grain where they began
  // and the grain is tossed onto the row: up 46 px on the toss curve, then
  // down on gravity's, moving across all the while. Where it lands the row
  // appears, the rows below give 3 px one after another, and dust puffs
  // out. A sync that redraws the list meanwhile does not show the row
  // early: what holds it is an animation, which a redraw leaves alone.
  //
  // Where the words were is taken as the key or press that lets them go
  // comes, before Rust has heard of it (the desktop's page,
  // `data-dui-page`), or from where an Android page closes to its grain
  // (`data-dui-from-kind="grain"`). Without either, or with reduced
  // motion, the row only fades in.
  const TOSS = 'cubic-bezier(.25,.7,.5,1)', GRAVITY = 'cubic-bezier(.55,0,.9,.45)';
  let pageShot = null, gathering = null;
  function shootPage() {
    const field = document.querySelector('[data-dui-page]');
    if (!field || !field.value.trim() || !shown(field)) return;
    const box = field.getBoundingClientRect(), style = getComputedStyle(field);
    const size = parseFloat(style.fontSize) || 15, line = parseFloat(style.lineHeight) || size * 1.65;
    const top = parseFloat(style.paddingTop) || 0, left = parseFloat(style.paddingLeft) || 0;
    pageShot = {
      at: performance.now(), text: field.value.slice(0, 600), size, line, font: style.fontFamily,
      left: box.left + left, top: box.top + top, width: Math.max(40, box.width - left * 2),
      height: Math.max(line, Math.min(box.height - top, line * 8)),
    };
  }
  document.addEventListener('keydown', event => {
    if (event.isComposing) return;
    const command = event.ctrlKey || event.metaKey;
    if (event.key === 'Escape' || (command && (event.key === 'Enter' || event.key.toLowerCase() === 'n'))) shootPage();
  }, true);
  document.addEventListener('pointerdown', shootPage, true);

  function dust(x, y, colour) {
    for (let index = 0; index < 7; index++) {
      const speck = document.createElement('i');
      speck.className = 'dui-dust';
      Object.assign(speck.style, { left: x + 'px', top: y + 'px', background: colour });
      document.body.append(speck);
      const angle = Math.PI * (1.08 + 0.84 * index / 6), far = 8 + Math.random() * 12;
      const ms = 420 + Math.random() * 180;
      after(speck.animate([{ transform: 'none', opacity: 0.9 },
        { transform: `translate(${Math.cos(angle) * far}px, ${Math.sin(angle) * far * 0.7 + 7}px) scale(.4)`, opacity: 0 }],
      { duration: ms, easing: 'cubic-bezier(.2,.6,.4,1)', fill: 'forwards' }), ms, () => speck.remove());
    }
  }
  // The newest grain of the pile drops 46 px onto the top of the heap,
  // when the heap is in sight. Nothing else moves.
  function pileGrows(list) {
    const top = list.querySelector('[data-dui-pile] .dui-pile-top');
    const svg = top && top.ownerSVGElement;
    if (!svg || !onScreen(svg)) return;
    const scroller = scrollerOf(list).getBoundingClientRect(), box = svg.getBoundingClientRect();
    if (box.bottom < scroller.top || box.top > scroller.bottom) return;
    // In the picture's own measure, which is finer than the page's.
    const fine = svg.viewBox.baseVal.width / (box.width || 1);
    top.animate([{ transform: `translateY(${-46 * fine}px)`, opacity: 0 }, { opacity: 1, offset: 0.3 }, { transform: 'none', opacity: 1 }],
      { duration: 640, easing: 'cubic-bezier(.55,0,1,.45)' });
  }
  function land(row, list, x, y, colour, hold) {
    hold.cancel();
    if (!row.isConnected) return;
    const snap = spring('snap');
    row.animate([{ opacity: 0, transform: 'scale(.96) translateY(-5px)', transformOrigin: '14px 16px' },
      { opacity: 1, transform: 'none', transformOrigin: '14px 16px' }], { duration: snap.ms, easing: snap.easing });
    // The rows below give a little under it, one after another.
    let below = row.nextElementSibling;
    for (let index = 0; below && index < 7; below = below.nextElementSibling) {
      if (below.dataset.key === undefined || below.classList.contains('dui-ghost')) continue;
      below.animate([{ transform: 'none' }, { transform: 'translateY(3px)', offset: 0.3 }, { transform: 'none' }],
        { duration: 420, delay: 30 + index * 24, easing: 'ease-out' });
      index += 1;
    }
    dust(x, y, colour);
    pileGrows(list);
  }
  // The row is held out of sight at once, and what to play is decided a
  // frame later: the change that marks the row may also be the one that
  // closes an Android page to its grain, whose watcher can hear of it
  // after the list's does.
  function settle(row, list) {
    const hold = row.animate([{ opacity: 0 }, { opacity: 0 }], { duration: 3000, fill: 'forwards' });
    requestAnimationFrame(() => toss(row, list, hold));
    // A page that is not being drawn runs no frame: the row comes back.
    setTimeout(() => { if (hold.playState !== 'finished' && !row._duiTossed) hold.cancel(); }, 1000);
  }
  function toss(row, list, hold) {
    const now = performance.now();
    const from = gathering && now - gathering.at < 1200 ? gathering
      : pageShot && now - pageShot.at < 1500 ? pageShot : null;
    gathering = null;
    pageShot = null;
    if (!from || reduced() || document.hidden || !shown(row)) {
      hold.cancel();
      row.animate([{ opacity: 0 }, { opacity: 1 }], { duration: reduced() ? tokens.reducedMs : 200, easing: tokens.standard });
      return;
    }
    row._duiTossed = true;
    const colour = row.dataset.duiGrain || 'hsl(32 46% 60%)';
    // The row it lands on is brought into sight first.
    const scroller = scrollerOf(list);
    const view = scroller.getBoundingClientRect(), box = row.getBoundingClientRect();
    if (box.top < view.top) scroller.scrollTop -= view.top - box.top + 8;
    else if (box.bottom > view.bottom) scroller.scrollTop += box.bottom - view.bottom + 8;
    let x, y, wait;
    if (from.text !== undefined) {
      // The words shrink toward where they began, fading as they go.
      const ghost = document.createElement('div');
      ghost.className = 'dui-settle-ghost';
      ghost.textContent = from.text;
      Object.assign(ghost.style, {
        left: from.left + 'px', top: from.top + 'px', width: from.width + 'px', height: from.height + 'px',
        fontSize: from.size + 'px', lineHeight: from.line + 'px', fontFamily: from.font,
        transformOrigin: `0px ${from.line / 2}px`,
      });
      document.body.append(ghost);
      after(ghost.animate([{ transform: 'none', opacity: 1 }, { opacity: 0.8, offset: 0.5 }, { transform: 'scale(.04)', opacity: 0 }],
        { duration: 280, easing: 'cubic-bezier(.5,0,.75,.3)', fill: 'forwards' }), 280, () => ghost.remove());
      x = from.left + 4;
      y = from.top + from.line / 2;
      wait = 170;
    } else {
      // An Android page closing to its grain: the grain leaves as the
      // circle is nearly shut.
      x = from.x;
      y = from.y;
      wait = Math.max(0, 250 - (now - from.at));
    }
    const grain = document.createElement('i');
    grain.className = 'dui-grain';
    Object.assign(grain.style, { left: x + 'px', top: y + 'px', background: colour });
    document.body.append(grain);
    // The grain pops out where the words began (150 ms).
    grain.animate([{ transform: 'scale(0)' }, { transform: 'scale(1.2)', offset: 0.7 }, { transform: 'scale(1)' }],
      { duration: 150, delay: wait, easing: 'ease-out', fill: 'backwards' });
    setTimeout(() => {
      // Where the row is now: the list may have moved meanwhile.
      const target = row.isConnected ? row.getBoundingClientRect() : box;
      const tx = target.left + 14, ty = target.top + 16;
      const dx = tx - x, dy = ty - y, peak = Math.min(0, dy) - 46;
      const ms = Math.round(Math.max(480, Math.min(640, 440 + Math.hypot(dx, dy) * 0.25)));
      grain.animate([{ translate: '0 0' }, { translate: `${dx}px 0` }], { duration: ms, easing: 'cubic-bezier(.3,.4,.6,1)', fill: 'both' });
      const flight = grain.animate([
        { transform: 'translateY(0) scale(1)', easing: TOSS },
        { transform: `translateY(${peak}px) scale(.9)`, offset: 0.42, easing: GRAVITY },
        { transform: `translateY(${dy}px) scale(.75)` },
      ], { duration: ms, fill: 'forwards' });
      after(flight, ms, () => {
        grain.remove();
        land(row, list, tx, ty, colour, hold);
      });
    }, wait + 150);
  }
  // An archived note sinks into the pile: its grain appears at the row's
  // left edge and falls on gravity's curve, fading (720 ms).
  function sink(list, was) {
    const colour = was.row.dataset.duiGrain;
    if (!colour) return;
    const box = list.getBoundingClientRect();
    const grain = document.createElement('i');
    grain.className = 'dui-grain';
    Object.assign(grain.style, {
      left: box.left + was.x + 22 + 'px',
      top: box.top + was.y + was.h / 2 + 'px',
      background: colour,
    });
    document.body.append(grain);
    after(grain.animate([{ transform: 'scale(0)', opacity: 0 }, { transform: 'scale(1.2)', opacity: 1, offset: 0.25 },
      { transform: 'translateY(0) scale(1)', opacity: 1, offset: 0.4 }, { transform: 'translateY(140px) scale(.7)', opacity: 0 }],
    { duration: 720, easing: GRAVITY, fill: 'forwards' }), 720, () => grain.remove());
  }
  // Rows of a list marked to settle that have not yet.
  function settleRows(list) {
    rowsOf(list).forEach(row => {
      if (row.dataset.duiSettle === undefined || row._duiSettled) return;
      row._duiSettled = true;
      settle(row, list);
    });
  }

  // ---- Focus while writing (section 7.7) ----------------------------------
  // Rust says on the screen's root whether a note is being written
  // (`data-dui-writing`: `hush` at the first key, `on` once focus has come
  // in) and in which style (`data-dui-focus`). The style sheet lays each
  // out; what depends on where the rows are is played here. Rail: the rows
  // fold to the left one after another, then the rail's grains pop in top
  // to bottom. Leaving any style: the rows come back in a cascade from the
  // left. Nearness starts again with each change: only a pointer that
  // moves after it counts.
  const writers = new Map();
  function rowsInSight(scope) {
    const list = scope.querySelector('[data-dui-list]');
    if (!list) return [];
    const view = list.getBoundingClientRect();
    return rowsOf(list).filter(row => {
      const box = row.getBoundingClientRect();
      return box.height && box.bottom > view.top && box.top < view.bottom;
    }).slice(0, 12);
  }
  function writingChanged(scope, was, now) {
    forgetNear();
    if (reduced() || was === now) return;
    if (now === 'on' && scope.dataset.duiFocus === 'rail') {
      const rows = rowsInSight(scope);
      rows.forEach((row, index) => row.animate(
        [{ transform: 'none', opacity: 1, transformOrigin: 'left center' },
          { transform: 'translateX(-24px) scaleX(.4)', opacity: 0, transformOrigin: 'left center' }],
        { duration: 180, delay: index * 18, easing: tokens.exit, fill: 'forwards' }));
      setTimeout(() => rows.forEach(row => row.getAnimations().forEach(run => run.cancel())), 760);
      const pop = spring('pop');
      const rail = scope.querySelector('[data-dui-rail]');
      const box = rail ? rail.getBoundingClientRect() : null;
      if (rail) {
        $$('button, [data-dui-stop]', rail)
          .filter(stop => stop.getBoundingClientRect().top < box.bottom)
          .slice(0, 40)
          .forEach((stop, index) => stop.animate([{ transform: 'scale(0)', opacity: 0 }, { transform: 'none', opacity: 1 }],
            { duration: pop.ms, easing: pop.easing, delay: 280 + index * 24, fill: 'backwards' }));
      }
    } else if (was === 'on' && now !== 'on') {
      const snap = spring('snap');
      // A new thought's row waits for its grain.
      rowsInSight(scope).filter(row => row.dataset.duiSettle === undefined).forEach((row, index) => row.animate(
        [{ transform: 'translateX(-14px)', opacity: 0 }, { transform: 'none', opacity: 1 }],
        { duration: snap.ms, easing: snap.easing, delay: 230 + index * 22, fill: 'backwards' }));
    }
  }
  function watchWriting(scope) {
    if (writers.has(scope)) return;
    const state = { now: scope.dataset.duiWriting || '' };
    state.observer = new MutationObserver(() => {
      const now = scope.dataset.duiWriting || '';
      if (now === state.now) return;
      const was = state.now;
      state.now = now;
      writingChanged(scope, was, now);
    });
    state.observer.observe(scope, { attributes: true, attributeFilter: ['data-dui-writing'] });
    writers.set(scope, state);
  }
  function forgetWriting(scope) {
    const state = writers.get(scope);
    if (!state) return;
    state.observer.disconnect();
    writers.delete(scope);
  }

  // ---- A file's tile, and files arriving (sections 5.5, 6.5, 7.3) --------
  // An element `[data-dui-press]` that is not a button is pressed by Space
  // and Enter as one is: a file's tile.
  document.addEventListener('keydown', event => {
    if (event.isComposing || event.repeat || (event.key !== ' ' && event.key !== 'Enter')) return;
    const target = event.target;
    if (!target || !target.matches || !target.matches('[data-dui-press]')) return;
    event.preventDefault();
    target.click();
  });
  // Where files were last dropped: a new tile grows out of it. Windows'
  // web view gives a drop no place, so the element it was dropped on
  // stands for it.
  document.addEventListener('drop', event => {
    if (event.clientX || event.clientY) { lastPress = [event.clientX, event.clientY]; return; }
    const on = window.dxDragLastElement || event.target;
    if (on && on.getBoundingClientRect) lastPress = centreOf(on);
  }, true);
  // While files are dragged over the window, an element `[data-dui-drop]`
  // has the class `dropping`, which shows what a drop would do. Windows'
  // web view says only when the pointer moves onto another element, so a
  // leave waits a moment for the arrival that follows it.
  let dropLeft = 0;
  const dropTarget = () => document.querySelector('[data-dui-drop]');
  const dragsFiles = event => !event.dataTransfer || !event.dataTransfer.types
    || [...event.dataTransfer.types].includes('Files') || event.dataTransfer.types.length === 0;
  ['dragenter', 'dragover'].forEach(name => document.addEventListener(name, event => {
    const target = dropTarget();
    if (!target || !dragsFiles(event) || document.querySelector('[data-dui-modal].on')) return;
    clearTimeout(dropLeft);
    if (!target.classList.contains('dropping')) target.classList.add('dropping');
  }, true));
  document.addEventListener('dragleave', () => {
    clearTimeout(dropLeft);
    dropLeft = setTimeout(() => { const target = dropTarget(); if (target) target.classList.remove('dropping'); }, 90);
  }, true);
  document.addEventListener('drop', () => {
    clearTimeout(dropLeft);
    const target = dropTarget();
    if (target) target.classList.remove('dropping');
  }, true);

  // A picture pasted into the page: `onPastedFiles` hears the files of a
  // paste into the field `selector` names that has no text, and the paste
  // puts nothing in the field. Only what a note can take is sent over to
  // the app: at most 20 files (`limits::MAX_FILES_PER_NOTE`), each named
  // but without its bytes past 10 MiB (`limits::MAX_FILE_BYTES`), which
  // the app then says is too large.
  const PASTE_MOST = 10 * 1024 * 1024, PASTE_FILES = 20;
  let pastedTo = null;
  function onPastedFiles(selector, send) { pastedTo = { selector, send }; }
  const asBase64 = file => new Promise(resolve => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ''));
    reader.onerror = () => resolve(null);
    reader.readAsDataURL(file);
  });
  document.addEventListener('paste', event => {
    if (!pastedTo || !event.clipboardData || !event.target || !event.target.closest) return;
    if (!event.target.closest(pastedTo.selector)) return;
    const data = event.clipboardData;
    if (data.getData('text/plain')) return;
    const files = [...(data.files || [])].slice(0, PASTE_FILES);
    if (!files.length) return;
    event.preventDefault();
    const { send } = pastedTo;
    Promise.all(files.map(async file => ({
      name: file.name || 'image.png',
      type: file.type || '',
      size: file.size,
      data: file.size > PASTE_MOST ? null : await asBase64(file),
    }))).then(send);
  }, true);

  // ---- The file preview (section 6.16) ------------------------------------
  // `[data-dui-modal="preview"]` is a dialog like a panel, opened by its
  // class `on`. It grows out of the tile its `data-dui-from` names and
  // shrinks back into it; ← and → say `previous` and `next`; Space closes
  // it, as Esc does; a press on its backdrop closes it on the desktop and
  // hides or shows its controls on Android, where a swipe down closes it
  // and one sideways goes to the next file. A picture zooms to twice its
  // size on a double click or a pinch.
  let previewStep = 1;
  const stageOf = modal => modal.querySelector('[data-dui-pv-stage]');
  const tileOf = modal => {
    const tile = document.getElementById(modal.dataset.duiFrom || '');
    return tile && onScreen(tile) ? (tile.querySelector('.dui-file-thumb') || tile) : null;
  };
  // The box of what the stage shows, or a part of the stage's middle.
  function shownBox(stage) {
    const item = stage.querySelector('.dui-pv-item');
    const inner = item && item.firstElementChild;
    const box = inner && inner.getBoundingClientRect();
    if (box && box.width > 8 && box.height > 8) return box;
    const whole = stage.getBoundingClientRect();
    const w = whole.width * 0.6, h = whole.height * 0.6;
    return { left: whole.left + (whole.width - w) / 2, top: whole.top + (whole.height - h) / 2, width: w, height: h };
  }
  function towardsTile(stage, tile) {
    const a = tile.getBoundingClientRect(), b = shownBox(stage), whole = stage.getBoundingClientRect();
    const k = Math.max(0.04, Math.min(a.width / b.width, a.height / b.height));
    // The stage scales about its own middle; the box shown moves with it.
    const bx = b.left + b.width / 2 - (whole.left + whole.width / 2);
    const by = b.top + b.height / 2 - (whole.top + whole.height / 2);
    const dx = a.left + a.width / 2 - (whole.left + whole.width / 2) - bx * k;
    const dy = a.top + a.height / 2 - (whole.top + whole.height / 2) - by * k;
    return `translate(${dx}px, ${dy}px) scale(${k})`;
  }
  function previewOpened(modal, state) {
    modal.classList.remove('leaving', 'bare');
    previewStep = 1;
    state.item = null;
    const stage = stageOf(modal), tile = tileOf(modal);
    if (state.run) state.run.cancel();
    if (!stage) return;
    if (reduced() || !tile) {
      state.run = stage.animate([{ opacity: 0 }, { opacity: 1 }], { duration: reduced() ? tokens.reducedMs : 200, easing: tokens.standard });
      return;
    }
    // It grows out of its tile (420 ms, enter).
    state.run = stage.animate([{ transform: towardsTile(stage, tile), opacity: 0.5 }, { transform: 'none', opacity: 1 }],
      { duration: 420, easing: tokens.enter });
  }
  function previewClosed(modal, state) {
    const stage = stageOf(modal), tile = tileOf(modal);
    if (state.run) state.run.cancel();
    modal.classList.add('leaving');
    if (!stage) { modal.classList.remove('leaving'); return; }
    const finish = () => { if (!modal.classList.contains('on')) modal.classList.remove('leaving'); };
    let run, ms;
    if (reduced() || !tile) {
      ms = reduced() ? tokens.reducedMs : 160;
      run = stage.animate([{ opacity: 1 }, { opacity: 0 }], { duration: ms, easing: tokens.exit, fill: 'forwards' });
    } else {
      // It shrinks back into the tile (320 ms, standard).
      ms = 320;
      run = stage.animate([{ transform: 'none', opacity: 1 }, { transform: towardsTile(stage, tile), opacity: 0.4 }],
        { duration: ms, easing: tokens.standard, fill: 'forwards' });
    }
    state.run = run;
    after(run, ms, () => { finish(); if (state.run === run) run.cancel(); });
  }
  // A new file on the stage comes in from the side it was stepped to
  // (280 ms, enter); the same file, read, fades in where it was.
  function previewItemChanged(modal, state) {
    const item = modal.querySelector('.dui-pv-item');
    if (!item || item === state.item) return;
    const was = state.item;
    state.item = item;
    if (!was || !modal.classList.contains('on')) return;
    if (reduced() || was.dataset.file === item.dataset.file) {
      item.animate([{ opacity: 0.6 }, { opacity: 1 }], { duration: tokens.reducedMs + 30, easing: tokens.standard });
      return;
    }
    item.animate([{ opacity: 0, transform: `translateX(${previewStep * 48}px)` }, { opacity: 1, transform: 'none' }],
      { duration: 280, easing: tokens.enter });
  }
  function previewKey(event, modal) {
    const at = event.target;
    const media = at && at.closest && at.closest('video, audio, .dui-pv-wave');
    if ((event.key === 'ArrowRight' || event.key === 'ArrowLeft') && !media) {
      event.preventDefault();
      previewStep = event.key === 'ArrowRight' ? 1 : -1;
      tell(modal, previewStep > 0 ? 'next' : 'previous');
      return true;
    }
    // Space presses a button that has the keyboard, plays a sound and
    // scrolls text or a PDF; anywhere else it closes.
    if (event.key === ' ' && !media && !(at && at.closest && at.closest('button, [data-dui-scroll] pre, .dui-pv-pdf'))) {
      event.preventDefault();
      tell(modal, 'dismiss');
      return true;
    }
    return false;
  }
  document.addEventListener('click', event => {
    const nav = event.target.closest && event.target.closest('.dui-pv-nav');
    if (nav) { previewStep = nav.classList.contains('prev') ? -1 : 1; return; }
    const modal = event.target.closest && event.target.closest('[data-dui-modal="preview"].on');
    if (!modal) return;
    const backdrop = event.target.matches('[data-dui-pv-stage], .dui-pv-item, .dui-pv-scrim, .dui-pv-failed');
    if (!backdrop) return;
    if (previewGestured) { previewGestured = false; return; }
    if (touchUi()) modal.classList.toggle('bare');
    else tell(modal, 'dismiss');
  });
  // What the preview shows is not handed out by the web view: no menu of
  // its own (Save image as, Save video as) and no dragging it out of the
  // window, which would write a copy where it was dropped. The app's own
  // menu, whose entries are the note's, is not shown over it either.
  ['contextmenu', 'dragstart'].forEach(name => document.addEventListener(name, event => {
    if (!event.target.closest || !event.target.closest('[data-dui-modal="preview"]')) return;
    event.preventDefault();
    event.stopPropagation();
  }, true));
  // A double click on a picture zooms it to twice the size it fitted at,
  // about the point clicked, and back (280 ms, enter). Zoomed, its item
  // scrolls, so that every part of it can be reached: the wheel, a drag
  // or a finger moves it (#313). The point clicked stays where it was,
  // unless that would leave the picture short of an edge.
  function zoom(picture, x, y, on) {
    const item = picture.closest('.dui-pv-item');
    if (!item || on === picture.classList.contains('zoom')) return;
    const box = picture.getBoundingClientRect();
    const fx = Math.min(1, Math.max(0, (x - box.left) / box.width));
    const fy = Math.min(1, Math.max(0, (y - box.top) / box.height));
    if (on) {
      picture.style.setProperty('--w', `${box.width * 2}px`);
      picture.style.setProperty('--h', `${box.height * 2}px`);
      item.classList.add('zoomed');
      picture.classList.add('zoom');
      const now = picture.getBoundingClientRect();
      item.scrollLeft += now.left + fx * now.width - x;
      item.scrollTop += now.top + fy * now.height - y;
    } else {
      picture.classList.remove('zoom', 'held');
      item.classList.remove('zoomed');
      picture.style.removeProperty('--w');
      picture.style.removeProperty('--h');
    }
    if (reduced()) return;
    const origin = `${fx * 100}% ${fy * 100}%`;
    picture.animate([{ transform: `scale(${on ? 0.5 : 2})`, transformOrigin: origin }, { transform: 'none', transformOrigin: origin }],
      { duration: 280, easing: tokens.enter });
  }
  document.addEventListener('dblclick', event => {
    const picture = event.target.closest && event.target.closest('img[data-dui-zoom]');
    if (picture) zoom(picture, event.clientX, event.clientY, !picture.classList.contains('zoom'));
  });
  // Android: a swipe down closes, a swipe sideways goes to the next file,
  // and two fingers spreading or closing zoom a picture, anywhere on the
  // preview but its buttons. Touch events, as the web view takes a drag of
  // pointer events for its own scrolling. On text or a PDF, which scroll,
  // a finger moving up or down scrolls them, and only one moving sideways
  // is a swipe: otherwise a PDF that fills the screen could not be left.
  //
  // A touch keeps going to the element it began on, even once that element
  // is taken out of the page, and then no longer reaches the document: a
  // picture's stand-in replaced by the picture as a finger moves. So the
  // gesture listens on that element as well, and each touch event is taken
  // once, wherever it is heard first; and what moves is whatever item is
  // on the stage now.
  let previewSwipe = null, previewGestured = false;
  const onceOnly = event => { if (event.__duiPreview) return false; event.__duiPreview = true; return true; };
  const stageItem = swipe => swipe.modal.querySelector('.dui-pv-item');
  function followTouch(target) {
    const stop = () => {
      target.removeEventListener('touchmove', movePreviewSwipe);
      target.removeEventListener('touchend', ended);
      target.removeEventListener('touchcancel', ended);
    };
    const ended = event => { endPreviewSwipe(event); if (!event.touches || !event.touches.length) stop(); };
    target.addEventListener('touchmove', movePreviewSwipe, { passive: true });
    target.addEventListener('touchend', ended, { passive: true });
    target.addEventListener('touchcancel', ended, { passive: true });
  }
  document.addEventListener('touchstart', event => {
    const modal = event.target.closest && event.target.closest('[data-dui-modal="preview"].on');
    if (!modal) return;
    // A tap after a swipe is a tap again.
    previewGestured = false;
    followTouch(event.target);
    if (event.touches.length === 2) {
      const [a, b] = event.touches;
      previewSwipe = { pinch: Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY), modal,
        picture: modal.querySelector('img[data-dui-zoom]'), at: [(a.clientX + b.clientX) / 2, (a.clientY + b.clientY) / 2] };
      return;
    }
    // One finger on a zoomed picture moves it, as its item scrolls.
    if (event.touches.length !== 1 || event.target.closest('button, video, .dui-pv-audio, .dui-pv-item.zoomed')) { previewSwipe = null; return; }
    const touch = event.touches[0];
    previewSwipe = { x: touch.clientX, y: touch.clientY, mode: null, d: 0, modal,
      scrolls: !!event.target.closest('[data-dui-scroll]') };
  }, { passive: true });
  function movePreviewSwipe(event) {
    if (!onceOnly(event)) return;
    const swipe = previewSwipe;
    if (!swipe) return;
    // Two fingers: how far apart they are now. The fingers are lifted one
    // at a time, so the pinch is judged by where they last were together.
    if (swipe.pinch !== undefined) {
      if (event.touches.length === 2) {
        const [a, b] = event.touches;
        swipe.now = Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);
      }
      return;
    }
    if (event.touches.length !== 1) return;
    const touch = event.touches[0];
    const dx = touch.clientX - swipe.x, dy = touch.clientY - swipe.y;
    if (!swipe.mode && Math.hypot(dx, dy) > 8) swipe.mode = Math.abs(dy) > Math.abs(dx) ? 'y' : 'x';
    // Up or down on what scrolls is its own scrolling.
    if (swipe.mode === 'y' && swipe.scrolls) { previewSwipe = null; return; }
    const item = stageItem(swipe);
    if (!swipe.mode || !item) return;
    stats.moves += 1;
    if (swipe.mode === 'y') {
      const d = Math.max(0, dy);
      item.style.transform = `translateY(${d}px) scale(${1 - Math.min(d, 300) / 1200})`;
      const scrim = swipe.modal.querySelector('.dui-pv-scrim');
      if (scrim) scrim.style.opacity = String(1 - Math.min(d, 300) / 400);
      swipe.d = dy;
    } else {
      item.style.transform = `translateX(${dx}px)`;
      swipe.d = dx;
    }
  }
  document.addEventListener('touchmove', movePreviewSwipe, { passive: true });
  // A mouse or pen dragging a zoomed picture moves it.
  let panning = null;
  document.addEventListener('pointerdown', event => {
    if (event.pointerType === 'touch' || event.button || !event.target.closest) return;
    const picture = event.target.closest('img[data-dui-zoom].zoom');
    const item = picture && picture.closest('.dui-pv-item');
    if (!item) return;
    event.preventDefault();
    panning = { item, picture, id: event.pointerId, x: event.clientX, y: event.clientY, left: item.scrollLeft, top: item.scrollTop };
    picture.classList.add('held');
    try { picture.setPointerCapture(event.pointerId); } catch (error) { /* the pointer has gone */ }
  });
  window.addEventListener('pointermove', event => {
    if (!panning || event.pointerId !== panning.id) return;
    panning.item.scrollLeft = panning.left - (event.clientX - panning.x);
    panning.item.scrollTop = panning.top - (event.clientY - panning.y);
  }, { passive: true });
  const endPan = event => {
    if (!panning || event.pointerId !== panning.id) return;
    panning.picture.classList.remove('held');
    panning = null;
  };
  window.addEventListener('pointerup', endPan);
  window.addEventListener('pointercancel', endPan);
  function endPreviewSwipe(event) {
    if (!onceOnly(event)) return;
    const swipe = previewSwipe;
    if (!swipe) return;
    if (swipe.pinch !== undefined) {
      if (event.touches && event.touches.length) return;
      previewSwipe = null;
      const now = swipe.now === undefined ? swipe.pinch : swipe.now;
      if (swipe.picture) {
        if (now > swipe.pinch * 1.2) zoom(swipe.picture, swipe.at[0], swipe.at[1], true);
        else if (now < swipe.pinch * 0.85) zoom(swipe.picture, swipe.at[0], swipe.at[1], false);
      }
      previewGestured = true;
      return;
    }
    previewSwipe = null;
    const { mode, d, modal } = swipe;
    const item = stageItem(swipe);
    previewGestured = !!mode;
    const scrim = modal.querySelector('.dui-pv-scrim');
    if (scrim) scrim.style.opacity = '';
    if (!mode || !item) return;
    const cancelled = event.type === 'touchcancel';
    if (!cancelled && mode === 'y' && d > 110) { tell(modal, 'dismiss'); setTimeout(() => { item.style.transform = ''; }, 400); return; }
    const several = modal.querySelectorAll('.dui-pv-dots i').length > 1;
    if (!cancelled && mode === 'x' && Math.abs(d) > 70 && several) {
      item.style.transform = '';
      previewStep = d < 0 ? 1 : -1;
      tell(modal, previewStep > 0 ? 'next' : 'previous');
      return;
    }
    const pop = spring('pop');
    item.animate([{ transform: item.style.transform || 'none' }, { transform: 'none' }], { duration: pop.ms, easing: pop.easing });
    item.style.transform = '';
  }
  document.addEventListener('touchend', endPreviewSwipe, { passive: true });
  document.addEventListener('touchcancel', endPreviewSwipe, { passive: true });

  // ---- A sound in the preview (section 6.16) -------------------------------
  // `[data-dui-audio]` holds the sound's address: the clay button plays and
  // pauses it, the bars are the sound's shape and fill with clay as it
  // plays, a press on them seeks, and the time is under them. The shape is
  // read from the sound itself; one the web view cannot read is drawn
  // flat, and still plays.
  const BARS = 46;
  const clock = seconds => {
    const whole = Math.max(0, Math.floor(seconds || 0));
    return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
  };
  function soundShape(address) {
    const Context = window.AudioContext || window.webkitAudioContext;
    if (!Context) return Promise.resolve(null);
    return fetch(address).then(answer => answer.arrayBuffer()).then(bytes => {
      const context = new Context();
      return context.decodeAudioData(bytes).then(sound => {
        context.close();
        const data = sound.getChannelData(0), step = Math.max(1, Math.floor(data.length / BARS));
        const peaks = [];
        for (let bar = 0; bar < BARS; bar++) {
          let peak = 0;
          for (let at = bar * step; at < Math.min(data.length, (bar + 1) * step); at += 16) peak = Math.max(peak, Math.abs(data[at]));
          peaks.push(peak);
        }
        const most = Math.max(...peaks, 0.001);
        return peaks.map(peak => 14 + 86 * (peak / most));
      });
    }).catch(() => null);
  }
  function watchSound(card) {
    if (card._duiSound) return;
    const sound = new Audio(card.dataset.duiAudio);
    sound.preload = 'auto';
    card._duiSound = sound;
    const wave = card.querySelector('.dui-pv-wave'), time = card.querySelector('.dui-pv-time');
    const button = card.querySelector('.dui-pv-play');
    const draw = heights => {
      wave.replaceChildren(...heights.map(height => { const bar = document.createElement('i'); bar.style.height = height + '%'; return bar; }));
    };
    draw(new Array(BARS).fill(30));
    soundShape(card.dataset.duiAudio).then(heights => { if (heights && card.isConnected) draw(heights); });
    const show = () => {
      const done = sound.duration ? sound.currentTime / sound.duration : 0;
      const filled = Math.round(done * BARS);
      [...wave.children].forEach((bar, index) => bar.classList.toggle('done', index < filled));
      time.textContent = sound.duration && isFinite(sound.duration) ? `${clock(sound.currentTime)} / ${clock(sound.duration)}` : clock(sound.currentTime);
    };
    const playing = on => {
      card.classList.toggle('playing', on);
      button.setAttribute('aria-label', on ? 'Pause' : 'Play');
      button.dataset.tip = on ? 'Pause' : 'Play';
    };
    sound.addEventListener('timeupdate', show);
    sound.addEventListener('loadedmetadata', show);
    sound.addEventListener('play', () => playing(true));
    sound.addEventListener('pause', () => playing(false));
    sound.addEventListener('ended', () => { playing(false); sound.currentTime = 0; show(); });
    button.addEventListener('click', () => { if (sound.paused) sound.play().catch(() => {}); else sound.pause(); });
    wave.addEventListener('click', event => {
      if (!sound.duration || !isFinite(sound.duration)) return;
      const box = wave.getBoundingClientRect();
      sound.currentTime = Math.max(0, Math.min(1, (event.clientX - box.left) / box.width)) * sound.duration;
      show();
    });
  }
  function forgetSound(card) {
    const sound = card._duiSound;
    if (!sound) return;
    sound.pause();
    sound.removeAttribute('src');
    sound.load();
    card._duiSound = null;
  }

  // ---- A PDF's pages, by the bundled renderer (sections 6.16, 11) ---------
  // `[data-dui-pdf]` holds a PDF's address. PDF.js is loaded from the
  // folder the app bundles it in the first time a PDF is shown. Each page
  // is a canvas as wide as the column allows, drawn when it comes near
  // sight; the PDF's own scripts never run.
  let pdfjs = null;
  function loadPdfJs() {
    if (!pdfjs) {
      const folder = new URL((tokens.pdfjs || '/assets/pdfjs').replace(/\/?$/, '/'), location.href);
      pdfjs = import(new URL('pdf.min.mjs', folder).href).then(library => {
        library.GlobalWorkerOptions.workerSrc = new URL('pdf.worker.min.mjs', folder).href;
        return { library, folder };
      });
      pdfjs.catch(() => { pdfjs = null; });
    }
    return pdfjs;
  }
  function pdfFailed(holder, why) {
    holder.replaceChildren();
    const said = document.createElement('p');
    said.className = 'dui-pv-pdf-failed';
    said.textContent = 'The pages could not be drawn. Open it in another app to see them.';
    said.title = String(why || '');
    holder.append(said);
  }
  function watchPdf(holder) {
    if (holder._duiPdf) return;
    const state = { task: null, observer: null, gone: false };
    holder._duiPdf = state;
    loadPdfJs().then(({ library, folder }) => {
      if (state.gone) return null;
      state.task = library.getDocument({
        url: holder.dataset.duiPdf,
        cMapUrl: new URL('cmaps/', folder).href,
        cMapPacked: true,
        standardFontDataUrl: new URL('standard_fonts/', folder).href,
        wasmUrl: new URL('wasm/', folder).href,
        enableXfa: false,
      });
      return state.task.promise;
    }).then(pdf => {
      if (!pdf || state.gone) return;
      const width = Math.min(holder.clientWidth || 760, 900);
      const ratio = Math.min(window.devicePixelRatio || 1, 3);
      state.observer = new IntersectionObserver(entries => entries.forEach(entry => {
        if (!entry.isIntersecting || entry.target._duiDrawn) return;
        entry.target._duiDrawn = true;
        const number = Number(entry.target.dataset.page);
        pdf.getPage(number).then(page => {
          if (state.gone) return;
          const unscaled = page.getViewport({ scale: 1 });
          const viewport = page.getViewport({ scale: (width / unscaled.width) * ratio });
          const canvas = document.createElement('canvas');
          canvas.width = Math.floor(viewport.width);
          canvas.height = Math.floor(viewport.height);
          entry.target.append(canvas);
          return page.render({ canvasContext: canvas.getContext('2d'), viewport }).promise;
        }).catch(() => { entry.target._duiDrawn = false; });
      }), { root: holder.closest('[data-dui-scroll]'), rootMargin: '600px 0px' });
      // Every page has its place at once, as large as its first page
      // says, so the scrollbar is right from the start.
      return pdf.getPage(1).then(first => {
        const box = first.getViewport({ scale: 1 });
        const height = (width / box.width) * box.height;
        for (let number = 1; number <= pdf.numPages; number++) {
          const page = document.createElement('div');
          page.className = 'dui-pv-page';
          page.dataset.page = String(number);
          page.style.width = width + 'px';
          page.style.minHeight = height + 'px';
          page.setAttribute('aria-label', `Page ${number} of ${pdf.numPages}`);
          holder.append(page);
          state.observer.observe(page);
        }
      });
    }).catch(error => { if (!state.gone) pdfFailed(holder, error); });
  }
  function forgetPdf(holder) {
    const state = holder._duiPdf;
    if (!state) return;
    state.gone = true;
    if (state.observer) state.observer.disconnect();
    if (state.task) state.task.destroy();
    holder._duiPdf = null;
  }

  // ---- Quick capture (sections 5.5 and 7.2) --------------------------------
  // The desktop's quick capture is a small window of its own, kept ready
  // and shown by the hotkey. Its box (`data-dui-capture`) is made afresh
  // each time it opens, and comes into focus: from 97% and 10 px up,
  // blurred 10 px, to sharp (snap; the blur on enter, 260 ms; the mockup's
  // fade in, 280 ms). Put away, Rust sets the attribute to how it goes:
  //
  // - `tray`: the box shrinks to its grain (`data-dui-grain`) over 600 ms
  //   while Rust carries the window to the tray icon, across quickly and
  //   down slowly then fast, so that the path curves; where it lands a
  //   ring pulses once.
  // - `fall`: the same grain, which falls and fades where the window was,
  //   where the tray's place cannot be known.
  // - `fade`: nothing was written; it fades (280 ms, standard).
  //
  // The script says `away` as the box begins to shrink, which is when Rust
  // starts moving the window, and `gone` when it has finished, when Rust
  // hides the window. With reduced motion the box only fades, and says no
  // `away`, so the window does not move. Each box has an ID of its own
  // for these, and one removed from the page (a new box opened while it
  // left) says nothing more.
  const CAPTURE_MS = 600, CAPTURE_DOWN = 'cubic-bezier(.6,0,.9,.45)';
  function captureOpened(box) {
    if (reduced()) {
      box.animate([{ opacity: 0 }, { opacity: 1 }], { duration: tokens.reducedMs });
      return;
    }
    const snap = spring('snap');
    box.animate([{ transform: 'translateY(-10px) scale(.97)' }, { transform: 'none' }], { duration: snap.ms, easing: snap.easing });
    box.animate([{ filter: 'blur(10px)' }, { filter: 'blur(0)' }], { duration: 260, easing: tokens.enter });
    box.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 280, easing: tokens.standard });
  }
  function captureLeft(box, how) {
    if (box._duiLeaving) return;
    box._duiLeaving = true;
    box.getAnimations().forEach(run => run.cancel());
    // A box replaced by a new one while it left is gone from the page:
    // it says nothing more, and draws no ring.
    const gone = () => { if (box.isConnected) tell(box, 'gone'); };
    if (how === 'fade' || reduced()) {
      const ms = reduced() ? tokens.reducedMs : 280;
      after(box.animate([{ opacity: 1 }, { opacity: 0 }], { duration: ms, easing: tokens.standard, fill: 'forwards' }), ms, gone);
      return;
    }
    tell(box, 'away');
    const grain = box.dataset.duiGrain || 'hsl(32 46% 60%)';
    const width = box.offsetWidth || 1, height = box.offsetHeight || 1;
    // Its words go first; the box closes into a round of the grain's shade.
    [...box.children].forEach(child => child.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 240, easing: tokens.exit, fill: 'forwards' }));
    const end = how === 'fall' ? [{ opacity: 1, offset: 0.45 }, { opacity: 0 }] : [{ opacity: 1 }];
    const last = end[end.length - 1];
    Object.assign(last, { transform: `scale(${12 / width}, ${12 / height})`, borderRadius: '50%', backgroundColor: grain, boxShadow: 'none' });
    const shrink = box.animate([{ transform: 'none', opacity: 1, borderRadius: '16px', backgroundColor: getComputedStyle(box).backgroundColor }].concat(end),
      { duration: CAPTURE_MS, easing: CAPTURE_DOWN, fill: 'forwards' });
    after(shrink, CAPTURE_MS, () => {
      if (how !== 'tray' || !box.isConnected) return gone();
      // The tray icon pulses once as the grain lands on it.
      const place = box.getBoundingClientRect();
      const ring = document.createElement('i');
      ring.className = 'dui-capture-pulse';
      Object.assign(ring.style, { left: place.left + place.width / 2 + 'px', top: place.top + place.height / 2 + 'px' });
      document.body.append(ring);
      after(ring.animate([{ transform: 'scale(.4)', opacity: 1 }, { transform: 'scale(2.6)', opacity: 0 }],
        { duration: 560, easing: tokens.enter, fill: 'forwards' }), 560, () => { ring.remove(); gone(); });
    });
  }
  function watchCapture(box) {
    if (box._duiCapture) return;
    box._duiCapture = new MutationObserver(() => {
      const how = box.dataset.duiCapture;
      if (how && how !== 'open') captureLeft(box, how);
    });
    box._duiCapture.observe(box, { attributes: true, attributeFilter: ['data-dui-capture'] });
    if (box.dataset.duiCapture === 'open') captureOpened(box);
    else captureLeft(box, box.dataset.duiCapture);
  }
  function forgetCapture(box) {
    if (box._duiCapture) box._duiCapture.disconnect();
    box._duiCapture = null;
  }

  // ---- Voice (sections 5.5 and 6.18) --------------------------------------
  // The mic (`data-dui-mic`) is one gesture everywhere, followed here from
  // the press to the release. The press says `listen` at once, so that the
  // first word is heard. Then: let go within 280 ms, a tap, says `lock`
  // (hands-free, until ✓ or ✕), as does sliding 60 px up while held;
  // sliding 90 px to the left says `throw`; letting go after holding says
  // `keep`. A press taken away by the system is thrown away. A click from
  // the keyboard is a tap. The mockup's numbers.
  const HOLD_MS = 280, LOCK_PX = 60, THROW_PX = 90;
  let talk = null;
  const micOf = event => (event.target.closest ? event.target.closest('[data-dui-mic]') : null);
  function endTalk(kind) {
    const was = talk;
    talk = null;
    if (!was) return;
    was.mic.classList.remove('held');
    if (kind) tell(was.mic, kind);
  }
  document.addEventListener('pointerdown', event => {
    const mic = micOf(event);
    if (!mic || event.button !== 0 || mic.disabled) return;
    event.preventDefault();
    hideTip();
    try { mic.setPointerCapture(event.pointerId); } catch (_) { /* gone already */ }
    talk = { mic, pointer: event.pointerId, x: event.clientX, y: event.clientY, at: performance.now() };
    mic.classList.add('held');
    tell(mic, 'listen');
  }, true);
  document.addEventListener('pointermove', event => {
    if (!talk || event.pointerId !== talk.pointer) return;
    const dx = event.clientX - talk.x, dy = event.clientY - talk.y;
    if (dy < -LOCK_PX) endTalk('lock');
    else if (dx < -THROW_PX) endTalk('throw');
  }, true);
  document.addEventListener('pointerup', event => {
    if (!talk || event.pointerId !== talk.pointer) return;
    endTalk(performance.now() - talk.at < HOLD_MS ? 'lock' : 'keep');
  }, true);
  document.addEventListener('pointercancel', event => {
    if (talk && event.pointerId === talk.pointer) endTalk('throw');
  }, true);
  document.addEventListener('click', event => {
    const mic = micOf(event);
    if (!mic) return;
    event.preventDefault();
    if (event.detail === 0 && !mic.disabled) tell(mic, 'lock');
  }, true);

  // While listening (`data-dui-voice="listening"`) a row of grains moves
  // with the voice, like sand on a drum: still in silence, most in the
  // middle, higher as it gets louder, from the level Rust gives
  // (`data-dui-level`), eased so that it does not flicker. A soft ring
  // around ✓ breathes with it. Reduced motion keeps the grains in a still
  // row, and only their strength changes. When Rust says how it ended:
  //
  // - `kept`: the row gathers into one grain in its middle (260 ms, exit),
  //   and the new note's row takes the grain from there to settle onto the
  //   pile, as a typed thought's does;
  // - `thrown`: the grains blow off to the left, blurring (260 ms, exit);
  // - `nothing`: nothing was heard, and the row shakes once (320 ms).
  //
  // Then it fades and says `gone`.
  function drawGrains(view, state) {
    const canvas = view.querySelector('.dui-voice-grains');
    const target = Math.min(1, Math.max(0, parseFloat(view.dataset.duiLevel) || 0));
    state.level += (target - state.level) * 0.2;
    if (canvas && canvas.clientWidth) {
      const dpr = devicePixelRatio || 1, w = canvas.clientWidth, h = canvas.clientHeight;
      if (canvas.width !== Math.round(w * dpr)) { canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr); }
      const pen = canvas.getContext('2d');
      pen.setTransform(dpr, 0, 0, dpr, 0, 0);
      pen.clearRect(0, 0, w, h);
      const count = Math.max(8, Math.floor(w / 10)), t = performance.now() / 1000, still = reduced();
      for (let index = 0; index < count; index++) {
        const middle = Math.sin(Math.PI * (index + 0.5) / count);
        const height = still ? 0 : state.level * middle * h * 0.4;
        const beat = Math.sin(t * 9 + index * 0.9) * 0.6 + Math.sin(t * 13.7 + index * 2.3) * 0.4;
        pen.globalAlpha = 0.45 + 0.55 * Math.min(1, state.level * middle * 1.6 + 0.15);
        pen.fillStyle = state.sand[index % state.sand.length];
        pen.beginPath();
        pen.arc(5 + index * (w - 10) / (count - 1), h / 2 + beat * height, 2.3 + state.level * middle * 1.4, 0, 7);
        pen.fill();
      }
    }
    const keep = view.querySelector('.dui-voice-keep');
    if (keep) keep.style.setProperty('--lvl', state.level.toFixed(3));
    if (!state.ended && view.isConnected) state.frame = requestAnimationFrame(() => drawGrains(view, state));
  }
  function voiceEnded(view, state, how) {
    if (state.ended) return;
    state.ended = true;
    cancelAnimationFrame(state.frame);
    const keep = view.querySelector('.dui-voice-keep');
    if (keep) keep.style.setProperty('--lvl', '0');
    const canvas = view.querySelector('.dui-voice-grains');
    const gone = () => { if (view.isConnected) tell(view, 'gone'); };
    const fadeOut = () => {
      const ms = reduced() ? tokens.reducedMs : 160;
      after(view.animate([{ opacity: 1 }, { opacity: 0 }], { duration: ms, easing: tokens.standard, fill: 'forwards' }), ms, gone);
    };
    if (how === 'kept' && canvas) {
      const box = canvas.getBoundingClientRect();
      gathering = { x: box.left + box.width / 2, y: box.top + box.height / 2, at: performance.now() };
    }
    if (reduced() || !canvas) return fadeOut();
    if (how === 'kept') {
      after(canvas.animate([{ transform: 'none', opacity: 1 }, { transform: 'scaleX(.03)', opacity: 0 }],
        { duration: 260, easing: tokens.exit, fill: 'forwards' }), 260, fadeOut);
    } else if (how === 'thrown') {
      after(canvas.animate([{ transform: 'none', opacity: 1, filter: 'blur(0)' }, { transform: 'translateX(-70px)', opacity: 0, filter: 'blur(5px)' }],
        { duration: 260, easing: tokens.exit, fill: 'forwards' }), 260, fadeOut);
    } else {
      after(view.animate([{ transform: 'none' }, { transform: 'translateX(-7px)' }, { transform: 'translateX(6px)' }, { transform: 'translateX(-3px)' }, { transform: 'none' }],
        { duration: 320, easing: 'ease-out' }), 320, fadeOut);
    }
  }
  function watchVoice(view) {
    if (view._duiVoice) return;
    const state = { level: 0, frame: 0, ended: false, sand: (view.dataset.duiSand || 'hsl(32 46% 60%)').split(';') };
    view._duiVoice = state;
    state.observer = new MutationObserver(() => {
      const how = view.dataset.duiVoice;
      if (how && how !== 'listening') voiceEnded(view, state, how);
    });
    state.observer.observe(view, { attributes: true, attributeFilter: ['data-dui-voice'] });
    if (view.dataset.duiVoice !== 'listening') voiceEnded(view, state, view.dataset.duiVoice);
    else state.frame = requestAnimationFrame(() => drawGrains(view, state));
  }
  function forgetVoice(view) {
    const state = view._duiVoice;
    if (!state) return;
    state.ended = true;
    cancelAnimationFrame(state.frame);
    state.observer.disconnect();
    view._duiVoice = null;
  }
  // A voice note's words fade in where its name was (280 ms, standard),
  // when they arrive: its title says `made` where it said `waiting`.
  // Nothing else moves.
  function wordsArrived(records) {
    records.forEach(record => {
      const title = record.target;
      if (record.oldValue === 'waiting' && title.dataset.duiWords === 'made' && !reduced()) {
        title.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 280, easing: tokens.standard });
      }
    });
  }

  // ---- Finding what Rust renders -----------------------------------------
  const WATCHED = '[data-dui-list],[data-dui-modal],[data-dui-scroll],[data-dui-toast],[data-dui-more],[data-dui-focus],[data-dui-grow],[data-dui-bars],[data-dui-audio],[data-dui-pdf],[data-dui-capture],[data-dui-voice],.dui-confirm';
  function arrived(element) {
    if (element.matches('[data-dui-list]')) watchList(element);
    if (element.matches('[data-dui-modal]')) watchModal(element);
    if (element.matches('[data-dui-scroll]')) watchScroll(element);
    if (element.matches('[data-dui-toast]') && !element.classList.contains('dui-toast-ghost')) toastShown(element);
    if (element.matches('.dui-confirm')) confirmShown(element);
    if (element.matches('[data-dui-more]')) watchMore(element);
    if (element.matches('[data-dui-focus]')) watchWriting(element);
    if (element.matches('[data-dui-grow]')) watchGrow(element);
    if (element.matches('[data-dui-bars]')) tellBars();
    if (element.matches('[data-dui-audio]')) watchSound(element);
    if (element.matches('[data-dui-pdf]')) watchPdf(element);
    if (element.matches('[data-dui-capture]')) watchCapture(element);
    if (element.matches('[data-dui-voice]')) watchVoice(element);
  }
  function left(element, parent) {
    if (element.matches('[data-dui-list]')) forgetList(element);
    if (element.matches('[data-dui-modal]')) forgetModal(element);
    if (element.matches('[data-dui-scroll]')) forgetScroll(element);
    if (element.matches('[data-dui-more]')) forgetMore(element);
    if (element.matches('[data-dui-focus]')) forgetWriting(element);
    if (element.matches('[data-dui-grow]')) forgetGrow(element);
    if (element.matches('[data-dui-bars]')) setTimeout(tellBars, 0);
    if (element.matches('[data-dui-audio]')) forgetSound(element);
    if (element.matches('[data-dui-pdf]')) forgetPdf(element);
    if (element.matches('[data-dui-capture]')) forgetCapture(element);
    if (element.matches('[data-dui-voice]')) forgetVoice(element);
    if (element.matches('[data-dui-toast]') && !element.classList.contains('dui-toast-ghost')) {
      // The copy closes where the toast was: the toast's place, or the
      // error's above it.
      toastGone(element, parent && parent.closest ? parent.closest('.dui-toasts, .dui-toasts-err, .dui-toast-layer') : null);
    }
    if (element.matches('.dui-confirm') && parent && parent.closest) {
      const host = parent.closest('[data-dui-confirm-host]');
      if (host) setTimeout(() => confirmGone(host), 0);
    }
  }
  const each = (node, parent, visit) => {
    if (node.nodeType !== 1 || node.classList.contains('dui-ghost')) return;
    if (node.matches(WATCHED)) visit(node, parent);
    $$(WATCHED, node).forEach(inner => visit(inner, parent));
  };
  function watchPage() {
    new MutationObserver(records => records.forEach(record => {
      record.removedNodes.forEach(node => each(node, record.target, left));
      record.addedNodes.forEach(node => each(node, record.target, arrived));
    })).observe(document.body, { childList: true, subtree: true });
    new MutationObserver(wordsArrived)
      .observe(document.body, { attributes: true, attributeFilter: ['data-dui-words'], attributeOldValue: true, subtree: true });
    $$(WATCHED).forEach(element => arrived(element));
  }

  let installed = false;
  function install(options) {
    if (installed) return;
    installed = true;
    tokens = Object.assign({}, options.tokens, { pdfjs: options.pdfjs });
    platform = options.platform;
    if (document.body) watchPage(); else document.addEventListener('DOMContentLoaded', watchPage);
  }

  window.DirtUi = { install, apply, connect, frames, reduced, stats, knowsLinear, animate, focus, setValue, mergeInto, appendText, paste, back, jump, onPastedFiles, typedAgain };
})();
