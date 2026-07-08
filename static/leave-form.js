(function () {
  document.addEventListener("DOMContentLoaded", function () {
    var startInput = document.getElementById("leave-start-date");
    var endInput = document.getElementById("leave-end-date");
    var portionSelect = document.getElementById("leave-day-portion");
    var durationHint = document.getElementById("leave-duration-hint");

    if (!startInput || !endInput || !portionSelect) {
      return;
    }

    function syncDurationOptions() {
      var sameDay =
        startInput.value && endInput.value && startInput.value === endInput.value;
      var halfOption = portionSelect.querySelector('option[value="half_day"]');

      if (halfOption) {
        halfOption.disabled = !sameDay;
      }

      if (!sameDay) {
        portionSelect.value = "full_day";
        if (durationHint) {
          durationHint.textContent =
            "Half day is only available when start and end are the same date.";
        }
      } else if (durationHint) {
        durationHint.textContent =
          portionSelect.value === "half_day"
            ? "This request will use half a day from your balance."
            : "Choose whole day or half day for this date.";
      }
    }

    startInput.addEventListener("change", function () {
      if (!endInput.value || endInput.value < startInput.value) {
        endInput.value = startInput.value;
      }
      syncDurationOptions();
    });

    endInput.addEventListener("change", function () {
      if (startInput.value && endInput.value < startInput.value) {
        endInput.value = startInput.value;
      }
      syncDurationOptions();
    });

    portionSelect.addEventListener("change", syncDurationOptions);

    syncDurationOptions();
  });
})();