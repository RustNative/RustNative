# Layer-solidity evidence loop: handover (after iteration 4, 2026-10-21)

The loop pauses here. This file is enough to resume it in a new conversation.

## Where everything is

- **Master plan:** [`../2026-10-15-layer-solidity.md`](../2026-10-15-layer-solidity.md)
  - Part A: the Q1–Q10 method.
  - Part B: universal indicators U1–U37. U29–U33 and the U24 extension come from iteration 2; U34 (slot pools reclaim idle holders) and U35 (credentials are bound to their origin and transport) from iteration 3; U36 (every oracle has a positive witness) and U37 (every check covers the indirect form) from iteration 4.
  - Part D: tasks. Wave 0 holds tasks 0.1–0.10. Tasks 0.11–0.29 are listed in E.7 under each iteration's "Wave 0 additions".
  - Part E.7: the loop record, with results for iterations 1–4, and the **iteration 5 queue**.
- **Findings, one file per layer:**
  - `L0-types.md`, `L1-style.md`, `L2-syntax-build.md`
  - `L3-core.md`, `L3-core-part2.md`, `L3-core-part3.md`
  - `L4-reference.md`, `L5-groups.md`, `L6-backends.md`, `L7-web.md`, `L8-app.md`, `L9-server.md`, `L10-L11-cli-ci.md`
  - Format: rows `L<n>-F<m>` (re-pass rows are `L<n>-RP<m>`) with a severity and file:line evidence. ✅ means reproduced by running code; "(fixed)" marks a finding a merged commit closed. Each layer's file ends with its latest "Iteration" sections and their "Indicators added" lists.
- **Ledger:** `ledger.md` lists 730 files (208 284 lines), each with a status of `unread`, `skimmed`, or `read`. Now: 608 `read` (176 737 lines), 122 `skimmed` (31 547 lines), 0 `unread`. Every `skimmed` row is a test. In-crate modules compiled only under `#[cfg(test)]` are labelled `test`.
- **Tools (outside the repo):** the `rustnative-tools` repository, `layer-solidity/` directory.
  - `mark.py` marks the ledger. Run `RUSTNATIVE_REPO=<repo> python -I mark.py [--skim] <path-prefix>...`, or run it from the repo root. **Caution:** a prefix also upgrades skimmed test files under it to `read`, so pass exact paths, or check the ledger afterwards.
  - Scratch crates `verify8`–`verify13` depend on the repo crates by path. Build them with `CARGO_TARGET_DIR=<repo>/target/verify`. Each `verifyN-*.txt` next to them holds a run's output. `verify14/<scope>/` holds iteration 4's per-reviewer crates, each built with `CARGO_TARGET_DIR=<repo>/target/verify14-<scope>`.
  - Browser reproductions (`verify12/tests/sw_replay.rs`) need `RUSTNATIVE_BROWSER`. In a container running as root, point it at `chrome-nosandbox.sh`, which adds `--no-sandbox`.
  - `verify10` (a GDI handle count while rendering) hung and is unfinished. Do not rerun it as is; drive the renders from a timer with a hard timeout, on Windows.

## Rules the loop runs under

- Change no repository code. Only write plan documents under `docs/superpowers/plans/`, and keep scratch programs outside the repo.
- For each file, ask Q1–Q10, record findings with file:line evidence, and reproduce P0/P1 candidates with a scratch crate.
- Run long builds in the background with timeouts.
- New P0/P1 findings become Wave 0 tasks. New shapes that recur across layers become universal indicators. Update the loop record in E.7.
- **Done** only when both hold:
  - (a) every source row is `read` (tests may stay `skimmed`);
  - (b) one full Q1–Q10 pass finds nothing new, neither a finding nor an indicator.
- Commit with the session's trailer lines, directly to `master`.

## Status

- (a) **holds**: every source row is `read`.
- (b) **fails**: iteration 4 found 1 new P1 (L6-F54), 20 P2s, 20 P3s, 2 universal indicators, and 16 layer indicators. Only L0 and L8 yielded nothing new.

Open P1 tasks are 0.11–0.29, except 0.20 (L6-F16), which is fixed. The P0s are tasks 0.1–0.3. Fixed since iteration 2: L6-F16, L9-F8, L9-F10, L9-F16, and L3-F19.

## Next: iteration 5 (in order)

1. Run a fresh Q1–Q10 pass over every layer, with U36 and U37 as new lenses, as parallel reviews that read each layer's findings file first and report only new defects. Check every reported P0/P1 against the code, and reproduce it, before recording.
2. Read `rustnative-markup`'s `lower.rs`, `rsx_file.rs`, `rsx_scan.rs`, and `parse.rs` in full.
3. Measure L6-F26 on Windows (Task 6.4), and confirm L6-F54–F60 on Windows and Android devices.
4. After each Wave 0 fix merges, rerun its finding's scratch program: it must fail before the fix and pass after it.
5. Update E.7 with an iteration-5 result and check the two conditions. Stop only when a full pass finds nothing new.
