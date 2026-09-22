# Contributing to Glaux Server

This repository implements the approved Glaux Server plan. These instructions make the existing planning rules easy to find; they do not add a separate requirements or governance system.

## License and contributions

Original Glaux Server code and accompanying documentation in this repository use [Apache-2.0](LICENSE), unless explicitly identified otherwise. Contributions intentionally submitted for inclusion follow section 5 of that licence; identify any different terms or third-party material before inclusion, and contribute only material you are authorised to submit. No separate contributor agreement or copyright assignment is introduced here.

Keep third-party licences and notices with their source material; the project licence does not relicense dependencies or the OGC standards/schema corpus. Original workspace package metadata uses the SPDX expression `Apache-2.0`, not `MIT OR Apache-2.0`. The initial workspace has no third-party Cargo dependencies. Maintain the [dependency/licence inventory](docs/dependencies.md), including the pinned workflow actions and separately licensed database-image components, as later tasks add dependencies.

## Controlling documents

- [Goal and Definition](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-goal-and-definition.md): intended outcome and scope.
- [Implementation Guide](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md): design, standards interpretations and verification.
- [Roadmap](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-roadmap.md): task scope, dependencies, publication and execution workflow.
- [Initial Planning Guidance](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Governance/initial-planning-guidance.md): planning-document authority and change rules.

Read the relevant sections rather than treating a summary or peer implementation as controlling authority. Goal v1.7 and Guide v1.2 remain the historical preparation baseline of the initial issues; current planning is Goal v1.10, Guide v1.21 and Roadmap v1.37. Follow their approved revisions and dated issue amendments. The Roadmap's §5 governs implementation issues; research-report approval procedures are not additional implementation gates. If an issue conflicts with a controlling source, identify the conflict and resolve it through the existing change process instead of silently changing scope.

## Creating implementation issues

Use [the implementation-task template](.github/ISSUE_TEMPLATE/implementation_task.md) for one existing three-level Roadmap task, titled `[1.1.1] Inspect the server checkout and approved prerequisites`, for example. Carry over the leaf's deliverable, scope, acceptance criteria, Guide references and explicit dependencies, along with relevant parent constraints. Parent context does not make the child responsible for every sibling's work.

Record source versions/commits and useful section links. Keep task-specific acceptance and verification concrete: identify the controlling source, independently expected answer, plausible wrong behavior the checks must catch, and applicable failure/access cases. Reference longer common rules rather than copying the whole Guide or the research corpus into each issue. Inspection and verification issues may legitimately deliver evidence rather than new code.

The initial 286 leaf issues are published. Before implementation, complete and verify issue coverage for approved scope additions under Roadmap §5.1; the Roadmap records the current total and links, including the 16 Part 5 additions. Inspect existing issues first and avoid duplicates. Resolve referenced capability-group dependencies to their leaf issue links; retain explicit leaf dependencies rather than adding self-parent or arbitrary sibling edges. Verify one-to-one task coverage, populated bodies, links and dependencies, and add each issue link beside its Roadmap task. Preserve existing task IDs and original issue preparation pins when adding dated amendments. Issue creation is not task completion.

Automated publication must populate the same Markdown body explicitly, omitting its YAML front matter and authoring comments. GitHub's API does not apply the web template for the publisher. Keep a recoverable record of returned issue numbers during publication; if interrupted, reconcile existing issues before creating more. Temporary unresolved prerequisite numbers must be filled and verified before publication is declared complete. Do not create a separate permanent issue catalog or use fabricated issue links.

## One authorized issue per iteration

For the project lead's assisted workflow, each `proceed` authorizes one dependency-ready implementation issue after complete issue publication. State the selected issue and intended result, inspect the current checkout and prerequisites, implement and verify the bounded change, update relevant documentation, and record evidence. Stop after its handoff; do not continue down the queue automatically.

An oversized or blocked issue remains open. State the precise blocker or remaining work and explicitly correct the issue/Roadmap if necessary; do not omit obligations, close partial work or invent a passed check. Missing tools may be a valid result of the prerequisite-inspection issue, while still blocking later build or database issues.

## Initial GitHub-hosted build and test path

On 21 September 2026 the project lead selected GitHub-hosted Linux for initial builds/tests. The company laptop is an editing/Git interface, not a required Rust/database host. See [setup inspection and limits](docs/setup.md) and Roadmap v1.37. This does not remove the eventual native/Compose reference instructions.

