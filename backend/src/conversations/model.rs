//! Conversation and message state machines.

use serde::{Deserialize, Serialize};

/// Lifecycle of a support conversation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    New,
    Open,
    WaitingCustomer,
    Resolved,
}

impl ConversationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Open => "open",
            Self::WaitingCustomer => "waiting_customer",
            Self::Resolved => "resolved",
        }
    }

    /// Applies a domain transition.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested transition is not allowed.
    pub fn transition(self, target: Self) -> Result<Self, InvalidConversationTransition> {
        let valid = matches!(
            (self, target),
            (Self::New, Self::Open | Self::Resolved)
                | (Self::New | Self::Open, Self::WaitingCustomer)
                | (Self::Open | Self::WaitingCustomer, Self::Resolved)
                | (Self::WaitingCustomer, Self::Open)
                | (Self::Resolved, Self::Open)
        );

        valid
            .then_some(target)
            .ok_or(InvalidConversationTransition {
                from: self,
                to: target,
            })
    }
}

impl TryFrom<&str> for ConversationStatus {
    type Error = UnknownConversationStatus;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "new" => Ok(Self::New),
            "open" => Ok(Self::Open),
            "waiting_customer" => Ok(Self::WaitingCustomer),
            "resolved" => Ok(Self::Resolved),
            _ => Err(UnknownConversationStatus(value.to_owned())),
        }
    }
}

/// Invalid state transition requested by a caller.
#[derive(Debug, thiserror::Error)]
#[error("conversation cannot transition from {from:?} to {to:?}")]
pub struct InvalidConversationTransition {
    from: ConversationStatus,
    to: ConversationStatus,
}

/// Unknown status loaded from an external boundary.
#[derive(Debug, thiserror::Error)]
#[error("unknown conversation status: {0}")]
pub struct UnknownConversationStatus(String);

/// Delivery state of one message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Queued,
    Sending,
    Sent,
    Delivered,
    Read,
    Failed,
}

impl MessageStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Delivered => "delivered",
            Self::Read => "read",
            Self::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConversationStatus;

    #[test]
    fn allows_reopening_a_resolved_conversation() {
        assert_eq!(
            ConversationStatus::Resolved
                .transition(ConversationStatus::Open)
                .unwrap(),
            ConversationStatus::Open
        );
    }

    #[test]
    fn allows_resolving_a_new_conversation_without_joining() {
        assert_eq!(
            ConversationStatus::New
                .transition(ConversationStatus::Resolved)
                .unwrap(),
            ConversationStatus::Resolved
        );
    }
}
