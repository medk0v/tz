# Project notes

The Notes section brings Structa's nested pages and Markdown editing into the
the workspace project workspace. Each page has a title, optional icon, Markdown body,
optional parent and shared favorite status. Notes use the workspace authentication,
project selection and role permissions.

## Pages and editing

The page tree supports root pages and child pages. Readers can search page
titles in the current project and filter favorites. Editors can create,
rename, duplicate and move pages, edit their icons, and mark favorites. Moving
a page below itself or one of its descendants is rejected. A page with children
cannot be deleted until its children are moved or deleted explicitly.

Editors can drag pages in the full tree to reorder siblings or move an entire
subtree. The top and bottom quarters of a row insert before or after it; the
middle nests the dragged page as its last child. Dropping on the top-level
target moves the page to the end of the root list. The order is shared,
stored on the server and retained after reload, including on public links.
Moving a page only updates its hierarchy and order; unsaved document content
stays in the editor. The parent selector also supports searching page titles.

The editor provides visual and Markdown source modes. Formatting includes
headings, emphasis, strikethrough, inline code, lists, checklists, quotes,
tables, links and dividers. Saves are explicit, and navigation prompts before
discarding unsaved edits. Concurrent changes are detected through a version
number: a stale save keeps the local draft and offers reloading the current
page or saving the draft as a copy.

This integration does not import existing Structa data. Notes are not
automatically indexed as an AI knowledge base or added to agent prompts.

## Project access

- `notes:read` allows listing and reading every note in the active project.
- `notes:write` allows creating, editing, moving and deleting project notes.

New standard roles include both permissions. The upgrade grants them to
administrators and unchanged manager/operator presets; customized roles retain
their existing grants and can be configured in Roles. Department menus showing
all sections gain Notes; configured subsets can enable it in Departments.

These are shared project documents. Their content and favorite status are not
personal to the author, and notes do not have separate department or page-level
access rules. A parent must belong to the same project. Switching the active
project changes the notes available to the request.

## Public links

A public link exposes a selected note or selected pages in one project.
The linked root page is always included. Owners can enable subpages and select
each available branch using checkboxes; excluding a parent also excludes its
descendants. New links store an explicit selection, so newly created pages stay
private until selected. Existing links retain their previous all-descendants
scope until the owner saves a selection.

The scope does not include other projects, the tenant's entire workspace or a
parent project's aggregated subprojects. Every read, edit and embedded-table
request rechecks the current hierarchy and selection. Moving a page outside a
shared subtree or beneath an excluded parent removes access through that link.

Links allow reading without a the workspace account. An editor with `notes:write` can
create, list, configure and revoke project links, optionally requiring a password and
allowing editing. Public editors may change an existing note's
title, Markdown body and icon, with the same version check as authenticated
editing. They cannot create, delete or move notes, change favorites or manage
links. Owners can change appearance, selected pages and editing permission on an
existing link without changing its address, password or access tokens. Settings
updates require the link's current version; stale updates return a conflict.

Public pages show the document without a the workspace header or footer. A single page
has no navigation panel; a shared hierarchy or a project with multiple visible
pages has navigation. Owners can preset the palette and light/dark appearance,
and separately allow visitors to change them. Visitor choices are isolated from
the authenticated workspace preferences. A locked publication always uses the
owner's preset. The Lite edition retains its appearance restrictions.

The address is `/notes/share/{token}`. A custom slug is the token itself; without
a slug the server creates a random secret beginning with `_`. The random secret
is returned only when the link is created, so it must be copied then. Lists of
existing links include their scope, password/edit flags and custom slug, but
not their random secret. Revoking a link invalidates subsequent public access.
Links have no configurable expiration date. Unlocking a password-protected link
returns an access token valid for 12 hours.

Custom slugs are trimmed and lowercased, then must contain 3–64 ASCII letters,
digits or hyphens, beginning and ending with a letter or digit. Passwords require
10–128 Unicode characters, at most 512 UTF-8 bytes, without NUL. Omitted or null
`note_id` shares the project; omitted or null `password` disables protection,
and `can_edit` defaults to false.

Publication appearance uses nullable `theme_palette` (`ocean`, `ink`, `graphite`,
`forest`, `plum`, `copper`, `paper`) and `theme_mode` (`light`, `dark`). Unset
values use Ocean and the visitor's system mode. `allow_theme_change` controls
the visitor's palette and mode controls independently of those defaults.
`include_descendants` controls recursion; `included_note_ids` is either an
explicit list of up to 5,000 unique page IDs or null for the legacy all-pages
scope. Explicit lists must include every ancestor between a selected page and
the publication root; the root of a single-page link is implicit. An empty list
shares only that root, or no pages for a project link.

