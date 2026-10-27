# S7c Task 5 regression record

2026-10-27; branch `feat/conversations-review`; Task 5 starts at `596cf11`.
Digger only, shared dependencies remain pinned to `libs-2026-10-26b`. No Dozer,
library, review page, daemon, protocol, or dependency-version changes. No installed
apps, user sandbox, automatic import, summary generation, models, release,
installation, merge, push, or tag operations were used.

## Executed evidence and its limits

| Requirement | Executed evidence | Remaining verification |
| --- | --- | --- |
| Huge 5,000 rounds | Gallery test uses the real transcript parser, shared review fold, panel adapter and fake service pagination. All 10,000 turns are appended in 200-entry pages, plus an empty terminal page. | Actual native Load More click-through, scroll behavior, layout, UI responsiveness and GUI CPU/memory. The native child may cover the iced Load More control; existing headless hit testing does not settle that. |
| Hostile content | Existing fixture has script/HTML/javascript-link text, a 100,000-character unbroken line, RTL/zero-width characters and an AI turn over 3 MB. Real parser, panel and snapshot path retain content without service errors. | Execution, network, CSP violation reporting and overflow require actual GUI observation. Serialization/string checks do not prove nonexecution. |
| Empty / broken | Empty entries produce a snapshot; existing adapter fallback tests confirm no native spec and readable empty/error widgets. Mixed `broken.jsonl` contains two valid turns plus malformed lines: the real parser skips bad lines and retains valid content. A separate malformed-only input parses to zero turns. | Mixed broken is **partial valid content**, not a forced failure. This follows current Dozer/parser semantics and differs from the original Task 5 fallback assumption; no new parity acceptance is inferred. Actual page fallback remains GUI-unverified. |
| Rapid operations | Headless stress opens 20 distinct row IDs across two projects, applies filters, lays out varying widths/collapse states, closes while not native-ready, and rejects all stale document events. Existing product/geometry tests cover dividers, maximize, minimize/fullscreen event decisions and overlay guards. | Actual user row clicks, native filter menu, divider dragging, minimize/restore and fullscreen transitions. |
| Native lifecycle | New probe API schedules `SharedHost.probe` through `window::run`; the executor reads the real handle table and on macOS NSView frame/visibility. Generation-guard tests cover delayed results after destroy/recreate; scheduled-window-action tests execute no NSView. | The real Gallery selftest described below is prepared, **not run successfully**. No live handle-count result has been observed. |
| Daemon death | Synthetic unavailable response: cached snapshot retained, unavailable reason visible in actual shared widget, next-row detail error readable, later successful open recovers. Existing retry-button tests dispatch the real shared retry message. | No daemon was killed. Actual cached WebView readability and daemon restart/retry require isolated GUI/backend run. |
| Dozer + Digger isolation | Launcher sentinel tests verify actual exec environment and Digger resolver use fresh HOME/DIGGER_DATA_DIR under actual TMPDIR, remove inherited DOZER_DATA_DIR and leave an outside sentinel untouched. | Simultaneous real Dozer+Digger processes, explicit separate imports and mutual nonappearance of new sessions were not run. User operates Dozer with an explicitly supplied own-built binary. |

The earlier root Gallery GUI attempt could not register a window:
`/private/tmp/s7c-gallery-observation/{launch.log,process.json,observation.json,app-registry.json}`.
Owned PID 67976 was absent from the app registry; Orca get-app-state exited 1
(`app_not_found`), list-apps exited 0. Logs report LSModifyNotification / hiservices
XPC failure. This is a GUI-unverified handoff, not a pass. No screenshots exist.

The observer boundary is explicit: wry 0.55.1 offers no applicable console/CSP-event
handler in the searched local API without injected script. The shared page and CSP
are unchanged, navigation is origin-restricted, and downloads/new windows are
rejected by the existing executor. These static policies are not proof of zero
external subresource requests or zero CSP events. No unsupported handler, injected
IPC or page workaround was added.

## Runnable native probe (user-run, isolated GUI session)

Build from this checkout using the normal repository target, then run:

```sh
cargo build -p digger-desktop --example conversations_gallery
scripts/launch-conversations-fixture.sh gallery --state list-and-detail --selftest-native
```

