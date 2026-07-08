(function () {
  function openDialog(dialog) {
    if (!dialog || typeof dialog.showModal !== "function") {
      return;
    }
    dialog.showModal();
    var focusable = dialog.querySelector(
      "input:not([type='hidden']), select, textarea, button:not([data-dialog-close])"
    );
    if (focusable) {
      focusable.focus();
    }
  }

  document.addEventListener("DOMContentLoaded", function () {
    var params = new URLSearchParams(window.location.search);
    if (params.get("add") === "1") {
      openDialog(document.getElementById("add-employee-dialog"));
    }

    document.querySelectorAll("[data-dialog-open]").forEach(function (trigger) {
      var targetId = trigger.getAttribute("data-dialog-open");
      if (!targetId) {
        return;
      }
      trigger.addEventListener("click", function () {
        openDialog(document.getElementById(targetId));
      });
    });

    document.querySelectorAll("[data-dialog-close]").forEach(function (trigger) {
      trigger.addEventListener("click", function () {
        var dialog = trigger.closest("dialog");
        if (dialog) {
          dialog.close();
        }
      });
    });

    document.querySelectorAll("dialog.modal-dialog").forEach(function (dialog) {
      dialog.addEventListener("click", function (event) {
        if (event.target === dialog) {
          dialog.close();
        }
      });
    });
  });
})();