# S7c conversations fixture and parity checklist

Task 4 provides synthetic reproducible inputs and a shared-view gallery. **GUI parity is UNVERIFIED**. The S7b environment could not complete GUI comparison. This document records setup and pending checks; it neither fabricates screenshots nor accepts differences for the user.

The pinned reference is `libs-2026-10-26b`. No Dozer, shared-library, protocol, dependency, version, or frontend changes accompany these fixtures. The gallery uses Digger's real `ConversationsPanel`, library list/review views, `ReviewNative`, native-menu adapter, shell layout, and `review_bounds` helper. It has no daemon/backend/model connection. Fake conversation results pass through the production Claude parser, trace-detail extractor, and the shared `review_entries_from_turns` update path. Summary and transcript service operations fail locally; capabilities stay at the production defaults (summaries off, no linked Todos).

## State comparison ledger

All differences below remain pending user comparison unless explicitly stated otherwise. The gallery's surrounding blank shell area is a fixture harness; compare the Conversations panel itself. `--check` reports model state headlessly and is not a screenshot or proof of native rendering.

| State (`--state`) | Difference / evidence | Reason | Treatment |
| --- | --- | --- | --- |
| `empty` | Empty shared list, no detail; import prompt suppressed by its own synthetic marker. GUI unverified. | Distinguish empty list from first-import disclosure. | Compare empty copy, spacing, search, buttons; pending. |
| `list-only` | Seven synthetic rows, no selected detail. GUI unverified. | Reproducible list independent of machine history. | Compare row spacing, title clipping, timestamps, search and scrollbar; pending. |
| `list-and-detail-loading` | Genuine tools entries and snapshot; native sync deliberately held before document creation. Shared `ReviewPaneState::Loading` is tested. | Empty entries mean Ready in the pinned library; waiting for service data would show empty copy, not its loading spinner. | Compare loading spinner and header. This state intentionally remains loading; pending. |
| `list-and-detail` | Tools fixture, real native lifecycle; loaded nonce is never premarked. Header padding is deduplicated in Digger; actual Dozer may apply it twice. | Avoid header control overlap while matching native content bounds. | Padding is a possible visual difference, **PENDING**, not accepted. Check thinking/tools/results/tokens and genuine Loaded transition. |
| `detail-error` | Synthetic detail service failure, shared error view and host notice. GUI unverified. | Deterministic error without stopping a real daemon. | Compare error placement, dismissal, ability to reopen; pending. |
| `detail-empty-entries` | System-only fixture parses to zero entries; shared no-data view, no native child. GUI unverified. | Exercise empty transcript independently of empty list. | Compare no-data copy and header; pending. |
| `long-detail` | 500 rounds; shared first page is 200 turns/entries, genuine native loading. Load More widget exists and has a prior headless click test. | Follow pinned library paging and handoff behavior. | Real native view could cover Load More; GUI clearance/click remains unverified. No frontend workaround. |
| `import-prompt` | Shared production disclosure and log-path disclosure; gallery import stays locally disabled. | Only real Digger's explicit user click should run backfill. | Compare disclosure, success count including zero, retry and log guidance in isolated Digger; pending. |
| `daemon-unavailable` | Shared unavailable shell and synthetic notice; no real connection attempted. | Avoid operating an existing daemon. | Compare failure, retry copy, preserved state in isolated Digger; pending. |
| `agent-filter-open` | Initial list is ready. User clicks the shared filter control after WindowReady to request the actual native menu on macOS; no new iced overlay flag or webview hiding. | Native menus require a live window and user action; the example prints this instruction. | Open actual menu, cancel/select, then reopen detail and repeat. Pending GUI observation. |

Previously accepted handoff scope is limited to S6 hover-step/tooltip precision **and explicit user-confirmed differences**. None of the rows above is newly accepted. Record any new acceptance with the user's confirmation and exact scope.

## Synthetic fixtures and reproducibility

Tracked JSONL files under `crates/desktop/tests/fixtures/conversations/` are complete in `git archive`; they contain no captured user conversations or machine input. Regenerate with `python3 scripts/generate-conversations-fixtures.py`.

- `short`: three human/assistant rounds.
- `tools`: thinking, two tool calls, two separately paired results (one error), four token counters.
- `long` / `huge`: 500 / 5,000 rounds, respectively 1,000 / 10,000 parsed turns.
- `hostile`: script/HTML/event handler text, `javascript:` and data links, 100,000 characters without spaces, zero-width/RTL characters, unclosed Markdown, and one assistant turn exceeding 3 MB.
- `empty`: system metadata only; zero parsed entries.
- `broken`: two malformed JSON lines interleaved with one valid round; invalid lines remain intact when seeded.

IDs and timestamps are fixed. `__SYNTHETIC_PROJECT__` is rewritten structurally only on valid JSON lines. The seed uses the exact `core::agent_paths` Claude key rule: ASCII alphanumeric unchanged, every other character becomes `-`. The integration test locates the seeded files with `claude_project_dir_in` and parses them with the real parser.

## Safe isolated launch

Builds use the clone's normal Cargo target, without installs:

```sh
cargo build -p digger-desktop --example conversations_gallery
scripts/launch-conversations-fixture.sh prepare
scripts/launch-conversations-fixture.sh gallery --state list-and-detail
```

The launcher creates a fresh root under the **actual `$TMPDIR`**, sets `HOME` and `DIGGER_DATA_DIR` in the child environment before startup, seeds fixtures only, and prints paths and repeatable commands. Inherited `DIGGER_DATA_DIR` is replaced explicitly; inherited `DOZER_DATA_DIR` is removed. GUI gallery startup requires both HOME and the canonical Digger data root below actual TMPDIR. The gallery separately owns its marker inside a temporary directory. Neither program changes the parent environment after threads start. On the validation Mac, `/private/tmp` is **not** the default `$TMPDIR`; use the printed root. No installed application, existing sandbox, automatic import, model invocation, or existing user application is used.

