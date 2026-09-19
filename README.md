# Glaux Server

Glaux Server aims to be a **gold-standard, open-source Rust reference implementation of OGC API - Connected Systems** for the DGIWG Glaux ecosystem.

This repository is home to the server implementation. Research and planning documents live in the [parent Glaux repository](https://github.com/DGIWG-P507/glaux). The server provides APIs for applications; it does not include the Glaux web or mobile application.

## Planned capabilities

- Discover, register and manage connected-system descriptions and relationships.
- Submit, retrieve and filter observations while preserving their source, timestamps, units and meaning.
- Receive supported live updates and retrieve reported system status and events.
- Assess feasibility, submit authorized commands, and follow their status and results.
- Enforce configured access rules, preserve supplied provenance and quality information, and support bounded recovery after interruptions.

## Current status

- Initial [design research and accepted supplements](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Research/Initial%20Designs/IDR/glaux-server/IDR%20Reports/final-idr-research-report.md) are complete.
- The Goal, Implementation Guide and Roadmap are baselined; the complete initial implementation issue set is published and verified.
- Server implementation and runtime verification have not started.

Standards conformance remains an implementation and verification target, not a current claim. Planned CSAPI Part 3 and selected Part 4 support are explicitly experimental. The Roadmap maintains the detailed scope, progress and next task.

## Project documents and contributions

- [Goal and Definition](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-goal-and-definition.md) — What the server must accomplish and its scope.
- [Implementation Guide](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md) — How the selected technologies deliver those capabilities, with design and testing details.
- [Roadmap](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-roadmap.md) — Implementation order, task-to-issue links, and current progress.
- [Implementation issues](https://github.com/DGIWG-P507/glaux-server/issues) — The published work to be completed.
- [Contributing](CONTRIBUTING.md) — Working rules, the task template, and the one-issue-per-iteration branch/PR workflow.
