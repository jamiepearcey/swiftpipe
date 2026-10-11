Upstream SWIFT FIN fixtures
===========================

Message files copied unmodified from public repositories, pinned to commits
(see `research/swift-message-references.json` for the full 467-entry manifest).
Parsed and schema-matched by `crates/swift-schema/tests/upstream_fixtures.rs`;
expected results there are SwiftPipe's own, not upstream's.

| File | Upstream | Commit | Licence |
|---|---|---|---|
| prowide-core-examples_MT537_large.fin | prowide/prowide-core-examples `src/main/resources/MT537_large.fin` | 2a2aff607b80ce8b67f8c9ef8323a61c25f98d2e | Apache-2.0 |
| prowide-core-examples_mt103.fin | prowide/prowide-core-examples `src/main/resources/mt103.txt` | 2a2aff607b80ce8b67f8c9ef8323a61c25f98d2e | Apache-2.0 |
| prowide-core_MT101.fin | prowide/prowide-core `src/test/resources/MT101.fin` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT300.fin | prowide/prowide-core `src/test/resources/SWIFTMT300_0000039099_0002.txt` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT305.fin | prowide/prowide-core `src/test/resources/MT305.fin` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT306.fin | prowide/prowide-core `src/test/resources/MT306.fin` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT320.fin | prowide/prowide-core `src/test/resources/MT320.txt` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT340.fin | prowide/prowide-core `src/test/resources/MT340.fin` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT341.fin | prowide/prowide-core `src/test/resources/MT341.fin` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT360.fin | prowide/prowide-core `src/test/resources/MT360.fin` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT361.fin | prowide/prowide-core `src/test/resources/MT361.fin` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT362.fin | prowide/prowide-core `src/test/resources/MT362.fin` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| prowide-core_MT940_sample_JPchar.fin | prowide/prowide-core `src/test/resources/sample_JPchar.txt` | c93572dba6e822efa9a471d506fa4966c73ae84f | Apache-2.0 |
| wolph-mt940_jejik_ing.sta | wolph/mt940 `mt940_tests/jejik/ing.sta` | 321a24bbf0d4feebeab3691cc4d6f6c3523c415d | BSD-3-Clause |
| wolph-mt940_sberbank.sta | wolph/mt940 `mt940_tests/sberbank/171011_01234945.sta` | 321a24bbf0d4feebeab3691cc4d6f6c3523c415d | BSD-3-Clause |
| mt940-rs_danskebank_SE.sta | svenstaro/mt940-rs `tests/data/mt940/full/danskebank/MT940_SE_Example.sta` | see research manifest | MIT |
| wolph-mt940_mBank_mt942.sta | wolph/mt940 `mt940_tests/mBank/mt942.sta` | 321a24bbf0d4feebeab3691cc4d6f6c3523c415d | BSD-3-Clause |

The two `.rje` files are multi-message (RJE `$` separators, ACK prefixes) and are split by
`swift_core::split_messages`; message #10 of `MT103-out-ack.rje` is a truncated-trailer upstream
negative case, asserted as one parse diagnostic.

The `.sta` files are bare bank statements with no `{1:}{2:}{4:}` envelope; they go through
`swift_core::split_bare_statements`, which wraps each `:20:` statement in a synthetic envelope.
