(function () {
  var form = document.getElementById("timesheet-filter-form");
  if (!form) {
    return;
  }

  var inputs = form.querySelectorAll('input[type="date"]');
  inputs.forEach(function (input) {
    input.addEventListener("change", function () {
      if (typeof form.requestSubmit === "function") {
        form.requestSubmit();
      } else {
        form.submit();
      }
    });
  });
})();