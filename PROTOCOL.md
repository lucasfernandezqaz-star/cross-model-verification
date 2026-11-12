# Cross-lab model review: study protocol

Status: DRAFT, not frozen (2026-11-11). No scored run has been made under this protocol.

Not yet settled, and to be fixed before the freeze: which labs author and which review each item (reciprocal assignments), the models used to generate and to review, checks for unintended defects in clean base items, how clean and defective versions are kept apart across sessions, how repeated runs are aggregated, the review baseline of arm C, validation of the run pipeline, and who scores judgment cases when only two labs take part (a label-blinded person is the fallback).
Licence: CC BY 4.0.

## 1. Question

When one language model reviews work produced by another model:

1. Does a reviewer from a different lab detect more defects than a reviewer from the same lab as the author?
2. Does telling the adjudicator who wrote a finding change which findings are accepted?
3. When two models from different labs implement the same numerical routine independently, how often do differential tests expose an error that review alone missed?

The study measures these on synthetic tasks with known answers. It makes no claim about tasks or model versions outside the tested set.

## 2. Items

Three item classes:

| Class | Content | Ground truth |
|---|---|---|
| Specification contradiction | A technical document of fixed length in which one statement contradicts another statement or a stated formula | The contradicting line and the correct statement |
| Numerical derivation | A worked derivation with one wrong step or one wrong value | The exact value, in rational or decimal form |
| Small Rust review task | A function of under 200 lines with its specification and one injected defect | A failing test vector and the defect class |

Rules for every item:

- Each defective item has a clean counterpart that is identical except for the defect. Clean items measure false positives.
- Each defect carries a documented class (for example: wrong constant, wrong unit, off-by-one boundary, sign error, missing refusal condition).
- For arm A, a model from one participating lab generates each clean base item from the benchmark builder's specification. Its lab is recorded as the author lab for both the clean and the defective version. The person injects the documented defect without model assistance.
- Ground truth is checked by a person before the item enters the pool. For code items, compiler and test output establish the facts.
- Items are synthetic. No private code and no market data enter the pool.

The item pool and the answer keys stay private until all scored runs finish. At freeze, a SHA-256 digest of the pool and of the keys is published in this repository, so that the pool can later be shown to be unchanged.

## 3. Arms

**A. Same-lab versus cross-lab review.** Every item is reviewed by a model from the author's lab and by a model from another lab, on identical prompts, at three context lengths (the item alone; the item inside a medium document; the item inside a long document), with repeated runs per cell.

**B. Authorship shown or hidden.** Findings from arm A are passed to an adjudicating model. Assignment is randomized between two conditions: the reviewer's lab is hidden, or it is labelled. The adjudicator must accept or reject each finding and cite the evidence.

**C. Independent implementations.** Models from two labs implement the same specified numerical routines without seeing each other's code. The implementations are compared by differential tests and by exact-arithmetic test vectors. Agreement between the two does not exclude an error in the shared specification; that limit is reported with the results.

## 4. Outcomes

Primary comparison (one, named before the pilot ends): defect recall of cross-lab review minus defect recall of same-lab review, on identical items.

Secondary outcomes:

- false-positive rate on clean items;
- unsupported acceptance and unsupported rejection of findings by the adjudicator (a decision is unsupported when the cited evidence does not contain the stated fact);
- detection added by a second reviewer beyond the first;
- token cost per item.

A reviewer that does not answer is not scored as a miss in the primary comparison, which uses the items both reviewers answered. So that selective non-answering cannot flatter a reviewer, a second detection rate is always reported next to it over all runs, in which a non-answer counts as not detected, together with the number of non-answers per reviewer. Non-answers are recorded in six separate categories: abstention the prompt asked for; a tool that was not available; a permission that was denied; a tool error; a choice by the model not to proceed; a safety refusal.

## 5. Scoring and adjudication

- A defect counts as detected only if the reviewer quotes or unmistakably identifies the defective statement. A correct verdict without the location does not count.
- Every quotation a reviewer gives is checked mechanically against the item text. A quotation with no exact match is recorded as unmatched and the finding is not credited on the strength of it; it is recorded as fabricated only after a person has confirmed that no reflowed or partial form of it is in the text.
- Scoring means deciding whether a reviewer detected a defect. It is done by a script wherever the ground truth allows it. Where a judgment is needed, the scorer does not belong to the lab of either the author or the reviewer and does not see which lab produced the finding (proposed; to be fixed at freeze).
- The scorer is not the adjudicating model of arm B. In arm B the adjudicator is the object of study, and in its labelled condition it is shown the reviewer's lab on purpose; its decisions are themselves scored by the blind procedure above.
- Disagreements that no source, derivation or compiler output settles are recorded as disputed and are decided by a person. They are reported as a separate count.

## 6. Order of work

1. Pilot on separate items to estimate paired disagreement, within-item dependence, non-answer rates and token usage, for feasibility and sample-size planning. Pilot items are excluded from scored evaluation.
2. Sample sizes chosen from the pilot estimates for the primary comparison, against a precision target declared before the pilot results are read (proposed: a target 95% confidence-interval half-width of at most 10 percentage points for the paired difference; a planning target, not a guarantee of the precision achieved). The rationale is published before scored runs. If the pilot shows that an informative design does not fit the available budget, that is reported and the protocol is revised before any scored run.
3. Freeze: item pool, keys, prompts, model versions, settings, scoring script, adjudication rules, the primary comparison, the analysis plan and the stopping rule. The digests of section 2 are published.
4. Scored runs.
5. Publication of prompts, items, keys, scoring script, model configurations, adjudication records, aggregate results and reproducible failure reports.

## 7. Analysis

- Paired comparison on identical items; uncertainty computed with the item as the cluster, because repeated runs of one item are not independent.
- No comparison beyond the primary one is called confirmatory.
- If every reviewer detects every defect of a class, that class cannot rank reviewers. It is reported as a ceiling, not as equality.

## 8. Run records

For every run: provider, exact model identifier, date, reasoning or effort setting, temperature where it can be set, full prompt, context length, tools and permissions available to the model, every tool event, token counts, and the unedited answer.

## 9. Limits known in advance

- Planted defects may be easier or harder to find than real ones. The study measures detection of the planted classes only.
- Models change. Results hold for the recorded versions.
- A single person specifies the items, injects the defects and checks the ground truth. Errors in the keys are possible; corrections after unblinding are logged with their date and reason.
- The comparison concerns the review of defects injected by a person into model-generated material. It does not show how reviewers detect errors that a lab's model makes on its own.
- The wrappers used to run the models record tool events; they are not a security sandbox.
