# Prowide MT message structures

`prowide-mt-structures.json` is derived from the `scheme` javadoc of the generated MT
classes in [prowide/prowide-core](https://github.com/prowide/prowide-core) (Apache-2.0),
commit `c93572dba6e822efa9a471d506fa4966c73ae84f`, path
`src/generated/java/com/prowidesoftware/swift/model/mt/mtNxx/MTnnn.java`. It lists, for
~270 MT types (SRU 2025), each sequence (mandatory/optional, repetitive) and its fields
(tag, option letters, M/O, repetitive). Structure only: no field formats or network rules.

Used by `repo/scripts/gen-schemas-from-prowide.py` to generate starter schemas for types with
no public message fixtures. Regenerate the JSON by re-running that script against the
downloaded classes (`gh api repos/prowide/prowide-core/git/trees/<commit>?recursive=1`).

Coverage: every non-service MT type in the structure file now has a schema (service messages
MT0xx are deliberately skipped). Nested sequences (MT304, 350, 600, 601, 620) are flattened to
sibling anchored sequences, one per sub-sequence; where a sub-sequence's first tag is also an
optional member of another sequence the anchor wins and the note is recorded in the schema's
`coverage.notes`. MT204 is modelled as one repeating group (A and B both start with `:20:`).
MT801's `:51A:`/`:51C:` loop uses `alt_anchor_tags`.
