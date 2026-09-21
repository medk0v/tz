# Agent skills

Use **AI → Skills** to create reusable instructions or import an existing
`SKILL.md` file. Review the imported text, give the skill a name and optional
description, select its agents, and save. Skills can be saved without an agent
and assigned later. One skill can be used by multiple agents.

You can also select skills in an agent's **Knowledge and tools** tab, immediately
below **Knowledge bases**. Each checkbox change saves automatically for that
agent in the current project, preserving other agents' assignments and the skill
instructions. A new agent must be saved before selecting skills here.

The importer reads a Markdown or text file into an editable draft. It preserves
the file's instruction text. This imports instructions only; it does not install
a skill's scripts, supporting files, dependencies, or external integrations.

Editing a skill updates the instructions used by subsequent agent requests.
Removing an agent from the selection disconnects that skill. Deleting a skill
also deletes all its assignments while preserving the agents themselves.

## Create with AI

Choose **Create with AI**, describe the skill's purpose, and select an active
model connection. Add pasted text or up to 10 TXT, Markdown, PDF, DOCX, EPUB,
JPEG, PNG or WebP files. The creator analyzes these sources and returns an
editable name, description and instructions. Review the result, assign agents,
then save it using the regular skill editor.

Source files are temporary inputs sent to the selected model. The creator does
not retain the originals or install a reference library for agents. Only the
reviewed instructions are stored when the skill is saved. Generation does not
execute the source instructions or grant the model access to agent tools.

Each document may be up to 20 MiB and each image up to 10 MiB, with 30 MiB total
input and at most 1,000,000 extracted source characters. Long text is summarized
in complete, bounded sections before drafting the skill. This can take several
minutes. Scanned PDFs without extractable text need OCR first or must be supplied
as page images. The selected model must support images to analyze pictures.
Document readers extract text; upload relevant embedded illustrations separately
as image files for the model to analyze them.
Unsupported, unreadable, encrypted or oversized sources produce an error rather
than silently creating a skill from an incomplete excerpt.

PDF ingestion accepts classic cross-reference tables and compressed page content.
For bounded memory use, it rejects compressed object/cross-reference streams and
incrementally updated PDFs before decoding. If a PDF uses these structures,
re-export it as a new PDF with object compression disabled, or supply TXT, DOCX or
EPUB instead. This restriction does not apply to those other document formats.

The deployment Nginx template includes a dedicated generation route with a
610-second upstream timeout. Apply this configuration with the backend update;
any additional reverse proxy must allow the request to run for the same duration.

## Scope and execution

Skills belong to the current project. Assignments may reference agents visible
in that project, including shared agents, but the skill only applies when the
agent executes within that project's context. A shared agent does not carry
another project's skills into its requests.

The runtime loads assigned skills for conversations, tasks, and agent API calls.
Agent tests include skills in their saved configuration snapshot. Skills
supplement the main instructions and do not grant tools, credentials, or access
to additional projects or resources.

Listing requires `ai:manage`. Creating, editing, assigning, or deleting skills
also requires access without an Inbox restriction. Demo accounts are read-only.
Names are limited to 200 characters, descriptions to 2,000, and instructions to
50,000. Each skill can be assigned to at most 64 agents. Content and assignments
are saved in one transaction; invalid agent references reject the whole save.

## API and storage

- `GET /api/v1/ai/skills`: list project skills and agent assignments.
- `POST /api/v1/ai/skills`: create a skill with its assignments.
- `POST /api/v1/ai/skills/generate`: generate an unsaved draft from temporary multipart sources.
- `PATCH /api/v1/ai/skills/{skill_id}`: replace its content and assignments.
- `DELETE /api/v1/ai/skills/{skill_id}`: delete it and its assignments.

Create and update accept `name`, `description`, `instructions`, and
`ai_profile_ids`. The backend OpenAPI document describes the complete contract.
Migration `0090_ai_skills.sql` creates the skill and assignment tables and must
be applied before running the updated API and worker.
