# Isolated System restore proof

Issue #26 / Roadmap 1.5.3 exercises [the backup/restore procedure](system-restore.md)
end to end. Guide §§4.7, 4.10, 4.12 and 8.2 scenario 6 govern restore
isolation, captured audit and recovery limits; §8.1.1 requires independent
expected results and demonstrated failure sensitivity.

Run on the authorised GitHub-hosted Linux runner:

```sh
python3 scripts/test_system_restore.py
python3 scripts/test-system-restore-failures.py
```

Everything runs in the existing pinned, network-disabled PostgreSQL/PostGIS
container, with synthetic data only. The source is the harness database. The
clones are new databases created by the procedure in that owned container, and
are removed with it. The actual server binary serves both copies, under a
least-privilege role.

## Independently expected results

The manifest comes from what the raw HTTP client submitted and observed:
- generated IDs from Location;
- UIDs, names, exact type spellings and submitted bytes;
- SHA-256 digests from the host's `sha256sum`, not the server or database;
- correlations returned to the client;
- the verified development actor and configured source.

It is checked against the source before backup, and against each clone after
restore. A clone is accepted only if the manifest holds and every inventory item
equals the source's captured state. Restored output is never its own expected
answer.

| Ordered group | Evidence |
| --- | --- |
| source-workflow-and-independent-manifest | Two Systems via public POST: one with a retry key, one with a full-URI type and ignored client ID and link. A cross-source attempt gets 403 and leaves denial audit. The source matches the manifest. |
| guarded-backup-and-isolated-restore | Backup refuses to overwrite its dump. Restore refuses the source database, an existing database, the role `public`, a superuser and a missing role; a refused attempt creates nothing. A restore from an unreadable dump fails, warns that its target is not isolated, and leaves that target closed to `PUBLIC`. The clone matches the manifest and the inventory: ten System-slice tables, the migration record, the parent-write guard row and a catalog fingerprint of tables, extension versions, constraints, triggers and indexes (constraint text compared without parentheses, since restore regroups equivalent CHECK terms). The source is unchanged. |
| restored-workflow-matches-manifest | A server on the clone: from `/`, `service-desc` leads to the API, and every System GET equals the representation built from the manifest. |
| clone-refuses-effects-and-outside-access | A POST to the clone returns 503, and the clone is unchanged, including outgoing work. An outside login role and the source's serving role are both refused with "permission denied for database". The clone is served and read by a separate inspection role that holds only write grants on the source. For that role, a new session defaults to read-only, and inside an explicitly read-write transaction every write it held on the source (inserts into all ten tables, and column and row updates) fails with permission error 42501. The source is unchanged. |
| incomplete-or-corrupt-clones-are-rejected | Three further clones from the same dump are corrupted administratively. One loses its captured denial audit; one has consistently re-digested, altered artifact bytes; one loses the audit immutability trigger. System GET still returns the expected body on all three, yet verification rejects each for the named reason, the last through the catalog fingerprint. |
| recovery-point-excludes-later-changes | A System created in the source after the backup is absent from the clone (404). The clone still equals the inventory. |

## Fault controls

The fault run first requires a complete passing baseline. It then runs the
same binaries with four disposable copies of the procedure:

- **writable-privileges** drops the write revocation and its self-check. The
  read-only default still makes the POST fail, so the proof must fail
  "inspection role can modify the clone".
- **read-write-default** drops the read-only database default. The revocation
  still refuses writes, so the proof must fail "clone does not default to
  read-only".
- **open-clone** drops the connection restrictions and their self-check. The
  proof must fail "outside role connected to the isolated clone".
- **dropped-retry-state** excludes retry-receipt data from the backup. The
  proof must fail "restored clone differs from the manifest or backup
  inventory", for the reason "retry: expected 1 rows".

Each fault must fail after the earlier groups pass. Each mutation target must
occur exactly as expected, and the real procedure must be byte-identical
afterwards. Compilation or setup failure, an unrelated assertion, timeout or
missing output does not count as detection.

Limits:
- No DELETE exists, so post-backup deletion is specified in the procedure
  document, not executed.
- No delivery worker exists, so "no delivery" means unchanged outgoing-work rows.
- Clone isolation uses database privileges and settings in a disposable
  container. It is not a network or tamper-proofing guarantee.