For API compatibility, creation defaults to null appearance, visitor changes
allowed, descendants included and null selection. The UI instead creates
root-only page links or an explicit selection for project links. Settings PATCH
requires all six settings (`theme_palette`, `theme_mode`, `allow_theme_change`,
`include_descendants`, `included_note_ids`, `can_edit`) and `expected_version`;
nullable settings must still be present. Active-link summaries include these
settings and `version`. Public responses expose appearance and the descendant
flag, but never the private selection IDs.

Public API requests carry the link token and, when needed, the unlock
`access_token` in JSON. The public client omits account credentials, avoids
caching and sends no referrer. A session cookie or an operator access token
does not unlock a public link.

Browser error reporting suppresses events and breadcrumbs while a public note
is open and redacts its token from navigation breadcrumbs. Deploy the updated
standard or Lite Nginx configuration together with this feature: its dedicated
`/notes/share/` location serves the SPA without an internal redirect, disables
access logging and sends `no-store`, `no-referrer` and `noindex, nofollow` headers.
Deploying only the application does not update an existing reverse proxy.

## API

Authenticated requests use the existing session cookie or bearer token.
`X-Tz-Project-Id` selects the accessible active project; requests without
the header follow the usual session or token project selection. Unsafe requests
authenticated with a session cookie require `X-CSRF-Token`.

| Method and path | Behavior |
| --- | --- |
| `GET /api/v1/notes` | Returns `{ items: NoteSummary[] }` without document bodies. |
| `GET /api/v1/notes/{note_id}` | Returns a complete note, including its Markdown body and version. |
| `POST /api/v1/notes` | Creates a note and returns it with HTTP 201. |
| `PATCH /api/v1/notes/{note_id}` | Replaces every editable field if `expected_version` matches. |
| `POST /api/v1/notes/{note_id}/move` | Moves a page at `expected_version` to `parent_id` before `before_id`; null means top level or append. |
| `DELETE /api/v1/notes/{note_id}?expected_version=N` | Deletes a leaf note at the expected version with HTTP 204. |

Creation requires `title`. Omitted `body` and `icon` default to empty strings,
`parent_id` to null and `is_favorite` to false. PATCH requires `title`, `body`,
`parent_id`, `icon`, `is_favorite` and `expected_version`; `parent_id` must be
present even when null. Successful saves increment the version.

Titles are trimmed and must contain 1–200 Unicode characters. Icons allow up to
32 Unicode characters. Bodies are preserved exactly and allow up to 200,000
Unicode characters and 800,000 UTF-8 bytes. All three fields reject NUL.
Unknown request fields are rejected. Missing required JSON fields or invalid
field types return 422; invalid content or a cyclic parent relationship returns
400.

Summaries contain `id`, `parent_id`, `sort_order`, `title`, `icon`, `is_favorite`, `version`,
`created_at` and `updated_at`; complete notes also contain `body`. Versions are
positive 32-bit integers. Notes outside the selected project return 404.
Lists are ordered by parent (roots first), `sort_order`, then `id`.
A stale version or deletion of a page with children returns 409. Clients must
resolve conflicts explicitly rather than retrying with a newer version and
silently overwriting another editor's changes.

| Sharing method and path | Behavior |
| --- | --- |
| `GET /api/v1/notes/shares` | Lists active public links for the selected project. |
| `POST /api/v1/notes/shares` | Creates a link with optional password, custom slug and editing permission. |
| `PATCH /api/v1/notes/shares/{share_id}` | Updates appearance, visible pages and editing permission at `expected_version`. |
| `DELETE /api/v1/notes/shares/{share_id}` | Revokes a link in the selected project. |
| `POST /api/v1/public/notes/resolve` | Returns the accessible note tree and the selected note. |
| `POST /api/v1/public/notes/unlock` | Verifies a link password and returns a 12-hour access token. |
| `PATCH /api/v1/public/notes/page` | Saves an existing accessible note's title, body and icon at `expected_version`. |

Public summaries omit `is_favorite` and private project/account metadata. The
shared subtree root has `parent_id: null`, hiding ancestors outside the scope.
Without a selected note, the response opens the subtree root or the first visible
project page in saved order. An empty selection returns an empty list and null note. Public
responses use 401 for a locked link, wrong password or expired unlock token;
403 for a read-only link; 404 for an unavailable/revoked link or an out-of-scope
note; 409 for a stale note version; and 429 when unlock attempts or concurrent
password operations are limited, with `Retry-After: 60`. Unlock attempts are
limited to 10 per minute per link, with four password operations per process.

The complete request and response schemas are in
[`backend/openapi/openapi.yaml`](../backend/openapi/openapi.yaml).

## Typed tables inside notes

