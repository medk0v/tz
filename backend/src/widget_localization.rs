//! Custom public widget languages and tenant-configurable translations.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::error::AppError;

const MAX_WIDGET_LANGUAGES: usize = 20;
const MAX_LANGUAGE_CODE_LENGTH: usize = 35;

fn default_offline_message() -> String {
    "Operators are offline. Leave a message and your contact details.".to_owned()
}

fn default_rating_prompt() -> String {
    "Thanks for chatting with us. Please rate the support you received.".to_owned()
}

fn default_rating_thanks() -> String {
    "Thank you! Your feedback helps us improve.".to_owned()
}

fn default_proactive_invitation_message() -> String {
    "Hi! Can I help you?".to_owned()
}

fn default_support_name() -> String {
    "Support".to_owned()
}

fn default_online_now() -> String {
    "Online now".to_owned()
}

fn default_offline_now() -> String {
    "Leave a message".to_owned()
}

fn default_message_placeholder() -> String {
    "Write a message…".to_owned()
}

fn default_contact_title() -> String {
    "Introduce yourself".to_owned()
}

fn default_contact_description() -> String {
    "Optional. Leave your name and email so we can contact you about this conversation.".to_owned()
}

fn default_contact_name() -> String {
    "Name".to_owned()
}

fn default_contact_name_placeholder() -> String {
    "Your name".to_owned()
}

fn default_contact_email() -> String {
    "Email".to_owned()
}

fn default_contact_email_placeholder() -> String {
    "you@example.com".to_owned()
}

fn default_contact_save() -> String {
    "Save details".to_owned()
}

fn default_contact_saving() -> String {
    "Saving…".to_owned()
}

fn default_contact_skip() -> String {
    "Continue anonymously".to_owned()
}

