"""Named behavioral checks that must be discovered AND actually execute."""

REQUIRED_RUST_TESTS = [
    "unfinished_server_does_not_report_success",
    "validation::tests::published_corpus_expectations",
    "validation::tests::parser_fuzz_regressions",
    "validation::tests::limits_and_safe_parse",
    "validation::tests::fixed_encoding_selection",
    "validation::tests::external_links_are_data_not_retrieval_instructions",
    "schema_guard::tests::rejects_direct_and_mutual_nonprogress_cycles",
    "schema_guard::tests::rejects_nonprogress_applicator_cycles",
    "schema_guard::tests::accepts_progressing_recursion_and_ignores_instance_data",
    "schema_guard::tests::rejects_missing_resources_pointers_and_anchors",
    "schema_guard::tests::rejects_http_file_data_and_uri_escape_canaries",
    "schema_guard::tests::resolves_relative_ids_anchors_aliases_and_escaped_pointers",
    "schema_guard::tests::resolves_embedded_resource_pointers_and_rejects_conflicting_ids",
    "schema_guard::tests::enforces_schema_node_and_depth_boundaries",
    "schema_guard::tests::enforces_reference_depth_including_previously_finished_branches",
]
