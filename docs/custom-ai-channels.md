# Custom channel configuration

Use **Channels → Add channel** to name a custom channel, choose its icon, and
select a connection method: **AI agent** or **External API**. There is no separate
External API row in the catalog. Each saved channel appears with its own name
and icon.

For AI, select an existing project agent and describe the work. An
instructions-only draft can also be saved before selecting an agent. External
API drafts do not require an agent. An optional HTTPS address can reference
the source or its documentation; it is not a generated receiving endpoint.

Select the intended result in the workspace: conversations, tasks, or another action
described in the instructions. The Inbox identifies the department context in
all cases. The source address is optional for work entirely inside the workspace.

Configuration is persisted by `/api/v1/custom-ai-channels`. List, create, update,
and soft-delete operations enforce project and Inbox access. Referencing an
agent additionally requires the same AI-management access used by the agent
settings screen. The Inbox determines which department can see the channel.

## Current execution boundary

These connections are **drafts**. Saving does not log into the source, run an
agent, import conversations, create tasks, or send replies. There is no incoming
API endpoint, polling worker, or outbound transport for these drafts yet. The
database prevents activating the internal `custom_ai` kind and
message creation rejects it instead of marking an undelivered reply
as sent. Drafts are excluded from regular AI channel assignments and routing
rules.

The current AI browser reads public pages in a fresh browser context. It has no
saved account session and cannot log into a site's private messages. Reading
an authenticated inbox requires a separate session-capable source executor and
an explicit account connection. Instructions do not grant account access.

Before enabling execution, implement source authorization, stable external
thread/message IDs, atomic deduplication and cursor updates, and delivery
receipts for replies. Do not store passwords or session cookies in the channel
instructions or URL; use the executor's credential storage when it is added.

Migration `0084_custom_ai_channels.sql` adds the channel kind and settings table.
Migration `0085_custom_channel_configuration.sql` adds connection methods and
intended results. Existing drafts retain their settings and default to AI with
a conversation destination. Choosing a destination does not grant permission
to execute actions: any future executor must check the corresponding current
project, department, agent, and action permissions. The configuration API is described
in the backend OpenAPI document. Integration tests cover persistence,
department and agent access, rejected updates, and prevention of false delivery.
