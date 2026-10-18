# Layer-solidity evidence loop: handover (after iteration 2, 2026-10-16)

The loop pauses here. This file is enough to resume it in a new conversation.

## Where everything is

- **Master plan:** [`../2026-10-15-layer-solidity.md`](../2026-10-15-layer-solidity.md)
  - Part A: the Q1–Q10 method.
  - Part B: universal indicators U1–U33. U29–U33 and the U24 extension come from iteration 2.
  - Part D: tasks. Wave 0 holds tasks 0.1–0.10; tasks 0.11–0.24 are listed in E.7 under "Wave 0 additions".
  - Part E.7: the loop record, with results for iteration 1, iteration 2 (so far) and iteration 2 (session 3), and the **iteration 3 queue**.
- **Findings, one file per layer:**
  - `L0-types.md`, `L1-style.md`, `L2-syntax-build.md`
  - `L3-core.md`, `L3-core-part2.md`, `L3-core-part3.md`
  - `L4-reference.md`, `L5-groups.md`, `L6-backends.md`, `L7-web.md`, `L8-app.md`, `L9-server.md`, `L10-L11-cli-ci.md`
  - Format: rows `L<n>-F<m>` (re-pass rows are `L<n>-RP<m>`) with a severity and file:line evidence. ✅ means reproduced by running code. Each file ends with its "Indicators added" lists.
- **Ledger:** `ledger.md` lists 721 files (205 477 lines), each with a status of `unread`, `skimmed` or `read`. Now: 435 `read`, 286 `skimmed`, 0 `unread`.
- **Skimmed source files still to read:** `skimmed-src.tsv` has 158 rows (layer, lines, path):
  - L6: 74 files
  - L7: 29 files
  - L10: 43 files
  - L8: 12 files
- **Tools (outside the repo):** `E:/Users/HP/Documents/Projects/rustnative-tools/layer-solidity/`
  - `mark.py` marks the ledger. Run `python -I mark.py [--skim] <path-prefix>` from the repo root. **Caution:** a prefix also upgrades skimmed test files under it to `read`, so check the ledger afterwards.
  - Scratch crates `verify8`–`verify11` depend on the repo crates by path and share the `target` directory. Earlier crates, `verify`–`verify7`, sit next to them as described in E.7.
  - `verify10` (a GDI handle count while rendering) hung and is unfinished. Do not rerun it as is; drive the renders from a timer with a hard timeout.
  - `l*-s3*.md` are staged copies of blocks that are already merged into the findings files. They are safe to delete.

## Rules the loop runs under

- Change no repository code. Only write plan documents under `docs/superpowers/plans/` and scratch programs outside the repo.
- For each file, ask Q1–Q10, record findings with file:line evidence, and reproduce P0/P1 candidates with a scratch crate.
- Run long builds in the background with timeouts.
- New P0/P1 findings become Wave 0 tasks. New shapes that recur across layers become universal indicators. Update the loop record in E.7.
- **Done** only when both hold:
  - (a) every source row is `read` (tests may stay `skimmed`);
  - (b) one full Q1–Q10 pass finds nothing new, neither a finding nor an indicator.
- Commit only on request, directly to master, with a Co-Authored-By trailer. **Nothing from this loop is committed yet:** `docs/superpowers/plans/2026-10-15-layer-solidity.md` and `layer-solidity/` are untracked.

## Status

Neither termination condition holds:
- (a) fails: 158 source files are only skimmed.
- (b) fails: iteration 2 found 9 new P1s and 5 new indicators.

Open P1 tasks are 0.11–0.24. The P0s are tasks 0.1–0.3.

## Next: iteration 3 (in order)

1. Read every file in `skimmed-src.tsv` in full, in this order:
   - L7: the rest of `rn.js`; `client.rs`, `dom.rs` and `wasm.rs`; the webgen translator.
   - L10: the CLI, especially `package/*`, `deploy`, `emulate/*`, `generate` and `project/templates`.
   - L6: the remaining Android modules; all terminal-backend source; Windows `uia/provider`, `uia/properties`, `input/pointer` and `graphics/d2d`; GTK rendering.
   - L8: `components/*`.
2. Confirm these by running code:
   - L6-F26: the GDI handle count over many colours.
   - L6-F16: `open_url` with a non-web target.
   - L7-F6: an offline replay across a sign-out.
3. Re-check every layer against U29–U33, the way L0–L5 were re-checked against U20–U28.
4. Mark each file in the ledger, update E.7 with an iteration-3 result, and check the two conditions.
