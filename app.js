// Resolve the newest release assets from the GitHub API so the download
// buttons keep working after the version number inside file names changes.
// Without JavaScript every button already points at the /releases/latest page.
(function () {
  var REPO = "hushmind3/dominion-rust";
  var API = "https://api.github.com/repos/" + REPO + "/releases/latest";

  function pick(assets, rules) {
    for (var i = 0; i < rules.length; i++) {
      var match = assets.filter(rules[i]);
      if (match.length) return match[0];
    }
    return null;
  }

  function apply(assets) {
    var map = {
      "windows-exe": pick(assets, [
        function (a) { return /\.exe$/i.test(a.name) && /setup/i.test(a.name); },
        function (a) { return /\.exe$/i.test(a.name); }
      ]),
      "windows-msi-ko": pick(assets, [
        function (a) { return /\.msi$/i.test(a.name) && /ko[-_]?KR/i.test(a.name); }
      ]),
      "windows-msi": pick(assets, [
        function (a) { return /\.msi$/i.test(a.name) && !/ko[-_]?KR/i.test(a.name); },
        function (a) { return /\.msi$/i.test(a.name); }
      ]),
      "mac-arm": pick(assets, [
        function (a) { return /\.dmg$/i.test(a.name) && /(aarch64|arm64|apple)/i.test(a.name); }
      ]),
      "mac-intel": pick(assets, [
        function (a) { return /\.dmg$/i.test(a.name) && /(x64|x86_64|intel)/i.test(a.name); }
      ])
    };

    var links = document.querySelectorAll("[data-asset]");
    for (var i = 0; i < links.length; i++) {
      var asset = map[links[i].getAttribute("data-asset")];
      if (asset) {
        links[i].setAttribute("href", asset.browser_download_url);
        links[i].setAttribute("title", asset.name + " 내려받기");
      }
    }
  }

  function setVersion(text) {
    ["version", "version-2"].forEach(function (id) {
      var el = document.getElementById(id);
      if (el) el.textContent = text;
    });
    // Keep the structured data in step with the release it points at.
    var ld = document.querySelector('script[type="application/ld+json"]');
    if (!ld) return;
    try {
      var data = JSON.parse(ld.textContent);
      data.softwareVersion = String(text).replace(/^v/i, "");
      ld.textContent = JSON.stringify(data, null, 2);
    } catch (e) {
      /* leave the static JSON-LD as it is */
    }
  }

  fetch(API, { headers: { Accept: "application/vnd.github+json" } })
    .then(function (res) {
      if (!res.ok) throw new Error("HTTP " + res.status);
      return res.json();
    })
    .then(function (release) {
      if (!release || !release.tag_name) return;
      setVersion(release.tag_name);
      if (Array.isArray(release.assets)) apply(release.assets);
    })
    .catch(function () {
      // Keep the static /releases/latest links and the pre-rendered version.
    });
})();
