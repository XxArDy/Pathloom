(function () {
  'use strict';
  var REPO = 'https://github.com/XxArDy/Pathloom/releases';
  var body = document.body, T = body.dataset;
  var reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var NS = 'http://www.w3.org/2000/svg';

  function h(tag, cls, html) { var e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; }
  function ico(name, cls) { return '<svg class="' + (cls || 'ico') + '" viewBox="0 0 24 24" aria-hidden="true"><use href="#i-' + name + '"/></svg>'; }
  function fmt(s, o) { return s.replace(/\{(\w+)\}/g, function (_, k) { return o[k]; }); }
  var CHK = '<span class="chk"><svg viewBox="0 0 10 10"><path d="M2.2 5.2 4.2 7.2 7.8 3"/></svg></span>';

  // ---- Edges: cubic curves between anchor points, like the canvas edgeCurve. ----
  function edge(svg, a, b, cls, vertical) {
    var p = document.createElementNS(NS, 'path'), head = document.createElementNS(NS, 'path'), d, hd;
    if (vertical) {
      var dy = Math.max(24, (b.y - a.y) / 2);
      d = 'M' + a.x + ' ' + a.y + 'C' + a.x + ' ' + (a.y + dy) + ' ' + b.x + ' ' + (b.y - dy) + ' ' + b.x + ' ' + (b.y - 7);
      hd = 'M' + b.x + ' ' + b.y + 'l-4.5 -8h9z';
    } else {
      var dx = Math.max(30, (b.x - a.x) / 2);
      d = 'M' + a.x + ' ' + a.y + 'C' + (a.x + dx) + ' ' + a.y + ' ' + (b.x - dx) + ' ' + b.y + ' ' + (b.x - 7) + ' ' + b.y;
      hd = 'M' + b.x + ' ' + b.y + 'l-8 -4.5v9z';
    }
    p.setAttribute('d', d); head.setAttribute('d', hd); head.setAttribute('class', 'head');
    if (cls) { p.setAttribute('class', cls); head.setAttribute('class', 'head ' + cls); }
    svg.appendChild(p); svg.appendChild(head);
    return [p, head];
  }

  // ---- Hero: the Pathloom window, built from the page's localized data. ----
  var dataEl = document.getElementById('mock-data'), win = document.getElementById('win');
  if (dataEl && win) hero(JSON.parse(dataEl.textContent), win);

  function hero(L, host) {
    var PH = [{ x: 20, cards: ['IDEA'] }, { x: 330, cards: ['CORE', 'DESIGN'] }, { x: 640, cards: ['AGENT', 'UI'] }];
    var DEPS = { CORE: ['IDEA'], DESIGN: ['IDEA'], AGENT: ['CORE'], UI: ['CORE', 'DESIGN'], BETA: ['AGENT', 'UI'], DOCS: ['UI'] };
    var WHO = { IDEA: L.me, CORE: 'AI', DESIGN: L.me + ' + AI', AGENT: 'AI', UI: 'AI', DOCS: 'AI' };
    var EXTRA = { CORE: { high: 1, branch: 'feat/core' }, DESIGN: { high: 1 } };
    var ORDER = ['AGENT', 'BETA', 'CORE', 'DESIGN', 'DOCS', 'IDEA', 'UI'];
    var MS = { x: 975, y: 150 }, GHOST = { x: 950, y: 318 };

    function initial() {
      var st = {};
      Object.keys(L.stages).forEach(function (k) {
        if (k === 'BETA') return;
        var titles = L.stages[k][1];
        st[k] = { tasks: titles.map(function (t, i) { return { id: k + '-0' + (i + 1), t: t, done: k === 'IDEA' || ((k === 'CORE' || k === 'DESIGN') && i === 0), claim: false }; }) };
      });
      st.CORE.tasks[1].claim = true;
      return { s: st, ghost: false, notice: false, hot: null };
    }
    function isDone(k, m) { return m.s[k].tasks.every(function (t) { return t.done; }); }
    function status(k, m) {
      if (isDone(k, m)) return 'done';
      if (!(DEPS[k] || []).every(function (d) { return isDone(d, m); })) return 'blocked';
      return m.s[k].tasks.some(function (t) { return t.done || t.claim; }) ? 'active' : 'ready';
    }

    // window chrome
    var sc = h('div', 'screen');
    var sb = h('div', 'sb');
    sb.innerHTML = '<div class="sb-proj"><img src="' + (document.querySelector('.brand img').getAttribute('src')) + '" alt="">' + L.project + ico('chev') + '</div>' +
      '<div class="sb-search">' + ico('search') + '<span>' + L.search + '</span></div>' +
      '<div class="sb-sec">' + L.boards + '</div><div class="sb-row">' + ico('map') + L.board + '</div>' +
      '<div class="sb-sec">' + L.onBoard + '</div><div class="sb-flt"><span class="on">' + L.all + '</span><span>' + L.active + '</span><span>' + L.doneF + '</span></div>';
    var outline = h('div'); sb.appendChild(outline);
    var main = h('div', 'main');
    main.innerHTML = '<div class="s-tabs"><div class="s-tab">' + ico('map') + L.board + ico('x', 'ico x') + '</div><div class="focus">' + ico('focus') + L.focus + '</div></div>' +
      '<div class="s-crumbs">' + L.project + ' <span>/</span> <b>' + L.board + '</b><div class="s-views"><span class="on">' + ico('map') + L.views[0] + '</span><span>' + ico('list') + L.views[1] + '</span><span>' + ico('overview') + L.views[2] + '</span></div></div>';
    var cv = h('div', 'canvas');
    cv.innerHTML = '<div class="s-tool">' + ['hand', 'pointer', 'square', 'diamond', 'circle', 'arrow', 'pencil', 'text', 'sticky', 'frame'].map(function (n, i) { return '<span' + (i === 1 ? ' class="on"' : '') + '>' + ico(n) + '</span>'; }).join('') + '</div>';
    var zoom = h('div', 's-zoom', '<span>' + ico('minus') + '</span><span class="zl"></span><span>' + ico('plus') + '</span><span>' + ico('fit') + '</span><span>' + ico('undo') + '</span><span>' + ico('redo') + '</span>');
    var notice = h('div', 'notice', ico('sticky') + '<span style="all:unset">' + L.notice + '</span><span>' + L.review + '</span>');
    var world = h('div', 'world');
    var svg = document.createElementNS(NS, 'svg'); svg.setAttribute('class', 'edges'); world.appendChild(svg);
    cv.appendChild(world); cv.appendChild(zoom); cv.appendChild(notice);
    var status_ = h('div', 's-status');
    main.appendChild(cv); main.appendChild(status_);
    sc.appendChild(sb); sc.appendChild(main); host.appendChild(sc);

    // phases + cards
    var cards = {}, phases = [];
    PH.forEach(function (p, i) {
      var fr = h('div', 'phase'); fr.innerHTML = '<div class="phase-h"><b>' + L.phases[i] + '</b><i></i><span></span></div>';
      world.appendChild(fr); phases.push({ el: fr, def: p });
    });
    function makeCard(k, x, y, ghost) {
      var el = h('div', 'card' + (ghost ? ' ghost' : ''));
      el.style.left = x + 'px'; el.style.top = y + 'px';
      var ex = EXTRA[k] || {};
      var foot = ex.high || ex.branch ? '<div class="cf">' + (ex.high ? '<span class="hi">' + ico('flag') + L.high + '</span>' : '') + (ex.branch ? '<span>' + ico('branch') + ex.branch + '</span>' : '') + '</div>' : '';
      el.innerHTML = '<div class="ch"><span class="key">' + k + '</span><span class="who' + (WHO[k].indexOf('AI') >= 0 ? ' ai' : '') + '">' + WHO[k] + '</span><span class="pill"></span></div>' +
        '<div class="ct">' + L.stages[k][0] + '</div><div class="cp"><i></i><span></span></div><ul class="tasks"></ul>' + foot;
      world.appendChild(el);
      cards[k] = { el: el, pill: el.querySelector('.pill'), bar: el.querySelector('.cp i'), num: el.querySelector('.cp span'), ul: el.querySelector('.tasks') };
    }
    PH.forEach(function (p) { p.cards.forEach(function (k) { makeCard(k, p.x + 20, 0); }); });
    makeCard('DOCS', GHOST.x, GHOST.y, true);
    var ms = h('div', 'ms', '<i></i><b>' + L.stages.BETA[0] + '</b><span></span>');
    ms.style.left = MS.x + 'px'; ms.style.top = MS.y + 'px'; world.appendChild(ms);

    var model = initial(), prevStatus = {}, edgeEls = [];
    var worldBox = { w: 1220, h: 520 };

    function layout() {
      // stack cards inside each phase column, then size the dashed phase frames around them
      phases.forEach(function (p) {
        var y = 70;
        p.def.cards.forEach(function (k) { var c = cards[k].el; c.style.top = y + 'px'; y += c.offsetHeight + 36; });
        p.el.style.left = p.def.x + 'px'; p.el.style.top = '50px'; p.el.style.width = '290px'; p.el.style.height = (y - 36 + 20 - 50) + 'px';
      });
      var maxB = 0;
      Object.keys(cards).forEach(function (k) { var c = cards[k].el; maxB = Math.max(maxB, c.offsetTop + c.offsetHeight); });
      phases.forEach(function (p) { maxB = Math.max(maxB, p.el.offsetTop + p.el.offsetHeight); });
      worldBox.h = maxB + 10;
      // edges from real card geometry
      while (svg.firstChild) svg.removeChild(svg.firstChild);
      edgeEls = [];
      function out(k) { var c = cards[k].el; return { x: c.offsetLeft + c.offsetWidth, y: c.offsetTop + 62 }; }
      function inn(k) { if (k === 'BETA') return { x: MS.x + 80 - 29, y: MS.y + 28 }; var c = cards[k].el; return { x: c.offsetLeft, y: c.offsetTop + 62 }; }
      Object.keys(DEPS).forEach(function (to) {
        DEPS[to].forEach(function (from) {
          var els = edge(svg, out(from), inn(to), to === 'DOCS' ? 'soft' : '');
          edgeEls.push({ from: from, to: to, els: els });
        });
      });
      fit();
    }

    function render() {
      var m = model, readyN = 0, blockedN = 0;
      Object.keys(cards).forEach(function (k) {
        var c = cards[k], st = status(k, m), tasks = m.s[k].tasks;
        var d = tasks.filter(function (t) { return t.done; }).length;
        c.pill.className = 'pill ' + st; c.pill.innerHTML = ico(st) + L.status[st];
        c.bar.style.setProperty('--p', Math.round(100 * d / tasks.length) + '%'); c.num.textContent = d + '/' + tasks.length;
        c.ul.innerHTML = tasks.map(function (t) {
          return '<li class="' + (t.done ? 'done' : t.claim ? 'claim' : '') + '">' + CHK + '<span class="tid">' + t.id + '</span><span class="tt">' + t.t + '</span>' + (t.claim ? '<span class="agent">claude</span>' : '') + '</li>';
        }).join('');
        c.el.classList.toggle('hot', m.hot === k);
        if (k !== 'DOCS') { if (st === 'blocked') blockedN++; else if (st !== 'done') readyN += tasks.length - d; }
      });
      cards.DOCS.el.classList.toggle('on', m.ghost);
      edgeEls.forEach(function (e) {
        var ok = isDone(e.from, m);
        e.els.forEach(function (el) { el.classList.toggle('done', ok); });
        e.els.forEach(function (el) { el.style.opacity = e.to === 'DOCS' && !m.ghost ? 0 : 1; });
      });
      var inDone = DEPS.BETA.filter(function (k) { return isDone(k, m); }).length;
      ms.querySelector('span').textContent = fmt(L.inputs, { d: inDone, n: DEPS.BETA.length });
      phases.forEach(function (p) {
        var dn = p.def.cards.filter(function (k) { return isDone(k, m); }).length;
        p.el.querySelector('.phase-h span').textContent = fmt(L.ofStages, { d: dn, n: p.def.cards.length });
        p.el.querySelector('.phase-h i').style.setProperty('--p', Math.round(100 * dn / p.def.cards.length) + '%');
      });
      notice.classList.toggle('on', m.notice);
      // sidebar outline, alphabetical like the app
      outline.innerHTML = ORDER.filter(function (k) { return k !== 'DOCS' || m.ghost; }).map(function (k) {
        if (k === 'BETA') return '<div class="ol">' + ico('diamond', 'ico sicon') + '<span class="k">BETA</span><span class="n">' + L.stages.BETA[0] + '</span><span class="c"></span></div>';
        var st = status(k, m), t = m.s[k].tasks, d = t.filter(function (x) { return x.done; }).length;
        var flash = prevStatus[k] && prevStatus[k] !== st ? ' flash' : '';
        prevStatus[k] = st;
        return '<div class="ol' + flash + '">' + ico(st, 'ico sicon ' + st) + '<span class="k">' + k + '</span><span class="n">' + L.stages[k][0] + '</span><span class="c">' + d + '/' + t.length + '</span></div>';
      }).join('');
      setTimeout(function () { outline.querySelectorAll('.flash').forEach(function (r) { r.classList.remove('flash'); }); }, 900);
      status_.innerHTML = '<span>' + ico('ready') + fmt(L.ready, { n: readyN }) + '</span><span>' + ico('blocked') + fmt(L.blockedN, { n: blockedN }) + '</span><span class="grow"></span><span>' + L.saved + '</span>';
    }

    // camera: fit the world (desktop) or its middle columns (narrow) into the canvas, like "zoom to fit"
    var narrow = null;
    function fit() {
      var cw = host.clientWidth; if (!cw) return;
      var isN = cw < 860;
      if (isN !== narrow) { narrow = isN; sb.style.display = isN ? 'none' : ''; }
      var W = isN ? 640 : 1100, H = isN ? 600 : 620, s = cw / W;
      sc.style.width = W + 'px'; sc.style.height = H + 'px'; sc.style.transform = 'scale(' + s + ')';
      host.style.height = Math.round(H * s) + 'px';
      var fx = isN ? 320 : 196, fw = isN ? 630 : worldBox.w - 196, fh = worldBox.h;
      var aw = cv.clientWidth - 32, ah = cv.clientHeight - 64 - 52;
      var z = Math.min(aw / fw, ah / fh, 1);
      var ox = (cv.clientWidth - fw * z) / 2 - fx * z, oy = 64 + (ah - fh * z) / 2;
      world.style.transform = 'translate(' + ox + 'px,' + oy + 'px) scale(' + z + ')';
      zoom.querySelector('.zl').textContent = Math.round(z * 100) + '%';
    }

    // the story: claude finishes CORE, AGENT unblocks, claude claims it and proposes a stage
    var STEPS = [
      [2200, function (m) { m.s.CORE.tasks[1].done = true; m.s.CORE.tasks[1].claim = false; m.s.CORE.tasks[2].claim = true; }],
      [2000, function (m) { m.s.CORE.tasks[2].done = true; m.s.CORE.tasks[2].claim = false; m.hot = 'AGENT'; }],
      [2000, function (m) { m.s.AGENT.tasks[0].claim = true; }],
      [1800, function (m) { m.hot = null; m.ghost = true; m.notice = true; }],
      [4200, function (m) { m.notice = false; }],
      [2200, null],
    ];
    var timer = null, step = 0, running = false;
    function tick() {
      if (step >= STEPS.length) { model = initial(); step = 0; render(); timer = setTimeout(tick, 1800); return; }
      var s = STEPS[step++];
      if (s[1]) { s[1](model); render(); }
      timer = setTimeout(tick, s[0]);
    }
    function play(on) {
      if (on === running) return; running = on;
      if (on) timer = setTimeout(tick, 1200); else clearTimeout(timer);
    }

    render(); layout();
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(layout);
    if (window.ResizeObserver) new ResizeObserver(fit).observe(host); else window.addEventListener('resize', fit);
    if (reduce) {
      STEPS.slice(0, 4).forEach(function (s) { s[1](model); }); model.hot = null; model.notice = false; render();
    } else if (window.IntersectionObserver) {
      new IntersectionObserver(function (es) { play(es[0].isIntersecting && !document.hidden); }, { threshold: 0.25 }).observe(host);
    } else play(true);
  }

  // ---- How it works: arrows between the three cards, recomputed on resize. ----
  var flow = document.getElementById('flow');
  if (flow) {
    var fsvg = document.createElementNS(NS, 'svg'); fsvg.setAttribute('class', 'edges'); flow.appendChild(fsvg);
    var drawFlow = function () {
      while (fsvg.firstChild) fsvg.removeChild(fsvg.firstChild);
      var cs = flow.querySelectorAll('.card'), r0 = flow.getBoundingClientRect();
      for (var i = 0; i + 1 < cs.length; i++) {
        var a = cs[i].getBoundingClientRect(), b = cs[i + 1].getBoundingClientRect();
        var vertical = b.top > a.bottom - 4;
        if (vertical) edge(fsvg, { x: a.left + a.width / 2 - r0.left, y: a.bottom - r0.top }, { x: b.left + b.width / 2 - r0.left, y: b.top - r0.top }, 'done', true);
        else edge(fsvg, { x: a.right - r0.left, y: a.top + 30 - r0.top }, { x: b.left - r0.left, y: b.top + 30 - r0.top }, i === 0 ? 'done' : '');
      }
    };
    drawFlow();
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(drawFlow);
    if (window.ResizeObserver) new ResizeObserver(drawFlow).observe(flow); else window.addEventListener('resize', drawFlow);
  }

  // ---- Feature tabs (WAI-ARIA tabs pattern). ----
  var tabs = Array.prototype.slice.call(document.querySelectorAll('[role=tab]'));
  function select(tab, focus) {
    tabs.forEach(function (t) {
      var on = t === tab;
      t.setAttribute('aria-selected', on); t.tabIndex = on ? 0 : -1;
      document.getElementById(t.getAttribute('aria-controls')).hidden = !on;
    });
    if (focus) tab.focus();
  }
  tabs.forEach(function (t, i) {
    t.addEventListener('click', function () { select(t); });
    t.addEventListener('keydown', function (e) {
      var j = e.key === 'ArrowRight' ? i + 1 : e.key === 'ArrowLeft' ? i - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? tabs.length - 1 : -1;
      if (j < 0) return;
      e.preventDefault(); select(tabs[(j + tabs.length) % tabs.length], true);
    });
  });

  // ---- Top bar: mark the section in view as the active tab. ----
  var links = Array.prototype.slice.call(document.querySelectorAll('.tabsnav a'));
  if (links.length && window.IntersectionObserver) {
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) {
        if (!e.isIntersecting) return;
        links.forEach(function (a) { a.classList.toggle('on', a.getAttribute('href') === '#' + e.target.id); });
      });
    }, { rootMargin: '-45% 0px -50% 0px' });
    links.forEach(function (a) { var s = document.querySelector(a.getAttribute('href')); if (s) io.observe(s); });
  }

  // ---- Downloads: pick the asset for the visitor's OS from the GitHub releases API. ----
  var ua = navigator.userAgent;
  var os = /android|iphone|ipad|ipod/i.test(ua) ? null : /windows/i.test(ua) ? 'win' : /mac/i.test(ua) ? 'mac' : /linux|x11/i.test(ua) ? 'linux' : null;
  var NAMES = { win: 'Windows', mac: 'macOS', linux: 'Linux' };
  var PATTERNS = { win: [/-setup\.exe$/i, /\.msi$/i], mac: [/\.dmg$/i], linux: [/\.AppImage$/i, /\.deb$/i] };
  var mainBtn = document.getElementById('dl-main'), also = document.getElementById('also');
  if (os) { var me = document.querySelector('[data-card="' + os + '"]'); if (me) me.classList.add('me'); }

  function pick(rel, key) {
    var assets = rel.assets || [];
    for (var p = 0; p < PATTERNS[key].length; p++) {
      for (var i = 0; i < assets.length; i++) {
        var u = assets[i].browser_download_url;
        if (PATTERNS[key][p].test(assets[i].name) && /^https:\/\/github\.com\//.test(u)) return u;
      }
    }
    return null;
  }

  fetch('https://api.github.com/repos/XxArDy/Pathloom/releases?per_page=5', { headers: { Accept: 'application/vnd.github+json' } })
    .then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); })
    .then(function (list) {
      var rel = list.filter(function (r) { return !r.draft && (r.assets || []).length; })[0];
      if (!rel) return;
      var urls = {};
      Object.keys(NAMES).forEach(function (k) { urls[k] = pick(rel, k); });
      var ver = rel.tag_name + (rel.prerelease ? ' ' + T.pre : '');
      document.querySelectorAll('[data-os]').forEach(function (a) { var u = urls[a.dataset.os]; if (u) a.href = u; });
      document.querySelectorAll('[data-ver]').forEach(function (el) { el.textContent = ver; });
      if (os && urls[os] && mainBtn) {
        mainBtn.href = urls[os];
        mainBtn.querySelector('span').textContent = T.dl.replace('{os}', NAMES[os]);
        also.textContent = ver + ' · ' + T.also + ' ';
        Object.keys(NAMES).filter(function (k) { return k !== os; }).forEach(function (k, i) {
          if (i) also.appendChild(document.createTextNode(' · '));
          var a = document.createElement('a'); a.textContent = NAMES[k]; a.href = urls[k] || REPO; also.appendChild(a);
        });
      }
    })
    .catch(function () { /* keep the static links to the releases page */ });
})();
