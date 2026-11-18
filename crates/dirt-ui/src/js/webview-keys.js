// The web view's own keys, zoom and menu, stopped (interface design,
// sections 7.4 and 10). Put in the page's head of the desktop app's
// release builds. The web view's settings turn most of these off already;
// this covers a web view whose settings do not.
//
// The key is not swallowed. Only what the browser would do with it is
// stopped, so the app's own meaning for it (Ctrl R syncs, Ctrl F goes to
// Find, Ctrl + and Ctrl - change the text size) still runs.
(function () {
  'use strict';
  window.addEventListener('keydown', function (event) {
    var key = (event.key || '').toLowerCase();
    var mod = event.ctrlKey || event.metaKey;
    var browsers =
      // Reload, find again, caret browsing, developer tools.
      key === 'f5' || key === 'f3' || key === 'f7' || key === 'f12' ||
      // Reload, find, find again, print, view source, zoom in, out and back.
      (mod && !event.altKey && ['r', 'f', 'g', 'p', 'u', '+', '=', '-', '_', '0'].indexOf(key) >= 0) ||
      // Developer tools and a reload that skips the cache.
      (mod && event.shiftKey && ['i', 'j', 'r'].indexOf(key) >= 0);
    if (browsers) event.preventDefault();
  }, true);
  // Ctrl and the wheel, or a pinch on a touchpad, would zoom the window.
  window.addEventListener('wheel', function (event) {
    if (event.ctrlKey || event.metaKey) event.preventDefault();
  }, { passive: false, capture: true });
  // The browser's menu. The app's own (Copy, Paste, Select all) takes its
  // place: `components::ContextMenu`, shown by the interface's script.
  window.addEventListener('contextmenu', function (event) { event.preventDefault(); }, true);
})();
