# Team, job positions and KPI definitions

The Team workspace at `/cabinet/p/{project_id}/team` groups human employees,
AI agents and job positions. Existing account creation and AI connection settings
remain available through the corresponding Team actions. Assigning a job position
does not change the employee's access role, department access, or an agent's tools.

Job positions are the shared catalog for job descriptions and KPI definitions.
An organization-chart seat or a process responsibility can refer to this catalog;
neither needs its own copy of instructions or KPI definitions. A position belongs
to a project and optionally a department. An empty department selection makes it
available throughout that project. Position assignments and organization-chart
seats also suggest performers when an approved process is launched as a task; the
suggestion grants no access (see [processes.md](processes.md)).

Each tab and position has its own address: `/team/agents`, `/team/positions`,
`/team/positions/{position_id}` and its `/instructions` or `/kpi` tab, and
`/team/positions/new` for a new position (all under `/cabinet/p/{project_id}`).

Each job position has Description, Job description and KPI tabs. Managers can
save position details, assign people or agents, and maintain the following KPI
fields: name, formula, required data source, target, evaluation period and data
owner. Targets and data owners are editable text; data sources describe required
evidence, not an already connected integration.

## Generation and approval

Generation requires saved job instructions, the manager's goal and an active,
visible OpenAI or OpenAI-compatible model connection. The server loads the saved
position and department context and requests three to five structured proposals
in English, Russian or Romanian. The request has no tool grants. Responses are
validated before they become editable rows. A missing or invalid model connection
produces an error; the feature does not substitute hardcoded examples.

Generation does not save or approve the returned proposals. Saving the draft
preserves the last approved KPI snapshot. Approval requires at least one complete
row, including the target and data owner, and records the approving account and
time. The UI saves current edits before approving their revision. Position updates
and approvals reject stale revisions with HTTP 409 so concurrent edits cannot
silently replace each other.

This MVP stores definitions and approved targets only. It does not calculate
actual performance or provide plan-versus-actual reporting. Actual values require
separately connected and verified sources.

## Access and storage

These routes require a password session in the selected project. Project-wide
accounts with `teams:manage` can manage definitions and assignments; generation
also requires `ai:manage`. Department-restricted employees have read access within
their department. Employees without management access see their own human entry
and approved KPI definitions; draft KPI rows and the draft goal are hidden by the
server. AI entries additionally require `ai:manage`. Demo mutations and access
token access are rejected.

When an employee's department or an agent's visibility changes, their existing
job assignment is cleared so a manager can confirm it in the new scope. A no-op
update preserves the assignment. Restoring revoked access also clears an old job
assignment. Position department changes reject incompatible existing assignments
and organization-chart seats. Team assignments reject a different job position
when the person or agent already occupies a seat linked to another catalog entry.
Unsaved edits are guarded when leaving the workspace or closing the browser page.

Migration `0100_job_positions.sql` adds the catalog and assignment references.
Migration `0101_team_navigation.sql` adds Team to existing department menus before
AI, or at the end if AI is absent, preserving the existing start page and order.
Department administrators can subsequently change that menu.

The API contract is documented in `backend/openapi/openapi.yaml`. Backend tests
cover authorization, assignment scope, draft/approval behavior, revision conflicts
and model response validation. PostgreSQL integration tests use disposable
databases and are opt-in with `--ignored`; the model transport test uses a local
mock provider. These checks do not exercise a live model or a deployment.