For direct seeding, an argument is mandatory:

```sh
scripts/seed-conversations-fixture.sh "$TMPDIR/my-fresh-conversations-home"
```

The canonical target must be a strict descendant of actual `$TMPDIR`. Root itself, prefix lookalikes, real HOME, outside paths, symlink paths and symlinks within an existing target are rejected before any mkdir/write. The script prints its own synthetic project path. Do not seed a real HOME.

## Digger comparison steps

1. Build the source clone's `digger` and `diggerd` if not already built; the launcher never builds or installs. Run `scripts/launch-conversations-fixture.sh digger` (or use the exact printed command from `prepare`).
2. Confirm the displayed HOME/DIGGER_DATA_DIR are fresh descendants of actual TMPDIR. Open the printed synthetic project and show Conversations.
3. Observe first-import disclosure, then **the user explicitly clicks production history import**. Check success count, zero-result copy where applicable, retry on a deliberately isolated failure, and log disclosure. The seed script does not import.
4. Visit short/tools/empty/broken/hostile, then long/huge. Check search/agent filters, title clipping, selection, detail content, paired tool results, tokens, rendering safety, scrolling and paging. Do not activate hostile links; confirm they are safely rendered/inert through visual inspection and existing security tests.
5. Launch each gallery state separately, using `--list` for all ten. For `agent-filter-open`, click the shared native agent-filter control after the window appears; on macOS check actual NSMenu cancellation/selection while the detail remains visible.
6. Compare at window 1200×800 and UI scales 0.5, 1 and 2 (`DOZER_ICON_SCALE=0.5 scripts/launch-conversations-fixture.sh gallery --state list-and-detail`, then repeat with 1 and 2). Record screenshots and observations only after an actual GUI run. Test Load More visibility/click with the native child present. No screenshot is currently claimed by this Task 4 handoff.

## Dozer comparison steps — user operates Dozer

1. Provide your own compiled Dozer executable. No Dozer GUI binary path is assumed; the temporary geometry probe is not a GUI application. Use `scripts/launch-conversations-fixture.sh dozer --dozer-bin /absolute/path/to/your/built/dozer`. Installed `.app` bundles are refused.
2. Confirm the fresh HOME/DIGGER_DATA_DIR and synthetic project printed by that invocation. Open that project in Dozer. Explicitly import its synthetic history through Dozer's normal UI.
3. Reproduce each row of the state ledger with the corresponding fixture/interaction. For loading/error/unavailable states, use Dozer's own supported workflow in that isolated instance; the gallery does not control Dozer. Capture the exact trigger if a transient state cannot be held.
4. Match window size, scale, theme and panel split with Digger, then compare each state. Check the possible header-padding difference, native bounds, Load More clearance, filter menu, tool traces, hostile content and scrolling. Record differences and user decisions; do not mark parity merely from shared assets or headless geometry.

HOME isolation was verified separately by the root's actual Dozer probe: config starts under `/private/tmp/s7c-dozer-home/Library/Application Support/ai.byteboy.dozer` (`/private/tmp/s7c-dozer-geometry.log`, exit 0). This proves HOME isolation only, **not GUI parity**.

## Existing geometry evidence and pending GUI work

At 1200×800 the actual Dozer headless geometry probe gives native bounds `(700, 84, 274.4, 690)`; Digger's actual shared-shell path gives `(701, 85, 275.2, 689)`, each component within 1 logical pixel. The reference log is `/private/tmp/s7c-dozer-geometry.log`. Headless real-widget clearance has passed at UI scales 0.5/1/2. These checks do not establish native child visibility, native menu appearance, screenshot parity, Load More hit testing under WKWebView, or stress performance.

Task 5 retains real native probing/stress and regression reporting. Task 4 provides fixtures, safe launch commands, and this pending comparison checklist. GUI status remains **UNVERIFIED** until actual isolated windows are observed and evidence is recorded.

## Focused validation commands

Build the example before the launch-script regression tests; those tests execute its headless `--check` mode to inspect the real config-directory resolver.

```sh
cargo build -p digger-desktop --example conversations_gallery
python3 scripts/tests/test_seed_conversations.py
python3 scripts/tests/test_launch_conversations.py
cargo test -p digger-desktop --test conversations_fixtures --example conversations_gallery -- --test-threads=1
cargo clippy -p digger-desktop --all-targets -- -D warnings
cargo fmt --all --check
```

The gallery contract test is run explicitly with `--example`; it is separate from the default workspace test count. The fixture/parser integration target is part of normal Cargo test discovery. The launcher safety tests also prove an unknown state fails before GUI/temp setup and an outside inherited Digger data root is refused before window startup.

## Root's isolated GUI attempt (2026-10-27)

After correcting the canonical `DIGGER_DATA_DIR` override and passing its red/green regression, the root launched only the fixture gallery (PID 67976) through the isolated wrapper, with root `/private/var/folders/9q/8pyxcmxn3_l8r62v4w_gzsnh0000gn/T/s7c-conversations-fktcndb4`. Stderr reported `LSModifyNotification` / `hiservices-xpc` connection invalid; Orca `get-app-state` for that PID exited 1 (`app_not_found`), and a successful app-registry listing had no match for the owned PID. No window or screenshot was observed, no controls were clicked, and no existing user applications were operated. Evidence: `/private/tmp/s7c-gallery-observation/{launch.log,process.json,observation.json,app-registry.json}`. This is an unsuccessful observation attempt in the current environment, not proof of a product rendering failure. **GUI parity remains UNVERIFIED**; every pending state comparison above remains pending.
