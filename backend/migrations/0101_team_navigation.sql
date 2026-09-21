-- Extend the existing configurable department menus without assuming generated
-- CHECK constraint names from older PostgreSQL versions.
DO $$
DECLARE existing_constraint record;
BEGIN
    FOR existing_constraint IN
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'departments'::regclass AND contype = 'c'
          AND (pg_get_constraintdef(oid) LIKE '%cardinality(sidebar_items)%'
               OR pg_get_constraintdef(oid) LIKE '%sidebar_items <@%')
    LOOP
        EXECUTE format('ALTER TABLE departments DROP CONSTRAINT %I', existing_constraint.conname);
    END LOOP;
END $$;

ALTER TABLE departments
    ADD CONSTRAINT departments_sidebar_size_check CHECK (cardinality(sidebar_items) BETWEEN 1 AND 12),
    ADD CONSTRAINT departments_sidebar_modules_check CHECK (sidebar_items <@ ARRAY[
        'conversations', 'contacts', 'online_visitors', 'support_quality',
        'inbox_routing', 'channels', 'team', 'ai', 'tasks', 'knowledge_base',
        'reply_templates', 'integrations'
    ]::text[]),
    ALTER COLUMN sidebar_items SET DEFAULT ARRAY[
        'conversations', 'contacts', 'online_visitors', 'support_quality',
        'inbox_routing', 'channels', 'team', 'ai', 'tasks', 'knowledge_base',
        'reply_templates', 'integrations'
    ];

-- Put Team immediately before AI, or at the end when AI is hidden. Preserve
-- existing ordering and the selected landing page.
UPDATE departments
SET sidebar_items =
        sidebar_items[1:COALESCE(array_position(sidebar_items, 'ai') - 1, cardinality(sidebar_items))]
        || ARRAY['team']
        || sidebar_items[COALESCE(array_position(sidebar_items, 'ai'), cardinality(sidebar_items) + 1):cardinality(sidebar_items)],
    updated_at = now()
WHERE NOT ('team' = ANY(sidebar_items));