fn default_contact_error() -> String {
    "Could not save your contact details.".to_owned()
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WidgetLanguage(String);

impl WidgetLanguage {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parses and normalizes a user-defined widget language identifier.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` when the value is blank, oversized, or contains
    /// control characters.
    pub fn parse(value: &str) -> Result<Self, AppError> {
        let normalized = value.trim().replace('_', "-").to_lowercase();
        if normalized.is_empty()
            || normalized.chars().count() > MAX_LANGUAGE_CODE_LENGTH
            || normalized.chars().any(char::is_control)
        {
            return Err(AppError::BadRequest(format!(
                "language identifier must contain between 1 and {MAX_LANGUAGE_CODE_LENGTH} printable characters"
            )));
        }
        Ok(Self(normalized))
    }

    fn primary(&self) -> &str {
        self.0.split('-').next().unwrap_or(self.as_str())
    }
}

impl Default for WidgetLanguage {
    fn default() -> Self {
        Self("en".to_owned())
    }
}

impl Serialize for WidgetLanguage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WidgetLanguage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetTranslation {
    #[serde(default = "default_support_name")]
    pub support_name: String,
    pub greeting: Option<String>,
    #[serde(default = "default_offline_message")]
    pub offline_message: String,
    pub launcher_label: String,
    #[serde(default = "default_rating_prompt")]
    pub rating_prompt: String,
    #[serde(default = "default_rating_thanks")]
    pub rating_thanks: String,
    #[serde(default = "default_proactive_invitation_message")]
    pub proactive_invitation_message: String,
    #[serde(default = "default_online_now")]
    pub online_now: String,
    #[serde(default = "default_offline_now")]
    pub offline_now: String,
    #[serde(default = "default_message_placeholder")]
    pub message_placeholder: String,
    #[serde(default = "default_contact_title")]
    pub contact_title: String,
    #[serde(default = "default_contact_description")]
    pub contact_description: String,
    #[serde(default = "default_contact_name")]
    pub contact_name: String,
    #[serde(default = "default_contact_name_placeholder")]
    pub contact_name_placeholder: String,
    #[serde(default = "default_contact_email")]
    pub contact_email: String,
    #[serde(default = "default_contact_email_placeholder")]
    pub contact_email_placeholder: String,
    #[serde(default = "default_contact_save")]
    pub contact_save: String,
    #[serde(default = "default_contact_saving")]
    pub contact_saving: String,
    #[serde(default = "default_contact_skip")]
    pub contact_skip: String,
    #[serde(default = "default_contact_error")]
    pub contact_error: String,
}

impl Default for WidgetTranslation {
    fn default() -> Self {
        Self {
            support_name: default_support_name(),
            greeting: None,
            offline_message: default_offline_message(),
            launcher_label: "Chat with us".to_owned(),
            rating_prompt: default_rating_prompt(),
            rating_thanks: default_rating_thanks(),
            proactive_invitation_message: default_proactive_invitation_message(),
            online_now: default_online_now(),
            offline_now: default_offline_now(),
            message_placeholder: default_message_placeholder(),
            contact_title: default_contact_title(),
            contact_description: default_contact_description(),
            contact_name: default_contact_name(),
            contact_name_placeholder: default_contact_name_placeholder(),
            contact_email: default_contact_email(),
            contact_email_placeholder: default_contact_email_placeholder(),
            contact_save: default_contact_save(),
            contact_saving: default_contact_saving(),
            contact_skip: default_contact_skip(),
            contact_error: default_contact_error(),
        }
    }
}

impl WidgetTranslation {
    fn validate(mut self, language: &str) -> Result<Self, AppError> {
        self.support_name = self.support_name.trim().to_owned();
        if self.support_name.chars().count() > 80 {
            return Err(AppError::BadRequest(format!(
                "translations.{language}.support_name must not exceed 80 characters"
            )));
        }
        if self.support_name.is_empty() {
            self.support_name = default_support_name();
        }

        self.greeting = self.greeting.and_then(|value| {
            let value = value.trim().to_owned();
            (!value.is_empty()).then_some(value)
        });
        if self
            .greeting
            .as_ref()
            .is_some_and(|value| value.chars().count() > 500)
        {
            return Err(AppError::BadRequest(format!(
                "translations.{language}.greeting must not exceed 500 characters"
            )));
        }

        self.offline_message = self.offline_message.trim().to_owned();
        if !(1..=500).contains(&self.offline_message.chars().count()) {
            return Err(AppError::BadRequest(format!(
                "translations.{language}.offline_message must contain between 1 and 500 characters"
            )));
        }

        self.launcher_label = self.launcher_label.trim().to_owned();
        if !(1..=40).contains(&self.launcher_label.chars().count()) {
            return Err(AppError::BadRequest(format!(
                "translations.{language}.launcher_label must contain between 1 and 40 characters"
            )));
        }

        self.rating_prompt = self.rating_prompt.trim().to_owned();
        if !(1..=500).contains(&self.rating_prompt.chars().count()) {
            return Err(AppError::BadRequest(format!(
                "translations.{language}.rating_prompt must contain between 1 and 500 characters"
            )));
        }
        self.rating_thanks = self.rating_thanks.trim().to_owned();
        if !(1..=300).contains(&self.rating_thanks.chars().count()) {
            return Err(AppError::BadRequest(format!(
                "translations.{language}.rating_thanks must contain between 1 and 300 characters"
            )));
        }
        self.proactive_invitation_message = self.proactive_invitation_message.trim().to_owned();
        if !(1..=240).contains(&self.proactive_invitation_message.chars().count()) {
            return Err(AppError::BadRequest(format!(
                "translations.{language}.proactive_invitation_message must contain between 1 and 240 characters"
            )));
        }
        for (field, value, maximum) in [
            ("online_now", &mut self.online_now, 80),
            ("offline_now", &mut self.offline_now, 80),
            ("message_placeholder", &mut self.message_placeholder, 160),
            ("contact_title", &mut self.contact_title, 120),
            ("contact_description", &mut self.contact_description, 500),
            ("contact_name", &mut self.contact_name, 80),
            (
                "contact_name_placeholder",
                &mut self.contact_name_placeholder,
                160,
            ),
            ("contact_email", &mut self.contact_email, 80),
            (
                "contact_email_placeholder",
                &mut self.contact_email_placeholder,
                160,
            ),
            ("contact_save", &mut self.contact_save, 80),
            ("contact_saving", &mut self.contact_saving, 80),
            ("contact_skip", &mut self.contact_skip, 120),
            ("contact_error", &mut self.contact_error, 300),
        ] {
            *value = value.trim().to_owned();
            if !(1..=maximum).contains(&value.chars().count()) {
                return Err(AppError::BadRequest(format!(
                    "translations.{language}.{field} must contain between 1 and {maximum} characters"
                )));
            }
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct WidgetTranslations(BTreeMap<String, WidgetTranslation>);

impl Default for WidgetTranslations {
    fn default() -> Self {
        Self(BTreeMap::from([
            (
                "en".to_owned(),
                WidgetTranslation {
                    support_name: default_support_name(),
                    greeting: Some("How can we help? Send us a message.".to_owned()),
                    offline_message: default_offline_message(),
                    launcher_label: "Chat with us".to_owned(),
                    rating_prompt: default_rating_prompt(),
                    rating_thanks: default_rating_thanks(),
                    proactive_invitation_message: default_proactive_invitation_message(),
                    online_now: default_online_now(),
                    offline_now: default_offline_now(),
                    message_placeholder: default_message_placeholder(),
                    contact_title: default_contact_title(),
                    contact_description: default_contact_description(),
                    contact_name: default_contact_name(),
                    contact_name_placeholder: default_contact_name_placeholder(),
                    contact_email: default_contact_email(),
                    contact_email_placeholder: default_contact_email_placeholder(),
                    contact_save: default_contact_save(),
                    contact_saving: default_contact_saving(),
                    contact_skip: default_contact_skip(),
                    contact_error: default_contact_error(),
                },
            ),
            (
                "ru".to_owned(),
                WidgetTranslation {
                    support_name: default_support_name(),
                    greeting: Some("Чем мы можем помочь? Напишите нам.".to_owned()),
                    offline_message: "Операторы офлайн. Оставьте сообщение и контакты.".to_owned(),
                    launcher_label: "Напишите нам".to_owned(),
                    rating_prompt: "Спасибо за обращение! Оцените, пожалуйста, качество поддержки."
                        .to_owned(),
                    rating_thanks: "Спасибо! Ваш отзыв поможет нам стать лучше.".to_owned(),
                    proactive_invitation_message: "Здравствуйте! Могу помочь?".to_owned(),
                    online_now: "Сейчас в сети".to_owned(),
                    offline_now: "Оставьте сообщение".to_owned(),
                    message_placeholder: "Напишите сообщение…".to_owned(),
                    contact_title: "Представьтесь".to_owned(),
                    contact_description: "Необязательно. Оставьте имя и email, чтобы мы могли связаться с вами по этому диалогу.".to_owned(),
                    contact_name: "Имя".to_owned(),
                    contact_name_placeholder: "Ваше имя".to_owned(),
                    contact_email: "Email".to_owned(),
                    contact_email_placeholder: "you@example.com".to_owned(),
                    contact_save: "Сохранить".to_owned(),
                    contact_saving: "Сохраняем…".to_owned(),
                    contact_skip: "Продолжить анонимно".to_owned(),
                    contact_error: "Не удалось сохранить контактные данные.".to_owned(),
                },
            ),
        ]))
    }
}

impl WidgetTranslations {
    /// Normalizes language keys and validates all public translations.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` when the language list is empty or oversized, a
    /// identifier is blank or duplicated after normalization, or a public string
    /// exceeds its limit.
    pub fn validate(self) -> Result<Self, AppError> {
        if self.0.is_empty() || self.0.len() > MAX_WIDGET_LANGUAGES {
            return Err(AppError::BadRequest(format!(
                "translations must contain between 1 and {MAX_WIDGET_LANGUAGES} languages"
            )));
        }

        let mut normalized = BTreeMap::new();
        for (language, translation) in self.0 {
            let language = WidgetLanguage::parse(&language)?;
            let code = language.as_str().to_owned();
            if normalized
                .insert(code.clone(), translation.validate(&code)?)
                .is_some()
            {
                return Err(AppError::BadRequest(format!(
                    "translations contains the duplicate language {code}"
                )));
            }
        }
        Ok(Self(normalized))
    }

    pub fn contains(&self, language: &WidgetLanguage) -> bool {
        self.0.contains_key(language.as_str())
    }

    /// Chooses the closest configured language for a visitor request.
    ///
    /// # Errors
    ///
    /// Returns an internal error when persisted translations are empty or contain an invalid key.
    pub fn resolve_language(
        &self,
        requested: Option<&WidgetLanguage>,
        default: &WidgetLanguage,
    ) -> Result<WidgetLanguage, AppError> {
        if let Some(requested) = requested {
            if self.contains(requested) {
                return Ok(requested.clone());
            }
            if let Ok(primary) = WidgetLanguage::parse(requested.primary())
                && self.contains(&primary)
            {
                return Ok(primary);
            }
        }
        if self.contains(default) {
            return Ok(default.clone());
        }
        let english = WidgetLanguage::default();
        if self.contains(&english) {
            return Ok(english);
        }
        let first =
            self.0.keys().next().ok_or_else(|| {
                AppError::internal(anyhow::anyhow!("widget translations are empty"))
            })?;
        WidgetLanguage::parse(first).map_err(|_| {
            AppError::internal(anyhow::anyhow!("invalid stored widget language identifier"))
        })
    }

    /// Returns the translation configured for a resolved language.
    ///
    /// # Errors
    ///
    /// Returns an internal error when the resolved language is missing from persisted translations.
    pub fn get(&self, language: &WidgetLanguage) -> Result<&WidgetTranslation, AppError> {
        self.0
            .get(language.as_str())
            .ok_or_else(|| AppError::internal(anyhow::anyhow!("widget translation is missing")))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{WidgetLanguage, WidgetTranslation, WidgetTranslations};

    #[test]
    fn validates_custom_languages_and_resolves_regional_tags() {
        let translations = WidgetTranslations(BTreeMap::from([
            (
                "EN".to_owned(),
                WidgetTranslation {
                    support_name: " Customer care ".to_owned(),
                    greeting: Some(" Welcome ".to_owned()),
                    offline_message: " Leave a message and your contacts ".to_owned(),
                    launcher_label: " Chat ".to_owned(),
                    rating_prompt: " Rate us ".to_owned(),
                    rating_thanks: " Thank you ".to_owned(),
                    proactive_invitation_message: " Need help? ".to_owned(),
                    ..WidgetTranslation::default()
                },
            ),
            (
                "zh".to_owned(),
                WidgetTranslation {
                    support_name: String::new(),
                    greeting: Some("您好".to_owned()),
                    offline_message: "请留言并留下您的联系方式".to_owned(),
                    launcher_label: "联系我们".to_owned(),
                    rating_prompt: "请为本次服务评分".to_owned(),
                    rating_thanks: "感谢您的反馈".to_owned(),
                    proactive_invitation_message: "需要帮助吗？".to_owned(),
                    ..WidgetTranslation::default()
                },
            ),
        ]))
        .validate()
        .unwrap();

        assert_eq!(
            translations
                .get(&WidgetLanguage::default())
                .unwrap()
                .support_name,
            "Customer care"
        );
        assert_eq!(
            translations
                .get(&WidgetLanguage::parse("zh").unwrap())
                .unwrap()
                .support_name,
            "Support"
        );
        assert_eq!(
            translations
                .get(&WidgetLanguage::default())
                .unwrap()
                .launcher_label,
            "Chat"
        );
        assert_eq!(
            translations
                .get(&WidgetLanguage::default())
                .unwrap()
                .proactive_invitation_message,
            "Need help?"
        );
        assert_eq!(
            translations
                .get(&WidgetLanguage::default())
                .unwrap()
                .offline_message,
            "Leave a message and your contacts"
        );
        assert_eq!(
            translations
                .resolve_language(
                    Some(&WidgetLanguage::parse("zh-CN").unwrap()),
                    &WidgetLanguage::default(),
                )
                .unwrap()
                .as_str(),
            "zh"
        );

        let mut invalid_invitation = translations.0.get("en").unwrap().clone();
        invalid_invitation.proactive_invitation_message = "   ".to_owned();
        assert!(
            WidgetTranslations(BTreeMap::from([("en".to_owned(), invalid_invitation)]))
                .validate()
                .is_err()
        );

        let mut invalid_offline_message = translations.0.get("en").unwrap().clone();
        invalid_offline_message.offline_message = "   ".to_owned();
        assert!(
            WidgetTranslations(BTreeMap::from([
                ("en".to_owned(), invalid_offline_message,)
            ]))
            .validate()
            .is_err()
        );
        assert_eq!(
            WidgetLanguage::parse("  Client's Language  ")
                .unwrap()
                .as_str(),
            "client's language"
        );
        assert_eq!(
            WidgetLanguage::parse("РУССКИЙ").unwrap().as_str(),
            "русский"
        );
        assert!(WidgetLanguage::parse("   ").is_err());
        assert!(WidgetLanguage::parse("language\nname").is_err());
    }

    #[test]
    fn defaults_new_interface_text_for_legacy_translation_json() {
        let translation: WidgetTranslation = serde_json::from_value(serde_json::json!({
            "support_name": "Support",
            "greeting": "Welcome",
            "offline_message": "Leave a message",
            "launcher_label": "Chat",
            "rating_prompt": "Rate us",
            "rating_thanks": "Thank you",
            "proactive_invitation_message": "Need help?"
        }))
        .unwrap();

        assert_eq!(translation.online_now, "Online now");
        assert_eq!(translation.offline_now, "Leave a message");
        assert_eq!(translation.message_placeholder, "Write a message…");
        assert_eq!(translation.contact_title, "Introduce yourself");
        assert_eq!(translation.contact_save, "Save details");
    }
}
