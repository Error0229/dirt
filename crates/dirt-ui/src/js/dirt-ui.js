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
//   focus while writing.
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
  // One channel, opened once by the root component. A message is how
  // something ended, never how it is going.
  let post = null;
  const waiting = [];
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
    if (post) post(message); else waiting.push(message);
  }
  function connect(send) {
    post = send;
    waiting.splice(0).forEach(send);
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
  }
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
    } else if (how === 'fold' && moving) {
      // An archived row folds shut where it was, while the rows below
      // rise into its place (snap).
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
        if (!seen.size || row.dataset.duiEnter === 'none' || !within(now, view)) return;
        if (quiet) { row.animate([{ opacity: 0 }, { opacity: 1 }], { duration: tokens.reducedMs }); return; }
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
      const rows = new Set(rowsOf(list));
      rows.forEach(row => { if (!state.known.has(row)) sizes.observe(row); });
      state.known.forEach(row => { if (!rows.has(row)) sizes.unobserve(row); });
      state.known = rows;
    });
    state.observer.observe(list, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['class', 'hidden', 'data-key'] });
    lists.set(list, state);
    sizes.observe(list);
    rowsOf(list).forEach(row => { state.known.add(row); sizes.observe(row); });
    remember(list);
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
    setTimeout(() => {
      if (!modal.classList.contains('on')) return;
      const first = focusables(modal)[0] || modal;
      first.focus({ preventScroll: true });
    }, 60);
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
    if (modal.dataset.duiModal === 'panel' && !reduced()) {
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
    modals.set(modal, state);
    if (state.open) opened(modal, state);
  }
  function forgetModal(modal) {
    const state = modals.get(modal);
    if (!state) return;
    state.observer.disconnect();
    modals.delete(modal);
    if (state.open && state.opener && state.opener.isConnected) state.opener.focus({ preventScroll: true });
  }
  // Tab stays inside the panel or sheet on top; Esc closes it.
  document.addEventListener('keydown', event => {
    if (event.isComposing) return;
    const top = openModals().pop();
    if (!top) return;
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
    const hold = () => drain.pause();
    const go = () => { if (!toast.matches(':hover') && !toast.contains(document.activeElement)) drain.play(); };
    toast.addEventListener('pointerenter', hold);
    toast.addEventListener('pointerleave', go);
    toast.addEventListener('focusin', hold);
    toast.addEventListener('focusout', () => setTimeout(go, 0));
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
  function setValue(selector, text) {
    const element = document.querySelector(selector);
    if (!element || element.value === text) return;
    const keep = document.activeElement === element ? [element.selectionStart, element.selectionEnd] : null;
    element.value = text;
    if (keep) element.setSelectionRange(Math.min(keep[0], text.length), Math.min(keep[1], text.length));
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
  // not heard. An element with `data-dui-home` takes it instead.
  document.addEventListener('focusout', () => {
    setTimeout(() => {
      const at = document.activeElement;
      if (at && at !== document.body && at !== root) return;
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
    field.value = '';
    field.setSelectionRange(0, 0);
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
      rowsInSight(scope).forEach((row, index) => row.animate(
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

  // ---- Finding what Rust renders -----------------------------------------
  const WATCHED = '[data-dui-list],[data-dui-modal],[data-dui-scroll],[data-dui-toast],[data-dui-more],[data-dui-focus],.dui-confirm';
  function arrived(element) {
    if (element.matches('[data-dui-list]')) watchList(element);
    if (element.matches('[data-dui-modal]')) watchModal(element);
    if (element.matches('[data-dui-scroll]')) watchScroll(element);
    if (element.matches('[data-dui-toast]') && !element.classList.contains('dui-toast-ghost')) toastShown(element);
    if (element.matches('.dui-confirm')) confirmShown(element);
    if (element.matches('[data-dui-more]')) watchMore(element);
    if (element.matches('[data-dui-focus]')) watchWriting(element);
  }
  function left(element, parent) {
    if (element.matches('[data-dui-list]')) forgetList(element);
    if (element.matches('[data-dui-modal]')) forgetModal(element);
    if (element.matches('[data-dui-scroll]')) forgetScroll(element);
    if (element.matches('[data-dui-more]')) forgetMore(element);
    if (element.matches('[data-dui-focus]')) forgetWriting(element);
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
    $$(WATCHED).forEach(element => arrived(element));
  }

  let installed = false;
  function install(options) {
    if (installed) return;
    installed = true;
    tokens = options.tokens;
    platform = options.platform;
    if (document.body) watchPage(); else document.addEventListener('DOMContentLoaded', watchPage);
  }

  window.DirtUi = { install, apply, connect, frames, reduced, stats, knowsLinear, animate, focus, setValue, appendText, paste };
})();