The launcher creates fresh fixture HOME and DIGGER_DATA_DIR as strict descendants
of actual TMPDIR and removes inherited DOZER_DATA_DIR. The Gallery also rejects
unsafe data roots before opening a window. The flag requires `list-and-detail` and
cannot be combined with headless `--check`.

This uses synthetic services only. It waits for genuine current-document IPC, reads
actual native bounds/visibility/handles, requests a 1000×760 window resize,
waits for a matching acknowledgement from that same window before probing resized
bounds, hides review and checks zero handles, restores and waits for the new document, then destroys review and
checks zero handles before exiting. Exit 0 means all five native observations
matched; failure or a 30-second readiness/probe timeout exits 1. It does not claim
full product interaction or page-security coverage. `last_probe` survives a destroy
observation while results from a replaced generation are ignored. Expected bounds
come from Gallery layout; this checks executor placement, not an independent
geometry oracle (the latter remains covered by existing pure geometry tests).

## Manual checklist (all pending; no screenshot paths)

- Open huge; use the real Load More control to reach 5,000 rounds; record elapsed
  time, sampling interval, peak CPU/RSS, responsiveness and scroll behavior.
- Open hostile; observe script/nonexecution, real external network requests with a
  system network/proxy observer, CSP diagnostics where available, long-line/RTL
  layout and MB-turn behavior. If library/page behavior fails, report it without
  altering the shared page or security policy.
- Open mixed broken, malformed-only and empty; distinguish retained valid rounds
  from empty fallback; check no blank or permanent loading page.
- Run the native selftest above and retain its exact log/exit. Then perform 20 row
  selections while switching projects/closing a loading project, native filter
  open/close, collapse/expand, divider drag, resize, minimize/restore and fullscreen.
- Keyboard focus and Chinese IME: **GUI_UNVERIFIED; screenshots: none.** Launch
  the user-built Digger with `scripts/launch-conversations-fixture.sh digger`;
  use only its printed fresh HOME/DIGGER_DATA_DIR and synthetic project. Explicitly
  import fixture history before opening a native Review. Close Review or switch
  away from it, focus the Agent terminal, and verify ordinary typing reaches the
  terminal again without a native child retaining focus. Focus Conversations
  search and verify its key events are Captured: search text appears only there
  and no characters reach the Agent terminal. With Chinese Pinyin IME, compose
  text in search, check the candidate window follows the insertion position,
  select a candidate, commit it, then delete committed text; also test cancelling
  composition. Return to Agent input and repeat Pinyin composition/candidate
  selection/commit/delete to verify input and focus restore after leaving Review
  and search. Record each result and screenshot path only after actual observation;
  the Gallery probe runner does not perform or prove these keyboard/IME checks.
- With an isolated own daemon, kill only that owned daemon after loading detail;
  verify cached text, list reason, next-row error, restart and explicit retry.
- Launch each product with its own fresh root and same synthetic project path;
  explicitly import independently; add synthetic history to each side and confirm
  the other product's list remains unchanged. Dozer is user-operated.

Task 0's eleven contract tests and existing Todo's seven contract tests remain
unchanged. Validation commands and exact exits are appended below; no workspace
suite/editor stress was repeated for Task 5.

## Commands and exits

Cargo commands ran in `/private/tmp/digger-s7c`, with
`HOME=/private/tmp/s7c-home DIGGER_DATA_DIR=/private/tmp/s7c-data
CARGO_HOME=/Users/chrischiang/.cargo RUSTUP_HOME=/Users/chrischiang/.rustup`,
using `/opt/homebrew/bin/bash`, inherited actual TMPDIR, and this checkout's normal
cargo target (no `CARGO_TARGET_DIR`). Exact execution history, including two new
test-setup failures corrected before the final run, is in
`/private/tmp/s7c-task5-validation.md`.