- #4 establishes the three-package workspace and minimum PR workflow that actually builds/tests it on a hosted Linux runner. It records the tested Rust toolchain/lockfile, runner image and action pins, suite outcomes and the existing disposable failing-assertion proof. This is the narrow exception to its original CI exclusion.
- #5 extends that workflow only enough to run its isolated pinned PostgreSQL/PostGIS harness and required lifecycle/failure checks. It needs no permanent cloud database or local installation.
- #6 extends the existing workflow into the complete initial check suite, clean-setup reproduction, dependency/licence inventory, full false-green checks and actual required-check enforcement. Earlier jobs are not completion of #6. The order and prerequisites remain #3 → #4 → #5 → #6.

When each owning issue is authorised, provision only its needed pinned tools and disposable services inside GitHub-hosted jobs, using synthetic test data and least-privilege permissions; never operational credentials or a user database. Record actual execution on the reviewed head before merging code. Actions being enabled is not a passing run; missing/failed runs leave the issue open. No company-laptop installation, paid runner upgrade, Oracle/Fly provisioning, self-hosted runner or persistent deployment is authorised by this choice. If the selected route actually fails, record the specific failure before proposing a different environment.

## Branches, pull requests and review

Use the complete [build/test commands](docs/ci.md#reproduce-the-checks) when changing the workspace. The checked-in workflow runs on the exact PR head, not the synthetic merge commit. The active main-branch ruleset separately requires its successful, up-to-date result. Any base movement must be reconciled and re-tested before merge. Cargo's normal workspace test command includes applicable doctests. The domain/standards libraries currently contain no behavior or executable examples; zero tests there are disclosed, not a conformance claim. The required executable regression is checked for discovery and actual execution.

The small `scripts/check-bootstrap.py` inventory checks this initial package graph. Update it, `scripts/check-execution.py`, the failure controls and dependency inventory with the controlling Guide and review when a later issue introduces legitimate dependencies or tests. Do not delete or weaken a guard merely to make an unexpected dependency or missing test pass.

For database-harness changes, also run the [disposable database tests](docs/database-tests.md) in the authorised hosted Linux environment. They use Python's standard library and the pinned image's psql, not an application SQLx adapter. Keep the exact image/version checks, owned-target validation, fixture/reset isolation and fatal setup/cleanup errors; never supply a user database or broaden cleanup to unrelated containers/volumes. A Rust-only green result does not cover these required database checks.

The project lead selected the branch/PR policy on September 18, 2026 and approved the explicit separate-review procedure and required-check enforcement decision on September 21, 2026:

- Use one task branch and linked PR per implementation issue, such as `task/1.1.1-prerequisites`; target `main`. Keep unrelated work out of the PR.
- Run applicable checks and obtain a separate assistant review using the procedure below. Review the actual diff, acceptance evidence and scope, explicitly including test setup, expected answers, assertions and demonstrated failure sensitivity. Review whether plausible wrong behavior could still pass, rather than accepting test names or a green job label. Record the review and resolve blocking findings; do not represent an assistant review as independent human approval.
- The assistant may merge after task-specific acceptance criteria, applicable checks and review pass. A separate human-approval pause is not mandatory unless the project lead requests it or repository controls require it. Do not bypass protections or merge against a different, unreviewed head.
- Missing required checks are not a pass. For documentation or early prerequisite work before CI exists, record the actual applicable checks and why runtime checks are inapplicable; do not invent a successful CI run.
- Merge before closing the completed issue, then record final commit/PR and verification evidence in it. A failed or blocked PR/issue remains open. Any automatic closing reference must not close incomplete work.

### Separate assistant review

1. Once the change and applicable check evidence are ready, the implementing assistant launches a separate reviewer agent/session without waiting for another user prompt. The reviewer must not be the agent that authored the change. Give it the issue and approved amendments, relevant controlling sources, base/head commits, actual changed files/diff and check results. It must inspect those materials, not merely endorse the implementer's summary. Keep review scoped to this change and its affected behavior; do not reopen the whole-project review.
2. The reviewer reports concrete findings with locations and reasons, or explicitly reports no blocking findings, and states any incomplete coverage or unverified assumptions. Record its agent/session identity, provider/model when actually known (otherwise say not exposed), reviewed head commit SHA, outcome and finding resolutions in the PR description or a linked PR comment. A separate session using the same model can share blind spots; it is not independent human approval or a formal GitHub approval unless such an approval actually exists.
3. Address blocking findings and rerun affected checks. After any further change, obtain reviewer coverage of the additional diff and its effect on the earlier review, and record the new reviewed head SHA. Recheck the live PR head and applicable checks immediately before merging; use an expected-head guard where supported. Do not merge a different, unreviewed commit.
4. If the review cannot be launched, does not return, is interrupted or incomplete, or leaves a blocking finding unresolved, keep the PR open and report the precise remaining work. Do not substitute the author's own review, silence, a pending request or an unchecked checklist for completion. Resume from the recorded commit/evidence after an interruption rather than silently skipping the step.

[AGENTS.md](AGENTS.md) makes this procedure a persistent instruction for assistant sessions. The PR holds the review record; the issue's execution record links it. Existing published issues already link these current contributor instructions and need not have their historical bodies rewritten to repeat the procedure. This is session-triggered review, not an enabled GitHub AI service or a mechanically enforced review-completion gate.

### Required automated checks

The project lead approved requiring PRs and passing automated checks on `main`, with assistant merging after checks and review, no new mandatory human-approval pause, and no routine bypass. [Ruleset 23796335](https://github.com/DGIWG-P507/glaux-server/rules/23796335) implements that decision: PRs, up-to-date `Rust bootstrap` from GitHub Actions app 15368, no bypass actors, no deletion or non-fast-forward main update, and zero required approvals. [Issue #6](https://github.com/DGIWG-P507/glaux-server/issues/6) / [PR #314](https://github.com/DGIWG-P507/glaux-server/pull/314) record authenticated effective settings and blocked merge states for failed and absent check results.

The historical job name now includes formatting, Clippy, build, actual initial Rust/database execution, nine false-green controls and inventory. Follow [the check and enforcement instructions](docs/ci.md), including their limits: GitHub accepts skipped/neutral check conclusions, so an unconditional job, execution guards and review of workflow changes remain essential. Do not treat absent, unexpectedly empty, filtered or skipped required execution as success. The earlier pre-CI exception is historical, not an exemption for new documentation or code changes. Recheck live settings/results when delivering; a recorded configuration is not assurance that settings can never change. No GitHub-triggered AI review service, additional required human reviewer or production deployment is introduced.

## Verification, safety and completion

Apply Roadmap §§2–3, 5, 7–8 and Guide §§7–11, particularly §8.1.1's test-quality rules:

- Use relevant positive, negative, access and failure tests with source-derived, independently expected results: exact identities, values and forbidden side effects or disclosures where applicable, not just status codes or counts. Routine tests use synthetic data and approved isolated services, not operational systems. Do not automatically approve server-generated golden outputs or weaken expected results to make a test pass; explain and review legitimate expectation changes against their source.
- For new behavior or a fix, normally establish a failing behavioral assertion before implementation, then make it pass and rerun affected checks. Setup/build failure is not this behavioral "red." For already-correct behavior, document an appropriate alternative, such as a controlled fault demonstrating that the assertion detects the wrong behavior; documentation or inspection tasks use their actual applicable evidence instead of invented runtime tests.
- Apply targeted fault/mutation checks and applicable property/fuzz tests under Guide §8.1.1 to the existing owning tasks. Establish that important mistakes are detected; no mutation/coverage score or test-count quota substitutes for correctness, and no new tool installation is authorized here.
- Arrange test cleanup when resources are created. Stop on setup/reset failures, report cleanup failures, isolate mutable fixtures and record the seeds/clocks/dependency versions needed to reproduce failures.
- Confirm required suites actually execute in the stated configuration. Missing, unexpectedly empty, filtered-out or skipped required checks are not passes. Preserve initial failures and retry outcomes; rerunning until green does not resolve unexplained flakiness or prove correctness.
- Preserve existing user changes and data. Respect organizational installation policy; do not install tools, reset persistent databases, expose credentials or operate external systems implicitly.
- Update affected README/help/API examples and any contributor/assistant instructions in the same issue. Link explanations to controlling sources; no assistant-specific tool or document set is required.
- Record the relevant commit/configuration and what actually ran. Distinguish passed, failed, not run and justified inapplicability; retain known unrelated failures explicitly. Do not claim conformance, performance or completion without applicable evidence.

The issue's execution record or a linked comment carries the result: changes, PR/final commit, commands/results, limitations, review outcome and the next ready issue. Explain in plain language what was proved, why the relevant wrong behavior would be caught, what actually ran and what remains unknown. Ordinary test output and a concise summary are enough; no additional evidence platform is required.
