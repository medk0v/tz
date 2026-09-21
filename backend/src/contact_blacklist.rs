//! Channel-local replies for contacts blocked because of spam.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

const DEFAULT_REPLY: &str = "You have been blocked because of spam. Please contact us at the email address listed on our website.";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlacklistReply {
    pub default_language: String,
    pub translations: BTreeMap<String, String>,
}

impl BlacklistReply {
    pub(crate) fn validate(self) -> Result<Self, AppError> {
        if self.translations.is_empty() || self.translations.len() > 32 {
            return Err(AppError::BadRequest(
                "blacklist translations must contain between 1 and 32 languages".to_owned(),
            ));
        }
        let default_language = normalize_language(&self.default_language)?;
        let mut translations = BTreeMap::new();
        for (language, body) in self.translations {
            let language = normalize_language(&language)?;
            let body = body.trim();
            if body.is_empty() || body.chars().count() > 4000 {
                return Err(AppError::BadRequest(
                    "blacklist reply text must contain between 1 and 4000 characters".to_owned(),
                ));
            }
            if translations.insert(language, body.to_owned()).is_some() {
                return Err(AppError::BadRequest(
                    "blacklist translations contain duplicate languages".to_owned(),
                ));
            }
        }
        if !translations.contains_key(&default_language) {
            return Err(AppError::BadRequest(
                "default_language must exist in blacklist translations".to_owned(),
            ));
        }
        Ok(Self {
            default_language,
            translations,
        })
    }

    /// Uses a matching regional translation, then its base language, then the channel default.
    pub(crate) fn resolve(&self, language: Option<&str>) -> &str {
        let language = language.and_then(|value| normalize_language(value).ok());
        language
            .as_deref()
            .and_then(|language| {
                self.translations.get(language).or_else(|| {
                    language
                        .split_once('-')
                        .and_then(|(base, _)| self.translations.get(base))
                })
            })
            .or_else(|| self.translations.get(&self.default_language))
            .map_or(DEFAULT_REPLY, String::as_str)
    }
}

impl Default for BlacklistReply {
    fn default() -> Self {
        Self {
            default_language: "en".to_owned(),
            translations: [
                ("en", DEFAULT_REPLY),
                ("ru", "Вы заблокированы из-за спама. Свяжитесь с нами по электронной почте, указанной на сайте."),
                ("uk", "Вас заблоковано через спам. Зв’яжіться з нами за адресою електронної пошти, вказаною на сайті."),
                ("ro", "Ați fost blocat din cauza spamului. Contactați-ne la adresa de e-mail indicată pe site."),
                ("zh", "您因发送垃圾信息已被封禁。请通过网站上列出的电子邮箱联系我们。"),
                ("hi", "स्पैम के कारण आपको ब्लॉक कर दिया गया है। कृपया वेबसाइट पर दिए गए ईमेल पते पर हमसे संपर्क करें।"),
            ]
            .into_iter()
            .map(|(language, text)| (language.to_owned(), text.to_owned()))
            .collect(),
        }
    }
}

fn normalize_language(language: &str) -> Result<String, AppError> {
    let language = language.trim().replace('_', "-").to_ascii_lowercase();
    let mut segments = language.split('-');
    let base = segments.next().unwrap_or_default();
    if language.len() > 35
        || !(2..=8).contains(&base.len())
        || !base.bytes().all(|byte| byte.is_ascii_alphabetic())
        || !segments.all(|segment| {
            (1..=8).contains(&segment.len())
                && segment.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        return Err(AppError::BadRequest(
            "blacklist language must be a valid language tag".to_owned(),
        ));
    }
    Ok(language)
}

#[cfg(test)]
mod tests {
    use super::BlacklistReply;

    #[test]
    fn selects_regional_base_and_default_languages() {
        let mut reply = BlacklistReply {
            default_language: "ro".to_owned(),
            ..BlacklistReply::default()
        };
        reply
            .translations
            .insert("ru-ru".to_owned(), "Regional".to_owned());
        assert_eq!(reply.resolve(Some(" RU_ru ")), "Regional");
        assert_eq!(reply.resolve(Some("ru-KZ")), reply.translations["ru"]);
        assert_eq!(reply.resolve(Some("zh-Hans-CN")), reply.translations["zh"]);
        assert_eq!(reply.resolve(Some("de-DE")), reply.translations["ro"]);
        assert_eq!(reply.resolve(None), reply.translations["ro"]);
    }

    #[test]
    fn normalizes_languages_and_trims_reply_text() {
        let reply = BlacklistReply {
            default_language: " PT_br ".to_owned(),
            translations: [("PT-BR".to_owned(), "  Mensagem\n ".to_owned())].into(),
        }
        .validate()
        .unwrap();
        assert_eq!(reply.default_language, "pt-br");
        assert_eq!(reply.resolve(Some("pt_br")), "Mensagem");
    }

    #[test]
    fn rejects_empty_missing_duplicate_and_oversized_translations() {
        for translations in [
            [].into(),
            [("en".to_owned(), " ".to_owned())].into(),
            [("ru".to_owned(), "Текст".to_owned())].into(),
            [("en".to_owned(), "x".repeat(4001))].into(),
            [
                ("en".to_owned(), "Text".to_owned()),
                ("EN".to_owned(), "Text".to_owned()),
            ]
            .into(),
            [("en-".to_owned(), "Text".to_owned())].into(),
        ] {
            assert!(
                BlacklistReply {
                    default_language: "en".to_owned(),
                    translations
                }
                .validate()
                .is_err()
            );
        }
        assert!(BlacklistReply::default().validate().is_ok());
    }
}
