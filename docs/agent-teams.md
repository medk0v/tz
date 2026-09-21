# Agents and team tasks

The AI workspace contains editable project profiles. Migration `0081` introduced
five preset profiles: sales, operations, quality, HR and universal. Later catalog
migrations can extend that initial set. Role instructions and descriptions are supplied; profiles
start as drafts without a model connection, knowledge access or tool permissions.
Administrators can edit them or create custom profiles. A preset's identity is
immutable through the API, while its settings remain editable. Seeding never
overwrites an existing profile or an edited preset.

Tasks have two execution modes:

- **Independent:** each selected agent receives the task and returns its own
  result. Existing schedules and history continue to work.
- **Team:** a separate coordinator plans work for one to eight members, members
  receive their assignments and prerequisite results, then the coordinator
  checks the outputs and publishes one result. Roles can be adjusted per task.
  The coordinator and members may be agents or employees; employee steps wait
  for the person's answer in My tasks. See [task-board.md](task-board.md).
  A team task may instead carry a predefined plan (`plan_steps`), as a launched
  process does: nobody plans, the performers of the steps are the team, and
  the coordinator only reviews. See [Predefined plans](#predefined-plans).

Both modes support manual execution and the existing once/daily/weekly/monthly/
yearly schedules. A manual team draft may reference unconfigured profiles. Before
running, all assigned profiles need active supported model connections. Scheduled
team tasks require that configuration when saved. Scheduled occurrences coalesce
while the same task has unfinished work, including executions needing attention.

## Execution lifecycle

Migration `0080` adds executions, steps, dependency edges, attempts and events.
The worker stores each transition before calling a provider and releases capacity
between steps. The coordinator's plan must be acyclic, contain only selected
members and stay within the step budget. Planning and review have no tool grants.
Specialists use their own profile's permitted tools.

Instructions, task text, participant names, assigned roles and permission ceilings
are captured at execution creation. Later task edits affect future executions.
Provider/model settings and current permissions are resolved again for each step;
revoked access cannot be restored by an old snapshot. Knowledge and tool results
are treated as data, not instructions granting additional authority.

The coordinator can request one correction round. Correcting an upstream result
also recomputes its dependent work, preserving the original outputs in history.
The round is rejected if it would exceed the execution budget or repeat a step
with possible external effects.

The initial limits are eight members, twelve planned work steps, twenty total
steps, forty provider attempts, three attempts per step and one hour per execution.
Each provider call has at most nine minutes, with an eleven-minute ownership
lease covering cleanup. Finished execution trees are retained for thirty days.

## Predefined plans

A team task with `plan_steps` holds its own ordered steps, each with a title,
instructions and a performer, either an agent or an employee. Launching an
approved process creates such a task (see [processes.md](processes.md)), and
the tasks API accepts `plan_steps` directly. The server derives the members
from the performers, so the one-to-eight rule does not apply: the coordinator
may perform steps itself, and a task may have no other members or up to thirty.
Every performer passes the same checks as a member. Step titles and
instructions, like the task text, must not contain a NUL character, which the
database cannot store; such a request answers HTTP 400.

An update checks the agents of a plan task every time. Its employees are
checked and rewritten only when the request names `plan_steps`, even with the
stored plan, or when the derived employee members or the employee coordinator
change, so rescheduling a task never fails over people it leaves as they are.
An update that omits `plan_steps` keeps the stored plan. If another update
changes that plan before this one is written, it answers HTTP 409 and changes
nothing: a full replacement sent by a calendar never undoes an edit of the plan.

An execution of such a task is seeded from the plan when it is created. It has
no planning step and makes no planning call, and logs `plan.predefined` instead
of `plan.accepted`. The steps form a linear chain: each step receives only the
result of the previous one, and the coordinator's final review sees every
result. The execution stays queued until its first step is claimed, or runs at
once when that step belongs to an employee. Review, the single correction
round, retry and cancellation work as for planned executions. The plan is frozen
with the rest of the snapshot, so editing the task affects later executions.

Predefined plans have their own limits: thirty work steps, `2 × steps + 2` step
rows so that the whole plan can be corrected once, `max(40, 2 × steps + 10)`
provider attempts, up to thirty corrections in one review instead of twelve, and
`max(60, 10 × steps + 10)` minutes for a team of agents alone. A team with an
employee keeps thirty days. Executions planned by a coordinator keep the limits
above.

A scheduled occurrence whose team cannot start, for example because a performer
lost access, no longer blocks the scheduler. It is recorded as a failed
execution with an `execution.not_started` event that carries the reason, and the
schedule moves on. This applies to every scheduled team task.

Independent ready steps may execute in parallel, including through OpenClaw. Task runs,
API calls, conversation replies and team steps share `worker.ai-run-concurrency` (default 2)
and each agent's `max_concurrent_runs` setting (default 2). A shared PostgreSQL admission lock
and expiring execution leases enforce both limits across worker processes. Dependencies still
require their parent steps to succeed first.

## Cancel, retry and history

Manual starts require a UUID `Idempotency-Key`. Repeating a key retrieves the
original execution; another unfinished execution prevents a duplicate start.
Cancellation stops further work and revokes tool access. Runtime ownership stays
occupied until cleanup completes or the bounded lease expires. Cancellation
cannot undo an external action already completed.

Transient failures can retry automatically only when a step has no possible
external effects. The UI exposes safe failed-step retries within the original
deadline and attempt limit. Uncertain effectful outcomes require checking the
external system instead of a blind retry. A cancelled or finished execution
cannot be revived by a late worker response.

The task workspace presents the result, step timeline, source material, errors
and run history. Starting, cancelling and retrying require `tasks:manage`;
participants with `tasks:own` can follow executions of their tasks and answer
their own steps. An Inbox-scoped access token cannot access task orchestration. Demo accounts can inspect
the new history endpoints but cannot mutate them.

## Validation and rollout

The OpenAPI specification documents task settings and the execution endpoints.
`backend/tests/ai_orchestration.rs` exercises actual worker transitions against a
deterministic local provider and disposable PostgreSQL databases.
`backend/tests/ai_profile_presets.rs` verifies seeding, editable settings and scope
boundaries. These SQLx tests need a PostgreSQL role with `CREATEDB` and must be run
explicitly with `--ignored`. They do not call a live model.

Deploy migrations, API, worker and frontend together. Starting the API with
automatic migrations enabled applies pending migrations, including `0080` and `0081`; do not rewrite earlier
applied migration files during a deployment. Build/test success is separate from
a live provider run and from production deployment.
