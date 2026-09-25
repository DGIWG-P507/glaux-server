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
cargo fetch --locked
git diff --exit-code -- Cargo.lock
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
python3 -m compileall -q scripts
cargo build --workspace --locked --offline
python3 scripts/check-bootstrap.py
python3 scripts/check_corpus.py
python3 -u scripts/check-execution.py rust
python3 -u scripts/check-execution.py schema-fuzz
python3 -u scripts/check-execution.py numeric-fuzz
python3 -u scripts/test-numeric-failures.py
python3 -u scripts/check-execution.py time-fuzz
python3 -u scripts/test-time-failures.py
python3 -u scripts/check_identity_boundary.py
python3 -u scripts/test-identity-failures.py
python3 scripts/check_corpus.py
python3 scripts/test_corpus.py
python3 -c 'import sys; sys.path.insert(0, "scripts"); from database_harness import PIN, docker; print(docker("pull", "--platform", PIN["platform"], PIN["image"], timeout=180))'
python3 -u scripts/check-execution.py database
python3 -u scripts/check-execution.py time-database
python3 -u scripts/check-execution.py system-storage
python3 -u scripts/check-execution.py revision-storage
python3 -u scripts/check-execution.py atomic-write
python3 -u scripts/test-atomic-write-failures.py
python3 -u scripts/check-execution.py conditional-write
python3 -u scripts/test-conditional-write-failures.py
python3 -u scripts/check-execution.py retry-write
python3 -u scripts/test-retry-write-failures.py
python3 -u scripts/check-execution.py runtime-health
python3 -u scripts/test-runtime-health-failures.py
python3 -u scripts/test-ci-failures.py
python3 -u scripts/test-validation-failures.py
python3 -u scripts/test-projection-failures.py
python3 scripts/check_corpus.py
python3 scripts/dependency_inventory.py
```

`RUNNER_TEMP` is supplied by GitHub. The failure controls use it for fresh temporary
source copies and ordinary evidence files. For an already approved Linux host,
set it to a newly created task-local temporary directory before those controls;
never point it at a user data directory. Docker must already be available at the
local Unix socket. Image/toolchain provisioning uses the network; Cargo operations
are offline after the explicit locked dependency fetch. No local prerequisites are installed implicitly.

Formatting and Clippy check Rust; compileall checks Python syntax, not a claim of
Python style/static-analysis coverage. The executable regression, named identity,
numeric, time and standards tests, bounded parser campaigns, exact-time storage
proof, System SQLx identity and revision/source storage proofs, and seven real database lifecycle tests run. Empty doctest targets have no executable examples;
they are not substituted for the named behavioral checks.

The [identity boundary harness](../scripts/check_identity_boundary.py) runs only
on GitHub-hosted Linux. A valid external client must compile, link and execute
before six specific type/member compiler rejections are counted. Successful
compilation must itself be rejected as negative evidence. The separate
[identity fault control](../scripts/test-identity-failures.py) removes the UUID
version check in a disposable copy and requires the exact intended assertion
failure after a passing baseline. See [contracts and limits](resource-identities.md).

The [standards corpus](standards-corpus.md) has a separate standard-library
Python packaging check and 15 controls. They check unchanged original bytes,
complete local reference targets and fixture metadata with socket access blocked.
They do not execute schema validation. Task #8's separate Rust test now executes
all 23 authored expectations and requires their exact expected verdicts.
Controlled corruption and missing-target rejection are the failure-sensitivity
proof for this packaging task, not a claim that schema validation ran.

## Evidence and failure sensitivity

[check-execution.py](../scripts/check-execution.py) preserves command failure and
requires actual successful execution evidence. It accepts exactly `rust`,
`schema-fuzz`, `numeric-fuzz`, `time-fuzz`, `database`, `time-database`,
`system-storage`, `revision-storage`, `atomic-write`, `conditional-write`, `retry-write` or `runtime-health`,
not arbitrary selectors. Every named Rust test must execute successfully, and
each fuzz campaign must emit its completed-invariant marker; the wrapper imposes
a 180-second process timeout. Both the [lifecycle runner](../scripts/test_database.py)
and [exact-time runner](../scripts/test_time_database.py) reject missing or
skipped cases. No step uses continue-on-error to turn
failure into success. Shell pipelines use pipefail.

After the unmodified Rust/database checks pass,
[test-ci-failures.py](../scripts/test-ci-failures.py) copies tracked source into
separate temporary directories and invokes those same execution entry points.
Nine bounded controls cover a failing Rust assertion, missing database image,
failed SQL setup, missing test runner, empty/filtered/ignored Rust tests, and
skipped/empty database tests. Each must fail with the expected diagnostic; a
compiler error cannot stand in for the intended assertion failure. Mutations
never modify the checked-out source or any user database.

After the passing validation baseline, [test-validation-failures.py](../scripts/test-validation-failures.py)
proves four named assertions fail for the intended reason: wrong Binary entry
point, skipped Quantity structural checking, bypassed raw-size limit, and ignored
unallowlisted references. Every control must compile and fail the exact test with
the expected left/right values; setup failure or timeout is not detection.
Disposable source copies share only their task-local compilation cache.

The [direction projections](direction-validation.md) add fourteen required Rust
tests and [four disposable faults](../scripts/test-projection-failures.py):
using original response requiredness for requests, leaking write-only schemas,
bypassing immutable UID checks, and permitting deletion of a locked schema.
Each exact assertion must first pass on unmodified source, then the faulty copy
must compile and fail with the expected values. Corpus digest checks run before
and after the projection tests/faults; in-memory adaptations never rewrite the
original source files. No dependency or hosted-service permission is added.

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

The [exact numeric checks](exact-numbers.md) add eight unit and four property tests,
a 2,048-case deterministic parsing campaign, and two deliberate lossy-conversion/
comparison faults in disposable copies. Their exact baselines must pass before
the intended assertion failures count. Results and per-fault logs are retained
alongside the existing suite evidence; no score replaces the value assertions.

The [atomic-write proof](atomic-write-tests.md) adds seven required groups,
including twelve full-state rollback boundaries and synchronized visibility
from a second database connection. Its separate disposable omission control
requires a passing baseline, the precise missing-outgoing-work assertion, and
a restored passing execution. Compile/setup failures are not detection.

The [retry-write proof](write-retries.md) checks retained original outcomes,
content/scope conflicts, local disclosure denial, expiry and synchronized
competing creation requests. A separate disposable content-comparison fault must
fail the exact intended assertion between passing baseline/restored executions.
Its required execution markers and logs remain part of the unconditional build.

## Main-branch rule

The approved configuration is recorded in [main-ruleset.json](main-ruleset.json):
main-only, active, no bypass actors (including no administrator exception),
pull requests required, zero required human approvals, no deletion or history
rewrite, and the actual `Rust bootstrap` check from GitHub Actions app `15368`.
The historical name is retained, but that one job now includes all checks above.
The branch must be up to date before merge.

The configuration was applied as [ruleset 23796335](https://github.com/DGIWG-P507/glaux-server/rules/23796335)
on 21 September 2026 and authenticated readback confirmed it is effective on main,
with no bypass available to the current administrator. [PR #314](https://github.com/DGIWG-P507/glaux-server/pull/314)
records the exact heads and GitHub's blocked, non-draft, conflict-free merge states
for a deliberately failed assertion and an intentionally missing check. Neither
probe was merged. Configuration is not immutable: recheck live settings and the
current head's execution before delivery. Never merge faulty source
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
