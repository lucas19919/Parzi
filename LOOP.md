# Parzi — Execution Loop (front to back, P0 → P10)

> How PLAN.md gets built without becoming a mess. One phase at a time,
> each phase runs BUILD → VERIFY → FIX (max 3) → REVIEW → ACCEPT.
> No phase starts unless the prior one is ACCEPTED. No commits unless asked.

## 0. State (resume-safe)

- `PROGRESS.md`: one row per phase — `status: pending|building|verifying|review|accepted|blocked`,
  attempts used, evidence (test/clippy output tail), timestamp.
- Loop reads `PROGRESS.md` on start, resumes at first non-`accepted` phase.
- Human gates: P3 (credential read-only confirm), P7 (screenshot parity),
  P9 (updater signing). Loop HALTS at gates, never auto-skips.

## 1. Per-phase cycle

```
SPEC    — orchestrator extracts scope + files + acceptance from PLAN.md §12.
BUILD   — one builder subagent, scoped prompt: files allowed, files forbidden,
          acceptance criteria verbatim, quality bars (§11) verbatim.
VERIFY  — orchestrator runs, in order:
          1. cargo test -p <crate(s)>          (must pass)
          2. cargo clippy -p <crate(s)> --all-targets -- -D clippy::correctness
             -D clippy::suspicious -D clippy::complexity -D clippy::perf
             (must be clean; style/pedantic stay advisory warns — JSON
             `and_then(|x| x.as_str())` idiom is exempt by decision)
          3. phase acceptance check from §12 (e.g. `parzi doctor` runs, mock stream green)
FIX     — on VERIFY fail: one fix subagent gets ONLY the failure log + the files
          it may touch. Max 3 attempts per phase. After 3rd fail → status=blocked,
          loop HALTS, report to human with logs. Never retry silently.
REVIEW  — elegance gate (no new run needed, static):
          files <400 lines · no `unwrap` outside tests · no new crates ·
          deps point inward only · PLAN.md/PROGRESS.md updated.
ACCEPT  — status=accepted + evidence appended. Next phase.
```

## 2. Builder prompt template (every BUILD uses this shape)

> Build PLAN.md phase <PX> only. Allowed files: <list>. Forbidden: everything else,
> no new crates, no GUI work in core, no secrets in code/logs.
> Acceptance: <verbatim from §12>. Quality bars: <verbatim from §11>.
> Verify yourself with the phase's cargo commands before returning.
> Return: files changed, test output, clippy output, acceptance evidence.

## 3. Fix prompt template (every FIX uses this shape)

> Phase <PX>, attempt <n>/3 FAILED. Failure log: <tail>. You may touch ONLY
> <files from BUILD>. Do not expand scope. Fix, re-run the failing command,
> return the new output. If the design itself is wrong, say BLOCKED + why —
> do not hack around PLAN.md.

## 4. Phase → verify map

| Phase | Verify |
|---|---|
| P0 skeleton | `cargo check --workspace`, `parzi doctor` exits 0, window opens |
| P1 core store | `cargo test -p parzi-core`, round-trip test (save→load→equal) |
| P2 providers | `cargo test -p parzi-providers` (mocked SSE), catalog cache test |
| P3 bespoke | `auth_status` matrix test (no live creds), adapter unit tests |
| P4 runtime | kill-mid-tool test, fork-equality test, budget-stop test |
| P5 MCP | spawn→list→call→idle-kill test, allowlist-deny test |
| P6 CLI | scripted `list/show/export/send/fork/kill/doctor` run |
| P7 UI | `cargo check`, screenshot parity (human gate), low-mem toggle |
| P8 md+widgets | widget schema tests, invalid-JSON-falls-back test, diagram cap test |
| P9 settings | toml round-trip via UI commands, diagnostics copy test |
| P10 harden | full `cargo test`, full clippy, 10k-line perf, release size check |

Human gates halt BEFORE accept: P3 (confirm read-only creds), P7 (screenshot),
P9 (signing keys). All other phases auto-advance on green.

## 5. Stop rules

- 3 failed FIX attempts in one phase → `blocked`, halt, report.
- Any step touches a forbidden file → reject, retry counts as an attempt.
- Secrets in output/logs → halt immediately, scrub, report.
- Antigravity adapter fails → isolate: mark P3-partial, continue others, report.
- Timeout per BUILD: 15 min. Per FIX: 10 min. Overrun = failed attempt.

## 6. Evidence format (PROGRESS.md row)

```
## P2 — accepted (2026-09-09, attempts: 1)
- test: parzi-providers 14 passed
- clippy: clean
- acceptance: mocked SSE stream green, catalog cached
```

## 7. Start command

Orchestrator runs phases strictly P0→P10. First action on start: read
PROGRESS.md, then SPEC for the first non-accepted phase. Nothing else runs first.
