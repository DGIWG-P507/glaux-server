-- Retained history is append-only for ordinary SQL as well as repository APIs.
-- Statement triggers also cover empty tables and TRUNCATE, not only row edits.
-- This does not claim protection against privileged DDL or trigger disabling.
CREATE FUNCTION public.reject_retained_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '55000',
        MESSAGE = 'retained history is immutable';
END;
$$;

CREATE TRIGGER source_artifact_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON public.source_artifact
FOR EACH STATEMENT EXECUTE FUNCTION public.reject_retained_history_mutation();

CREATE TRIGGER system_revision_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON public.system_revision
FOR EACH STATEMENT EXECUTE FUNCTION public.reject_retained_history_mutation();
