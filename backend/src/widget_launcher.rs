//! Public, tenant-configurable widget launcher behavior.

use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetLauncherType {
    Icon,
    Text,
    IconText,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetLauncherPosition {
    BottomRight,
    BottomLeft,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetLauncherAnimation {
    Pulse,
    Lift,
    Sway,
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WidgetLauncher {
    pub launcher_type: WidgetLauncherType,
    pub position: WidgetLauncherPosition,
    pub label: String,
    pub show_greeting: bool,
    pub show_operator_profile: bool,
    pub offset_x: i32,
    pub offset_y: i32,
    pub attention_animation: WidgetLauncherAnimation,
    pub animation_interval_seconds: u16,
    pub proactive_invitation_enabled: bool,
    pub proactive_invitation_delay_seconds: u16,
}

impl Default for WidgetLauncher {
    fn default() -> Self {
        Self {
            launcher_type: WidgetLauncherType::Icon,
            position: WidgetLauncherPosition::BottomRight,
            label: "Chat with us".to_owned(),
            show_greeting: true,
            show_operator_profile: false,
            offset_x: 20,
            offset_y: 20,
            attention_animation: WidgetLauncherAnimation::Pulse,
            animation_interval_seconds: 5,
            proactive_invitation_enabled: false,
            proactive_invitation_delay_seconds: 15,
        }
    }
}

impl WidgetLauncher {
    /// Validates and normalizes public launcher settings.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` for a blank or oversized label, browser-unsafe
    /// offsets, or timing settings outside the supported range.
    pub fn validate(mut self) -> Result<Self, AppError> {
        self.label = self.label.trim().to_owned();
        if !self.show_greeting && self.attention_animation == WidgetLauncherAnimation::Pulse {
            self.attention_animation = WidgetLauncherAnimation::Lift;
        }
        if !(1..=40).contains(&self.label.chars().count()) {
            return Err(AppError::BadRequest(
                "launcher.label must contain between 1 and 40 characters".to_owned(),
            ));
        }
        for (name, value) in [("offset_x", self.offset_x), ("offset_y", self.offset_y)] {
            if !(0..=120).contains(&value) {
                return Err(AppError::BadRequest(format!(
                    "launcher.{name} must be between 0 and 120"
                )));
            }
        }
        if !(1..=300).contains(&self.animation_interval_seconds) {
            return Err(AppError::BadRequest(
                "launcher.animation_interval_seconds must be between 1 and 300".to_owned(),
            ));
        }
        if !(1..=300).contains(&self.proactive_invitation_delay_seconds) {
            return Err(AppError::BadRequest(
                "launcher.proactive_invitation_delay_seconds must be between 1 and 300".to_owned(),
            ));
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{WidgetLauncher, WidgetLauncherAnimation};

    #[test]
    fn validates_and_normalizes_launcher_settings() {
        let launcher = WidgetLauncher {
            label: "  Ask support  ".to_owned(),
            ..WidgetLauncher::default()
        }
        .validate()
        .unwrap();
        assert_eq!(launcher.label, "Ask support");

        assert!(
            WidgetLauncher {
                label: "   ".to_owned(),
                ..WidgetLauncher::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            WidgetLauncher {
                offset_x: 121,
                ..WidgetLauncher::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            WidgetLauncher {
                animation_interval_seconds: 0,
                ..WidgetLauncher::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            WidgetLauncher {
                animation_interval_seconds: 301,
                ..WidgetLauncher::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            WidgetLauncher {
                proactive_invitation_delay_seconds: 0,
                ..WidgetLauncher::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            WidgetLauncher {
                proactive_invitation_delay_seconds: 301,
                ..WidgetLauncher::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn defaults_animation_for_existing_saved_launchers() {
        let launcher: WidgetLauncher = serde_json::from_value(serde_json::json!({
            "launcher_type": "icon",
            "position": "bottom_right",
            "label": "Chat with us",
            "offset_x": 20,
            "offset_y": 20
        }))
        .unwrap();

        assert_eq!(launcher.attention_animation, WidgetLauncherAnimation::Pulse);
        assert_eq!(launcher.animation_interval_seconds, 5);
        assert!(launcher.show_greeting);
        assert!(!launcher.show_operator_profile);
        assert!(!launcher.proactive_invitation_enabled);
        assert_eq!(launcher.proactive_invitation_delay_seconds, 15);
    }

    #[test]
    fn replaces_pulse_with_lift_only_when_greeting_is_hidden() {
        let shown = WidgetLauncher::default().validate().unwrap();
        assert_eq!(shown.attention_animation, WidgetLauncherAnimation::Pulse);

        let hidden = WidgetLauncher {
            show_greeting: false,
            ..WidgetLauncher::default()
        }
        .validate()
        .unwrap();
        assert_eq!(hidden.attention_animation, WidgetLauncherAnimation::Lift);
    }
}
