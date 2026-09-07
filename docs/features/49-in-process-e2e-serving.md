# In-Process E2E Serving - Implementation Plan

**Feature:** Serve the e2e suite's pages from an in-process service instead
of an ephemeral listener

**User Story:** As someone running panschema's browser tests, I want the
pages served without binding a port, so that a test fails because the app is
wrong rather than because two tests wanted the same port or a server had not
finished starting.

**Related:** playwright-rs `route_service` (page and context), which fulfills
matching requests from a tower `Service` with no socket. panschema tracks
that crate's `main` deliberately, so the capability is available now.

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

Every e2e test binds `127.0.0.1:0`, spawns an axum `ServeDir` over the
generated docs, sleeps 100ms "to give the server time to start", and tears
the whole thing down afterwards. Twenty-eight sites do this. The sleep is a
guess and the port is a race, and the suite runs on three browsers across
three operating systems, which is where both bite — a Windows e2e timeout
cost a CI re-run during this feature's own development.

`route_service` removes the socket: the browser's requests for a matching
pattern are handed to a `Service` in the test process. The service is the
same `ServeDir` the harness already builds, so what is served does not
change; only how it reaches the browser does.

The reason this is not a single mechanical sweep is cost. Each response
crosses the driver's JSON-RPC channel, and panschema's page pulls a 3.2 MB
wasm bundle. At 28 tests times three browsers times two nextest passes, that
is a lot of bundle. The crate's own documentation puts an app bundle inside
the intended range and a load test outside it, which is close enough to the
line to measure rather than assume. So slice 1 converts one test and times
it; slice 2 converts the rest only if the number justifies it.

One test keeps its listener deliberately. Not for coverage of panschema's
own serving — the harness's server is a test-local `ServeDir`, so the socket
exercises tower-http rather than anything this repo ships — but as a
control. panschema tracks playwright-rs `main`, so an interception
regression can arrive at any time; with every test intercepted, that
presents as "the entire e2e suite is broken" with nothing to compare
against.

---

## Vertical Slices

### Slice 1: One test, and the number that decides the rest

**Status:** Complete

**User Value:** One browser test runs with no listener, no port, and no
start-up sleep, and the suite's maintainers know what interception costs
before the pattern spreads.

**Acceptance Criteria:**
- [x] One e2e test serves its page through an in-process service and passes
      on all three browsers, asserting exactly what it asserted before.
- [x] That test binds no port and sleeps for no server.
- [x] The wall-clock cost of that test is recorded against its
      listener-backed equivalent, on all three browsers, in this document.
- [x] The helper that opens a served page is shared, so converting a
      further test is a call-site change rather than a copied block.

**Measured 2026-09-07** on one laptop, `e2e_happy_path`, `BROWSER=all`
(the test drives all three engines in one run):

| Serving | Wall clock |
|---|---|
| Ephemeral listener (before) | 10.74s |
| In-process service | 10.84s, 10.07s, 10.00s, 9.94s |

Interception costs nothing measurable here, and is if anything marginally
faster than binding, spawning, and sleeping. The 3.2 MB wasm bundle
crossing the driver channel each load does not show up — the run-to-run
spread is nine times any difference between the two modes. Slice 2 goes
ahead on that number.

**Notes:**
- Every per-browser helper launches its own browser and page, so the route
  registers inside that helper; the shared opener is what keeps that from
  being twenty-eight copies.
- Measure with one `cargo nextest` invocation per sample. A shell loop is a
  compound command that this environment sandboxes, and a sandboxed
  Chromium cannot register its Mach port, so every run inside a loop fails
  at launch with a permission error that has nothing to do with the suite.
- The pattern serves an origin the tests choose rather than
  `127.0.0.1:<port>`, so `base_url` stops being a port-dependent string.
- Out of scope: converting the other tests, and removing the listener
  harness, which the control test keeps using.

---

### Slice 2: The rest of the suite, and one deliberate listener

**Status:** Complete

**User Value:** The suite no longer races for ports, and a future
interception regression is distinguishable from an application failure.

**Acceptance Criteria:**
- [x] Every e2e test but one serves through the in-process service; the
      remaining one binds a listener and says in its documentation why it
      is the control.
- [x] The suite passes on all three browsers, and its total wall-clock time
      is recorded here beside the pre-conversion number.
- [x] `bind_ephemeral`, the start-up sleep, and the shutdown plumbing
      survive only where the control test uses them.

**Measured 2026-09-07**, whole `e2e` binary, `BROWSER=all`, one sample each:

| Suite | Wall clock | Notes |
|---|---|---|
| Before | 11.14s | nextest reports `1 leaky` |
| After | 11.94s | no leak reported |

Twenty-four tests changed serving mode in this slice: the binary holds 28,
of which three drive `file://` pages for the mdbook plugin and never bound
anything, and one — the happy path — converted in slice 1.

The totals are the same within the run-to-run spread slice 1 measured, so
the conversion neither costs nor buys wall clock at this size. What it does
buy is the `leaky` flag going away: nextest was reporting a handle
outliving its test, which is what a spawned server task and its listener
look like.

**The control is `e2e_legends_adapt_to_what_each_graph_contains`**, the one
test that serves two sites at once — a schema-and-instances build plus an
attribute-only build, on two listeners. Keeping it bound serves two ends:
it is the awkward case to express as interception, and it is the
differential. panschema tracks playwright-rs `main`, so a `route_service`
regression can land at any time; if the control passes while the other
twenty-seven fail, the fault is the interception path, not the app.

**Notes:**
- Depends on slice 1's measurement. If interception proves materially
  slower, the split changes — more tests keep listeners, or the bundle is
  served differently — and this slice's plan is rewritten before it starts,
  not quietly abandoned.
- WebKit aborts a 3xx under interception, including `ServeDir`'s
  trailing-slash redirect. Every navigation in the suite names
  `/index.html` explicitly today, so nothing redirects; a future test that
  navigates to a directory path would need the trailing slash.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Complete |
| Slice 2 | Should Have | Slice 1 | Complete |

## Things to watch

- Interception is not the network: no compression negotiation, no HTTP/2,
  no keep-alive, and timing data reflects the driver round trip. Nothing in
  this suite asserts on those, but a future performance test could not live
  here.
- Request bodies are withheld for uploads and very large payloads. The
  suite posts nothing today.
- The measurement in slice 1 is the input to slice 2's scope. Record it
  even if it is unremarkable, because the next person to wonder whether
  interception is affordable should find the number rather than repeat the
  experiment.

## Definition of Done

- [x] All acceptance criteria met
- [x] All slices Complete
- [x] `cargo nextest run --workspace` passes with `BROWSER=all`
- [x] Code formatted and clippy clean
- [x] The cross-repo handoff this came from is answered with what the
      conversion actually cost
