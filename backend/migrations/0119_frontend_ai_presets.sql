-- Preset agents are now frontend templates. A profile is stored only after a
-- user saves it, keeping preset_key to show its origin.
DROP TRIGGER IF EXISTS projects_seed_ai_profile_presets ON projects;
DROP FUNCTION IF EXISTS seed_new_project_ai_profile_presets();
DROP FUNCTION IF EXISTS seed_ai_profile_presets(uuid, uuid);

-- Remove seeded presets whose content still matches a template from any earlier
-- seed version (0081-0094). Timestamps are not a reliable signal: saving without
-- changes or assigning a skill bumps updated_at. Presets with edited name,
-- description or instructions, a model, a non-draft status, or a reference that
-- blocks deletion (tasks, company positions, ...) stay among the project's agents.
DO $$
DECLARE
    candidate uuid;
BEGIN
    FOR candidate IN
        SELECT profile.id
        FROM ai_profiles profile
        JOIN (VALUES
        ('sales', 'Агент продаж', 'Выявляет потребность, квалифицирует лид и помогает сделать следующий шаг.', '648a85851026b78bda62de6c6853237e'),
        ('operations', 'Операционный агент', 'Собирает данные, ставит задачи и синхронизирует работу команды.', 'ecc8302633473c88937d79965faf2a76'),
        ('quality', 'Агент контроля качества', 'Проверяет диалоги, замечает отклонения и показывает точки роста.', 'b74be4fd318420d35c80a27272ffa7ab'),
        ('hr', 'HR-агент', 'Разбирает отклики, проводит первичный отбор и готовит кандидатов к следующему этапу.', '6abf7d885cee017f1aaace626792de1f'),
        ('universal', 'Универсальный агент', 'Настраивается под ваш процесс: роль, знания, инструменты и правила работы.', '58e86ff6413b5a2aeb81a005d5d95cdd'),
        ('coordinator', 'Агент-координатор', 'Распределяет работу между агентами, проверяет результаты и собирает общий ответ.', 'a76dfb46f3efbffcf0d9a0b047011ba6'),
        ('support', 'Агент службы поддержки', 'Отвечает на вопросы клиентов, помогает решить проблему и передаёт сложные обращения оператору.', 'bc6b483a22eb89a5325378854b167ba7'),
        ('finance', 'Агент-финансист', 'Анализирует доходы и расходы, помогает планировать бюджет и оценивать финансовые риски.', '89a97a1f6690f23340d8bc50770c176a'),
        ('legal', 'Агент-юрист', 'Разбирает документы и договоры, отмечает правовые риски и помогает подготовить вопросы юристу.', 'e4ebc26c711ba873b5fbc68731378c32'),
        ('marketing', 'Агент-маркетолог', 'Изучает аудиторию, готовит кампании и контент, помогает оценивать результаты продвижения.', '158b6ba30596c8de3b1abe3a8ebb3130'),
        ('programmer', 'Агент-программист', 'Пишет и объясняет код, исправляет ошибки и помогает проверять технические решения.', '282ddf5fff752fa3bdaa73c6bc1b85f8')
        ) AS template(preset_key, name, description, instructions_md5)
          ON template.preset_key = profile.preset_key
         AND (profile.name = template.name OR profile.name LIKE template.name || ' (%)')
         AND profile.description = template.description
         AND md5(profile.instructions) = template.instructions_md5
        WHERE profile.created_by IS NULL
          AND profile.status = 'draft'
          AND profile.provider_connection_id IS NULL
          AND profile.model IS NULL
          AND profile.tool_instructions = ''
    LOOP
        BEGIN
            DELETE FROM ai_profiles WHERE id = candidate;
        EXCEPTION WHEN foreign_key_violation THEN
            NULL;
        END;
    END LOOP;
END;
$$;
