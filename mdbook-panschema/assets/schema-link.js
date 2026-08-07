// @generated companion asset for `mdbook-panschema install`.
// Do not hand-edit; re-run `mdbook-panschema install` to refresh.
//
// Adds a toolbar control linking from this mdbook book to the
// panschema-generated schema docs it fronts. One schema renders a plain
// button; several render a drop-down in the same slot. The list is baked
// in at install time from `[book_link]` in panschema-publish.toml.
(function () {
  "use strict";

  // Substituted at install time with a JSON array of {schemaPath, label}.
  var links = __PANSCHEMA_LINKS__;
  if (!links || !links.length) {
    return;
  }

  // mdbook renders a per-page `path_to_root` so links resolve at any
  // depth and under a project-path (GitHub Pages) prefix. Fall back to a
  // site-relative root if it isn't defined.
  var root = typeof path_to_root !== "undefined" && path_to_root ? path_to_root : "";

  // Select by class, not id: mdbook 0.5 prefixed the toolbar ids
  // (`#menu-bar` -> `#mdbook-menu-bar`); the classes survived.
  var leftButtons = document.querySelector(".menu-bar .left-buttons");
  if (!leftButtons || leftButtons.querySelector(".schema-docs-button")) {
    return;
  }

  // Stroke-based node-and-edges glyph. Wrapped in `.fa-svg` so mdbook's
  // `.icon-button`/`.fa-svg` rules size and center it; schema-link.css
  // overrides mdbook's `fill: currentColor` with `fill: none` so the
  // strokes show instead of a filled blob.
  function glyph() {
    return (
      '<span class="fa-svg" aria-hidden="true">' +
      '<svg viewBox="0 0 16 16" stroke="currentColor" stroke-width="1.3" fill="none">' +
      '<circle cx="3.5" cy="8" r="2"/>' +
      '<circle cx="12.5" cy="3.5" r="2"/>' +
      '<circle cx="12.5" cy="12.5" r="2"/>' +
      '<path d="M5.4 7 10.8 4.2M5.4 9l5.4 2.8"/>' +
      "</svg>" +
      "</span>"
    );
  }

  if (links.length === 1) {
    // Exactly what a single-schema book has always rendered.
    var link = document.createElement("a");
    link.className = "icon-button schema-docs-button";
    link.href = root + links[0].schemaPath;
    link.title = links[0].label;
    link.setAttribute("aria-label", links[0].label);
    link.innerHTML = glyph();
    leftButtons.appendChild(link);
    return;
  }

  // Several schemas: one control in the same slot, opening a menu. Built
  // as a real <button> + list so it is reachable by keyboard, which a
  // hover-only menu would not be.
  var wrap = document.createElement("div");
  wrap.className = "schema-docs-button schema-docs-menu";

  var toggle = document.createElement("button");
  toggle.className = "icon-button";
  toggle.type = "button";
  toggle.title = "Schema reference";
  toggle.setAttribute("aria-label", "Schema reference");
  toggle.setAttribute("aria-haspopup", "true");
  toggle.setAttribute("aria-expanded", "false");
  toggle.innerHTML = glyph();

  var menu = document.createElement("ul");
  menu.className = "schema-docs-menu-list";
  menu.hidden = true;

  links.forEach(function (entry) {
    var item = document.createElement("li");
    var a = document.createElement("a");
    a.href = root + entry.schemaPath;
    a.textContent = entry.label;
    item.appendChild(a);
    menu.appendChild(item);
  });

  function setOpen(open) {
    menu.hidden = !open;
    toggle.setAttribute("aria-expanded", open ? "true" : "false");
  }

  toggle.addEventListener("click", function (e) {
    e.stopPropagation();
    setOpen(menu.hidden);
  });
  document.addEventListener("click", function () {
    setOpen(false);
  });
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") {
      setOpen(false);
    }
  });

  wrap.appendChild(toggle);
  wrap.appendChild(menu);
  leftButtons.appendChild(wrap);
})();
