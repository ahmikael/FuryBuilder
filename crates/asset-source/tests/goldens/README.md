# Catalog diff snapshots

`catalog-diff-1.20.6-1.21.4.json` captures the complete serialized `CatalogDiff` for the
synthetic from/to snapshots in `src/catalog_diff.rs`. It covers a known vanilla rename,
an added and a removed entry, one changed entry and one unchanged entry. The test compares the
whole JSON value, so field names, ordering, nulls and evidence flags are all part of the
contract. Regenerate only together with an intentional schema change.
