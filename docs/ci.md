# Initial checks and merge enforcement

[Issue #6](https://github.com/DGIWG-P507/glaux-server/issues/6) records actual
execution, rule application, failure/missing-check probes and separate review.
The initial suite covers the code that exists now, not later CSAPI capabilities.

## Reproduce the checks

Use a fresh checkout at the commit being reviewed on the selected GitHub-hosted
Ubuntu 24.04 runner. The [Build workflow](../.github/workflows/build.yml) provides
that clean checkout, verifies its actual PR head rather than the synthetic merge
commit, and logs the runner/compiler/Python/Docker versions. It installs only
the [pinned toolchain components](../rust-toolchain.toml) inside that disposable
runner. No company-laptop installation or user database is needed.

From the workspace root, with those approved prerequisites:

```sh
cargo fmt --all --check
cargo generate-lockfile --offline
git diff --exit-code -- Cargo.lock
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
python3 -m compileall -q scripts
cargo build --workspace --locked --offline
python3 scripts/check-bootstrap.py
python3 -u scripts/check-execution.py rust
python3 -c 'import sys; sys.path.insert(0, "scripts"); from database_harness import PIN, docker; print(docker("pull", "--platform", PIN["platform"], PIN["image"], timeout=180))'
python3 -u scripts/check-execution.py database
python3 -u scripts/test-ci-failures.py
python3 scripts/dependency_inventory.py
```

`RUNNER_TEMP` is supplied by GitHub. The failure controls use it for fresh temporary
source copies and ordinary evidence files. For an already approved Linux host,
set it to a newly created task-local temporary directory before those controls;
never point it at a user data directory. Docker must already be available at the
local Unix socket. Image/toolchain provisioning uses the network; Cargo operations
are offline after provisioning. No local prerequisites are installed implicitly.

Formatting and Clippy check Rust; compileall checks Python syntax, not a claim of
Python style/static-analysis coverage. The executable regression and seven real
database lifecycle tests actually run. Domain/standards unit/doctest targets are
empty because they contain no behavior or executable examples yet, not because
required tests were waived. Later owners add their behavioral tests and inventory.

## Evidence and failure sensitivity

[check-execution.py](../scripts/check-execution.py) preserves command failure and
requires actual successful execution evidence. It accepts exactly `rust` or
`database`, not arbitrary selectors. The [database runner](../scripts/test_database.py)
also rejects missing or skipped cases. No step uses continue-on-error to turn
failure into success. Shell pipelines use pipefail.

After the unmodified Rust/database checks pass,
[test-ci-failures.py](../scripts/test-ci-failures.py) copies tracked source into
separate temporary directories and invokes those same execution entry points.
Nine bounded controls cover a failing Rust assertion, missing database image,
failed SQL setup, missing test runner, empty/filtered/ignored Rust tests, and
skipped/empty database tests. Each must fail with the expected diagnostic; a
compiler error cannot stand in for the intended assertion failure. Mutations
never modify the checked-out source or any user database.

Passing these controls means the expected bad executions were rejected, not that
the bad versions themselves passed. Baseline results and every control's output,
exit status and expected diagnostic are retained as ordinary artifacts. This is
a small check of the current CI path, not a new general testing framework or a
substitute for later independent standards/HTTP/broker tests.

The `initial-ci-evidence` artifact includes suite logs, per-control logs/results
and the [resolved dependency/licence inventory](dependencies.md). Retention is
14 days; issue/PR summaries preserve durable run/commit links, limitations and
outcomes. Rerunning the pinned scripts regenerates evidence, not the exact old
run identity. Upload failure is itself a failed check; early failed setup may
leave no artifact and remains a failure, with normal GitHub logs.

## Main-branch rule

The approved configuration is recorded in [main-ruleset.json](main-ruleset.json):
main-only, active, no bypass actors (including no administrator exception),
pull requests required, zero required human approvals, no deletion or history
rewrite, and the actual `Rust bootstrap` check from GitHub Actions app `15368`.
The historical name is retained, but that one job now includes all checks above.
The branch must be up to date before merge.

This JSON is a reproducible configuration, not evidence that settings were
applied. Issue #6 and its PR record authenticated settings readback and observed
merge blocking before claiming enforcement complete. Never merge faulty source
to test whether the rule works: inspect the exact head's failed/missing check and
GitHub's blocked merge state instead. Do not confuse a conflict or draft status
with a required-check block.

GitHub permits success, skipped or neutral conclusions for required checks.
Consequently the required job itself is unconditional, and its executed scripts
must reject skipped/empty work; a repository rule alone does not prove tests ran.
Changing a workflow can still weaken its meaning, so the existing separate
assistant review must inspect the actual current diff/evidence before any merge.
That review remains session-triggered and procedural, not a GitHub human-approval
or independently operating AI-review gate. Reconcile base movement and retest.

Sources: [GitHub rules REST API](https://docs.github.com/en/rest/repos/rules),
[required-check behavior](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches),
[Clippy usage](https://doc.rust-lang.org/clippy/usage.html).
