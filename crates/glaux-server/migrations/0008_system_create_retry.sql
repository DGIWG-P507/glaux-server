-- Terminal creation receipts only. No command admission or automatic purge.
ALTER TABLE public.outgoing_work ADD CONSTRAINT outgoing_retry_binding
    UNIQUE (id, system_id, revision_id, artifact_id, audit_id, audit_operation);

CREATE TABLE public.system_create_retry (
    actor text COLLATE "C" NOT NULL
        CHECK (octet_length(actor) BETWEEN 1 AND 256 AND actor !~ '[[:cntrl:]]'),
    source text COLLATE "C"
        CHECK (octet_length(source) BETWEEN 1 AND 256 AND source !~ '[[:cntrl:]]'),
    operation text COLLATE "C" NOT NULL CHECK (operation = 'system.create'),
    -- Empty means the root creation target; otherwise the exact parent System.
    target text COLLATE "C" NOT NULL CHECK (target = '' OR
        target ~ '^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'),
    key text COLLATE "C" NOT NULL
        CHECK (octet_length(key) BETWEEN 1 AND 256 AND key !~ '[[:cntrl:]]'),
    digest bytea NOT NULL CHECK (octet_length(digest) = 32),
    system_id uuid NOT NULL,
    revision_id uuid NOT NULL,
    artifact_id uuid NOT NULL,
    audit_id uuid NOT NULL,
    event_id uuid NOT NULL,
    retained_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL CHECK (expires_at > retained_at),
    CONSTRAINT system_create_retry_scope
        UNIQUE NULLS NOT DISTINCT (actor, source, operation, target, key),
    CONSTRAINT system_create_retry_outcome
        FOREIGN KEY (event_id, system_id, revision_id, artifact_id, audit_id, operation)
        REFERENCES public.outgoing_work
            (id, system_id, revision_id, artifact_id, audit_id, audit_operation)
);
