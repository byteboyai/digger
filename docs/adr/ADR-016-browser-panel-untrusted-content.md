# ADR-016: Browser Panel and the Untrusted-Content Webview

## Status

Accepted (2026-10-28). Implementation complete; real-window acceptance (checklist I zone) pending.

## Context

Digger gets a browser panel (`digger.browser`, right rail, after conversations): tabs, an address bar, back/forward/refresh, in-page navigation followed by the address bar, `target=_blank` as a new tab, bookmarks.

Every other native child in Digger or Dozer shows **trusted** content: our own pages on a fixed origin (`native_child`), or a bytehost app on its own origin (`app_webview`). A browser shows **arbitrary web pages**, and the user decided it must also open local `file://` pages. A page that can reach host capabilities (IPC, custom protocols, injected scripts, downloads, media) would turn the panel into a hole in the product. The design therefore starts from "the page gets nothing from the host".

The probe report `docs/superpowers/reviews/2026-10-28-browser-panel-spikes.md` (macOS 26.6.2, wry 0.55.1, iced 0.14) supplies the measured facts used below.

## Decision

### 1. A separate webview kind, shared libraries

- Panel state, tabs, address bar and bookmarks are the shared library `dozer-panel-browser` (also used by Dozer). Bookmarks need no new storage: `dozer_client::Client` implements `BookmarkService` and `diggerd`'s database already has the table.
- The page itself is carried by bytehost `bytehost_desktop::web_child` (`WebPool`, bytehost v0.4.15+): a pool of wry child views whose handle table is **never shared** with `native_child` or `app_webview`. Pure policy lives in `bytehost_webview::browse`.
- Digger adds only thin adapters (`adapters/browser*.rs`). Nothing in Digger builds a wry view for untrusted pages.

### 2. What the page can never get (pinned by a source-scan test, mutation-verified)

`web_child_pins_untrusted_content_settings` reads the production source of the pool and requires/forbids:

| Setting | Decision |
|---|---|
| Custom protocols (`dozer://`, `digger-*://`) | **none registered** |
| Initialization scripts, IPC handler | **none** — the page cannot see `window.ipc`; titles and load events come from wry's native callbacks |
| Developer tools | off |
| Downloads | always refused |
| Link preview | off |
| New windows | always denied; an allowed URL is reported to the host, which opens a **tab** |
| Media capture (camera/microphone) | denied by a proxy `WKUIDelegate` (`DenyMediaProxy`) that forwards every other method to wry's own delegate |
| Website data | persistent and **isolated** (`WKWebsiteDataStore` id `bytehost-browser`); not shared with trusted pages. Changing this id makes every user lose their logins, so it is frozen |

The media proxy is defence in depth: wry 0.55.1 grants *every* media-capture request unconditionally, and today only `navigator.mediaDevices` being `undefined` in this WKWebView keeps it unreachable (measured). The proxy makes that independent of wry/WebKit defaults. It was verified to install, forward and deny when called directly; it was **not** possible to prove end to end, because no page can call `getUserMedia`. Geolocation was not tested (wry has no hook; it depends on system authorization).

### 3. Navigation allowlist

`allows_browse_navigation` (and the same table for new tabs) allows only:

- `http` / `https` with a non-empty host;
- `file` with an empty or `localhost` host (network-share `file://host/...` is refused);
- exactly `about:blank` (case-insensitive).

Everything else — `javascript:`, `data:`, `blob:`, `view-source:`, `dozer://`, `digger-review://`, `mailto:`, `tel:`, `ftp:` — is refused. Input containing control characters, a backslash, or leading/trailing whitespace is rejected **before** URL parsing, because the standard parser silently strips tabs/newlines, turns `\` into `/` and trims. A refused URL given to the pool is replaced by `about:blank` and reported (the host shows a one-line notice naming only the scheme, never the rest of the URL).

URLs are percent-encoded by the host before they reach wry (`encode_for_wry`, idempotent). wry unwraps `NSURL::URLWithString`, which on older systems may return nil for raw spaces or CJK; macOS 26 encodes them itself.

### 4. `file://` is supported — and what that means

