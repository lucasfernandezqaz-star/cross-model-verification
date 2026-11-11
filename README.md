# cross-model-verification

A protocol and a small toolkit for measuring how reliably language models from different labs catch errors when they review each other's work.

Status: early. The protocol is a draft and no scored run has been made under it. The method and the tools are in daily use by the author; the numbers in `results/` come from that use, not from a controlled study.

Author: Lucas Mateo Ruiz. Contact: GitHub issues on this repository.

The code and the documents here were written with the assistance of language models from Anthropic, OpenAI and Google, under the author's direction and review. The method this repository describes is the one that was used to check them.

## The question

When one model reviews work written by another model, how much can the review be trusted? Does a reviewer from a different lab find more than one from the same lab? Does knowing who wrote a finding change which findings get accepted?

## The method in one line

author → a copy with planted single-line errors → reviewer from another lab → mechanical check of every quotation → reconciliation on evidence only → a score ledger per reviewer.

`METHOD.md` describes each step and what it does and does not show.

## What is here

| Path | Content |
|---|---|
| `PROTOCOL.md` | Draft study protocol: items, arms, outcomes, scoring, order of work, limits |
| `METHOD.md` | The audit method as practised: planted controls, prompt structure, reconciliation, tool-event audit, score ledger |
| `crates/xfam-audit/` | A dependency-free Rust program that audits the stream-JSON event log of an agent CLI: event kinds, tool-related events, the final answer. `--strict` rejects a log that shows a fatal error, a tool-related event, a logged error, an unparsed line or no final non-blank answer |
| `scripts/` | Shell wrappers that run one reviewer job and refuse to print an answer they cannot vouch for |
| `results/control_recall.md` | Planted-control counts recorded so far, with the misses and how to read them |
| `examples/reconcile_example.md` | A synthetic example of a reconciliation record |

## Results so far, and their limits

Across 38 review jobs, reviewers from two labs found 106 of 110 planted single-line errors. See `results/control_recall.md` for the breakdown and the four misses.

- The sample is small and comes from one person's engineering work.
- Every control was planted and scored by models of a single lab.
- No clean documents were included, so nothing is known about false positives.
- Where every control was found, the controls cannot tell reviewers apart.

## Building and running the auditor

Requirements: Linux, a recent stable Rust toolchain. No dependencies are downloaded.

```
cd crates/xfam-audit
cargo test --offline
cargo build --offline --release
./target/release/xfam-audit --strict <events.jsonl> <answer.md>
```

The wrappers in `scripts/` additionally need Bash, GNU coreutils, `jq`, `bubblewrap` (web lane only), and the vendor CLIs you want to drive, each logged in by you. Directories and binaries are passed through environment variables; the job name, the prompt file, the timeout and (in the text and web lanes) the model are command-line arguments. Both are documented at the top of each script. `scripts/tests/run_cases.sh` replays 23 canned logs and a few bad invocations through the web lane, and through the text lane when given the auditor binary, without any model or network; its header says what it does not cover.

## Known limits

- The wrappers record what a model did and refuse to vouch for runs they cannot read. They are not a security sandbox.
- The auditor reports what the event log contains. It cannot detect an action that the CLI did not log.
- The tests of the auditor use synthetic transcripts only. Its earlier byte-for-byte comparison against a reference implementation was done on private transcripts and cannot be reproduced from this repository (see `crates/xfam-audit/PARITY.md`).

## What is withheld

- The item pool and answer keys of the study, until all scored runs are finished. A SHA-256 commitment will be published when they are frozen.
- The documents that were reviewed in the jobs counted in `results/`, and the reviewers' answers. They belong to a private project.

## Licence

Everything under `crates/` and `scripts/`, including the Markdown files inside `crates/`: MIT or Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). The Markdown documents outside `crates/`: CC BY 4.0 (`LICENSE-CC-BY-4.0`). Third-party notices: `NOTICE`.

## Citing

Ruiz, L. M. (2026). *cross-model-verification: a protocol and toolkit for cross-lab model review.* GitHub repository.
