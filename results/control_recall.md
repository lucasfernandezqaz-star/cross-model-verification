# Planted-control recall recorded so far

Status: observations from engineering use of the method in `METHOD.md`, 2026-11-10 and 2026-11-11. They were not produced under `PROTOCOL.md` and are not a controlled comparison.

Each job is one review of one document by a model from a different lab than the document's author. Before each job, a fixed number of single-line factual alterations were planted in a copy of the document. "Found" counts the alterations the reviewer quoted or unmistakably identified. Job contents are private; only the counts are published.

## By reviewer

| Reviewer (provider, model, mode) | Jobs | Controls found / planted |
|---|---|---|
| OpenAI, `gpt-6.1-sol`, Codex CLI, read-only access to the files under review | 18 | 67 / 70 |
| Google, `gemini-4-argon-high`, text-only (document inline, no tools) | 19 | 38 / 38 |
| Google, `gemini-3.8-flash-high`, text-only | 1 | 1 / 2 |

## By job group

| Date | Reviewer model | What was reviewed | Jobs | Controls per job | Found / planted |
|---|---|---|---|---|---|
| 2026-11-10 | `gemini-4-argon-high` | sections of a technical research report | 19 | 2 | 38 / 38 |
| 2026-11-10 | `gemini-3.8-flash-high` | an engineering plan | 1 | 2 | 1 / 2 |
| 2026-11-10 | `gpt-6.1-sol` | the same engineering plan | 1 | 2 | 2 / 2 |
| 2026-11-10 | `gpt-6.1-sol` | three successive revisions of a statistical design | 3 | 4 | 11 / 12 |
| 2026-11-10 | `gpt-6.1-sol` | three short documents, fact-checked against records | 3 | 4 | 11 / 12 |
| 2026-11-11 | `gpt-6.1-sol` | ten publication dossiers, checked against source files | 10 | 4 | 39 / 40 |
| 2026-11-11 | `gpt-6.1-sol` | a code-versus-rules gap matrix, checked against source files | 1 | 4 | 4 / 4 |

## The misses

| Reviewer model | Job group | What was missed | Recorded reason |
|---|---|---|---|
| `gpt-6.1-sol` | statistical design, second revision | an altered definition (whether two adjacent intervals merge) | not identified |
| `gpt-6.1-sol` | short documents | an altered count | the true count was not in the files the reviewer could read; it declined to guess |
| `gpt-6.1-sol` | publication dossiers | an altered statement about a second file | the reviewer treated the file as missing and audited no claim about it |
| `gemini-3.8-flash-high` | engineering plan | an altered structural statement | in its place the reviewer reported a different, real inconsistency |

## How to read these numbers

- With all S controls found, the one-sided 95% lower bound on the probability of finding such a control is 0.05^(1/S) if the trials are independent (Clopper-Pearson): 0.924 for 38 of 38. The bound assumes independent, exchangeable trials; jobs on related documents are not independent, so for them it does not hold as stated.
- For 67 of 70, the one-sided 95% Clopper-Pearson lower bound under the same assumption is 0.892. Bounds are rounded down.
- These are detection rates for planted single-line contradictions. They are not rates for real defects, which can be harder to find, and they say nothing about false positives: no clean documents were included.
- The reviewers worked in different modes on different documents. The rows cannot be used to rank the models.
- All controls were planted and scored by models of one lab (Anthropic), under the scoring rule in `METHOD.md`. No second scorer checked the counts.
- The number of controls per job changed from two to four on 2026-11-10; both kinds are in the table.
- One of the four misses could not have been found: the true value of the altered count was in a file the reviewer could not open. It is counted as a miss because the rule was fixed beforehand, and it is a defect of the control, not evidence about the reviewer.
