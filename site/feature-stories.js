/* Each illustration explains one boundary. Decisions in these examples are
   local visual controls: they never call Dalo, install skills, or change files. */
(function () {
  "use strict";
  var roots = Array.from(document.querySelectorAll("[data-feature-demo]"));
  var disposers = [];

  function mount(root) {
    var kind = root.dataset.featureDemo;
    var scene = root.querySelector("[data-feature-scene]");
    var control = root.querySelector("[data-feature-control]");
    var action = root.querySelector("[data-feature-action]");
    var status = root.querySelector("[data-feature-status]");
    var svg = root.querySelector("[data-feature-paths]");
    var packets = Array.from(root.querySelectorAll("[data-feature-packet]"));
    var owned = Array.from(root.querySelectorAll("[data-owned-skill]"));
    var media = window.matchMedia("(prefers-reduced-motion: reduce)");
    var supported = typeof scene.animate === "function";
    var name = { pins: "pinned versions", review: "catalog approval", ownership: "file ownership" }[kind];
    var animations = [];
    var master;
    var timer;
    var generation = 0;
    var phase = "waiting";
    var manualPause = false;
    var visible = false;
    var canStart = false;
    var disposed = false;
    var played = root.dataset.featureStarted === "true";
    var complete = root.dataset.featureCompleted === "true";
    var previousSize = "";
    var duration;
    var checkpoints = [];
    var paths = [];
    var shift = 0;
    var observer;
    var resizeObserver;

    function write(element, value) {
      if (element.textContent !== value) element.textContent = value;
    }
    function versions(value) {
      root.querySelectorAll("[data-feature-version]").forEach(function (element) { write(element, value); });
    }
    function pinState(advanced, delivered) {
      root.dataset.result = advanced ? "updated" : "pending";
      versions(delivered ? "v2" : "v1");
      if (advanced) write(root.querySelector(".pin-team-set [data-feature-version]"), "v2");
      write(root.querySelector("[data-pin-label=previous]"), advanced ? "Previous team pin" : "Current team pin");
      write(root.querySelector("[data-pin-label=next]"), advanced ? "Chosen team pin" : "Available update");
      write(root.querySelector("[data-pin-new-label]"), advanced ? "Chosen" : "New");
    }
    function reviewState(approved, linked) {
      root.dataset.approved = String(approved);
      root.dataset.linked = String(linked);
      var badge = root.querySelector(".approval-person .status");
      badge.className = "status" + (approved ? "" : " pending");
      write(badge, approved ? "approved by you" : "review first");
      write(root.querySelector("[data-review-target-label]"), linked ? "Skill linked" : approved ? "Linking skill…" : "Not linked yet");
    }
    function ownState(index, updated) {
      owned[index].dataset.result = updated ? "updated" : "ready";
      write(owned[index].querySelector("[data-owned-result]"), updated ? "Updated" : "Ready to sync");
    }
    function snapshot(done) {
      complete = done;
      root.dataset.featureCompleted = String(done);
      if (kind === "pins") {
        pinState(done, done);
        write(status, done ? "You advanced the pin. Both teammates now use v2." : "v2 is available. Your team stays on v1.");
      } else if (kind === "review") {
        reviewState(done, done);
        write(status, done ? "You approved this example. The skill can now reach Codex." : "Checks do not approve a skill for you.");
      } else {
        owned.forEach(function (item, i) { ownState(i, done); });
        write(status, done ? "Two managed links updated. Your files are unchanged." : "Sync updates only the links Dalo manages.");
      }
    }
    function paused() { return manualPause || !visible || document.hidden; }
    function active() { return phase === "intro" || phase === "delivery"; }
    function controls() {
      control.hidden = !supported || media.matches;
      var label = active() ? paused() ? "Resume" : "Pause" : "Replay";
      write(control.querySelector("span"), label);
      control.setAttribute("aria-label", label + " " + name + " example");
      control.querySelector("path").setAttribute("d", label === "Pause" ? "M5 3v10M11 3v10" : label === "Resume" ? "M5 3l7 5-7 5Z" : "M3 5a5 5 0 1 1-1 5M3 1v4h4");
      root.dataset.featurePhase = phase;
      root.dataset.motionState = active() ? paused() ? "paused" : "playing" : phase;
      if (action) {
        action.hidden = false;
        var busy = phase === "delivery";
        action.setAttribute("aria-disabled", String(busy));
        write(action, busy ? kind === "pins" ? "Updating example…" : "Linking example…"
          : complete ? "Reset example ↺" : kind === "pins" ? "Update example ↗" : "Approve example →");
      }
    }
    function stop() {
      generation++;
      clearTimeout(timer);
      timer = undefined;
      animations.forEach(function (animation) { animation.cancel(); });
      animations = [];
      master = undefined;
    }
    function finish(done) {
      stop();
      manualPause = false;
      phase = done ? "complete" : "waiting";
      snapshot(done);
      controls();
    }
    function point(element, x, y) {
      var base = scene.getBoundingClientRect();
      var rect = element.getBoundingClientRect();
      return { x: rect.left - base.left + rect.width*x, y: rect.top - base.top + rect.height*y };
    }
    function horizontal(a, b) {
      var mid = (a.x + b.x)/2;
      return { a: a, b: b, c: { x: mid, y: a.y }, d: { x: mid, y: b.y } };
    }
    function draw() {
      paths = [];
      if (kind === "pins") {
        shift = point(root.querySelector("[data-pin-to]"), .5, 0).x - point(root.querySelector("[data-pin-from]"), .5, 0).x;
        root.style.setProperty("--pin-shift", shift + "px");
        return;
      }
      svg.replaceChildren();
      svg.setAttribute("viewBox", "0 0 " + scene.clientWidth + " " + scene.clientHeight);
      if (kind === "review") {
        var source = root.querySelector("[data-review-source]");
        var gate = root.querySelector("[data-review-gate]");
        var target = root.querySelector("[data-review-target]");
        paths = [horizontal(point(source, 1, .5), point(gate, .5, .3)), horizontal(point(gate, .5, .3), point(target, 0, .5))];
      } else {
        var origin = point(root.querySelector("[data-own-source]"), 1, .5);
        paths = owned.map(function (item) {
          var destination = point(item, 1, .5);
          var edge = scene.clientWidth + 6;
          return { a: origin, b: destination, c: { x: edge, y: origin.y }, d: { x: edge, y: destination.y } };
        });
      }
      paths.forEach(function (path) {
        var element = document.createElementNS("http://www.w3.org/2000/svg", "path");
        element.setAttribute("d", "M" + path.a.x + " " + path.a.y + " C" + path.c.x + " " + path.c.y + " " + path.d.x + " " + path.d.y + " " + path.b.x + " " + path.b.y);
        svg.appendChild(element);
      });
    }
    function animate(element, frames) {
      var animation = element.animate(frames, { duration: duration, fill: "both", easing: "linear" });
      animations.push(animation);
      return animation;
    }
    function fly(packet, path, begin, end) {
      var frames = [{ offset: 0, opacity: 0, transform: "translate(" + path.a.x + "px," + path.a.y + "px) scale(.7)" }];
      for (var i = 0; i <= 24; i++) {
        var t = i/24;
        var v = t*t*(3 - 2*t);
        var u = 1 - v;
        var x = u*u*u*path.a.x + 3*u*u*v*path.c.x + 3*u*v*v*path.d.x + v*v*v*path.b.x;
        var y = u*u*u*path.a.y + 3*u*u*v*path.c.y + 3*u*v*v*path.d.y + v*v*v*path.b.y;
        frames.push({ offset: begin + (end - begin)*t, opacity: i === 0 || i === 24 ? 0 : 1,
          transform: "translate(" + x + "px," + y + "px) rotate(" + (-8 + 16*t) + "deg) scale(" + (.85 + Math.sin(t*Math.PI)*.25) + ")" });
      }
      frames.push({ offset: 1, opacity: 0, transform: "translate(" + path.b.x + "px," + path.b.y + "px) scale(.7)" });
      animate(packet, frames);
    }
    function update(time) {
      if (phase !== "delivery") return;
      if (kind === "pins") {
        var advanced = time >= 1400;
        // Delivery to the teammates follows the deliberate pin change.
        pinState(advanced, time >= 2200);
        write(status, time >= 2200 ? "Both teammates now use the chosen team version." : advanced ? "The pin is now v2. Updating the team set…" : "You chose to advance the team pin.");
      } else if (kind === "review") {
        reviewState(true, time >= 1800);
        write(status, time >= 1800 ? "The approved skill is linked into Codex." : "You approved this example. Linking the skill…");
      } else {
        ownState(0, time >= 1300);
        ownState(1, time >= 2300);
        write(status, time >= 2300 ? "Two managed links updated. Your files are unchanged." : "Updating managed links. Your own folder stays unchanged.");
      }
    }
    function schedule() {
      clearTimeout(timer);
      var time = master && typeof master.currentTime === "number" ? master.currentTime : 0;
      update(time);
      if (!active() || paused() || disposed) return;
      var next = checkpoints.find(function (checkpoint) { return checkpoint > time; });
      if (next !== undefined) timer = setTimeout(schedule, Math.max(0, next - time) + 20);
    }
    function syncPlayback() {
      if (disposed) return;
      if (media.matches || !supported) {
        if (active()) finish(kind === "ownership" || phase === "delivery");
        controls();
        return;
      }
      if (!played && canStart && !document.hidden) { intro(); return; }
      if (active()) {
        animations.forEach(function (animation) { if (paused()) animation.pause(); else animation.play(); });
        schedule();
      }
      controls();
    }
    function timeline(length, boundaries, build, done) {
      stop();
      duration = length;
      checkpoints = boundaries;
      var run = generation;
      draw();
      build();
      master = animate(status, [{ opacity: 1 }, { opacity: 1 }]);
      master.finished.then(function () { if (!disposed && run === generation) finish(done); }).catch(function () {});
      syncPlayback();
    }
    function delivery() {
      if (disposed) return;
      if (media.matches || !supported) { finish(true); return; }
      manualPause = false;
      played = true;
      root.dataset.featureStarted = "true";
      phase = "delivery";
      if (kind === "review") reviewState(true, false);
      var length = kind === "pins" ? 3400 : kind === "review" ? 2600 : 3200;
      var boundaries = kind === "pins" ? [1400, 2200] : kind === "review" ? [1800] : [1300, 2300];
      timeline(length, boundaries, function () {
        if (kind === "pins") {
          animate(root.querySelector("[data-pin-pointer]"), [
            { offset: 0, transform: "translateX(0) translateY(0)" },
            { offset: .12, transform: "translateX(0) translateY(-10px)" },
            { offset: 1400/length, transform: "translateX(" + shift + "px) translateY(0)" },
            { offset: 1, transform: "translateX(" + shift + "px) translateY(0)" }
          ]);
          root.querySelectorAll("[data-pin-recipient]").forEach(function (item) {
            animate(item, [{ transform: "translateY(0)" }, { offset: .6, transform: "translateY(0)" },
              { offset: .7, transform: "translateY(-4px)" }, { offset: .85, transform: "translateY(0)" }, { transform: "translateY(0)" }]);
          });
        } else if (kind === "review") {
          fly(packets[0], paths[1], .16, 1800/length);
          animate(root.querySelector("[data-review-gate]"), [{ transform: "scale(1)" },
            { offset: .12, transform: "scale(1.08)" }, { offset: .24, transform: "scale(1)" }, { transform: "scale(1)" }]);
        } else {
          fly(packets[0], paths[0], .1, 1300/length);
          fly(packets[1], paths[1], .35, 2300/length);
          owned.forEach(function (item, i) {
            var arrival = (i ? 2300 : 1300)/length;
            animate(item.querySelector(".feature-star"), [{ transform: "scale(1)" },
              { offset: arrival, transform: "scale(1)" }, { offset: arrival + .08, transform: "scale(1.12)" },
              { offset: arrival + .16, transform: "scale(1)" }, { transform: "scale(1)" }]);
          });
        }
      }, true);
    }
    function intro() {
      finish(false);
      played = true;
      root.dataset.featureStarted = "true";
      if (kind === "ownership") { delivery(); return; }
      if (media.matches || !supported) return;
      phase = "intro";
      timeline(1600, [], function () {
        if (kind === "pins") animate(root.querySelector("[data-pin-to]"), [{ opacity: .25, transform: "translateY(8px)" },
          { offset: .7, opacity: 1, transform: "translateY(0)" }, { opacity: 1, transform: "translateY(0)" }]);
        else fly(packets[0], paths[0], .1, .85);
      }, false);
    }
    function onControl() {
      if (!active()) intro();
      else { manualPause = !manualPause; syncPlayback(); }
    }
    function onAction() {
      if (phase === "delivery") return;
      if (complete) intro(); else delivery();
    }
    function onResize() {
      var size = scene.clientWidth + ":" + scene.clientHeight;
      if (size === previousSize || disposed) return;
      if (previousSize && active()) finish(phase === "delivery");
      previousSize = size;
      draw();
    }
    control.addEventListener("click", onControl);
    if (action) action.addEventListener("click", onAction);
    document.addEventListener("visibilitychange", syncPlayback);
    media.addEventListener("change", syncPlayback);
    finish(complete || (kind === "ownership" && (media.matches || !supported)));
    onResize();
    if ("ResizeObserver" in window) {
      resizeObserver = new ResizeObserver(onResize);
      resizeObserver.observe(scene);
    } else window.addEventListener("resize", onResize);
    if ("IntersectionObserver" in window) {
      observer = new IntersectionObserver(function (entries) {
        visible = entries[0].isIntersecting;
        canStart = entries[0].intersectionRatio >= .35;
        syncPlayback();
      }, { threshold: [0, .35] });
      observer.observe(root);
    } else { visible = true; canStart = true; syncPlayback(); }
    return function () {
      disposed = true;
      finish(complete || (phase === "delivery"));
      control.hidden = true;
      if (action) action.hidden = true;
      control.removeEventListener("click", onControl);
      if (action) action.removeEventListener("click", onAction);
      document.removeEventListener("visibilitychange", syncPlayback);
      media.removeEventListener("change", syncPlayback);
      window.removeEventListener("resize", onResize);
      if (observer) observer.disconnect();
      if (resizeObserver) resizeObserver.disconnect();
    };
  }
  function mountAll() { disposers = roots.map(mount); }
  mountAll();
  window.addEventListener("pagehide", function () { disposers.forEach(function (dispose) { dispose(); }); disposers = []; });
  window.addEventListener("pageshow", function () { if (!disposers.length) mountAll(); });
})();
