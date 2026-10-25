(function () {
  var C = window.DONATE || {};
  var $ = function (id) { return document.getElementById(id); };
  function qr(el, text) {
    try {
      var q = qrcode(0, 'M'); q.addData(text); q.make();
      el.innerHTML = q.createSvgTag({ cellSize: 4, margin: 0, scalable: true });
      var s = el.querySelector('svg');
      s.style.width = '100%'; s.style.height = 'auto'; s.style.maxWidth = '260px';
      s.setAttribute('role', 'img'); s.setAttribute('aria-label', 'QR code');
    } catch (e) { el.textContent = 'QR code unavailable'; }
  }
  function copy(btn, text) {
    var label = btn.textContent;
    function done() { btn.textContent = 'Copied'; setTimeout(function () { btn.textContent = label; }, 1500); }
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, function () {});
  }
  function soon(prefix, msg) {
    $(prefix + '-qr').style.display = 'none';
    $(prefix + '-text') && ($(prefix + '-text').textContent = msg);
    $(prefix + '-addr') && ($(prefix + '-addr').textContent = msg);
    ['copy', 'next'].forEach(function (k) { var b = $(prefix + '-' + k); if (b) b.style.display = 'none'; });
  }
  // Lightning (BOLT12)
  var offer = (C.bolt12 || '').trim();
  if (!offer) { soon('ln', 'Lightning donations are coming soon.'); var n = $('ln-name'); if (n) n.style.display = 'none'; }
  else {
    qr($('ln-qr'), 'lightning:' + offer);
    $('ln-text').textContent = offer;
    $('ln-copy').addEventListener('click', function () { copy(this, offer); });
    var name = $('ln-name');
    if (name) { if (C.bip353) { name.textContent = '\u20BF' + C.bip353; name.style.display = 'block'; } else name.style.display = 'none'; }
  }
  // On-chain: random address from the list (only if the page has that card)
  if ($('oc-qr')) {
  var list = (C.addresses || []).filter(Boolean), cur = -1;
  function pick() {
    if (list.length < 2) return 0;
    var i, buf = new Uint32Array(1);
    do { crypto.getRandomValues(buf); i = buf[0] % list.length; } while (i === cur);
    return i;
  }
  function show() {
    cur = pick(); var a = list[cur];
    qr($('oc-qr'), 'bitcoin:' + a); $('oc-addr').textContent = a;
  }
  if (!list.length) soon('oc', 'On-chain donations are coming soon.');
  else {
    show();
    $('oc-next').addEventListener('click', show);
    $('oc-copy').addEventListener('click', function () { copy(this, list[cur]); });
  }
  }
})();
