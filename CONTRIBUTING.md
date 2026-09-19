# Contributing to Glaux Server

This repository implements the approved Glaux Server plan. These instructions make the existing planning rules easy to find; they do not add a separate requirements or governance system.

## Controlling documents

- [Goal and Definition](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-goal-and-definition.md): intended outcome and scope.
- [Implementation Guide](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md): design, standards interpretations and verification.
- [Roadmap](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-roadmap.md): task scope, dependencies, publication and execution workflow.
- [Initial Planning Guidance](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Governance/initial-planning-guidance.md): planning-document authority and change rules.

Read the relevant sections rather than treating a summary or peer implementation as controlling authority. Goal v1.7 and Guide v1.2 are the preparation baseline; follow subsequent approved revisions. The Roadmap's §5 governs implementation issues; research-report approval procedures are not additional implementation gates. If an issue conflicts with a controlling source, identify the conflict and resolve it through the existing change process instead of silently changing scope.

## Creating implementation issues

Use [the implementation-task template](.github/ISSUE_TEMPLATE/implementation_task.md) for one existing three-level Roadmap task, titled `[1.1.1] Inspect the server checkout and approved prerequisites`, for example. Carry over the leaf's deliverable, scope, acceptance criteria, Guide references and explicit dependencies, along with relevant parent constraints. Parent context does not make the child responsible for every sibling's work.

Record source versions/commits and useful section links. Keep task-specific acceptance and verification concrete: identify the controlling source, independently expected answer, plausible wrong behavior the checks must catch, and applicable failure/access cases. Reference longer common rules rather than copying the whole Guide or the research corpus into each issue. Inspection and verification issues may legitimately deliver evidence rather than new code.

Before implementation, publish the complete initial set of 286 leaf issues under Roadmap §5.1. Inspect existing issues first and avoid duplicates. Resolve referenced capability-group dependencies to their leaf issue links; retain explicit leaf dependencies rather than adding self-parent or arbitrary sibling edges. Verify one-to-one task coverage, populated bodies, links and dependencies, and add each issue link beside its Roadmap task. Issue creation is not task completion.

Automated publication must populate the same Markdown body explicitly, omitting its YAML front matter and authoring comments. GitHub's API does not apply the web template for the publisher. Keep a recoverable record of returned issue numbers during publication; if interrupted, reconcile existing issues before creating more. Temporary unresolved prerequisite numbers must be filled and verified before publication is declared complete. Do not create a separate permanent issue catalog or use fabricated issue links.

## One authorized issue per iteration

For the project lead's assisted workflow, each `proceed` authorizes one dependency-ready implementation issue after complete issue publication. State the selected issue and intended result, inspect the current checkout and prerequisites, implement and verify the bounded change, update relevant documentation, and record evidence. Stop after its handoff; do not continue down the queue automatically.

An oversized or blocked issue remains open. State the precise blocker or remaining work and explicitly correct the issue/Roadmap if necessary; do not omit obligations, close partial work or invent a passed check. Missing tools may be a valid result of the prerequisite-inspection issue, while still blocking later build or database issues.

## Branches, pull requests and review

The project lead selected this policy on September 18, 2026:

- Use one task branch and linked PR per implementation issue, such as `task/1.1.1-prerequisites`; target `main`. Keep unrelated work out of the PR.
- Run applicable checks and obtain an assistant review of the actual diff, acceptance evidence and scope, explicitly including test setup, expected answers, assertions and demonstrated failure sensitivity. Review whether plausible wrong behavior could still pass, rather than accepting test names or a green job label. Record the review and resolve blocking findings; do not represent an assistant review as independent human approval.
- The assistant may merge after task-specific acceptance criteria, applicable checks and review pass. A separate human-approval pause is not mandatory unless the project lead requests it or repository controls require it. Do not bypass protections or merge against a different, unreviewed head.
- Missing required checks are not a pass. For documentation or early prerequisite work before CI exists, record the actual applicable checks and why runtime checks are inapplicable; do not invent a successful CI run.
- Merge before closing the completed issue, then record final commit/PR and verification evidence in it. A failed or blocked PR/issue remains open. Any automatic closing reference must not close incomplete work.

This policy does not itself configure branch protection, required reviewers or CI. Respect whatever controls actually exist. It does not authorize installations, production deployment, new scope or additional issues. The issue-template preparation is a documentation-only setup change, not completion of a Roadmap implementation task.

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
