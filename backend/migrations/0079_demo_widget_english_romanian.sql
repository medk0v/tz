-- Configure both languages on the isolated demo widget. Existing translations
-- remain available because active widget sessions retain their resolved language.
UPDATE widget_configs AS config
SET default_language = 'en',
    greeting = 'Hi! Send a message to try the Tzomet widget.',
    launcher = jsonb_set(config.launcher, '{label}', to_jsonb('Chat with us'::text)),
    translations = config.translations || $demo_translations$
    {
        "en": {
            "support_name": "Tzomet Support",
            "greeting": "Hi! Send a message to try the Tzomet widget.",
            "offline_message": "Leave a message. You can reply to it from the demo account.",
            "launcher_label": "Chat with us",
            "rating_prompt": "Thanks for chatting with us. Please rate the support you received.",
            "rating_thanks": "Thank you! Your feedback helps us improve.",
            "proactive_invitation_message": "Hi! Would you like to try the Tzomet widget?",
            "online_now": "Online now",
            "offline_now": "Leave a message",
            "message_placeholder": "Write a message…",
            "contact_title": "Introduce yourself",
            "contact_description": "Optional. Leave your name and email so we can contact you about this conversation.",
            "contact_name": "Name",
            "contact_name_placeholder": "Your name",
            "contact_email": "Email",
            "contact_email_placeholder": "you@example.com",
            "contact_save": "Save details",
            "contact_saving": "Saving…",
            "contact_skip": "Continue anonymously",
            "contact_error": "Could not save your contact details."
        },
        "ro": {
            "support_name": "Asistență Tzomet",
            "greeting": "Salut! Trimite un mesaj pentru a încerca widgetul Tzomet.",
            "offline_message": "Lasă un mesaj. Poți răspunde din contul demo.",
            "launcher_label": "Scrie-ne",
            "rating_prompt": "Îți mulțumim pentru conversație. Cum apreciezi asistența primită?",
            "rating_thanks": "Îți mulțumim! Părerea ta ne ajută să ne îmbunătățim.",
            "proactive_invitation_message": "Salut! Vrei să încerci widgetul Tzomet?",
            "online_now": "Online acum",
            "offline_now": "Lasă un mesaj",
            "message_placeholder": "Scrie un mesaj…",
            "contact_title": "Prezintă-te",
            "contact_description": "Opțional. Lasă-ne numele și adresa de email ca să te putem contacta în legătură cu această conversație.",
            "contact_name": "Nume",
            "contact_name_placeholder": "Numele tău",
            "contact_email": "Email",
            "contact_email_placeholder": "nume@exemplu.ro",
            "contact_save": "Salvează datele",
            "contact_saving": "Se salvează…",
            "contact_skip": "Continuă anonim",
            "contact_error": "Nu am putut salva datele de contact."
        }
    }
    $demo_translations$::jsonb,
    updated_at = now()
WHERE config.channel_connection_id = '9d299e1f-b99f-41da-9330-863d94385058'
  AND config.tenant_id = 'c11ebede-f63c-4315-ab9f-b7c42e5515b6';
