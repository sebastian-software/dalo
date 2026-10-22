/* Share the team's own skills: choose a set, connect its repository, then sync.
   Default HTML shows the complete result; motion explains only these steps. */
(function () {
  "use strict";
  var root = document.querySelector("[data-team-demo]");
  if (!root) return;

  function mount() {
    var scene = root.querySelector("[data-team-scene]");
    var svg = root.querySelector("[data-team-paths]");
    var repository = root.querySelector("[data-team-repository]");
    var sync = root.querySelector("[data-team-sync]");
    var inputs = Array.from(root.querySelectorAll("[data-team-input]"));
    var machines = Array.from(root.querySelectorAll("[data-team-machine]"));
    var packets = Array.from(root.querySelectorAll("[data-team-packet]"));
    var chapters = Array.from(root.querySelectorAll("[data-team-chapter]"));
    // Every recipient receives every selected skill. Colors belong to the
    // source skills, never to a recipient or an agent.
    var deliveries = machines.map(function (machine, i) {
      var start = 3600 + i*300;
      var skills = inputs.map(function (input, j) {
        return { start: start + j*420, end: start + j*420 + 1400,
          skill: input.dataset.teamSkill, color: input.dataset.teamColor };
      });
      return { start: start, end: Math.max.apply(null, skills.map(function (skill) { return skill.end; })), skills: skills };
    });
    var connection = root.querySelector("[data-team-connected]");
    var control = root.querySelector("[data-team-control]");
    var media = window.matchMedia("(prefers-reduced-motion: reduce)");
    var supported = typeof sync.animate === "function";
    var duration = 8000;
    var checkpoints = [1400, 3200].concat(deliveries.flatMap(function (delivery) {
      return [delivery.start, delivery.end];
    })).sort(function (a, b) { return a - b; });
    var animations = [];
    var master;
    var timer;
    var generation = 0;
    var finished = true;
    var manualPause = false;
    var visible = false;
    var disposed = false;
    var played = root.dataset.teamStarted === "true";
    var previousSize = "";
    var observer;
    var resizeObserver;
    var namespace = "http://www.w3.org/2000/svg";

    function point(element, x, y) {
      var base = scene.getBoundingClientRect();
      var rect = element.getBoundingClientRect();
      return { x: rect.left - base.left + rect.width*x, y: rect.top - base.top + rect.height*y };
    }
    function curve(a, b) {
      var horizontal = Math.abs(b.x - a.x) > Math.abs(b.y - a.y);
      var mid = horizontal ? (a.x + b.x)/2 : (a.y + b.y)/2;
      return { a: a, b: b, c: horizontal ? { x: mid, y: a.y } : { x: a.x, y: mid },
        d: horizontal ? { x: mid, y: b.y } : { x: b.x, y: mid } };
    }
    function coordinate(route, t) {
      var u = 1 - t;
      return { x: u*u*u*route.a.x + 3*u*u*t*route.c.x + 3*u*t*t*route.d.x + t*t*t*route.b.x,
        y: u*u*u*route.a.y + 3*u*u*t*route.c.y + 3*u*t*t*route.d.y + t*t*t*route.b.y };
    }
    function route(from, to) {
      var horizontal = window.matchMedia("(min-width: 821px)").matches;
      if (horizontal) return curve(point(from, 1, .5), point(to, 0, .5));
      // Keep the mobile path beside the reading column, away from the labels.
      var a = point(from, 1, .5);
      var b = point(to, 1, .5);
      var edge = scene.clientWidth + 3;
      return { a: a, b: b, c: { x: edge, y: a.y }, d: { x: edge, y: b.y } };
    }
    function drawPath(path, className) {
      var element = document.createElementNS(namespace, "path");
      element.setAttribute("d", "M" + path.a.x + " " + path.a.y + " C" + path.c.x + " " + path.c.y + " " + path.d.x + " " + path.d.y + " " + path.b.x + " " + path.b.y);
      element.setAttribute("class", className);
      svg.appendChild(element);
    }
    function drawPaths() {
      svg.replaceChildren();
      svg.setAttribute("viewBox", "0 0 " + scene.clientWidth + " " + scene.clientHeight);
      drawPath(route(repository, sync), "team-path-source");
      machines.forEach(function (machine) {
        drawPath(route(sync, machine), "team-path-sync");
      });
    }
    function paused() { return manualPause || !visible || document.hidden; }
    function buttonState() {
      control.hidden = !supported || media.matches;
      var label = finished ? "Replay" : paused() ? "Resume" : "Pause";
      control.querySelector("span").textContent = label;
      control.setAttribute("aria-label", label + " team repository animation");
      control.querySelector("path").setAttribute("d", label === "Pause" ? "M5 3v10M11 3v10" : label === "Resume" ? "M5 3l7 5-7 5Z" : "M3 5a5 5 0 1 1-1 5M3 1v4h4");
      root.dataset.teamState = finished ? "finished" : paused() ? "paused" : "playing";
      root.dataset.playing = String(!finished);
    }
    function writeText(element, value) {
      if (element.textContent !== value) element.textContent = value;
    }
    function updateStory(time) {
      var connected = time >= 3200;
      var chapter = time < 1400 ? 0 : !connected ? 1 : 2;
      root.dataset.phase = finished ? "complete" : ["choose", "connect", "sync"][chapter];
      chapters.forEach(function (item, i) {
        if (!finished && i === chapter) item.setAttribute("aria-current", "step");
        else item.removeAttribute("aria-current");
      });
      writeText(connection, connected ? "Repository connected" : "Connect once on your machine");
      connection.dataset.connected = String(connected);
      machines.forEach(function (machine, i) {
        var delivery = deliveries[i];
        var progress = Math.max(0, Math.min(1, (time - delivery.start)/(delivery.end - delivery.start)));
        var indicator = machine.querySelector("[data-team-indicator]");
        var mode = time >= delivery.end ? "complete" : time >= delivery.start ? "syncing" : "waiting";
        indicator.dataset.mode = mode;
        indicator.setAttribute("aria-valuenow", String(Math.round(progress*100)));
        indicator.setAttribute("aria-valuetext", mode === "complete" ? "Team skills synced"
          : mode === "syncing" ? "Synchronizing team skills" : "Ready to sync");
      });
    }
    function cancelTimer() { clearTimeout(timer); timer = undefined; }
    function scheduleStory() {
      cancelTimer();
      var time = master && typeof master.currentTime === "number" ? master.currentTime : 0;
      updateStory(time);
      if (finished || paused() || disposed) return;
      var next = checkpoints.find(function (checkpoint) { return checkpoint > time; });
      // Keep accessible progress values in step with the native ring animation.
      // Only active progress needs short updates; there is no per-frame loop.
      var syncing = deliveries.some(function (delivery) { return time >= delivery.start && time < delivery.end; });
      var delay = next !== undefined ? Math.max(0, next - time) + 30 : Infinity;
      if (syncing) delay = Math.min(delay, 250);
      if (isFinite(delay)) timer = setTimeout(scheduleStory, delay);
    }
    function settle() {
      generation++;
      cancelTimer();
      animations.forEach(function (animation) { animation.cancel(); });
      animations = [];
      master = undefined;
      finished = true;
      manualPause = false;
      updateStory(duration);
      buttonState();
    }
    function animate(element, frames) {
      var animation = element.animate(frames, { duration: duration, fill: "both", easing: "linear" });
      animations.push(animation);
      return animation;
    }
    function fly(packet, path, start, end, color) {
      packet.style.background = color;
      var frames = [{ offset: 0, opacity: 0, transform: "translate(" + path.a.x + "px," + path.a.y + "px) scale(.6)" }];
      for (var i = 0; i <= 20; i++) {
        var t = i/20;
        var p = coordinate(path, t*t*(3 - 2*t));
        frames.push({ offset: start + (end - start)*t, opacity: i === 0 || i === 20 ? 0 : 1,
          transform: "translate(" + p.x + "px," + p.y + "px) rotate(" + (-10 + t*20) + "deg) scale(" + (.75 + Math.sin(t*Math.PI)*.4) + ")" });
      }
      frames.push({ offset: 1, opacity: 0, transform: "translate(" + path.b.x + "px," + path.b.y + "px) scale(.6)" });
      animate(packet, frames);
    }
    function syncPlayback() {
      if (disposed) return;
      if (media.matches) { settle(); return; }
      if (!played && visible && !document.hidden) { start(); return; }
      if (!finished) {
        animations.forEach(function (animation) { if (paused()) animation.pause(); else animation.play(); });
        scheduleStory();
      }
      buttonState();
    }
    function start() {
      if (!supported || media.matches || disposed) return;
      settle();
      played = true;
      root.dataset.teamStarted = "true";
      finished = false;
      drawPaths();
      var run = generation;
      inputs.forEach(function (input, i) {
        animate(input, [{ transform: "translateY(5px)" },
          { offset: .1 + i*.04, transform: "translateY(0)" },
          { transform: "translateY(0)" }]);
        fly(packets[i], route(input, sync),
          .18 + i*.04, .36 + i*.04, input.dataset.teamColor);
      });
      machines.forEach(function (machine, i) {
        var begin = deliveries[i].start/duration;
        var end = deliveries[i].end/duration;
        var indicator = machine.querySelector("[data-team-indicator]");
        deliveries[i].skills.forEach(function (skill, j) {
          var packet = packets[inputs.length + i*inputs.length + j];
          packet.dataset.skill = skill.skill;
          packet.dataset.recipient = machine.querySelector("h4").textContent;
          fly(packet, route(sync, machine), skill.start/duration, skill.end/duration, skill.color);
        });
        animate(indicator.querySelector("[data-team-progress]"), [
          { offset: 0, strokeDashoffset: "100" }, { offset: begin, strokeDashoffset: "100" },
          { offset: end, strokeDashoffset: "0" }, { offset: 1, strokeDashoffset: "0" }
        ]);
        animate(indicator.querySelector(".team-progress-ring"), [
          { offset: 0, opacity: 1 }, { offset: end, opacity: 1 },
          { offset: end + .045, opacity: 0 }, { offset: 1, opacity: 0 }
        ]);
        animate(indicator.querySelector(".team-progress-check"), [
          { offset: 0, opacity: 0, transform: "scale(.7)" },
          { offset: end, opacity: 0, transform: "scale(.7)" },
          { offset: end + .045, opacity: 1, transform: "scale(1)" },
          { offset: 1, opacity: 1, transform: "scale(1)" }
        ]);
      });
      // Only the connection label moves; the original logo stays unchanged.
      master = animate(connection, [{ transform: "translateY(0)" },
        { offset: .36, transform: "translateY(0)" },
        { offset: .4, transform: "translateY(-3px)" },
        { offset: .44, transform: "translateY(0)" }, { transform: "translateY(0)" }]);
      master.finished.then(function () {
        if (!disposed && run === generation) settle();
      }).catch(function () { /* Cancellation restores the complete result. */ });
      syncPlayback();
    }
    function onControl() {
      if (finished) start();
      else { manualPause = !manualPause; syncPlayback(); }
    }
    function onResize() {
      if (disposed) return;
      var size = scene.clientWidth + ":" + scene.clientHeight;
      if (size === previousSize) return;
      if (previousSize && !finished) settle();
      previousSize = size;
      drawPaths();
    }
    control.addEventListener("click", onControl);
    document.addEventListener("visibilitychange", syncPlayback);
    media.addEventListener("change", syncPlayback);
    onResize();
    settle();
    if ("ResizeObserver" in window) {
      resizeObserver = new ResizeObserver(onResize);
      resizeObserver.observe(scene);
    } else window.addEventListener("resize", onResize);
    if ("IntersectionObserver" in window) {
      observer = new IntersectionObserver(function (entries) {
        visible = entries[0].isIntersecting && entries[0].intersectionRatio >= .2;
        syncPlayback();
      }, { threshold: .2 });
      observer.observe(scene);
    } else { visible = true; syncPlayback(); }
    return function () {
      disposed = true;
      settle();
      control.hidden = true;
      control.removeEventListener("click", onControl);
      document.removeEventListener("visibilitychange", syncPlayback);
      media.removeEventListener("change", syncPlayback);
      window.removeEventListener("resize", onResize);
      if (observer) observer.disconnect();
      if (resizeObserver) resizeObserver.disconnect();
    };
  }
  var dispose = mount();
  window.addEventListener("pagehide", function () { if (dispose) { dispose(); dispose = null; } });
  window.addEventListener("pageshow", function () { if (!dispose) dispose = mount(); });
})();
