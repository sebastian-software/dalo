/* Dalo site: progressive enhancement only.
   Without JS the page is fully visible; this just adds reveal + active nav. */
document.documentElement.classList.add("js");

(function () {
  "use strict";

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var revealEls = document.querySelectorAll(".reveal");

  // Reveal-on-scroll. Falls back to fully visible when unsupported or reduced.
  if (reduceMotion || !("IntersectionObserver" in window)) {
    revealEls.forEach(function (el) { el.classList.add("is-visible"); });
  } else {
    var revealObserver = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-visible");
          revealObserver.unobserve(entry.target);
        }
      });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.08 });
    revealEls.forEach(function (el) { revealObserver.observe(el); });
  }

  // Active-section highlight in the header nav.
  var navLinks = Array.prototype.slice.call(document.querySelectorAll(".site-nav a"));
  var sections = navLinks
    .map(function (a) { return document.querySelector(a.getAttribute("href")); })
    .filter(Boolean);

  if ("IntersectionObserver" in window && sections.length) {
    var navObserver = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          var id = entry.target.getAttribute("id");
          navLinks.forEach(function (a) {
            a.setAttribute("aria-current", a.getAttribute("href") === "#" + id ? "true" : "false");
          });
        }
      });
    }, { rootMargin: "-45% 0px -50% 0px" });
    sections.forEach(function (s) { navObserver.observe(s); });
  }

  function commandTextFrom(target) {
    var raw = target.getAttribute("data-copy-text") || target.textContent || "";
    var lines = raw.split(/\r?\n/).map(function (line) { return line.trim(); }).filter(Boolean);
    var commands = lines
      .filter(function (line) { return line.indexOf("$") === 0; })
      .map(function (line) { return line.replace(/^\$\s*/, ""); });
    return (commands.length ? commands : lines).join("\n");
  }

  function fallbackCopy(text) {
    var area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.top = "-999px";
    document.body.appendChild(area);
    area.select();
    document.execCommand("copy");
    document.body.removeChild(area);
  }

  function setCopyState(button, state) {
    var label = button.querySelector("span");
    if (!button.dataset.copyLabel && label) {
      button.dataset.copyLabel = label.textContent;
    }
    button.dataset.copied = state ? "true" : "false";
    if (label) {
      label.textContent = state ? "Copied" : button.dataset.copyLabel;
    }
  }

  document.querySelectorAll("[data-copy-target]").forEach(function (button) {
    button.addEventListener("click", function () {
      var target = document.getElementById(button.getAttribute("data-copy-target"));
      if (!target) return;
      var text = commandTextFrom(target);
      var copy = navigator.clipboard && window.isSecureContext
        ? navigator.clipboard.writeText(text)
        : Promise.resolve(fallbackCopy(text));

      copy.then(function () {
        setCopyState(button, true);
        window.setTimeout(function () { setCopyState(button, false); }, 1600);
      }).catch(function () {
        setCopyState(button, false);
      });
    });
  });

  document.querySelectorAll(".mobile-nav a").forEach(function (link) {
    link.addEventListener("click", function () {
      var menu = link.closest("details");
      if (menu) menu.removeAttribute("open");
    });
  });

  var castTarget = document.getElementById("quickstart-cast");
  if (castTarget && window.AsciinemaPlayer) {
    window.AsciinemaPlayer.create("/assets/dalo-quickstart.cast", castTarget, {
      cols: 80,
      rows: 15,
      theme: "asciinema",
      preload: true
    });
  }
})();
