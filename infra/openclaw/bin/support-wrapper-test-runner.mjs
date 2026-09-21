#!/usr/bin/env node

const [wrapper, grantDirectory, actionDirectory, ...arguments_] = process.argv.slice(2);
const directories = { grantDirectory, actionDirectory };

switch (wrapper) {
  case "resolve": {
    const { runResolveConversation } = await import("./support-resolve-conversation.mjs");
    runResolveConversation(arguments_, directories);
    break;
  }
  case "reminder": {
    const { runScheduleReminder } = await import("./support-schedule-reminder.mjs");
    runScheduleReminder(arguments_, directories);
    break;
  }
  case "knowledge": {
    const { runKnowledgeArticle } = await import("./support-knowledge-article.mjs");
    await runKnowledgeArticle(arguments_, directories);
    break;
  }
  default:
    process.stderr.write("unknown test wrapper\n");
    process.exit(2);
}
