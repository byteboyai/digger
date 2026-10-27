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
actual native bounds/visibility/handles, resizes the window, hides review and checks
zero handles, restores and waits for the new document, then destroys review and
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
598 → 601. Gallery adds 2 tests (1 → 3), explicit example target only. Task 5 did
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
