-- The initial local composition edge, not a full hierarchy or Deployment API.
CREATE TABLE public.system_parent (
    child_id uuid PRIMARY KEY REFERENCES public.system_identity(id),
    parent_id uuid NOT NULL REFERENCES public.system_identity(id),
    CONSTRAINT system_parent_not_self CHECK (child_id <> parent_id)
);
CREATE INDEX system_parent_reverse ON public.system_parent(parent_id);

-- Serialize edge mutations only. An actual row UPDATE also makes stale
-- REPEATABLE READ / SERIALIZABLE writers fail instead of checking an old graph.
CREATE TABLE public.system_parent_write_guard (
    singleton boolean PRIMARY KEY CHECK (singleton),
    flip boolean NOT NULL
);
INSERT INTO public.system_parent_write_guard VALUES (true, false);

CREATE FUNCTION public.serialize_system_parent() RETURNS trigger
LANGUAGE plpgsql VOLATILE SET search_path = pg_catalog, public AS $$
BEGIN
    UPDATE public.system_parent_write_guard SET flip = NOT flip WHERE singleton;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'system parent guard missing' USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER system_parent_serialize
BEFORE INSERT OR UPDATE OR DELETE ON public.system_parent
FOR EACH STATEMENT EXECUTE FUNCTION public.serialize_system_parent();

CREATE FUNCTION public.check_system_parent_cycle() RETURNS trigger
LANGUAGE plpgsql VOLATILE SET search_path = pg_catalog, public AS $$
DECLARE replaced_child uuid;
BEGIN
    IF TG_OP = 'UPDATE' THEN replaced_child := OLD.child_id; END IF;
    IF EXISTS (
        WITH RECURSIVE ancestors(id) AS (
            SELECT NEW.parent_id
            UNION
            SELECT edge.parent_id FROM public.system_parent edge
            JOIN ancestors ON edge.child_id = ancestors.id
            WHERE edge.child_id IS DISTINCT FROM replaced_child
        )
        SELECT 1 FROM ancestors WHERE id = NEW.child_id
    ) THEN
        RAISE EXCEPTION 'system parent cycle' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER system_parent_cycle_check
BEFORE INSERT OR UPDATE ON public.system_parent
FOR EACH ROW EXECUTE FUNCTION public.check_system_parent_cycle();
