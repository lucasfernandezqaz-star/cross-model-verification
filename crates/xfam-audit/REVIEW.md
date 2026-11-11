# Historical review note

A private review examined the Rust auditor against a Python reference implementation.
The recorded final comparison covered 65 private transcripts with byte comparisons of
stdout and answer files. Two stored-reference defects required special handling: an
appended prior run and a truncated stdout capture completed by an independently derived
pin. PARITY.md describes the evidence and its limits. Neither the reference implementation
nor those transcripts is included, so a public reader cannot reproduce that comparison.

The review led to even-digit handling for exact float ties, a larger explicit nesting
limit, exit-status handling for broken output streams, streamed input, and stricter
reference classification. Mutation tests reject changed tool dumps, crashes, missing
or changed answers, truncated output, and trailing output. Pins are compared in full;
appended references require the exact listed prior length and complete final-run bytes.

Known differences remain: concise fatal diagnostics, answer-file ordering when stdout
fails, an explicit nesting cutoff, and omission of an unused helper. These are historical
observations rather than a current independent certification. Public regression data is
entirely synthetic. The new strict policy has unit and binary tests; it was not part of
the historical parity claim.
