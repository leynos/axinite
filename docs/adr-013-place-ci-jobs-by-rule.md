# Architectural decision record (ADR) 013: Place CI jobs by rule

## Status

Accepted. Jobs are placed on runners by what they do, not by a list: build and
test jobs on right-sized Ubicloud runners, short utility jobs that wait on
GitHub's hosted pool on the smallest Ubicloud runner, and everything else on
GitHub-hosted runners, with the contracts in `tests/workflow_contracts/`
enforcing each rule.

## Date

2026-09-25.

## Context and problem statement

Until August 2026 every Linux job ran on `ubicloud-standard-8`, whatever it
did. The placement work that followed (pull requests #366 and #372) moved to
rules: a job may use a paid Ubicloud runner only when it builds or tests the
product, each such job holds a reviewed shape, cron work and Windows jobs stay
GitHub-hosted, and a pull-request lane falls back to a hosted runner for a
fork, which cannot obtain an Ubicloud runner. Those rules lived in the
developers' guide and the contracts, and no ADR recorded them.

On 25 September 2026 the user ruled that short utility jobs must not queue
behind GitHub's hosted-runner contention: they run on the smallest Ubicloud
runner, and on ARM (`ubicloud-standard-2-arm`) when the job does not depend on
the CPU architecture. The lead clarified that contention is the only reason to
move a job, so a utility job that does not wait stays hosted.

## Decision drivers

- A developer waits on pull-request checks. A six-second check that queues for
  twelve minutes costs the wait, not the minutes.
- Paid minutes should buy either compilation or the removal of a wait.
- Placement must be decided by a rule a contract can check, so a new job is
  judged the day it is written.

## Options considered

### Option A: an exhaustive per-job runner table

Pull request #322 proposed a table recording the runner of every job. It fails
on every placement change by design, it pinned `ubicloud-standard-8`
throughout, and it states what each job uses, not why. Closed unmerged.

### Option B: rules, with a named utility set

Place by rule, and list only the exceptions a rule cannot derive: the utility
jobs that move for contention, each with its measured wait.

## Decision outcome

Option B.

- A job that builds or tests the product runs on a right-sized Ubicloud label
  (`ubicloud-standard-4` for workspace compilation, `ubicloud-standard-2` for
  little or none), recorded with its measurement in `runner_sizing_test.py`.
- A pull-request lane on Ubicloud falls back to `ubuntu-latest` for a fork.
- Scheduled workflows and Windows jobs stay GitHub-hosted.
- A short utility job moves to the smallest Ubicloud shape only when the
  hosted pool makes it wait: `ubicloud-standard-2-arm` when it is
  architecture-independent, `ubicloud-standard-2` otherwise. The moved set,
  with the measurements below, is named in `_utility_jobs.py`, and
  `runner_placement_test.py` holds each to its shape.
- Proof lanes stay hosted.

Measured from the jobs API over each workflow's last ten completed runs
(duration, then the wait before the job started, in seconds):

| Job                                           | Trigger               | Duration | Hosted wait             | Placement                                    |
| --------------------------------------------- | --------------------- | -------- | ----------------------- | -------------------------------------------- |
| `pr-label-classify.yml` `classify`            | `pull_request_target` | 13-20    | up to 2,098, median 316 | `ubicloud-standard-2-arm`                    |
| `pr-label-scope.yml` `scope`                  | `pull_request_target` | 4-7      | up to 1,495             | `ubicloud-standard-2-arm`                    |
| `regression-test-check.yml` `regression-test` | `pull_request`        | 6-8      | up to 734               | `ubicloud-standard-2-arm`, hosted for a fork |
| `audit.yml` `audit`                           | schedule              | 21-33    | 1-2                     | stays `ubuntu-latest`                        |

_Table 1: The utility jobs and why each is placed where it is._

The scheduled audit waits one or two seconds on the hosted pool, so there is
nothing to buy back; it stays hosted, and the scheduled-placement rule is
unchanged.

## Known risks and limitations

- The two labelling workflows run on `pull_request_target`, which executes the
  base branch's workflow. Their ARM placement is first exercised by the first
  pull request after it merges.
- `pull_request_target` runs in this repository's context for a fork too, so
  those two jobs take no fork fallback. They check out the base branch, never
  the fork's code.
