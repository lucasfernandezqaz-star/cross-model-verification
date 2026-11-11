# Reconciliation record: a synthetic example

Everything here is invented to show the format. The reviewed document is a two-page specification of a train timetable generator.

Job: `timetable_spec_review_01`. Author: a model of lab A. Reviewer: a model of lab B, text-only. Reconciler: a model of lab A. Planted controls: 4 (key held by the preparer).

Run check: exit code 0; auditor `--strict` passed (0 tool-related events, answer of 2,114 characters, 0 unparsed lines).

## Controls

| Control | Planted line | Found by the reviewer | How it was credited |
|---|---|---|---|
| CA | "Trains depart every 20 minutes" where section 2 says every 15 | yes | quoted the planted line and section 2 |
| CB | "The last departure is 23:40" where the table ends at 23:45 | yes | quoted the planted line and the table row |
| CC | "Platform 3 serves the northern branch" where the map says platform 2 | no | not mentioned |
| CD | "A day has 94 departures" where 15-minute spacing from 05:00 to 23:45 gives 76 | yes | quoted the line and gave the arithmetic |

Score for the ledger: 3 found of 4 planted.

## Findings

| # | Reviewer's finding | Check of the source quotation the reviewer gave | Evidence | Decision |
|---|---|---|---|---|
| 1 | "Section 4 rounds dwell time down, but the worked example rounds it up" | reviewer's two quotations each match the document exactly once | section 4 line 12 and example line 3 contradict each other | ACCEPTED: section 4 corrected to match the example, which follows the stated rule |
| 2 | "The document states that the generator will overflow for more than 255 stations" | the reviewer presented that sentence as a verbatim quotation from section 5; it does not occur in the document (0 matches) | none | REJECTED: unmatched quotation; a person then confirmed that section 5 says nothing about a station limit, so it is recorded as fabricated. The claim itself is not evaluated |
| 3 | "Holiday service should use the Sunday pattern" | quotation matches once | the document does not say which pattern applies; no source settles it | DISPUTED: goes to the person who owns the specification |
| 4 | "The interval formula divides by zero when first and last departure coincide" | quotation of the formula matches once | substituting equal times into the formula in section 3 gives a zero denominator | ACCEPTED: a guard clause is added to the specification |

## Outcome

Accepted 2, rejected 1, disputed 1. The disputed finding is recorded with both positions and is not resolved by the reconciler.

The missed control (CC) is reported with the job. It is a reminder that the reviewer's silence about a statement is not evidence that the statement is right.
