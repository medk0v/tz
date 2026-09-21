# Contact blacklist

Operators with `contacts:manage` can block or unblock a contact from the contact
directory or the conversation's contact details. A block applies to that stored
contact across its channels within the project. It does not identify new visitors
or merge separate contact records.

Each channel has a **Blacklist** settings page. Configure a default language and
one message per language. English, Russian, Ukrainian, Romanian, Chinese, and
Hindi are supplied initially. The default text tells the customer they were
blocked for spam and asks them to use the email address listed on the website.
The message can be replaced with the channel's preferred wording.

AI agents also have **Text when blacklisted** and **Use the message language**
settings. A nonempty agent text overrides the channel response. With language
matching disabled, it is sent exactly as saved. With matching enabled, a separate
model request detects the language of the incoming message and translates only
the saved text. It has no conversation instructions, knowledge, or tool grants.
For attachments without text, the widget/Telegram language is used as a hint.
Translation failure or timeout sends the original saved text. An empty agent
text retains the existing channel translations.

Selection prefers the active agent participating in the conversation, then an
active agent connected to the channel, preferring its configured language and
automatic participation setting. Translation runs through a separate durable
worker queue; duplicate incoming delivery does not enqueue another reply.

Replies use the widget session language or Telegram sender language. Selection
tries the exact language tag, its base language, and then the channel's default.
Regional tags such as `ru-RU` are supported. The default language must have a
nonempty translation; each message is limited to 4,000 characters.

While blocked, each new incoming message receives one automatic system reply.
Widget attachments and Telegram media are included. Retries of the same message
do not create another response. Incoming messages remain in history, but bypass
normal routing, AI generation, and new-message browser, Telegram, and mobile notifications. Existing
AI work is cancelled and checked again before its result is saved or delivered.
Unblocking restores normal handling for subsequent messages.

Management endpoints:

- `PATCH /api/v1/contacts/{contact_id}/block` with `{ "blocked": true }`.
- `PATCH /api/v1/channels/{channel_id}/blacklist` with
  `{ "default_language": "en", "translations": { "en": "Reply text" } }`.

Contact mutations enforce tenant, project, and every associated Inbox scope.
Channel settings require `channels:manage`. Changes are audited. Explicit
membership and access-token grants are preserved; grant `contacts:manage` to
restricted roles or keys when needed.

Deploy migrations `0073_contact_blacklist.sql` and
`0075_ai_profile_blacklist_reply.sql` with the updated API, worker, and
frontend. Local test results do not verify delivery by a production Telegram bot.
