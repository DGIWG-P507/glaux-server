-- Shared canonical namespace, with only the first System path implemented.
-- UID/source equality is lexical. Hash exclusions recheck full text equality;
-- unlike B-tree keys, they preserve the domain's 4096-byte input budgets.
CREATE TABLE public.resource_identity (
    id uuid PRIMARY KEY,
    family text NOT NULL CHECK (family = 'system'),
    uid text COLLATE "C" NOT NULL CHECK (octet_length(uid) BETWEEN 1 AND 4096),
    CONSTRAINT local_id_v7 CHECK (
        substring(id::text FROM 15 FOR 1) = '7'
        AND substring(id::text FROM 20 FOR 1) IN ('8', '9', 'a', 'b')
    ),
    CONSTRAINT resource_uid_unique EXCLUDE USING hash (uid WITH =)
);

CREATE TABLE public.system_identity (
    id uuid PRIMARY KEY REFERENCES public.resource_identity(id),
    label text NOT NULL
);

CREATE TABLE public.source_identity (
    resource_id uuid NOT NULL REFERENCES public.resource_identity(id),
    authority text COLLATE "C" NOT NULL CHECK (octet_length(authority) BETWEEN 1 AND 4096),
    identifier text COLLATE "C" NOT NULL CHECK (octet_length(identifier) BETWEEN 1 AND 4096),
    -- Length prefix makes the pair injective, even when a field contains ':'.
    -- Multiple aliases per resource and per authority are deliberately allowed.
    CONSTRAINT source_pair_unique EXCLUDE USING hash (
        ((octet_length(authority)::text || ':' || authority || identifier) COLLATE "C") WITH =
    )
);
CREATE INDEX source_identity_resource ON public.source_identity(resource_id);
