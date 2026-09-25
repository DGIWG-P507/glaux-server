-- Authoritative accepted-write head, not a valid-time/history selector.
CREATE TABLE public.system_write_head (
    system_id uuid PRIMARY KEY REFERENCES public.system_identity(id),
    revision_id uuid NOT NULL,
    artifact_id uuid NOT NULL,
    CONSTRAINT system_head_revision_binding FOREIGN KEY (revision_id, system_id, artifact_id)
        REFERENCES public.system_revision(id, system_id, artifact_id)
);

-- Only accepted application CREATE evidence establishes an old head.
-- Multiple candidates violate the primary key and abort this migration.
-- Bare identities and unrelated later history deliberately remain unheaded.
INSERT INTO public.system_write_head (system_id, revision_id, artifact_id)
SELECT w.system_id, w.revision_id, w.artifact_id
FROM public.outgoing_work w
JOIN public.server_audit a
  ON a.id = w.audit_id AND a.target_id = w.system_id
 AND a.revision_id = w.revision_id AND a.outcome = w.outcome
WHERE w.kind = 'system.created' AND w.outcome = 'accepted'
  AND a.operation = 'system.create';

ALTER TABLE public.server_audit DROP CONSTRAINT server_audit_operation_check;
ALTER TABLE public.server_audit ADD CONSTRAINT server_audit_operation_check
    CHECK (operation IN ('system.create', 'system.update'));
ALTER TABLE public.outgoing_work DROP CONSTRAINT outgoing_work_kind_check;
ALTER TABLE public.outgoing_work ADD CONSTRAINT outgoing_work_kind_check
    CHECK (kind IN ('system.created', 'system.updated'));

-- Keep the operation binding that the old single-operation checks implied.
ALTER TABLE public.server_audit ADD CONSTRAINT audit_operation_binding
    UNIQUE (id, target_id, revision_id, outcome, operation);
ALTER TABLE public.outgoing_work ADD COLUMN audit_operation text
    GENERATED ALWAYS AS (
        CASE kind WHEN 'system.created' THEN 'system.create'
                  WHEN 'system.updated' THEN 'system.update' END
    ) STORED NOT NULL;
ALTER TABLE public.outgoing_work DROP CONSTRAINT outgoing_audit_binding;
ALTER TABLE public.outgoing_work ADD CONSTRAINT outgoing_audit_binding
    FOREIGN KEY (audit_id, system_id, revision_id, outcome, audit_operation)
    REFERENCES public.server_audit(id, target_id, revision_id, outcome, operation);
