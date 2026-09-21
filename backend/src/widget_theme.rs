//! Public, tenant-configurable widget presentation settings.

use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetComponentColors {
    pub background_color: String,
    pub control_background_color: String,
    pub control_text_color: String,
    pub muted_text_color: String,
    pub divider_color: String,
    pub online_color: String,
    pub offline_color: String,
    pub danger_color: String,
    pub rating_color: String,
    pub shadow_color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_text_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_background_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_icon_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_background_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_text_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_placeholder_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_text_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_icon_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_icon_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_background_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_icon_color: Option<String>,
}

impl WidgetComponentColors {
    fn validate(&self, path: &str) -> Result<(), AppError> {
        for (name, value) in [
            ("background_color", &self.background_color),
            ("control_background_color", &self.control_background_color),
            ("control_text_color", &self.control_text_color),
            ("muted_text_color", &self.muted_text_color),
            ("divider_color", &self.divider_color),
            ("online_color", &self.online_color),
            ("offline_color", &self.offline_color),
            ("danger_color", &self.danger_color),
            ("rating_color", &self.rating_color),
            ("shadow_color", &self.shadow_color),
        ] {
            if !is_hex_color(value) {
                return Err(AppError::BadRequest(format!(
                    "{path}.{name} must be a #RRGGBB color"
                )));
            }
        }
        for (name, value) in [
            ("status_text_color", &self.status_text_color),
            ("launcher_background_color", &self.launcher_background_color),
            ("launcher_icon_color", &self.launcher_icon_color),
            ("input_background_color", &self.input_background_color),
            ("input_text_color", &self.input_text_color),
            ("input_placeholder_color", &self.input_placeholder_color),
            ("footer_text_color", &self.footer_text_color),
            ("sound_icon_color", &self.sound_icon_color),
            ("close_icon_color", &self.close_icon_color),
            ("send_background_color", &self.send_background_color),
            ("send_icon_color", &self.send_icon_color),
        ] {
            if value.as_ref().is_some_and(|color| !is_hex_color(color)) {
                return Err(AppError::BadRequest(format!(
                    "{path}.{name} must be a #RRGGBB color"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WidgetDarkTheme {
    pub accent_color: String,
    pub accent_text_color: String,
    pub surface_color: String,
    pub text_color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_colors: Option<WidgetComponentColors>,
}

impl Default for WidgetDarkTheme {
    fn default() -> Self {
        Self {
            accent_color: "#F05A28".to_owned(),
            accent_text_color: "#FFFFFF".to_owned(),
            surface_color: "#1C1E21".to_owned(),
            text_color: "#F2F3F4".to_owned(),
            component_colors: None,
        }
    }
}

impl WidgetDarkTheme {
    fn validate(&self) -> Result<(), AppError> {
        validate_colors([
            ("theme.dark.accent_color", &self.accent_color),
            ("theme.dark.accent_text_color", &self.accent_text_color),
            ("theme.dark.surface_color", &self.surface_color),
            ("theme.dark.text_color", &self.text_color),
        ])?;
        if let Some(colors) = &self.component_colors {
            colors.validate("theme.dark.component_colors")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WidgetBorder {
    pub enabled: bool,
    pub width: u8,
    pub light_color: String,
    pub dark_color: String,
}

impl Default for WidgetBorder {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 1,
            light_color: "#E3E4E6".to_owned(),
            dark_color: "#3A3D42".to_owned(),
        }
    }
}

impl WidgetBorder {
    fn validate(&self) -> Result<(), AppError> {
        if !matches!(self.width, 1 | 2) {
            return Err(AppError::BadRequest(
                "theme.border.width must be 1 or 2".to_owned(),
            ));
        }
        validate_colors([
            ("theme.border.light_color", &self.light_color),
            ("theme.border.dark_color", &self.dark_color),
        ])
    }
}

/// Built-in font stacks available to the widget without external font downloads.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetFontFamily {
    #[default]
    System,
    Arial,
    Verdana,
    Tahoma,
    TrebuchetMs,
    Georgia,
    TimesNewRoman,
    CourierNew,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WidgetTheme {
    pub accent_color: String,
    pub accent_text_color: String,
    pub surface_color: String,
    pub text_color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_colors: Option<WidgetComponentColors>,
    pub dark: WidgetDarkTheme,
    pub border: WidgetBorder,
    pub border_radius: u8,
    pub font_family: WidgetFontFamily,
    pub use_site_font: bool,
    pub reply_typing_effect: bool,
    pub opening_animation: bool,
    pub launcher_icon: WidgetLauncherIcon,
    pub send_icon: WidgetSendIcon,
    pub footer_text: Option<String>,
}

impl Default for WidgetTheme {
    fn default() -> Self {
        Self {
            accent_color: "#F05A28".to_owned(),
            accent_text_color: "#FFFFFF".to_owned(),
            surface_color: "#FFFFFF".to_owned(),
            text_color: "#1B1B1D".to_owned(),
            component_colors: None,
            dark: WidgetDarkTheme::default(),
            border: WidgetBorder::default(),
            border_radius: 20,
            font_family: WidgetFontFamily::default(),
            use_site_font: false,
            reply_typing_effect: false,
            opening_animation: true,
            launcher_icon: WidgetLauncherIcon::default(),
            send_icon: WidgetSendIcon::default(),
            footer_text: None,
        }
    }
}

impl WidgetTheme {
    /// Validates the public widget colors and corner radius.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` when a color is not exact six-digit hex or the radius exceeds 32 px.
    pub fn validate(self) -> Result<Self, AppError> {
        validate_colors([
            ("theme.accent_color", &self.accent_color),
            ("theme.accent_text_color", &self.accent_text_color),
            ("theme.surface_color", &self.surface_color),
            ("theme.text_color", &self.text_color),
        ])?;
        if let Some(colors) = &self.component_colors {
            colors.validate("theme.component_colors")?;
        }
        self.dark.validate()?;
        self.border.validate()?;
        if self.border_radius > 32 {
            return Err(AppError::BadRequest(
                "theme.border_radius must be between 0 and 32".to_owned(),
            ));
        }
        if self
            .footer_text
            .as_ref()
            .is_some_and(|value| value.chars().count() > 160)
        {
            return Err(AppError::BadRequest(
                "theme.footer_text must not exceed 160 characters".to_owned(),
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetLauncherIcon {
    #[default]
    Chat,
    MessageCircle,
    Headphones,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetSendIcon {
    #[default]
    Send,
    ArrowUp,
    ArrowRight,
}

fn validate_colors<'a>(
    colors: impl IntoIterator<Item = (&'a str, &'a String)>,
) -> Result<(), AppError> {
    for (name, value) in colors {
        if !is_hex_color(value) {
            return Err(AppError::BadRequest(format!(
                "{name} must be a #RRGGBB color"
            )));
        }
    }
    Ok(())
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
    use super::{WidgetFontFamily, WidgetTheme};

    #[test]
    fn validates_theme_colors_and_border_radius() {
        assert!(WidgetTheme::default().validate().is_ok());
        assert!(
            WidgetTheme {
                accent_color: "not-a-color".to_owned(),
                ..WidgetTheme::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            WidgetTheme {
                border_radius: 33,
                ..WidgetTheme::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            WidgetTheme {
                dark: super::WidgetDarkTheme {
                    surface_color: "transparent".to_owned(),
                    ..super::WidgetDarkTheme::default()
                },
                ..WidgetTheme::default()
            }
            .validate()
            .is_err()
        );
        for width in [0, 3] {
            assert!(
                WidgetTheme {
                    border: super::WidgetBorder {
                        width,
                        ..super::WidgetBorder::default()
                    },
                    ..WidgetTheme::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            WidgetTheme {
                border: super::WidgetBorder {
                    dark_color: "currentColor".to_owned(),
                    ..super::WidgetBorder::default()
                },
                ..WidgetTheme::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn defaults_border_radius_for_existing_saved_themes() {
        let theme: WidgetTheme = serde_json::from_value(serde_json::json!({
            "accent_color": "#F05A28",
            "accent_text_color": "#FFFFFF",
            "surface_color": "#FFFFFF",
            "text_color": "#1B1B1D"
        }))
        .unwrap();

        assert_eq!(theme.border_radius, 20);
        assert_eq!(theme.font_family, WidgetFontFamily::System);
        assert!(!theme.use_site_font);
        assert!(!theme.reply_typing_effect);
        assert!(theme.opening_animation);
        assert_eq!(theme.launcher_icon, super::WidgetLauncherIcon::Chat);
        assert_eq!(theme.send_icon, super::WidgetSendIcon::Send);
        assert_eq!(theme.footer_text, None);
        assert_eq!(theme.dark.surface_color, "#1C1E21");
        assert_eq!(theme.dark.text_color, "#F2F3F4");
        assert!(theme.border.enabled);
        assert_eq!(theme.border.width, 1);
        assert_eq!(theme.border.light_color, "#E3E4E6");
        assert_eq!(theme.border.dark_color, "#3A3D42");
        assert!(theme.component_colors.is_none());
        assert!(theme.dark.component_colors.is_none());
    }

    #[test]
    fn round_trips_supported_font_settings() {
        for font_family in [
            "system",
            "arial",
            "verdana",
            "tahoma",
            "trebuchet_ms",
            "georgia",
            "times_new_roman",
            "courier_new",
        ] {
            let theme: WidgetTheme = serde_json::from_value(serde_json::json!({
                "font_family": font_family,
                "use_site_font": true
            }))
            .unwrap();
            let serialized = serde_json::to_value(theme.validate().unwrap()).unwrap();

            assert_eq!(serialized["font_family"], font_family);
            assert_eq!(serialized["use_site_font"], true);
        }
    }

    #[test]
    fn rejects_unsupported_font_settings() {
        for font_family in [
            "",
            "unknown",
            "Arial, sans-serif",
            "url(https://example.com/font)",
        ] {
            assert!(
                serde_json::from_value::<WidgetTheme>(serde_json::json!({
                    "font_family": font_family
                }))
                .is_err()
            );
        }
        assert!(
            serde_json::from_value::<WidgetTheme>(serde_json::json!({
                "use_site_font": "true"
            }))
            .is_err()
        );
    }

    #[test]
    fn round_trips_reply_typing_effect() {
        assert!(!WidgetTheme::default().reply_typing_effect);

        for enabled in [false, true] {
            let theme: WidgetTheme = serde_json::from_value(serde_json::json!({
                "reply_typing_effect": enabled
            }))
            .unwrap();
            let serialized = serde_json::to_value(theme.validate().unwrap()).unwrap();

            assert_eq!(serialized["reply_typing_effect"], enabled);
        }
    }

    #[test]
    fn rejects_non_boolean_reply_typing_effect() {
        for value in [
            serde_json::json!("true"),
            serde_json::json!(1),
            serde_json::Value::Null,
        ] {
            assert!(
                serde_json::from_value::<WidgetTheme>(serde_json::json!({
                    "reply_typing_effect": value
                }))
                .is_err()
            );
        }
    }

    #[test]
    fn round_trips_opening_animation() {
        assert!(WidgetTheme::default().opening_animation);

        for enabled in [false, true] {
            let theme: WidgetTheme = serde_json::from_value(serde_json::json!({
                "opening_animation": enabled
            }))
            .unwrap();
            let serialized = serde_json::to_value(theme.validate().unwrap()).unwrap();

            assert_eq!(serialized["opening_animation"], enabled);
        }
    }

    #[test]
    fn rejects_non_boolean_opening_animation() {
        for value in [
            serde_json::json!("true"),
            serde_json::json!(1),
            serde_json::Value::Null,
        ] {
            assert!(
                serde_json::from_value::<WidgetTheme>(serde_json::json!({
                    "opening_animation": value
                }))
                .is_err()
            );
        }
    }

    #[test]
    fn validates_complete_component_color_palettes() {
        let component_colors = serde_json::json!({
            "background_color": "#FAFAFA",
            "control_background_color": "#FFFFFF",
            "control_text_color": "#1B1B1D",
            "muted_text_color": "#737375",
            "divider_color": "#E4E4E4",
            "online_color": "#27A857",
            "offline_color": "#8A8F98",
            "danger_color": "#A93027",
            "rating_color": "#F3B72F",
            "shadow_color": "#000000"
        });
        let valid: WidgetTheme = serde_json::from_value(serde_json::json!({
            "accent_color": "#F05A28",
            "accent_text_color": "#FFFFFF",
            "surface_color": "#FFFFFF",
            "text_color": "#1B1B1D",
            "component_colors": component_colors
        }))
        .unwrap();
        assert!(valid.validate().is_ok());

        let invalid: WidgetTheme = serde_json::from_value(serde_json::json!({
            "accent_color": "#F05A28",
            "accent_text_color": "#FFFFFF",
            "surface_color": "#FFFFFF",
            "text_color": "#1B1B1D",
            "component_colors": {
                "background_color": "#FAFAFA",
                "control_background_color": "#FFFFFF",
                "control_text_color": "#1B1B1D",
                "muted_text_color": "#737375",
                "divider_color": "transparent",
                "online_color": "#27A857",
                "offline_color": "#8A8F98",
                "danger_color": "#A93027",
                "rating_color": "#F3B72F",
                "shadow_color": "#000000"
            }
        }))
        .unwrap();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn validates_independent_colors_in_both_palettes() {
        let mut colors = serde_json::json!({
            "background_color": "#FAFAFA",
            "control_background_color": "#FFFFFF",
            "control_text_color": "#1B1B1D",
            "muted_text_color": "#737375",
            "divider_color": "#E4E4E4",
            "online_color": "#27A857",
            "offline_color": "#8A8F98",
            "danger_color": "#A93027",
            "rating_color": "#F3B72F",
            "shadow_color": "#000000"
        });
        let fields = [
            "status_text_color",
            "launcher_background_color",
            "launcher_icon_color",
            "input_background_color",
            "input_text_color",
            "input_placeholder_color",
            "footer_text_color",
            "sound_icon_color",
            "close_icon_color",
            "send_background_color",
            "send_icon_color",
        ];
        for field in fields {
            colors[field] = serde_json::json!("#123456");
        }
        for dark in [false, true] {
            let mut payload = serde_json::json!({});
            let palette = if dark {
                payload["dark"] = serde_json::json!({});
                &mut payload["dark"]
            } else {
                &mut payload
            };
            palette["component_colors"] = colors.clone();
            let theme: WidgetTheme = serde_json::from_value(payload.clone()).unwrap();
            let saved = serde_json::to_value(theme.validate().unwrap()).unwrap();
            let saved_palette = if dark { &saved["dark"] } else { &saved };
            assert_eq!(saved_palette["component_colors"], colors);

            for field in fields {
                let mut invalid = payload.clone();
                let palette = if dark {
                    &mut invalid["dark"]
                } else {
                    &mut invalid
                };
                palette["component_colors"][field] = serde_json::json!("transparent");
                let theme: WidgetTheme = serde_json::from_value(invalid).unwrap();
                assert!(theme.validate().is_err(), "{field} must be validated");
            }
        }
    }

    #[test]
    fn round_trips_button_icons_and_rejects_unknown_icons() {
        for launcher_icon in ["chat", "message_circle", "headphones"] {
            for send_icon in ["send", "arrow_up", "arrow_right"] {
                let theme: WidgetTheme = serde_json::from_value(serde_json::json!({
                    "launcher_icon": launcher_icon,
                    "send_icon": send_icon,
                }))
                .unwrap();
                let saved = serde_json::to_value(theme.validate().unwrap()).unwrap();
                assert_eq!(saved["launcher_icon"], launcher_icon);
                assert_eq!(saved["send_icon"], send_icon);
            }
        }
        for field in ["launcher_icon", "send_icon"] {
            for value in [serde_json::json!("unknown"), serde_json::json!(42)] {
                let mut payload = serde_json::json!({});
                payload[field] = value;
                assert!(serde_json::from_value::<WidgetTheme>(payload).is_err());
            }
        }
    }

    #[test]
    fn validates_custom_and_hidden_footer_text() {
        assert!(
            WidgetTheme {
                footer_text: Some("Customer support".to_owned()),
                ..WidgetTheme::default()
            }
            .validate()
            .is_ok()
        );
        assert!(
            WidgetTheme {
                footer_text: Some(String::new()),
                ..WidgetTheme::default()
            }
            .validate()
            .is_ok()
        );
        assert!(
            WidgetTheme {
                footer_text: Some("x".repeat(161)),
                ..WidgetTheme::default()
            }
            .validate()
            .is_err()
        );
    }
}
