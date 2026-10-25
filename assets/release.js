// Reads the newest GitHub release and fills in version and file links.
// If anything fails, the page keeps its normal links to /releases/latest.
(function () {
  var API = 'https://api.github.com/repos/Solo-in-Code/SeedLock/releases/latest';
  fetch(API, { headers: { Accept: 'application/vnd.github+json' } })
    .then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); })
    .then(function (rel) {
      var version = String(rel.tag_name || '').replace(/^v/, '');
      var assets = rel.assets || [];
      var keys = { windows: 'windows', macos: 'macos', linux: 'linux', android: 'android' };
      document.querySelectorAll('.card').forEach(function (card) {
        var tag = card.querySelector('.tag');
        if (!tag) return;
        var os = tag.textContent.replace('//', '').trim().toLowerCase();
        if (!keys[os]) return;
        var asset = assets.filter(function (a) {
          return a.name.toLowerCase().indexOf(keys[os]) !== -1 && !/\.(asc|txt|sig)$/i.test(a.name);
        })[0];
        if (!asset) return;
        var name = card.querySelector('p.mono');
        if (name) name.textContent = asset.name;
        var btn = card.querySelector('a.btn');
        if (btn) btn.href = asset.browser_download_url;
      });
      var info = document.getElementById('release-info');
      if (info) {
        var d = rel.published_at ? new Date(rel.published_at).toISOString().slice(0, 10) : '';
        info.textContent = 'Latest release: ' + rel.tag_name + (d ? ' (' + d + ')' : '');
      }
      if (version) {
        var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
        var n;
        while ((n = walker.nextNode())) {
          if (n.nodeValue.indexOf('<version>') !== -1) n.nodeValue = n.nodeValue.split('<version>').join(version);
        }
      }
    })
    .catch(function () {});
})();
