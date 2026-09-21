CREATE INDEX messages_text_body_search_idx
ON messages USING GIN (to_tsvector('simple'::regconfig, body))
WHERE kind = 'text' AND body <> '';
