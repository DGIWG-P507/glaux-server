# Glaux Server assistant instructions

Read [CONTRIBUTING.md](CONTRIBUTING.md) before working. It links the controlling Goal, Implementation Guide and Roadmap; use their current approved instructions and any dated issue amendments, not conversation memory as authority.

- Work on one expressly authorised, dependency-ready issue per iteration, on its own branch and PR. Stop after the handoff; `proceed` is not permission to run the remaining backlog.
- **Before starting any task, check the open [`review-gate` issues](https://github.com/DGIWG-P507/glaux-server/issues?q=is%3Aopen+label%3Areview-gate).** All fifteen gates are open until the project lead closes them.
  - Do not start a task inside an open gate's blocked range (see [CONTRIBUTING](CONTRIBUTING.md#review-gates)). Skip those tasks when naming the next ready issue.
  - If no unblocked task is ready, stop and tell the project lead that the lowest-numbered open gate is next.
  - If you cannot check the gates, do not start a task.
  - Never close a gate; only the project lead does.
- After the change and applicable check evidence are ready, launch a **separate reviewer agent/session** before merging, without requiring another user prompt. Follow [the review procedure](CONTRIBUTING.md#separate-assistant-review): give the reviewer the actual diff, relevant sources and execution evidence, not only the implementation summary.
- Record the reviewer, reviewed head commit, result and finding resolutions on the PR. Every later change needs review coverage; verify that the PR head still matches the reviewed commit before merging.
- If review is unavailable, interrupted, incomplete or has unresolved blocking findings, leave the PR open and report the blocker. Self-review, silence or an unreturned request is not a substitute.
- Required checks must actually pass; never bypass protections. Follow CONTRIBUTING's explicit pre-CI documentation/prerequisite rules where applicable, without calling absent CI a pass.
- Preserve unrelated work and user data. Do not install software, expand scope or change external permissions implicitly.

These instructions trigger session-based delegation. They do not configure GitHub-triggered AI review or mechanically enforce review completion. A separate reviewer may use the same model; it is not independent human approval. The [active main-branch CI rule and its limits](docs/ci.md#main-branch-rule) are separate from this review procedure; issue #6 / PR #314 record configuration and proof.
