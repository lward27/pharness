// PHarness operator tutorial — small page behaviors.
// 1. "Copy" buttons on every <pre> block.
// 2. Active sidebar link highlighting via hash + section scroll position.

(function () {
  "use strict";

  // ---------- copy buttons ----------
  document.querySelectorAll("pre").forEach(function (pre) {
    var btn = document.createElement("button");
    btn.className = "copy-btn";
    btn.type = "button";
    btn.textContent = "Copy";

    var timer = null;
    btn.addEventListener("click", function () {
      var code = pre.querySelector("code");
      var text = (code ? code.textContent : pre.textContent).trim();
      var done = function () {
        btn.textContent = "Copied";
        btn.classList.add("done");
        if (timer) clearTimeout(timer);
        timer = setTimeout(function () {
          btn.textContent = "Copy";
          btn.classList.remove("done");
        }, 1500);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, function () {
          fallbackCopy(text);
          done();
        });
      } else {
        fallbackCopy(text);
        done();
      }
    });

    pre.appendChild(btn);
  });

  function fallbackCopy(text) {
    var ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    try { document.execCommand("copy"); } catch (e) { /* ignore */ }
    document.body.removeChild(ta);
  }

  // ---------- sidebar active state ----------
  var links = Array.prototype.slice.call(
    document.querySelectorAll(".sidebar nav a")
  );

  function currentFile() {
    return location.pathname.split("/").pop() || "index.html";
  }

  function highlight() {
    var file = currentFile();
    var hash = location.hash.replace("#", "");
    var active = null;

    for (var i = 0; i < links.length; i++) {
      var a = links[i];
      var href = a.getAttribute("href") || "";
      var m = href.match(/^([^#]*)(?:#([^#]*))?$/);
      var linkFile = (m && m[1]) ? m[1] : file;
      var linkHash = m ? m[2] : null;

      a.classList.remove("active");
      if (linkFile !== file) continue;

      if (hash) {
        // match exact hash, or a link whose section contains the hash target
        var target = document.getElementById(hash);
        if (linkHash === hash) { active = a; break; }
        if (target && a.contains && a.contains(target)) { active = a; break; }
      }
    }

    if (!active && file === currentFile()) {
      // no hash: first link pointing at this file is the default
      for (var j = 0; j < links.length; j++) {
        var h = links[j].getAttribute("href") || "";
        if (h.split("#")[0] === file) { active = links[j]; break; }
      }
    }

    if (active) active.classList.add("active");
  }

  window.addEventListener("hashchange", highlight);
  window.addEventListener("load", highlight);
  highlight();
})();
