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
// current edition, and the button of a schema whose docs are published
// per version lists every version, grouped by edition. The editions
// build bakes that context in; a plain `mdbook-panschema install` bakes
// none, and the switcher stays away.
(function () {
  "use strict";

  // Substituted at install time with a JSON array of {schemaPath, label},
  // each with a versionsRoot under editions when its docs are published
  // per version.
  var links = __PANSCHEMA_LINKS__;
  // Substituted at install time with null, or with {editions, current,
  // own}: `current` and `own` are edition ids, and each edition is
  // {id, name, releases, versions}, where `releases` is null or
  // [first, newest] and `versions` runs newest first.
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

  // A toolbar button opening a list: a real <button> and real links, so
  // it is reachable by keyboard, which a hover-only menu would not be.
  function menu(title) {
    var wrap = document.createElement("div");
    wrap.className = "schema-docs-button schema-docs-menu";

    var toggle = document.createElement("button");
    toggle.className = "icon-button";
    toggle.type = "button";
    toggle.title = title;
    toggle.setAttribute("aria-label", title);
    toggle.setAttribute("aria-haspopup", "true");
    toggle.setAttribute("aria-expanded", "false");
    toggle.innerHTML = glyph();

    var list = document.createElement("ul");
    list.className = "schema-docs-menu-list";
    list.hidden = true;

    function setOpen(open) {
      list.hidden = !open;
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
    }

    toggle.addEventListener("click", function (e) {
      e.stopPropagation();
      setOpen(list.hidden);
    });
    document.addEventListener("click", function () {
      setOpen(false);
    });
    document.addEventListener("keydown", function (e) {
      if (e.key !== "Escape") {
        return;
      }
      // A reader inside the list would be left on a hidden link.
      if (list.contains(document.activeElement)) {
        toggle.focus();
      }
      setOpen(false);
    });

    wrap.appendChild(toggle);
    wrap.appendChild(list);
    return { wrap: wrap, list: list };
  }

  function linkItem(text, href) {
    var item = document.createElement("li");
    var a = document.createElement("a");
    a.href = href;
    a.textContent = text;
    item.appendChild(a);
    return item;
  }

  // A list item holding a label and the list it names.
  function labelledGroup(className, label) {
    var item = document.createElement("li");
    item.className = className;
    var name = document.createElement("span");
    name.className = "schema-docs-group-label";
    name.textContent = label;
    item.appendChild(name);
    var list = document.createElement("ul");
    list.setAttribute("role", "group");
    list.setAttribute("aria-label", label);
    item.appendChild(list);
    return { item: item, list: list };
  }

  // Every version of one schema's docs, a group per edition in the
  // baked order. The page's own edition is marked, and so is the
  // version its book describes, which is the one the entry itself links
  // to; the stylesheet mutes the rest.
  function versionGroups(list, entry) {
    editions.editions.forEach(function (edition) {
      var own = edition.id === editions.own;
      var group = labelledGroup(
        own ? "schema-docs-edition own" : "schema-docs-edition",
        edition.name
      );
      edition.versions.forEach(function (version) {
        var item = linkItem(
          version,
          base + entry.versionsRoot + encodeURIComponent(version) + "/"
        );
        if (entry.versionsRoot + version + "/" === entry.schemaPath) {
          item.firstChild.setAttribute("aria-current", "true");
        }
        group.list.appendChild(item);
      });
      list.appendChild(group.item);
    });
  }

  function schemaLink(links) {
    // Without a site root there is nowhere to resolve a version against,
    // and each link still reaches the version its edition describes.
    function versioned(entry) {
      return site !== null && typeof entry.versionsRoot === "string";
    }

    if (links.length === 1 && !versioned(links[0])) {
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

    // Several schemas, or one with a version to choose: one control in
    // the same slot, opening a list.
    var several = links.length > 1;
    var control = menu(several ? "Schema reference" : links[0].label);
    links.forEach(function (entry) {
      if (!versioned(entry)) {
        control.list.appendChild(linkItem(entry.label, base + entry.schemaPath));
        return;
      }
      if (!several) {
        versionGroups(control.list, entry);
        return;
      }
      // Its versions sit under its label, apart from the next entry.
      var schema = labelledGroup("schema-docs-schema", entry.label);
      versionGroups(schema.list, entry);
      control.list.appendChild(schema.item);
    });
    leftButtons.appendChild(control.wrap);
  }

  function editionSwitcher(editions, site) {
    function editionRoot(id) {
      return site + "book/" + encodeURIComponent(id) + "/";
    }
    function editionById(id) {
      return editions.editions.filter(function (edition) {
        return edition.id === id;
      })[0];
    }

    if (leftButtons && !leftButtons.querySelector(".edition-switcher")) {
      var select = document.createElement("select");
      select.className = "edition-switcher";
      select.title = "Book edition";
      select.setAttribute("aria-label", "Book edition");
      editions.editions.forEach(function (edition) {
        var option = document.createElement("option");
        option.value = edition.id;
        option.textContent = edition.releases ? edition.name : edition.name + " (unreleased)";
        option.selected = edition.id === editions.own;
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
    var own = editionById(editions.own);
    var current = editionById(editions.current);
    var covers = "is unreleased";
    if (own.releases) {
      covers =
        own.releases[0] === own.releases[1]
          ? "covers " + own.releases[0]
          : "covers " + own.releases[0] + " to " + own.releases[1];
    }
    var banner = document.createElement("div");
    banner.className = "edition-banner";
    banner.setAttribute("role", "note");
    banner.appendChild(
      document.createTextNode(
        "This edition, " + own.name + ", " + covers + ". The current edition is "
      )
    );
    var link = document.createElement("a");
    link.href = editionRoot(current.id);
    link.textContent = current.name;
    banner.appendChild(link);
    banner.appendChild(document.createTextNode("."));
    var content =
      document.querySelector(".content main") ||
      document.querySelector(".content") ||
      document.body;
    content.insertBefore(banner, content.firstChild);
  }
})();
