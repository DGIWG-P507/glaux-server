---
name: "Implementation task"
about: "Deliver one approved Glaux Server roadmap task with verifiable completion evidence."
title: "[TASK-ID] Task title"
---

<!--
Use one three-level Roadmap leaf per issue. Replace every {{...}} field before
publication; use "None" or a justified "Not applicable" where appropriate.
The title must be [task ID] followed by the Roadmap leaf title.
The API publisher must populate this same body; GitHub does not fill it for API calls.
Keep completion checkboxes unchecked at creation. Do not invent commands or results.
-->

## Task and intended result

- **Roadmap task:** {{task_id}}
- **Parent capability group:** {{parent_context}}
- **Deliverable:** {{deliverable}}

## Scope and boundaries

{{scope}}

**Not included:** {{out_of_scope}}

<!-- Parent scope supplies context and constraints, not permission to implement every sibling. -->

## Sources and applicable rules

- **Roadmap task and planning baseline:** {{roadmap_source}}
- **Applicable Guide sections and controlling standards:** {{guide_sources}}
- **Supporting research or examples:** {{supporting_sources}}

Follow [CONTRIBUTING.md](https://github.com/DGIWG-P507/glaux-server/blob/main/CONTRIBUTING.md) and the linked current Goal, Guide and Roadmap. Record the version/commit used to prepare this issue; resolve later approved changes before execution. Research and peer examples do not override the standards or approved scope.

## Prerequisites

{{dependencies}}

<!--
Link actual prerequisite issues and show their Roadmap IDs, or write "None".
Expand a referenced capability group to its leaf issues under Roadmap §5.1.
Use the leaf's explicit dependencies; do not invent a self-parent or sibling dependency.
Record relevant approved-tool/environment constraints separately below.
-->

**Environment or other constraints:** {{environment_constraints}}

## Acceptance and verification

{{acceptance_checks}}

**Verification approach and independently expected results:**

{{verification}}

<!--
Convert the leaf's Done criteria into concrete unchecked checkboxes.
Include applicable negative, access, failure and non-mutation cases.
A prerequisite-inspection or verification task may deliver evidence rather than code;
explain inapplicable checks instead of inventing builds or claiming tests passed.
Exact commands can be recorded during execution once the implementation exists.
-->

## Common completion checklist

- [ ] Work stayed within the authorized issue and approved scope; prerequisites were verified before dependent work.
- [ ] Task-specific acceptance checks passed; required checks are not missing, skipped or relabeled as success.
- [ ] Applicable tests use independent expected results; failures, unrun checks and justified inapplicability are recorded honestly.
- [ ] Relevant documentation, fixtures, dependency pins and any contributor/assistant instructions were updated.
- [ ] Existing work, user data and credentials were protected; no implicit software installation or unapproved external effects occurred.
- [ ] The linked PR received assistant review and applicable checks, blocking findings were resolved, and the change was merged without bypassing repository controls.
- [ ] Execution evidence and final commit/PR links are recorded below or in a linked issue comment; only complete work is closed, with a handoff before the next issue.

## Execution record

**Not started.** Complete during the authorized implementation iteration, or link the corresponding issue comment.

- Changes and deliverables:
- Branch / PR / final commit:
- Checks run, commands, relevant versions/configuration/fixtures, and results:
- Failures, checks not run, justified inapplicability and remaining limitations:
- Assistant review and resolution:
- Outcome: complete or still open/blocked, with the precise remaining work:
- Next dependency-ready issue (not authorization to begin it):
