UPDATE messages
SET body_format = 'markdown'
WHERE direction = 'outbound'
  AND kind = 'text'
  AND author_kind = 'ai'
  AND body_format = 'plain';
