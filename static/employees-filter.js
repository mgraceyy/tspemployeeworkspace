(function () {
  var form = document.getElementById("employee-filter-form");
  if (!form) {
    return;
  }

  var status = form.querySelector('select[name="status"]');
  var search = form.querySelector('input[name="q"]');
  var debounceMs = 350;
  var timer;
  var focusKey = "employee-search-focus";

  function submitForm(source) {
    if (source === "search" && search && typeof search.selectionStart === "number") {
      sessionStorage.setItem(
        focusKey,
        JSON.stringify({
          selectionStart: search.selectionStart,
          selectionEnd: search.selectionEnd,
        })
      );
    } else {
      sessionStorage.removeItem(focusKey);
    }

    if (typeof form.requestSubmit === "function") {
      form.requestSubmit();
      return;
    }
    form.submit();
  }

  if (status) {
    status.addEventListener("change", function () {
      submitForm("status");
    });
  }

  if (search) {
    var savedFocus = sessionStorage.getItem(focusKey);
    if (savedFocus) {
      sessionStorage.removeItem(focusKey);
      try {
        var state = JSON.parse(savedFocus);
        search.focus();
        if (typeof search.setSelectionRange === "function") {
          var start = state.selectionStart;
          var end = state.selectionEnd;
          if (typeof start === "number" && typeof end === "number") {
            search.setSelectionRange(start, end);
          }
        }
      } catch (_error) {
        search.focus();
      }
    }

    search.addEventListener("input", function () {
      clearTimeout(timer);
      timer = setTimeout(function () {
        submitForm("search");
      }, debounceMs);
    });

    search.addEventListener("keydown", function (event) {
      if (event.key === "Enter") {
        event.preventDefault();
        clearTimeout(timer);
        submitForm("search");
      }
    });
  }
})();