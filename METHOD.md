# Auditing one model's work with another lab's model: the method

Status: DRAFT (2026-11-11). This describes a working practice; section 7 lists defects that were found in its tooling and what remains limited.
Licence: CC BY 4.0.

## 1. Roles

| Role | Does | Must not |
|---|---|---|
| Author | Writes the document or code under review | Review its own work |
| Preparer | Plants the controls, builds the prompt, keeps the key | Show the key to the reviewer |
| Reviewer | A model from a different lab than the author. Reads; reports findings with quotations | Write, build, run tools it was not given |
| Reconciler | Decides which findings change the work | Decide on judgment alone (section 4) |
| Person | Decides the disputed findings | |

The author and the reconciler may belong to the same lab. The reviewer may not belong to the author's lab.

## 2. Planted controls

Before a job is sent, the preparer makes a copy of the work and alters a fixed number of single lines of fact in it: four in current use (the earliest jobs used two). Each alteration contradicts either the source material or another line of the same document: a wrong count, a wrong constant, a wrong file name, a threshold that differs from the one stated elsewhere.

- The original is never altered. Only the copy inside the prompt is.
- The key (label, original line, planted line, how it can be detected) is stored where the reviewer cannot read it.
- The prompt tells the reviewer how many lines were altered and asks it to find them, and to say how many it found if it finds fewer. It is told not to guess.
- All controls of one job are planted the same way, so that jobs can be pooled.
- A control must be detectable from what the reviewer can read. A control whose true value lies in a file the reviewer cannot open measures nothing; when that happens it is reported as such with the job.

A control counts as found only if the reviewer quotes or unmistakably names the altered statement.

## 3. The prompt

Fixed structure:

1. Hard rules: what the reviewer may read and what it may never open.
2. The sandbox it runs in, and a marker to use when it needs something outside it.
3. The outcome: who will use the audit and for what decision.
4. The controls notice.
5. Numbered tasks.
6. The output format, with a line limit and a first line that carries the verdict.
7. The work under review, between explicit markers that label it as untrusted data.

The reviewed text is data. Instructions that appear inside it are not followed.

## 4. Reconciliation on evidence only

For each finding the reconciler records one row:

| Field | Content |
|---|---|
| Finding | The reviewer's claim, in the reviewer's words |
| Source quotation | The passage the reviewer presents as a verbatim quotation from the reviewed text, if it gives one |
| Quotation check | Count of exact matches of that source quotation in the reviewed text (a fixed-string search). Zero matches: the row is marked as an unmatched quotation and the finding is not credited on the strength of it. An exact search also fails on reflowed whitespace or on an excerpt with an omission, so a quotation is called fabricated only after a person has looked for it in the text. The reviewer's own prose is never searched for; a finding without a source quotation is judged on evidence alone |
| Evidence | A source line, a derivation or compiler output that settles the claim |
| Decision | ACCEPTED, REJECTED or DISPUTED |

A finding is accepted or rejected only when the evidence field settles it. Everything else is DISPUTED and goes to the person. The reconciler does not resolve a disagreement between its own lab and another lab by preference.

## 5. Tool events

Reviews that are meant to be text-only are run through a wrapper that keeps the model's event log. An auditor program reads the log after the run. Any tool-related event makes the answer untrusted, whatever the answer says. The auditor counts as tool-related every logged step that is neither the user's input nor the model's response. That includes error events: an error event is not a tool call, but a run that logged one is rejected all the same, and the record says which kind of event it was.

The strict check is a policy about the log, not a judgment of the run: it requires a readable log, zero such events and a non-blank answer. It does not validate the CLI's event schema, and it cannot see an action the CLI did not log.

## 6. The score ledger

Every job adds one row: job, reviewer lab and model, controls found, controls planted, verdict, anything unusual about the run. Rows are pooled per reviewer lab.

How to read the pooled number. If a reviewer found all S planted controls, the one-sided 95% lower bound on its probability of finding such a control is 0.05^(1/S) under independent trials (Clopper-Pearson). Some values:

| Controls found, none missed | Lower bound |
|---|---|
| 2 | 0.223 |
| 4 | 0.472 |
| 14 | 0.807 |
| 38 | 0.924 |

Values are rounded down, so a printed bound never exceeds the computed one. One job with four controls certifies very little, and at least 14 hits with no miss are needed before the bound passes 0.80. Three cautions apply to every such number:

- The bound assumes independent, exchangeable trials. Controls in related jobs are not independent, so for them the bound does not hold as stated.
- It is a bound on finding planted single-line contradictions. Real defects can be harder.
- When every reviewer finds every control, the controls cannot rank the reviewers.

## 7. Defects found in the tooling, and what remains limited

An outside-lab review of the wrappers found three defects, fixed on 2026-11-11 in `scripts/`:

- A failed job could leave an earlier output file in place, which could then be read as the new answer. Names are now single-use and an existing output aborts the run.
- "Zero tool events" passed even when the log was empty or the answer was missing. A run is now trusted only with exit code 0, a readable log, zero tool-related events and a non-empty answer (`xfam-audit --strict`).
- The reviewing model was not selected explicitly. It is now passed by name in every lane. In the Codex lane the model the CLI reports is also recorded and compared with the one requested; the text and web lanes record only the model that was requested.

Three further reviews of the published scripts each found more ways a run could end as trusted without deserving it: a log the audit queries could not read, an earlier answer hiding an empty or missing final one, a prompt that failed to load, a result file that could not be written, a tool name crafted to slip through the allow-list, an invalid allow-list expression, a log whose lines were not one JSON object each, a logged error next to a valid answer, files hidden under a reserved name. Each of those is now rejected in the code. `scripts/tests/run_cases.sh` replays the log-shaped cases, the invalid expression, a blank or unreadable prompt and a repeated name with a stand-in agent that prints canned logs; it does not replay a result file that cannot be written or a job directory that cannot be listed or removed, which were checked by reading the code only. Three rounds that each found new cases are evidence that more may exist: the checks cover the cases listed, not every way a log can mislead.

What remains limited:

- The wrappers record what a model did and refuse to vouch for runs they cannot read. They are not a security boundary.
- The web lane hides the directories it is told to hide. The rest of the system stays readable inside it, the network is open, and by default the text lane gives the CLI the caller's whole home directory. Neither lane is a boundary around private data.
- Single-use names are enforced with an exclusive file creation; nothing stronger than that reserves a name.
- The auditor sees only what the CLI logs.

These are listed so that nobody relies on the tooling for more than it does.

## 8. What the method does not show

It does not show that a review is complete, that an accepted finding is correct beyond the cited evidence, or that a clean result means clean work. It shows how often a given reviewer finds a given kind of planted error, and it leaves a record that a person can check.
