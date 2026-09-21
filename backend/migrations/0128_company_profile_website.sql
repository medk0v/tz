ALTER TABLE projects
    DROP CONSTRAINT projects_company_profile_check;

UPDATE projects
SET company_profile = company_profile || jsonb_build_object('website', '')
WHERE project_kind = 'company'
  AND company_profile IS NOT NULL
  AND NOT (company_profile ? 'website');

ALTER TABLE projects
    ADD CONSTRAINT projects_company_profile_check CHECK (
        (project_kind = 'project' AND company_profile IS NULL)
        OR (
            project_kind = 'company'
            AND company_profile IS NOT NULL
            AND jsonb_typeof(company_profile) = 'object'
            AND (company_profile ?& ARRAY[
                'description', 'industry', 'country', 'website', 'employee_count', 'products_services', 'goals'
            ])
            AND (jsonb_typeof(company_profile -> 'description') = 'string') IS TRUE
            AND char_length(company_profile ->> 'description') <= 10000
            AND (jsonb_typeof(company_profile -> 'industry') = 'string') IS TRUE
            AND char_length(company_profile ->> 'industry') <= 200
            AND (jsonb_typeof(company_profile -> 'country') = 'string') IS TRUE
            AND char_length(company_profile ->> 'country') <= 200
            AND (jsonb_typeof(company_profile -> 'website') = 'string') IS TRUE
            AND char_length(company_profile ->> 'website') <= 200
            AND (jsonb_typeof(company_profile -> 'products_services') = 'string') IS TRUE
            AND char_length(company_profile ->> 'products_services') <= 10000
            AND (jsonb_typeof(company_profile -> 'goals') = 'string') IS TRUE
            AND char_length(company_profile ->> 'goals') <= 10000
            AND CASE jsonb_typeof(company_profile -> 'employee_count')
                WHEN 'null' THEN true
                WHEN 'number' THEN
                    (company_profile ->> 'employee_count')::numeric BETWEEN 0 AND 10000000
                    AND trunc((company_profile ->> 'employee_count')::numeric)
                        = (company_profile ->> 'employee_count')::numeric
                ELSE false
            END
        )
    );
