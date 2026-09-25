-- Consistent immutable payload binding, not independent unrelated foreign keys.
ALTER TABLE public.system_revision
    ADD CONSTRAINT system_revision_payload UNIQUE (id, system_id, artifact_id);

-- Targets are retained facts, not cascading references to public resources.
-- A safely recorded denied target need not exist or be disclosed.
CREATE TABLE public.server_audit (
    id uuid PRIMARY KEY,
    actor text,
    source text,
    operation text NOT NULL CHECK (operation = 'system.create'),
    target_id uuid,
    revision_id uuid,
    time_civil_second bigint NOT NULL,
    time_leap boolean NOT NULL,
    time_fraction numeric NOT NULL,
    time_source text NOT NULL,
    outcome text NOT NULL CHECK (outcome IN ('accepted', 'denied')),
    correlation text NOT NULL,
    CONSTRAINT audit_id_v7 CHECK (
        substring(id::text FROM 15 FOR 1) = '7'
        AND substring(id::text FROM 20 FOR 1) IN ('8', '9', 'a', 'b')
    ),
    CONSTRAINT audit_context_bounded CHECK (
        (actor IS NULL OR (octet_length(actor) BETWEEN 1 AND 256 AND actor COLLATE "C" !~ '[[:cntrl:]]'))
        AND (source IS NULL OR (octet_length(source) BETWEEN 1 AND 256 AND source COLLATE "C" !~ '[[:cntrl:]]'))
        AND octet_length(correlation) BETWEEN 1 AND 256
        AND correlation COLLATE "C" !~ '[[:cntrl:]]'
    ),
    CONSTRAINT audit_outcome_context CHECK (
        (outcome = 'accepted' AND actor IS NOT NULL AND target_id IS NOT NULL AND revision_id IS NOT NULL)
        OR (outcome = 'denied' AND revision_id IS NULL)
    ),
    CONSTRAINT audit_fraction_exact CHECK (
        time_fraction NOT IN ('NaN'::numeric, 'Infinity'::numeric, '-Infinity'::numeric)
        AND time_fraction >= 0 AND time_fraction < 1
    ),
    CONSTRAINT audit_time_source_bounded CHECK (octet_length(time_source) BETWEEN 20 AND 4096),
    CONSTRAINT audit_accepted_binding UNIQUE (id, target_id, revision_id, outcome)
);
CREATE TRIGGER server_audit_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON public.server_audit
FOR EACH STATEMENT EXECUTE FUNCTION public.reject_retained_history_mutation();

CREATE TABLE public.outgoing_work (
    id uuid PRIMARY KEY,
    system_id uuid NOT NULL,
    revision_id uuid NOT NULL,
    artifact_id uuid NOT NULL,
    audit_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind = 'system.created'),
    outcome text NOT NULL CHECK (outcome = 'accepted'),
    CONSTRAINT outgoing_id_v7 CHECK (
        substring(id::text FROM 15 FOR 1) = '7'
        AND substring(id::text FROM 20 FOR 1) IN ('8', '9', 'a', 'b')
    ),
    CONSTRAINT outgoing_revision_binding FOREIGN KEY (revision_id, system_id, artifact_id)
        REFERENCES public.system_revision(id, system_id, artifact_id),
    CONSTRAINT outgoing_audit_binding FOREIGN KEY (audit_id, system_id, revision_id, outcome)
        REFERENCES public.server_audit(id, target_id, revision_id, outcome)
);
CREATE INDEX outgoing_revision ON public.outgoing_work(revision_id);
CREATE INDEX outgoing_audit ON public.outgoing_work(audit_id);
CREATE TRIGGER outgoing_work_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON public.outgoing_work
FOR EACH STATEMENT EXECUTE FUNCTION public.reject_retained_history_mutation();
