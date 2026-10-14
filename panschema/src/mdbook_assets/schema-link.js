// @generated companion asset for `mdbook-panschema install`.
// Do not hand-edit; re-run `mdbook-panschema install` to refresh.
//
// Adds a toolbar control linking from this mdbook book to the
// panschema-generated schema docs it fronts. One schema renders a plain
// button; several render a drop-down in the same slot. The list is baked
// in at install time from `[book_link]` in panschema-publish.toml.
//
// When the book is published in editions, the same asset renders the
// edition switcher beside it and a banner on every page that is not the
// current edition. The editions build bakes that context in; a plain
// `mdbook-panschema install` bakes none, and the switcher stays away.
(function () {
  "use strict";

  // Substituted at install time with a JSON array of {schemaPath, label}.
  var links = __PANSCHEMA_LINKS__;
  // Substituted at install time with null, or with
  // {editions: [{label, unreleased}], current, own}.
  var editions = __PANSCHEMA_EDITIONS__;

  // mdbook renders a per-page `path_to_root` so links resolve at any
  // depth and under a project-path (GitHub Pages) prefix. Fall back to a
  // site-relative root if it isn't defined.
  var root = typeof path_to_root !== "undefined" && path_to_root ? path_to_root : "";

  // Select by class, not id: mdbook 0.5 prefixed the toolbar ids
  // (`#menu-bar` -> `#mdbook-menu-bar`); the classes survived.
  var leftButtons = document.querySelector(".menu-bar .left-buttons");

  // Under editions the book sits at book/<edition>/ below the site root
  // and the schema docs beside it, not under it, so the schema links
  // resolve against the site root rather than the book's.
  var site = editions ? siteRoot(editions) : null;
  var base = site !== null ? site : root;

  if (links && links.length && leftButtons && !leftButtons.querySelector(".schema-docs-button")) {
    schemaLink(links);
  }
  if (editions && site !== null) {
    editionSwitcher(editions, site);
  }

  // The editions build gives every page a canonical link to itself at
  // its stable path, `<site>/book/<own>/<page>`, so the site root is
  // whatever precedes `book/<own>/`. That holds for the copy at the site
  // root too, whose canonicals point back at the stable path. The
  // browser percent-encodes the href, and the build encoded the label
  // the way encodeURIComponent does.
  function siteRoot(editions) {
    var canonical = document.querySelector('link[rel="canonical"]');
    if (!canonical) {
      return null;
    }
    var marker = "/book/" + encodeURIComponent(editions.own) + "/";
    var at = canonical.href.indexOf(marker);
    return at < 0 ? null : canonical.href.slice(0, at + 1);
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

  function schemaLink(links) {
    if (links.length === 1) {
      // Exactly what a single-schema book has always rendered.
      var link = document.createElement("a");
      link.className = "icon-button schema-docs-button";
      link.href = base + links[0].schemaPath;
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
      a.href = base + entry.schemaPath;
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
  }

  function editionSwitcher(editions, site) {
    function editionRoot(label) {
      return site + "book/" + encodeURIComponent(label) + "/";
    }

    if (leftButtons && !leftButtons.querySelector(".edition-switcher")) {
      var select = document.createElement("select");
      select.className = "edition-switcher";
      select.title = "Book edition";
      select.setAttribute("aria-label", "Book edition");
      editions.editions.forEach(function (edition) {
        var option = document.createElement("option");
        option.value = edition.label;
        option.textContent = edition.unreleased
          ? edition.label + " (unreleased)"
          : edition.label;
        option.selected = edition.label === editions.own;
        select.appendChild(option);
      });
      // Chapters need not exist under the same path in every edition, so
      // switching lands on the edition's first page.
      select.addEventListener("change", function () {
        window.location.href = editionRoot(select.value);
      });
      leftButtons.appendChild(select);
    }

    if (editions.own === editions.current || document.querySelector(".edition-banner")) {
      return;
    }
    var unreleased = editions.editions.some(function (edition) {
      return edition.label === editions.own && edition.unreleased;
    });
    var banner = document.createElement("div");
    banner.className = "edition-banner";
    banner.setAttribute("role", "note");
    banner.appendChild(
      document.createTextNode(
        unreleased
          ? "This is the unreleased edition of the book, built from " +
              editions.own +
              ". The current edition is "
          : "This is the " + editions.own + " edition of the book. The current edition is "
      )
    );
    var link = document.createElement("a");
    link.href = editionRoot(editions.current);
    link.textContent = editions.current;
    banner.appendChild(link);
    banner.appendChild(document.createTextNode("."));
    var content =
      document.querySelector(".content main") ||
      document.querySelector(".content") ||
      document.body;
    content.insertBefore(banner, content.firstChild);
  }
})();
