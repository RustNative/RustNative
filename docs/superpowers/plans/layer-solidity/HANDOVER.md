# Layer-solidity evidence loop: handover (stopped after iteration 5, 2026-10-21)

> [!IMPORTANT]
> **Before acting on any finding, read the "⚠ READ BEFORE ACTING ON ANY FINDING: patch or redesign" block at the top of [`../2026-10-15-layer-solidity.md`](../2026-10-15-layer-solidity.md). It is binding.** The codebase needs targeted redesigns, not a ground-up rearchitecture and not only patches. **Any part may be redesigned when that block's criteria hold; its six named areas are examples, not a limit.** Never downgrade a redesign to a patch to keep a diff small.

The loop was stopped on request after iteration 5, before its second termination condition held. This file is enough to resume it in a new conversation.

## Where everything is

- **Master plan:** [`../2026-10-15-layer-solidity.md`](../2026-10-15-layer-solidity.md)
  - Part A: the Q1–Q10 method.
  - Part B: universal indicators U1–U38. U20–U28 come from iteration 1; U29–U33 and the U24 extension from iteration 2; U34–U35 from iteration 3; U36–U37 from iteration 4; U38 from iteration 5.
  - Part D: tasks. Wave 0 holds tasks 0.1–0.10. Tasks 0.11–0.31 are listed in E.7 under each iteration's "Wave 0 additions".
  - Part E.7: the loop record, with a result for every iteration.
  - Part E.8: where the loop stopped, a per-iteration summary, and every P0 and P1 with its status.
  - Appendix 4: the findings register, every row from every layer file (397 rows).
- **Findings, one file per layer:**
  - `L0-types.md`, `L1-style.md`, `L2-syntax-build.md`
  - `L3-core.md`, `L3-core-part2.md`, `L3-core-part3.md`
  - `L4-reference.md`, `L5-groups.md`, `L6-backends.md`, `L7-web.md`, `L8-app.md`, `L9-server.md`, `L10-L11-cli-ci.md`
  - Format: rows `L<n>-F<m>` (re-pass rows are `L<n>-RP<m>`) with a severity and file:line evidence. ✅ means reproduced by running code; "(fixed)" marks a finding a merged commit closed. Each file has one section per iteration, each ending with its "Indicators added" list.
- **Ledger:** `ledger.md` lists 730 files (208 284 lines). 608 are `read` (176 737 lines) and 122 `skimmed` (31 547 lines); every `skimmed` row is a test. In-crate modules compiled only under `#[cfg(test)]` are labelled `test`.
- **Tools (outside the repo):** the `rustnative-tools` repository, `layer-solidity/` directory.
  - `mark.py` marks the ledger: `RUSTNATIVE_REPO=<repo> python -I mark.py [--skim] <path-prefix>...`. A prefix also upgrades skimmed test files under it, so pass exact paths.
  - Scratch crates `verify8`–`verify13` build with `CARGO_TARGET_DIR=<repo>/target/verify`. `verify14/<scope>/` and `verify15/<scope>/` hold the per-reviewer crates of iterations 4 and 5, each built with `CARGO_TARGET_DIR=<repo>/target/verify14-<scope>` or `verify15-<scope>`. Output files sit next to each program.
  - Browser reproductions need `RUSTNATIVE_BROWSER`. In a container running as root, point it at `chrome-nosandbox.sh`.
  - `verify10` (a GDI handle count) hung; drive renders from a timer with a hard timeout, on Windows.

## Rules the loop runs under

- Change no repository code. Only write plan documents under `docs/superpowers/plans/`, and keep scratch programs outside the repo.
- For each file, ask Q1–Q10, record findings with file:line evidence, and reproduce P0/P1 candidates with a scratch crate.
- Parallel reviews read each layer's findings file first and report only new defects; check every reported P0/P1 against the code, and read its reproduction output, before recording.
- New P0/P1 findings become Wave 0 tasks. Shapes that recur across layers become universal indicators. Update E.7 and E.8.
- **Done** only when both hold:
  - (a) every source row is `read` (tests may stay `skimmed`);
  - (b) one full Q1–Q10 pass finds nothing new, neither a finding nor an indicator.
- Commit with the session's trailer lines, directly to `master`.

## Status

- (a) **holds**.
- (b) **does not hold**. P1s per iteration: 9 (iteration 2, session 3), 4, 1, 2. Iteration 5 also found 23 P2s and 32 P3s, so the loop has not converged.

Open P1 tasks are 0.11–0.31, except 0.20 (L6-F16), which is fixed. The P0s are tasks 0.1–0.3. Fixed by merged commits: L6-F16, L9-F8, L9-F10, L9-F16, and L3-F19.

## Next, if the loop resumes: iteration 6 (in order)

1. Run a fresh Q1–Q10 pass over every layer with U38 as a new lens, as parallel reviews.
2. Confirm the Windows and Android findings on devices (L6-F54–F60, L6-F66–F75), and measure L6-F26 on Windows (Task 6.4).
3. After each Wave 0 fix merges, rerun its finding's scratch program: it must fail before the fix and pass after it.
4. Update E.7, E.8, and Appendix 4 (regenerate it from the layer files), and check the two conditions.
