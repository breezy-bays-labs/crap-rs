# Hosted CRAP reports for fork PRs

The hosted per-PR report recipe (epic #377) publishes a clickable HTML
scorecard to GitHub Pages for every pull request and links it from the
production sticky comment. By default this works for **same-repo PRs
only**: a fork PR's `GITHUB_TOKEN` is read-only, so the fork's own CI run
can neither push to `gh-pages` nor post a comment. Such PRs degrade to a
summary-only sticky with no hosted link.

This document describes the **opt-in, advanced** pattern that lifts that
limit so fork PRs get hosted reports too. It is intentionally separate
from the canonical recipe because it relies on a privileged workflow
topology that must be understood before it is copied.

> **You almost certainly do not need this for a repo that never receives
> fork PRs.** The same-repo recipe is the safe default. Enable this only
> when external contributors open PRs from forks and you want them to get
> the same hosted-report experience.

## Why fork PRs are different

CI has to run untrusted code (the PR's diff) but sometimes needs
privilege (push to `gh-pages`, post a comment). Handing a privileged
token to untrusted code is how secrets get exfiltrated, so GitHub
withholds privilege from fork PR runs: a `pull_request` event from a fork
gets a **read-only** token and **no secrets**, and the `permissions:`
block cannot elevate above that ceiling. The workflow file that runs is
the **PR head's** version (attacker-controlled), so a read-only token is
the only safe option.

GitHub's Security Lab calls the class of bug where this protection is
bypassed a **"pwn request."** The pattern below is structured
specifically to avoid it.

## The topology

Three pieces cooperate. Two trust zones never share a token.

```
  Fork PR run (pull_request)            Privileged run (workflow_run)
  ────────────────────────────         ──────────────────────────────
  token: read-only, no secrets         token: read-write, secrets
  YAML:  PR head (untrusted)           YAML:  base branch (trusted)
  sees fork code: yes (it IS it)       sees fork code: NEVER
  ───────────────────────────────────────────────────────────────────
  build + render report                download the artifact
  upload handoff ARTIFACT  ───────►    VALIDATE everything in it
  (report.html, pr_number, body)       publish to gh-pages pr-<N>/
  skip the (doomed) sticky post        post the production sticky
```

One artifact crosses the boundary, in one direction, and the privileged
side treats it as hostile input. The decoupling prevents code execution;
the validation prevents injection through the data channel. **Both halves
matter** — a `workflow_run` job that blindly trusts its downloaded
artifact is still exploitable.

### 1. Producer — the scorecard action (`fork-handoff: true`)

The `scorecard` composite action's `fork-handoff` input (default
`false`) is the producer half. On a fork PR with `html-report: true` and
`comment-mode: sticky`, it:

- stages a self-contained handoff bundle: the rendered `report.html`, the
  PR number, and the composed (link-free) sticky body;
- uploads it as the `crap-scorecard-pages-handoff` artifact;
- **skips** posting the sticky comment (a fork's read-only token would
  403 and red the job).

The sticky body is staged **without** a report link on purpose: the
privileged consumer appends the hosted Pages URL, not the
artifact-download URL the inline path would otherwise emit.

This input is a no-op for same-repo PRs, push events, and every consumer
that does not set it — output is byte-identical to before it existed.

### 2. Privileged consumer — `pages-publish-fork.yml`

A `workflow_run` workflow that fires after CI completes, runs in the base
repo's trusted context, and:

- gates to **fork PRs whose CI succeeded** — on the `github.event.workflow_run`
  payload: `event == 'pull_request'`, `conclusion == 'success'`, and
  `head_repository.full_name != repository.full_name` (the fork gate is
  also the dedup that stops it double-publishing same-repo PRs);
- downloads the handoff artifact from the triggering run (best-effort —
  a fork PR that produced no report is a clean no-op);
- **guards the PR number twice** before using it: (1) it must be a run of
  digits — a value like `../../evil` would otherwise be a path-traversal
  write into `gh-pages` when it becomes `pr-<N>/`; and (2) it is **bound to
  the triggering run's identity** — PR N's head SHA and head repo (read
  from the trusted GitHub API) must equal the `workflow_run` payload's
  `head_sha` + `head_repository`. A fork controls its entire run, so a
  numeric-but-arbitrary number could otherwise spoof a maintainer's PR
  (post the production sticky there, overwrite its `gh-pages` dir); guard
  2 rejects any PR this run did not actually produce;
- publishes `report.html` to `gh-pages` at `pr-<N>/index.html`;
- posts the production sticky from the trusted context, with the hosted
  link appended.

It **never checks out or runs the fork's code.** Its only inputs are the
artifact's contents, each validated before use.

### 3. Cleanup — `pages-cleanup.yml` (`pull_request_target`)

When a PR closes, the cleanup workflow removes `pr-<N>/` from `gh-pages`.
For fork closes to push the deletion it needs a write token, so the
cleanup triggers on `pull_request_target` rather than `pull_request`.

This is the **safe** use of `pull_request_target`: the cleanup job checks
out no code (base or fork) and uses exactly one input — the PR number,
validated numeric — to delete a directory. The "pwn request" footgun
applies only when you check out *and run* head code under this trigger;
a metadata-only job does not.

## Enabling it on your own repo

1. Have the same-repo hosted-report recipe working first (GitHub Pages
   enabled on `gh-pages`, the push-to-main job seeding the root report and
   baseline). Fork reports publish into the same branch and depend on it
   existing.
2. In your PR scorecard job, set `fork-handoff: true` on the `scorecard`
   action (alongside `html-report: true` and `comment-mode: sticky`). Keep
   `pages-publish` gated to same-repo PRs — the two are complementary.
3. Copy `pages-publish-fork.yml` into `.github/workflows/`. Confirm its
   `on.workflow_run.workflows` value matches your CI workflow's `name:`.
4. Switch `pages-cleanup.yml` from `pull_request:` to
   `pull_request_target:` (the numeric PR-number guard is already in
   place).
5. If you serve Pages from a **custom domain** or a user/organization
   site (not the default `https://<owner>.github.io/<repo>/`), adjust the
   one `PAGES_URL` line in `pages-publish-fork.yml`.

## Security invariants (do not regress)

- The privileged workflow **never** runs `actions/checkout` of the PR
  head, and never executes any fork-supplied script.
- Every value read from the handoff artifact is treated as untrusted. The
  PR number is validated `^[0-9]+$` (path-traversal) **and** bound to the
  triggering run's head SHA + head repo via the trusted GitHub API
  (identity-spoofing) before it is used as a path or a comment target.
  Both guards are load-bearing; do not drop either.
- Tokens flow through `env:`, never interpolated into a `run:` script
  (template-injection hygiene).
- The fork gate (`head_repository != repository`) is what prevents
  double-publishing same-repo PRs; do not loosen it.