The address bar accepts `file://…`, and a bare path (`/Users/me/a.html`, `~/a.html`) is turned into one (a change in the shared library that also applies to Dozer). Measured (S1): a local page loads through plain `load_url`; its same-directory subresources load; `fetch('file:///…/other.txt')` from the page **fails** in every case tried.

Known and accepted risks, so users know what they open:

- A local page is still a web page: it can load remote resources and make network requests. There is no CSP on browsed pages.
- Only `fetch` was tested for reading other local files. `XMLHttpRequest`, `<iframe>`, `<form>` and other paths were not.
- A local page gets **no host capability** — that is the property this ADR guarantees; it is not a sandbox against everything a web page can do.

### 5. Keyboard: a standard Edit menu, no scripts

wry's child view returns `NO` from `performKeyEquivalent:`, so with no Edit menu ⌘A/⌘C/⌘V/⌘X do nothing inside the page (measured; typing and Chinese IME work regardless). Installing a standard Edit menu (Cut/Copy/Paste/Select All with a nil target, i.e. through the responder chain) fixes all four with **zero injected script**. Digger has no main menu of its own, so the panel installs one the first time it is shown (macOS, idempotent, main thread). The user confirmed this does not disturb ⌘ shortcuts in iced inputs; the acceptance checklist keeps a regression item.

### 6. Lifecycle and layering

- **One generation per project.** Tab ids are per-`Tabs` counters and collide across projects, so switching project destroys the pool and rebuilds it; switching back also rebuilds (pages are not kept alive per project). Each tab's URL is kept in state and reloads.
- **Hidden panel or an iced overlay is open → parked** (views hidden, pages kept). A webview always sits above iced content, so iced overlays must hide it; native menus are separate windows and do not.
- Every event carries its generation; events from a destroyed generation are dropped without touching state.
- The pool records the address a page actually reached (a page can navigate itself); otherwise feeding `PageLoaded` back would reload the same page (found by smoke test, fixed in v0.4.16).
- **wry has no "navigation failed" callback.** An unreachable address only produces the initial blank document's load events, which the pool ignores while a real navigation is pending. If no load starts within 15 s the pool reports `NavigationStalled` and the host shows "page not responding". The meaning is "no response", not "definitely failed".

### 7. Geometry

The page is placed below the tab bar and address bar. Its top is `chrome_top_px()` (forwarded from byteui, the single source) plus the height of the notice bar if shown; left/right/bottom insets come from the same region style the view uses; with the bookmarks sidebar open the page gives up its width using the same split the view uses. A source-scan test forbids hand-written pixel constants in the geometry.

## Consequences

- One more webview kind to maintain, but it shares nothing with the trusted kinds, so a flaw in one cannot widen the other.
- Dozer is unaffected except through the shared library: its address bar now also turns bare local paths into `file://` URLs.
- Browsed pages persist cookies/localStorage in their own store. There is no private mode in v1.

## Not in v1 (deliberate)

In-page find (⌘F); clipboard hand-off beyond the Edit menu; downloads; developer tools; multiple windows; tab drag-reorder; dragging the bookmarks sidebar divider (fixed at 28 %); keeping pages alive per project; a visible message when a link to a refused scheme (`mailto:`, `tel:`) is clicked inside a page — the navigation is silently refused.

## Open items

- **Minimum macOS version is unverified.** Everything above was measured on macOS 26.6.2 only. Host-side encoding is meant to protect older systems from the `NSURL` unwrap, but that has not been run on one. Decide and test a floor before release.
- Geolocation behaviour is untested.
- Whether `chrome_top_px()` equals the real height of Digger's tab bar + address bar cannot be checked headlessly; it is a manual acceptance item.
- Real-window behaviour (layering, focus timing, IME, the Edit menu's effect on iced inputs) is covered by checklist I zone, not by automated tests.

## References

- Plan: `dozer/docs/superpowers/plans/2026-10-28-digger-browser-panel.md`
- Probe report: `docs/superpowers/reviews/2026-10-28-browser-panel-spikes.md`
- bytehost: `crates/bytehost-webview/src/browse.rs`, `crates/bytehost-desktop/src/web_child/`
- Library: `dozer/crates/dozer-panel-browser`