| Command | Exit / result | Log |
| --- | --- | --- |
| `cargo test -p digger-desktop --lib actual_probe_is_scheduled -- --test-threads=1` before implementation | 101, expected missing probe API compile errors | `/private/tmp/s7c-task5-red.log` |
| `cargo test -p digger-desktop --lib review_native -- --test-threads=1` after implementation | 0, 38 selected tests | `/private/tmp/s7c-task5-green.log` |
| `cargo test -p digger-desktop -- --test-threads=1` final | 0, 601 unit + 7 integration; doc/bin targets empty | `/private/tmp/s7c-task5-final.log` |
| `cargo test -p digger-desktop --example conversations_gallery -- --test-threads=1` final | 0, 3 example tests, 10.00 seconds total | same final log |
| `cargo build -p digger-desktop --example conversations_gallery` | 0 | same final log |
| `cargo clippy -p digger-desktop --all-targets -- -D warnings` | 0 | same final log |
| `cargo fmt --all -- --check` | 0 | same final log |
| `python3 -m unittest discover -s scripts/tests -p 'test_*conversations.py' -v` | 0, 6 safety/isolation tests | same final log |
| `target/aarch64-apple-darwin/debug/examples/conversations_gallery --help` | 0, advertises selftest flag | `/private/tmp/s7c-task5-cli-help.log` |
| same Gallery binary `--selftest-native --check` | 2, rejects headless native selftest | `/private/tmp/s7c-task5-cli-headless-rejected.log` |
| `scripts/launch-conversations-fixture.sh gallery --state empty --selftest-native` | 2, rejects invalid state before setup/GUI | `/private/tmp/s7c-task5-cli-invalid.log` |
| `/usr/bin/time -l` on the compiled huge example test | test itself 0; time wrapper 1 because sandbox denies `sysctl kern.clockrate`; no RSS result | `/private/tmp/s7c-task5-headless-resources.log` |

Default workspace test inventory gains **3** tests (785 → 788); desktop unit count
598 → 601. After the review correction below, Gallery adds 3 tests (1 → 4),
explicit example target only. Task 5 did
not rerun the full workspace. The existing `block v0.1.6` future-compatibility
warning remains; no dependency was modified.

Headless resource observation (separate compiled-test child, Python
`subprocess.run` + `resource.getrusage(RUSAGE_CHILDREN)`, exit 0): huge test elapsed
9.716 s, child user 9.666 s, system 0.044 s, macOS maximum RSS 39,567,360 bytes
(37.7 MiB). Exact command:
`target/aarch64-apple-darwin/debug/examples/conversations_gallery-3299a43fe2f15db0
--exact stress_tests::huge_load_more_reaches_all_5000_rounds_without_duplicates
--nocapture --test-threads=1`.
Log: `/private/tmp/s7c-task5-headless-resource-python.log`. This measures only
parser/fold/snapshot/pagination work with synthetic services; it excludes native
webview rendering, GUI interaction and GUI CPU/memory. It supplies no evidence of
UI responsiveness, layout safety or network behavior.


## Task 5 review correction: resize acknowledgement and nonce wrap

The initial native selftest could advance its resize stage using the old layout
if the resize event was delayed or missing. The corrected runner records the
requested window ID and 1000×760 size, clears any previous acknowledgement and
withholds the stage-1 probe until a matching `WindowReady` resize event arrives.
A different size or window cannot acknowledge it; a subsequently invalidated
acknowledgement cannot pass the observation. The existing 30-second deadline exits
1 if readiness or acknowledgement never arrives. **Actual native GUI execution
remains UNVERIFIED; screenshots: none.**

The new headless runner test dispatches real Gallery update messages and counts
scheduled iced window actions. Its initial runtime RED was an emitted probe
(1) where old-size state required no probe (0), exit 101. After correction it
checks old size, wrong height and a different window emit zero probes, the matching
acknowledgement emits one, and an outstanding probe is not duplicated. These
scheduled actions do not execute a live NSView.

The existing Load More widget test now runs both the ordinary nonce case and
`review_nonce = u64::MAX - 1`: the real initial service response/library update
increments to MAX, the real headless Load More button sends the normal request,
and its response/library update wraps to 0. An old MAX Loaded event leaves the
0 document pending; the current 0 Loaded event transitions to Ready. No shared
library message handling, page, protocol or dependency was changed.

Latest validation is recorded in `/private/tmp/s7c-task5-resize-wrap-final.log`:
`cargo test -p digger-desktop -- --test-threads=1` (601 unit + 7 integration),
`cargo test -p digger-desktop --example conversations_gallery -- --test-threads=1`
(4 example tests), example build, clippy all-targets with `-D warnings`, and fmt
check. The earlier command table records the original Task 5 run; these corrected
counts supersede its Gallery total. Default workspace inventory remains 788;
this adds one explicit example test only. All six correction command steps exited
0 (the shell chain exited 0): desktop tests, example tests, example build, clippy,
fmt application and fmt check. No full workspace rerun was performed here.
