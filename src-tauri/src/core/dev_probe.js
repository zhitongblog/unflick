// The probe: what `dev click`, `dev text`, `dev snapshot` and `dev wait`
// actually are.
//
// `core::dev` builds a call like `return __unflickDev.click({...})` and the
// GUI host prepends this file to it. There is exactly one copy of this file
// and it runs unchanged on WKWebView, WebView2 and WebKitGTK, which is the
// whole reason `eval` is the only primitive: a snapshot bug that exists on
// one platform and not the others cannot be written here.
//
// Two rules the rest of this file obeys:
//
//   * Nothing throws across the boundary. A thrown error arrives in Rust as
//     a stack trace with the useful sentence buried in it, so every refusal
//     is `{ok: false, error: "..."}` — a sentence someone can act on,
//     naming the selector or the element that got in the way.
//   * Nothing reports an action it did not perform. `el.click()` on an
//     element under a modal overlay returns quietly and changes nothing;
//     that false pass is the exact failure class this bridge exists to
//     remove, so a click hit-tests first and refuses by name.
//
// ES5 on purpose. This is prepended to every eval, including ones issued at
// a moment when the page's own bundle has failed to parse, and it must not
// be the second thing that breaks.

;(function () {
  'use strict';

  var VERSION = 1;
  // Idempotent: every eval carries the whole probe, and re-defining it on
  // each call would throw away nothing but cost the parse every time.
  if (globalThis.__unflickDev && globalThis.__unflickDev.version === VERSION) {
    return;
  }

  /** Longest accessible name kept. Past this it is prose, not a name. */
  var NAME_CAP = 120;
  /** Longest text `dev text` returns per match, before it is a file dump. */
  var TEXT_CAP = 2000;
  /** Most nodes one snapshot may carry. A bigger tree is a DOM dump. */
  var NODE_CAP = 400;
  /** How often the polling verbs look again. */
  var POLL_MS = 50;

  var SKIP_TAGS = [
    'script', 'style', 'noscript', 'template', 'link', 'meta', 'head',
    'title', 'br', 'base'
  ];

  var ROLE_BY_TAG = {
    a: 'link', button: 'button', textarea: 'textbox', select: 'combobox',
    option: 'option', img: 'img', svg: 'img', h1: 'heading', h2: 'heading',
    h3: 'heading', h4: 'heading', h5: 'heading', h6: 'heading',
    p: 'paragraph', ul: 'list', ol: 'list', li: 'listitem', table: 'table',
    tr: 'row', td: 'cell', th: 'columnheader', nav: 'navigation',
    main: 'main', header: 'banner', footer: 'contentinfo',
    aside: 'complementary', form: 'form', label: 'label', dialog: 'dialog',
    video: 'video', audio: 'audio', canvas: 'canvas',
    progress: 'progressbar', section: 'region', article: 'article',
    input: 'textbox'
  };

  // The accname order, and the order matters. `title` is a *fallback*
  // after the element's own content, not a label that overrides it: a
  // `<p title="/long/path/to/film.mkv">film</p>` announces "film", and a
  // snapshot that said "/long/path/to/film.mkv" would be describing an
  // interface nobody is looking at.
  var NAME_ATTRS_BEFORE_TEXT = ['alt'];
  var NAME_ATTRS_AFTER_TEXT = ['title', 'placeholder', 'aria-valuetext'];

  // ── small helpers ───────────────────────────────────────────────────

  function fail(message) {
    return { ok: false, error: message };
  }

  /**
   * What to add to a "not yet" when the window is not on screen.
   *
   * Measured, not guessed: in a hidden WKWebView `requestAnimationFrame`
   * never fires at all, while `setTimeout` still does. Framer Motion drives
   * its exit animations on animation frames and `AnimatePresence` keeps a
   * component mounted until the exit finishes — so a panel that has been
   * closed stays in the page indefinitely, and `dev wait --gone` on it is
   * waiting for something that cannot happen. Without this sentence that
   * looks exactly like a UI bug, and someone spends an afternoon on it.
   */
  function hiddenNote() {
    if (typeof document.hidden !== 'boolean' || !document.hidden) return '';
    return ' — note the window is not on screen (' + document.visibilityState +
      '), so animation frames are suspended and anything waiting on an exit ' +
      'animation to be removed will never be';
  }

  function cap(text, limit) {
    var s = String(text).replace(/\s+/g, ' ').trim();
    return s.length > limit ? s.slice(0, limit) + '…' : s;
  }

  function escapeIdent(value) {
    if (globalThis.CSS && typeof CSS.escape === 'function') {
      return CSS.escape(value);
    }
    return String(value).replace(/[^\w-]/g, '\\$&');
  }

  /** Every match, or `null` when the string is not a selector at all. */
  function queryAll(selector) {
    try {
      return Array.prototype.slice.call(document.querySelectorAll(selector));
    } catch (e) {
      return null;
    }
  }

  function countOf(selector) {
    var found = queryAll(selector);
    return found === null ? -1 : found.length;
  }

  function badSelector(selector) {
    return JSON.stringify(String(selector)) +
      ' is not a CSS selector this page understands';
  }

  /**
   * Can a person see this element right now.
   *
   * Deliberately stricter than "is in the DOM". A panel that animated out
   * to opacity 0 is still in the tree, and reporting it as present is how
   * `dev wait --gone` would come back the instant it was asked.
   */
  function visible(el) {
    if (!el || el.nodeType !== 1) return false;
    if (el.closest && el.closest('[aria-hidden="true"]')) return false;
    if (typeof el.getClientRects !== 'function') return false;
    if (el.getClientRects().length === 0) return false;
    var style = globalThis.getComputedStyle ? getComputedStyle(el) : null;
    if (!style) return true;
    if (style.visibility === 'hidden' || style.visibility === 'collapse') return false;
    if (parseFloat(style.opacity) === 0) return false;
    return true;
  }

  /** The text that belongs to this element rather than to its children. */
  function ownText(el) {
    var out = '';
    for (var i = 0; i < el.childNodes.length; i++) {
      var node = el.childNodes[i];
      if (node.nodeType === 3) out += node.nodeValue;
    }
    return out.replace(/\s+/g, ' ').trim();
  }

  /**
   * The name a screen reader would announce.
   *
   * The order is the accname order, and the last resort — the element's own
   * text — is what makes an "undefined" leaking into a label show up here
   * as `name: "undefined"` rather than as a screenshot nobody looked at.
   */
  function accessibleName(el) {
    if (!el.getAttribute) return '';

    var label = el.getAttribute('aria-label');
    if (label && label.trim()) return cap(label, NAME_CAP);

    var by = el.getAttribute('aria-labelledby');
    if (by) {
      var parts = [];
      var ids = by.trim().split(/\s+/);
      for (var i = 0; i < ids.length; i++) {
        var target = document.getElementById(ids[i]);
        if (target) parts.push(target.textContent || '');
      }
      var joined = parts.join(' ').trim();
      if (joined) return cap(joined, NAME_CAP);
    }

    var attr = firstAttr(el, NAME_ATTRS_BEFORE_TEXT);
    if (attr) return attr;

    var own = ownText(el);
    if (own) return cap(own, NAME_CAP);

    // A button whose label sits in a nested span still has a name. A page
    // wrapper does not — and `body` least of all, which is what naming by
    // textContent alone would have called "0:200:20selftest1×100". So the
    // nested text only counts when there is barely any nesting.
    if (shallow(el)) {
      var inner = (el.textContent || '').replace(/\s+/g, ' ').trim();
      if (inner && inner.length <= NAME_CAP) return inner;
    }

    attr = firstAttr(el, NAME_ATTRS_AFTER_TEXT);
    if (attr) return attr;

    if (typeof el.value === 'string' && el.value.trim()) {
      return cap(el.value, NAME_CAP);
    }
    return '';
  }

  function firstAttr(el, names) {
    for (var i = 0; i < names.length; i++) {
      var value = el.getAttribute(names[i]);
      if (value && value.trim()) return cap(value, NAME_CAP);
    }
    return '';
  }

  /**
   * Is this element a label rather than a layout.
   *
   * One level of nesting at most, and no more than two children — enough
   * for `<button><svg/><span>Play</span></button>`, nowhere near enough for
   * a panel. Answered from `childElementCount` alone so it stays O(1): the
   * obvious version, `querySelectorAll('*').length`, walks the subtree of
   * every node in the tree and turns a snapshot quadratic.
   */
  function shallow(el) {
    if (el.childElementCount === 0) return true;
    if (el.childElementCount > 2) return false;
    for (var i = 0; i < el.children.length; i++) {
      if (el.children[i].childElementCount > 0) return false;
    }
    return true;
  }

  function roleOf(el) {
    var explicit = el.getAttribute && el.getAttribute('role');
    if (explicit && explicit.trim()) return explicit.trim().split(/\s+/)[0];

    var tag = el.tagName ? el.tagName.toLowerCase() : '';
    if (tag === 'input') {
      var type = (el.getAttribute('type') || 'text').toLowerCase();
      if (type === 'checkbox') return 'checkbox';
      if (type === 'radio') return 'radio';
      if (type === 'range') return 'slider';
      if (type === 'button' || type === 'submit' || type === 'reset') return 'button';
      if (type === 'hidden') return 'none';
      return 'textbox';
    }
    if (tag === 'a' && !el.getAttribute('href')) return 'generic';
    return ROLE_BY_TAG[tag] || 'generic';
  }

  function stateOf(el) {
    var state = {};
    if (el.disabled === true || (el.getAttribute && el.getAttribute('aria-disabled') === 'true')) {
      state.disabled = true;
    }
    var checked = el.getAttribute && el.getAttribute('aria-checked');
    if (checked !== null && checked !== undefined) {
      state.checked = checked === 'true';
    } else if (typeof el.checked === 'boolean' &&
               (el.type === 'checkbox' || el.type === 'radio')) {
      state.checked = el.checked;
    }
    var expanded = el.getAttribute && el.getAttribute('aria-expanded');
    if (expanded !== null && expanded !== undefined) state.expanded = expanded === 'true';
    var selected = el.getAttribute && el.getAttribute('aria-selected');
    if (selected !== null && selected !== undefined) state.selected = selected === 'true';
    var current = el.getAttribute && el.getAttribute('aria-current');
    if (current) state.current = current;
    if (document.activeElement === el) state.focused = true;
    if (typeof el.value === 'string' && el.value !== '' && el.tagName !== 'BUTTON') {
      state.value = cap(el.value, NAME_CAP);
    }
    for (var key in state) {
      if (Object.prototype.hasOwnProperty.call(state, key)) return state;
    }
    return null;
  }

  /**
   * A selector that finds this element again, and only this element.
   *
   * `#id` first, then `[data-testid]`, then an `nth-of-type` path rooted at
   * `body` — the last of which is unique by construction. It is checked
   * anyway, because a selector in a snapshot that does not feed back into
   * `dev click` is worse than no selector at all.
   */
  function uniqueSelector(el) {
    if (!el || el.nodeType !== 1) return null;
    if (el === document.body) return 'body';
    if (el === document.documentElement) return 'html';

    if (el.id) {
      var byId = '#' + escapeIdent(el.id);
      if (countOf(byId) === 1) return byId;
    }
    var testid = el.getAttribute && el.getAttribute('data-testid');
    if (testid) {
      var byTest = '[data-testid="' + testid.replace(/["\\]/g, '\\$&') + '"]';
      if (countOf(byTest) === 1) return byTest;
    }

    var parts = [];
    var node = el;
    var reachedBody = false;
    while (node && node.nodeType === 1 && parts.length < 12) {
      if (node === document.body) { reachedBody = true; break; }
      var parent = node.parentElement;
      if (!parent) break;
      var tag = node.tagName.toLowerCase();
      var same = [];
      for (var i = 0; i < parent.children.length; i++) {
        if (parent.children[i].tagName === node.tagName) same.push(parent.children[i]);
      }
      parts.unshift(same.length > 1
        ? tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')'
        : tag);
      node = parent;
    }
    // Truncated before reaching `body`, so the path is a fragment and might
    // match somewhere else entirely. Say nothing rather than something
    // wrong.
    if (!reachedBody) return null;

    var path = 'body > ' + parts.join(' > ');
    return countOf(path) === 1 ? path : null;
  }

  /** What an element is, in the few words a refusal has room for. */
  function describe(el) {
    if (!el) return 'nothing';
    if (el.nodeType !== 1) return String(el.nodeName || 'a node').toLowerCase();
    var out = el.tagName.toLowerCase();
    if (el.id) out += '#' + el.id;
    else if (el.classList && el.classList.length) {
      out += '.' + Array.prototype.slice.call(el.classList, 0, 2).join('.');
    }
    var name = accessibleName(el);
    if (name) out += ' "' + name + '"';
    return out;
  }

  // ── the four entry points ───────────────────────────────────────────

  /**
   * One look: does the selector have a visible match — or, with `gone`,
   * has it stopped having one.
   *
   * One look, not a loop. The loop lives in `core::dev` on the Rust side,
   * and the reason is measured rather than stylistic: WebKit stops a
   * hidden page's timers a few seconds after it is hidden, so a
   * `setTimeout` chain in a window that is covered, minimised, on another
   * Space or behind a locked screen fires at 50 ms for about three
   * seconds and then never again. That is precisely the unattended case
   * this whole surface exists for, and a `wait` that hangs forever there
   * would be worse than no `wait` at all.
   */
  function waitFor(args) {
    var selector = String(args.selector);
    var gone = !!args.gone;
    var found = queryAll(selector);
    if (found === null) return fail(badSelector(selector));

    var live = found.filter(visible);
    var done = gone ? live.length === 0 : live.length > 0;
    var result = {
      ok: true,
      done: done,
      selector: selector,
      count: live.length,
      present: found.length
    };
    if (!done) {
      // The distinction that saves an hour: it IS in the page, it is just
      // not showing. A wrong selector and a panel that never opened need
      // different fixes, and only this sentence tells them apart.
      result.why = (gone
        ? selector + ' is still visible (' + live.length + ' match(es))' + hiddenNote()
        : (found.length
            ? found.length + ' match(es) for ' + selector +
              ' are in the page but none is visible'
            : 'nothing matches ' + selector));
    }
    return result;
  }

  /**
   * One attempt at clicking one element, having first proved it is the one
   * that would be hit.
   *
   * Also one attempt rather than a loop, for the same measured reason as
   * `waitFor`. What is retryable is reported as `done: false` with a
   * `why`; what is the caller's mistake — a selector that is not one, a
   * selector that matches several things — is refused outright, because
   * retrying will not change the answer and clicking the first of several
   * is how a test comes back green having pressed the wrong button.
   */
  function click(args) {
    var selector = String(args.selector);
    var index = (args.index === null || args.index === undefined) ? null : args.index | 0;

    var found = queryAll(selector);
    if (found === null) return fail(badSelector(selector));

    var matches = found.filter(visible);
    if (!matches.length) {
      return {
        ok: true,
        done: false,
        why: found.length
          ? found.length + ' match(es) for ' + selector + ' are in the page but ' +
            'none is visible'
          : 'nothing matches ' + selector
      };
    }

    if (index === null && matches.length > 1) {
      var names = matches.slice(0, 5).map(function (el, i) {
        return i + ': ' + describe(el);
      });
      return fail(selector + ' matches ' + matches.length +
        ' visible elements — pass an index (0–' + (matches.length - 1) +
        ') to say which. ' + names.join('; ') +
        (matches.length > 5 ? '; …' : ''));
    }

    var chosen = index === null ? 0 : index;
    if (chosen < 0 || chosen >= matches.length) {
      return fail('index ' + chosen + ' is out of range — ' + selector +
        ' matches ' + matches.length + ' visible element(s)');
    }
    var el = matches[chosen];

    if (typeof el.scrollIntoView === 'function') {
      el.scrollIntoView({ block: 'center', inline: 'center' });
    }
    // Re-measured after the scroll, never before: the rect that decided
    // where to click has to be the rect the element is at.
    var rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) {
      return {
        ok: true,
        done: false,
        why: describe(el) + ' is in the page but has no size'
      };
    }

    var x = rect.left + rect.width / 2;
    var y = rect.top + rect.height / 2;
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) {
      return {
        ok: true,
        done: false,
        why: describe(el) + ' is outside the visible viewport even after ' +
          'scrolling to it'
      };
    }

    var top = document.elementFromPoint(x, y);
    if (!top) {
      return {
        ok: true,
        done: false,
        why: 'nothing is at (' + Math.round(x) + ', ' + Math.round(y) +
          '), where ' + describe(el) + ' claims to be'
      };
    }
    if (!(el === top || el.contains(top) || top.contains(el))) {
      // The whole reason this is not a bare `el.click()`: naming the
      // covering element turns "the test clicked and nothing happened"
      // into "a dialog is over it".
      return {
        ok: true,
        done: false,
        why: describe(el) + ' is covered by ' + describe(top) +
          ' at (' + Math.round(x) + ', ' + Math.round(y) + ')' + hiddenNote()
      };
    }

    press(el, x, y);

    return {
      ok: true,
      done: true,
      clicked: describe(el),
      selector: uniqueSelector(el) || selector,
      index: chosen,
      matches: matches.length,
      x: Math.round(x),
      y: Math.round(y)
    };
  }

  /**
   * The event sequence a real press produces, in the order it produces it.
   *
   * React and Framer Motion both listen on `pointerdown` far more often
   * than on `click`, so a bare click misses handlers and reports success
   * for a press nothing heard.
   *
   * The last step is a dispatched `click` carrying coordinates, not
   * `el.click()`. `el.click()` is the same dispatch with a default init,
   * which means `clientX` and `clientY` are **zero** — so a handler that
   * reads them, as every bar in this interface does, saw every click land
   * on its left edge. Driving the progress bar at 75% seeked to 0:00 and
   * reported success. Activation behaviour runs for a dispatched click
   * too (a checkbox still toggles, an anchor still navigates); the only
   * thing `el.click()` added was the wrong position.
   */
  function press(el, x, y) {
    var init = {
      bubbles: true, cancelable: true, composed: true,
      clientX: x, clientY: y, button: 0, buttons: 1, detail: 1,
      view: globalThis
    };
    var pointerInit = {};
    for (var k in init) if (Object.prototype.hasOwnProperty.call(init, k)) pointerInit[k] = init[k];
    pointerInit.pointerId = 1;
    pointerInit.pointerType = 'mouse';
    pointerInit.isPrimary = true;

    if (typeof PointerEvent === 'function') {
      el.dispatchEvent(new PointerEvent('pointerover', pointerInit));
      el.dispatchEvent(new PointerEvent('pointerenter', pointerInit));
      el.dispatchEvent(new PointerEvent('pointerdown', pointerInit));
    }
    el.dispatchEvent(new MouseEvent('mouseover', init));
    el.dispatchEvent(new MouseEvent('mousemove', init));
    el.dispatchEvent(new MouseEvent('mousedown', init));
    if (typeof el.focus === 'function') {
      try { el.focus({ preventScroll: true }); } catch (e) { /* not focusable */ }
    }
    if (typeof PointerEvent === 'function') {
      el.dispatchEvent(new PointerEvent('pointerup', pointerInit));
    }
    el.dispatchEvent(new MouseEvent('mouseup', init));
    el.dispatchEvent(new MouseEvent('click', init));
  }

  /**
   * The visible text of every match.
   *
   * Every one, with its index, rather than the refusal `click` gives for an
   * ambiguous selector: reading is not acting, so there is no wrong element
   * to read, and "which of these three rows is it" is answered by looking
   * at all three.
   */
  function text(args) {
    var selector = String(args.selector);
    var found = queryAll(selector);
    if (found === null) return fail(badSelector(selector));
    if (!found.length) return fail('nothing matches ' + selector);

    var matches = found.map(function (el, i) {
      // innerText, not textContent: it is what is *rendered*, so a
      // display:none sibling does not end up in the answer.
      var raw = (typeof el.innerText === 'string' && el.innerText !== '')
        ? el.innerText
        : (el.textContent || '');
      var value = raw.replace(/[ \t]+/g, ' ').replace(/\n{3,}/g, '\n\n').trim();
      var entry = {
        index: i,
        text: value.length > TEXT_CAP ? value.slice(0, TEXT_CAP) + '…' : value,
        truncated: value.length > TEXT_CAP,
        visible: visible(el),
        selector: uniqueSelector(el),
        role: roleOf(el)
      };
      return entry;
    });
    return { ok: true, selector: selector, matches: matches };
  }

  /**
   * The accessibility tree, not a DOM dump.
   *
   * Invisible nodes are skipped and unnamed wrappers are elided with their
   * children lifted into their place, so what comes back is the interface
   * as a person meets it: roles, names, states, and a selector per node
   * that feeds straight back into `dev click` and `dev text`.
   */
  function snapshot(args) {
    var root = document.body;
    if (args && args.selector) {
      var found = queryAll(args.selector);
      if (found === null) return fail(badSelector(args.selector));
      if (!found.length) return fail('nothing matches ' + args.selector);
      root = found[0];
    }
    if (!root) return fail('the page has no body yet — it may still be loading');

    var maxDepth = (args && args.depth > 0) ? args.depth : 12;
    var nodes = [];
    var truncated = false;

    function emit(el, depth) {
      var node = {
        ref: nodes.length,
        depth: depth,
        role: roleOf(el),
        name: accessibleName(el),
        selector: uniqueSelector(el)
      };
      var state = stateOf(el);
      if (state) node.state = state;
      nodes.push(node);
    }

    function walk(el, depth) {
      if (truncated) return;
      for (var i = 0; i < el.children.length; i++) {
        if (truncated) return;
        var child = el.children[i];
        var tag = child.tagName ? child.tagName.toLowerCase() : '';
        if (SKIP_TAGS.indexOf(tag) >= 0) continue;
        if (!visible(child)) continue;

        var role = roleOf(child);
        var elide = role === 'generic' && !accessibleName(child) && !stateOf(child);
        if (elide) {
          // A layout wrapper is scaffolding, not interface. Its children
          // take its place at the same depth, so the tree reads like the
          // screen rather than like the JSX.
          if (depth < maxDepth) walk(child, depth);
          continue;
        }
        if (nodes.length >= NODE_CAP) { truncated = true; return; }
        emit(child, depth);
        if (depth < maxDepth) walk(child, depth + 1);
      }
    }

    emit(root, 0);
    walk(root, 1);

    return {
      ok: true,
      url: location.href,
      title: document.title,
      root: uniqueSelector(root) || 'body',
      viewport: { width: innerWidth, height: innerHeight },
      truncated: truncated,
      nodes: nodes
    };
  }

  globalThis.__unflickDev = {
    version: VERSION,
    click: click,
    text: text,
    snapshot: snapshot,
    waitFor: waitFor
  };
})();
