ALTER TABLE ai_profiles ADD COLUMN http_allowed_hosts text[] NOT NULL DEFAULT '{}';
ALTER TABLE ai_profiles ADD CONSTRAINT ai_profiles_http_allowed_hosts_count CHECK (cardinality(http_allowed_hosts) <= 64);
