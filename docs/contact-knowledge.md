# Contact context and knowledge titles

Customer replies, scheduled conversation follow-ups, operator reply suggestions,
and automatic closing drafts receive a `tz_contact` block containing the
current contact card's `contact_id` and `display_name`. A missing name is `null`.
The ID identifies the the workspace contact record, not a conversation or order. The
name is customer data and does not establish a verified identity.

Articles can contain facts, instructions, procedures, and response wording.
The saved agent instructions define how to use them and which rules take
precedence. The runtime does not impose a reference-only policy on articles.

To mark an article for a contact, use an explicit title such as:

```text
Контакт: Иван Петров — Согласованные условия
Контакт: Иван Петров | contact_id: 018f4d65-7b79-7a01-b305-f0b8f6634321 — Согласованные условия
```

Use the actual ID from that contact's record in place of the example UUID.
Configure matching in the saved agent instructions: an explicit `contact_id`
should take precedence over the name; otherwise match the full card name,
ignoring letter case and extra whitespace. Names can repeat across contacts,
so use the ID when the article must refer to one particular record.

The agent instructions should require reading matching articles before the
first reply, including a greeting, and following their applicable procedures
and response rules. They should also exclude articles targeting other contacts.

This is a model instruction for selecting relevant articles, not a server-side
access restriction: the complete catalog assigned to the agent remains
available. Standalone tasks and API invocations have no current contact.
Saved agent-test scenarios can supply an artificial `contact` card with a UUID
`contact_id` and an optional `display_name`. The runner uses the same contact
context builder for each customer message and scheduled continuation, without
looking up or creating contact records. Scenarios without a card retain their
previous behavior. To test ID matching independently, leave the name empty. To
test name matching independently, use the full name and a different artificial
ID that is absent from the article. Store artificial articles and their prepared
answers in the scenario's `knowledge_articles`, without publishing them to the
real knowledge base. The agent retrieves them using the normal article tool.
The expected response stays in the test article and assertions, not in the
customer message. An unrelated artificial article can serve as a negative
control through forbidden article-read and response assertions.
