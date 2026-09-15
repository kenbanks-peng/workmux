# Issue tracker: GitHub

Issues and specifications live in GitHub Issues for
`kenbanks-peng/workmux`. Use the `gh` CLI.

Use `--repo kenbanks-peng/workmux` for issue and PR commands.
This selects this fork instead of the upstream repository.

## Issue operations

- Create: `gh issue create --repo kenbanks-peng/workmux --title "..." --body-file -`.
  Supply a heredoc for a multi-line body.
- Read: `gh issue view <number> --repo kenbanks-peng/workmux --comments`.
- List: `gh issue list --repo kenbanks-peng/workmux --state open --json number,title,body,labels,comments`.
  Add label and state filters as needed.
- Comment: `gh issue comment <number> --repo kenbanks-peng/workmux --body "..."`.
- Add labels: `gh issue edit <number> --repo kenbanks-peng/workmux --add-label "..."`.
- Remove labels: `gh issue edit <number> --repo kenbanks-peng/workmux --remove-label "..."`.
- Close: `gh issue close <number> --repo kenbanks-peng/workmux --comment "..."`.

“Publish to the issue tracker” means create a GitHub issue.
“Fetch the relevant ticket” means read the issue and its comments.

## Pull requests

**PRs as a request surface: no.**

GitHub issues and PRs share numbers. If the type is unknown,
try `gh pr view` first, then `gh issue view`.

## Wayfinding operations

- Map: one issue with the `wayfinder:map` label. Its body contains
  Notes, Decisions-so-far, and Fog.
- Child ticket: link an issue to the map as a GitHub sub-issue.
  If sub-issues are unavailable, use a task list in the map and
  `Part of #<map>` in the child body.
- Child labels: `wayfinder:research`, `wayfinder:prototype`,
  `wayfinder:grilling`, or `wayfinder:task`.
- Blocking: use native GitHub issue dependencies. If unavailable,
  put `Blocked by: #<number>, #<number>` in the child body.
  A ticket is unblocked when all blockers are closed.
- Next ticket: select the first open child in map order with no
  open blockers and no assignee.
- Claim: assign the ticket to `@me` before other session writes.
- Resolve: comment with the result, close the child, and add a
  summary and link to the map's Decisions-so-far section.

Use `repos/kenbanks-peng/workmux` for repository API paths.