In addition to ordinary Markdown tables, saved notes can embed project databases.
Use **Insert table** to create a database or select an existing database and its
saved view. Multiple notes can reference the same database. Removing an embed
preserves the database and its records. Embed ordering is independent of the
note's Markdown body; tables appear below that body.

Databases support text, long text, number, date, checkbox, single select,
multiple select, URL, email, relation and rollup fields. Records have typed
values and a separate Markdown card. Tables support record search, pagination,
manual record/field order, and saved views with AND-combined filters, up to five
sort fields, column visibility/order/width and pinned columns. Relations support
one-to-one, one-to-many and many-to-many links with automatic inverse fields.
Rollups calculate count, sum, minimum, maximum or related values.

A project role's existing `notes:read` and `notes:write` permissions also control
its databases. References stay within the active project. Table, field, record,
view and embed edits use optimistic versions; conflicting saves keep the draft.
A database allows up to 100 fields and 5,000 records, with 64 KiB of typed values
per record. Record Markdown has the same size limits as note Markdown. Selection
fields allow up to 100 options. Relation sets allow up to 100 records. Changing
a field type or its options must remain valid for existing values and rollups.
Relation/rollup structure is fixed after creation.

Public links show tables embedded in their currently accessible notes. Password
protection and revocation apply to every table request. Editable links allow
creating, updating and deleting records, including their Markdown cards, and editing the published table fields.
Views, embeds and ordering are managed inside the workspace. Relations and
rollups pointing to unpublished databases are omitted before filtering, sorting,
search and pagination. Public saves preserve values hidden by this projection.
A saved view is a presentation setting, not a row-level access restriction:
publishing an embedded database makes its records accessible through the link.

| Database API | Behavior |
| --- | --- |
| `GET/POST /api/v1/notes/databases` | List or create project databases. |
| `GET/PATCH/DELETE /api/v1/notes/databases/{database_id}` | Read schema/views, rename, or delete an unreferenced database. |
| `GET/POST /api/v1/notes/databases/{database_id}/fields` | List or create typed fields. |
| `PATCH/DELETE /api/v1/notes/databases/{database_id}/fields/{field_id}` | Update or delete a field with `expected_version`. |
| `GET/POST /api/v1/notes/databases/{database_id}/records` | Search/page records or create a record. |
| `GET/PATCH/DELETE /api/v1/notes/databases/{database_id}/records/{record_id}` | Read or change the complete record/card at its version. |
| `GET/POST /api/v1/notes/databases/{database_id}/views` | List or create saved views. |
| `GET/PATCH/DELETE /api/v1/notes/databases/{database_id}/views/{view_id}` | Read, update or delete a saved view. |
| `PUT /api/v1/notes/databases/{database_id}/{fields,records,views}/order` | Submit every current ID once and the current database version. |
| `GET/POST /api/v1/notes/{note_id}/databases` | Read or insert table embeds. |
| `PATCH/DELETE /api/v1/notes/{note_id}/databases/{embed_id}` | Move, change or remove an embed. |
| `POST /api/v1/public/notes/databases` | Publication-scoped table reads and record mutations using a JSON action and capability tokens. |

Record list queries accept `q`, `view_id`, `page` (default 1) and `per_page`
(default 200, maximum 500). Lists omit record Markdown; single-record reads and
writes include it. Typed values are keyed by field UUID; writes omit computed
rollup values. PATCH is a complete editable snapshot, not a partial cell update.
DELETE requires an `expected_version` query parameter. Embedding changes also
increment the parent note version.

## Remaining Structa capabilities

The following capabilities exist in Structa and are not part of this integration:

- **Individual page blocks:** independently added, duplicated, deleted and
  reordered blocks. the workspace edits one Markdown document; drag-and-drop page
  ordering and nesting are supported.
- **Standalone record links:** record cards currently open as dialogs rather
  than separate pages with their own URLs.
- **Additional embedded references:** existing page references, cross-project
  table/page references and separate filters for each embedded table. Same-project
  databases and their saved views can now be inserted in notes.
- **Media and visual code blocks:** image insertion by URL or upload, file
  attachments, authenticated/public file downloads, and a visual fenced-code
  editor with language selection. Inline code and Markdown source mode are supported.
- **Workspace search and export:** title search across pages and databases in
  every workspace project with Cmd/Ctrl+K, and ZIP export containing nested
  Markdown, database CSV, record Markdown and a structured manifest. Structa's
  ZIP export does not include stored binary attachments.
- **Data migration and external identities:** existing Structa pages, databases
  and files have not been imported. Its supplied UUID and `source_system` /
  `source_id` API conventions for repeatable imports and SQL Notion snapshot
  are not included. Structa has no general import UI.
- **Extended publication scope:** public files and one link for an entire
  workspace with multiple projects.
  the workspace links cover one project's notes or one note subtree, including their
  embedded tables and editable record cards.
