// @generated companion asset for `mdbook-panschema install`.
// Do not hand-edit; re-run `mdbook-panschema install` to refresh.
//
// Adds a toolbar button linking from this mdbook book to its
// panschema-generated schema docs. `schemaPath` and `label` are baked in
// at install time from `[book_link]` in panschema-publish.toml.
(function () {
  "use strict";

  // Substituted at install time with JSON string literals.
  var schemaPath = __PANSCHEMA_SCHEMA_PATH__;
  var label = __PANSCHEMA_LABEL__;

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

  var link = document.createElement("a");
  link.className = "icon-button schema-docs-button";
  link.href = root + schemaPath;
  link.title = label;
  link.setAttribute("aria-label", label);
  // Stroke-based node-and-edges glyph. Wrapped in `.fa-svg` so mdbook's
  // `.icon-button`/`.fa-svg` rules size and center it; schema-link.css
  // overrides mdbook's `fill: currentColor` with `fill: none` so the
  // strokes show instead of a filled blob.
  link.innerHTML =
    '<span class="fa-svg" aria-hidden="true">' +
    '<svg viewBox="0 0 16 16" stroke="currentColor" stroke-width="1.3" fill="none">' +
    '<circle cx="3.5" cy="8" r="2"/>' +
    '<circle cx="12.5" cy="3.5" r="2"/>' +
    '<circle cx="12.5" cy="12.5" r="2"/>' +
    '<path d="M5.4 7 10.8 4.2M5.4 9l5.4 2.8"/>' +
    "</svg>" +
    "</span>";

  leftButtons.appendChild(link);
})();
