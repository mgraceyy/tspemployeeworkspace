(function () {
  var root = document.getElementById("shift-simple");
  if (!root) {
    return;
  }

  var form = root.closest("form");
  if (!form) {
    return;
  }

  var masterStart = document.getElementById("shift-master-start");
  var masterEnd = document.getElementById("shift-master-end");
  var checkboxes = root.querySelectorAll(".shift-day-checkbox");

  function syncHidden() {
    if (!masterStart || !masterEnd) {
      return;
    }
    var start = masterStart.value;
    var end = masterEnd.value;
    checkboxes.forEach(function (checkbox) {
      var day = checkbox.getAttribute("data-day");
      var startInput = root.querySelector('.shift-input-start[data-day="' + day + '"]');
      var endInput = root.querySelector('.shift-input-end[data-day="' + day + '"]');
      if (!startInput || !endInput) {
        return;
      }
      if (checkbox.checked) {
        startInput.value = start;
        endInput.value = end;
      } else {
        startInput.value = "00:00";
        endInput.value = "00:00";
      }
    });
  }

  form.addEventListener("submit", syncHidden);

  if (masterStart) {
    masterStart.addEventListener("change", syncHidden);
  }
  if (masterEnd) {
    masterEnd.addEventListener("change", syncHidden);
  }

  checkboxes.forEach(function (checkbox) {
    checkbox.addEventListener("change", syncHidden);
  });
})();