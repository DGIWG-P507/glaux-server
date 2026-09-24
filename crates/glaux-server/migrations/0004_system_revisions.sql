-- Exact source storage, not JSONB reserialization or a content-addressed ID.
CREATE TABLE public.source_artifact (
    id uuid PRIMARY KEY,
    media_type text NOT NULL,
    bytes bytea NOT NULL,
    digest bytea NOT NULL,
    CONSTRAINT source_artifact_id_v7 CHECK (
        substring(id::text FROM 15 FOR 1) = '7'
        AND substring(id::text FROM 20 FOR 1) IN ('8', '9', 'a', 'b')
    ),
    CONSTRAINT source_artifact_size CHECK (octet_length(bytes) <= 1048576),
    CONSTRAINT source_artifact_media CHECK (
        octet_length(media_type) BETWEEN 1 AND 1024
        AND media_type COLLATE "C" !~ '[[:cntrl:]]'
    ),
    CONSTRAINT source_artifact_digest_matches CHECK (
        octet_length(digest) = 32 AND digest = sha256(bytes)
    )
);

-- First-family revision binding. No current-revision pointer, ordering by UUID,
-- audit/outbox orchestration, or public history/interval API is introduced.
CREATE TABLE public.system_revision (
    id uuid PRIMARY KEY,
    system_id uuid NOT NULL REFERENCES public.system_identity(id),
    artifact_id uuid NOT NULL REFERENCES public.source_artifact(id),
    semantic_civil_second bigint,
    semantic_leap boolean,
    semantic_fraction numeric,
    semantic_source text,
    receipt_civil_second bigint NOT NULL,
    receipt_leap boolean NOT NULL,
    receipt_fraction numeric NOT NULL,
    receipt_source text NOT NULL,
    CONSTRAINT system_revision_id_v7 CHECK (
        substring(id::text FROM 15 FOR 1) = '7'
        AND substring(id::text FROM 20 FOR 1) IN ('8', '9', 'a', 'b')
    ),
    CONSTRAINT semantic_time_presence CHECK (
        num_nonnulls(semantic_civil_second, semantic_leap,
                     semantic_fraction, semantic_source) IN (0, 4)
    ),
    CONSTRAINT semantic_fraction_exact CHECK (
        semantic_fraction IS NULL OR (
            semantic_fraction NOT IN ('NaN'::numeric, 'Infinity'::numeric, '-Infinity'::numeric)
            AND semantic_fraction >= 0 AND semantic_fraction < 1
        )
    ),
    CONSTRAINT receipt_fraction_exact CHECK (
        receipt_fraction NOT IN ('NaN'::numeric, 'Infinity'::numeric, '-Infinity'::numeric)
        AND receipt_fraction >= 0 AND receipt_fraction < 1
    ),
    CONSTRAINT semantic_source_bounded CHECK (
        semantic_source IS NULL OR octet_length(semantic_source) BETWEEN 20 AND 4096
    ),
    CONSTRAINT receipt_source_bounded CHECK (
        octet_length(receipt_source) BETWEEN 20 AND 4096
    )
);
CREATE INDEX system_revision_system ON public.system_revision(system_id);
CREATE INDEX system_revision_artifact ON public.system_revision(artifact_id);
