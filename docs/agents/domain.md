# Domain docs

## Layout

This repository uses a single-context layout:

- `CONTEXT.md`: domain terms at the repository root.
- `docs/adr/`: architecture decision records.

## Before code exploration

1. Read `CONTEXT.md`.
2. Read the ADRs in `docs/adr/` that apply to the work.

If a file or directory is absent, continue silently.
Create domain documentation through domain-modeling when terms
or decisions are resolved, not as a setup requirement.

## Use domain terms

Use the terms defined in `CONTEXT.md` in issues, proposals,
tests, and code. Respect its listed terms to avoid.

If a needed concept has no definition, check existing project
usage. Record a real gap for domain-modeling.

## Report decision conflicts

If a proposal conflicts with an ADR, identify the ADR and
explain why the decision should change.
