(function () {
  var KEY = "dtr-theme";

  function systemTheme() {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  function storedTheme() {
    var value = localStorage.getItem(KEY);
    return value === "dark" || value === "light" ? value : null;
  }

  function applyTheme(theme) {
    document.documentElement.setAttribute("data-theme", theme);
    document.documentElement.style.colorScheme = theme;
    var meta = document.querySelector('meta[name="theme-color"]');
    if (meta) {
      meta.setAttribute("content", theme === "dark" ? "#151d2e" : "#0d9488");
    }
  }

  applyTheme(storedTheme() || systemTheme());

  function normalizePath(path) {
    if (!path || path === "/") {
      return "/";
    }
    return path.replace(/\/+$/, "") || "/";
  }

  function isActivePath(current, href) {
    if (!href || href.indexOf("http") === 0) {
      return false;
    }
    var target = normalizePath(href.split("?")[0]);
    return (
      current === target ||
      (target !== "/" && current.indexOf(target + "/") === 0)
    );
  }

  function findBestActiveLink(links, current) {
    var best = null;
    var bestLen = -1;

    links.forEach(function (link) {
      var href = link.getAttribute("href");
      if (!isActivePath(current, href)) {
        return;
      }
      var target = normalizePath(href.split("?")[0]);
      if (target.length > bestLen) {
        best = link;
        bestLen = target.length;
      }
    });

    return best;
  }

  function markActiveNav() {
    var current = normalizePath(window.location.pathname);
    var links = document.querySelectorAll(".sidebar-nav a[href]");
    var best = findBestActiveLink(links, current);

    links.forEach(function (link) {
      link.classList.toggle("is-active", link === best);
    });
  }

  function closeOtherMenus(except) {
    document
      .querySelectorAll(".nav-menu[open], .user-menu[open], .action-menu[open]")
      .forEach(function (menu) {
        if (menu !== except) {
          menu.removeAttribute("open");
        }
      });
  }

  function setSidebarOpen(open) {
    var toggle = document.getElementById("sidebar-toggle");
    var overlay = document.getElementById("sidebar-overlay");

    document.body.classList.toggle("sidebar-open", open);

    if (toggle) {
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
      toggle.setAttribute("aria-label", open ? "Close navigation menu" : "Open navigation menu");
    }

    if (overlay) {
      overlay.hidden = !open;
    }
  }

  document.addEventListener("DOMContentLoaded", function () {
    var button = document.getElementById("theme-toggle");
    if (button) {
      var icon = button.querySelector(".theme-icon");

      function currentTheme() {
        return document.documentElement.getAttribute("data-theme") === "dark"
          ? "dark"
          : "light";
      }

      function syncButton() {
        var dark = currentTheme() === "dark";
        button.setAttribute(
          "aria-label",
          dark ? "Switch to light mode" : "Switch to dark mode"
        );
        button.setAttribute("aria-pressed", dark ? "true" : "false");
        if (icon) {
          icon.textContent = dark ? "☀" : "☾";
        }
      }

      syncButton();

      button.addEventListener("click", function () {
        var next = currentTheme() === "dark" ? "light" : "dark";
        localStorage.setItem(KEY, next);
        applyTheme(next);
        syncButton();
      });

      window
        .matchMedia("(prefers-color-scheme: dark)")
        .addEventListener("change", function (event) {
          if (storedTheme()) {
            return;
          }
          applyTheme(event.matches ? "dark" : "light");
          syncButton();
        });
    }

    markActiveNav();

    var current = normalizePath(window.location.pathname);
    var tabLinks = document.querySelectorAll(".tab-nav a[href]");
    var bestTab = findBestActiveLink(tabLinks, current);

    tabLinks.forEach(function (link) {
      link.classList.toggle("active", link === bestTab);
    });

    document.querySelectorAll(".nav-menu, .user-menu, .action-menu").forEach(function (menu) {
      menu.addEventListener("toggle", function () {
        if (menu.open) {
          closeOtherMenus(menu);
        }
      });
    });

    document.addEventListener("click", function (event) {
      if (!event.target.closest(".nav-menu, .user-menu, .action-menu")) {
        closeOtherMenus(null);
      }
    });

    var sidebarToggle = document.getElementById("sidebar-toggle");
    var sidebarOverlay = document.getElementById("sidebar-overlay");

    if (sidebarToggle) {
      sidebarToggle.addEventListener("click", function () {
        setSidebarOpen(!document.body.classList.contains("sidebar-open"));
      });
    }

    if (sidebarOverlay) {
      sidebarOverlay.addEventListener("click", function () {
        setSidebarOpen(false);
      });
    }

    document.querySelectorAll(".sidebar-nav a, .user-menu-link").forEach(function (el) {
      el.addEventListener("click", function () {
        if (window.matchMedia("(max-width: 768px)").matches) {
          setSidebarOpen(false);
        }
      });
    });

    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && document.body.classList.contains("sidebar-open")) {
        setSidebarOpen(false);
      }
    });
  });
})();