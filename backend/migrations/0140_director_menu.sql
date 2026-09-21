ALTER TABLE projects ADD COLUMN director_menu jsonb NOT NULL DEFAULT '{"sidebar_items":["director","conversations","contacts","tasks","notes","processes","online_visitors","support_quality","team","ai","inbox_routing","channels","knowledge_base","reply_templates","integrations"],"default_page":"director","show_default_channels":true}'::jsonb
    CHECK (jsonb_typeof(director_menu) = 'object');
