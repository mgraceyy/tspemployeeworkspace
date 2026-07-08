(function () {
  var el = document.querySelector(".clock-now[data-timezone]");
  if (!el) {
    return;
  }

  var tz = el.getAttribute("data-timezone");
  if (!tz) {
    return;
  }

  function formatNow() {
    try {
      return new Intl.DateTimeFormat("en-US", {
        timeZone: tz,
        hour: "2-digit",
        minute: "2-digit",
        hour12: true,
      }).format(new Date());
    } catch (err) {
      return null;
    }
  }

  function tick() {
    var value = formatNow();
    if (value) {
      el.textContent = value;
    }
  }

  tick();
  setInterval(tick, 1000);
})();