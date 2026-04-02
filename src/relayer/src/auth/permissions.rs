//! Permission types matching the Groups SDK smart contract.

use std::fmt;

/// Messaging-specific permissions from the Groups SDK.
/// `MessagingSender`, can send new messages to the group
/// `MessagingReader`, can read/decrypt messages in the group
/// `MessagingEditor`, can edit existing messages
/// `MessagingDeleter`, can delete messages

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)]
pub enum MessagingPermission {
    MessagingSender,

    MessagingReader,

    MessagingEditor,

    MessagingDeleter,
}

impl MessagingPermission {
    pub fn from_type_name(type_name: &str) -> Option<Self> {
        // Extract the last segment after `::`
        let permission_name = type_name.split("::").last()?;

        match permission_name {
            "MessagingSender" => Some(MessagingPermission::MessagingSender),
            "MessagingReader" => Some(MessagingPermission::MessagingReader),
            "MessagingEditor" => Some(MessagingPermission::MessagingEditor),
            "MessagingDeleter" => Some(MessagingPermission::MessagingDeleter),
            _ => None,
        }
    }

    /// Returns the simple name of the permission (without module path).
    pub fn as_str(&self) -> &'static str {
        match self {
            MessagingPermission::MessagingSender => "MessagingSender",
            MessagingPermission::MessagingReader => "MessagingReader",
            MessagingPermission::MessagingEditor => "MessagingEditor",
            MessagingPermission::MessagingDeleter => "MessagingDeleter",
        }
    }
}

///  Display for printing in logs and errors.
impl fmt::Display for MessagingPermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_type_name_valid() {
        assert_eq!(
            MessagingPermission::from_type_name("0x123aabbcc::messaging::MessagingSender"),
            Some(MessagingPermission::MessagingSender)
        );
        assert_eq!(
            MessagingPermission::from_type_name("0x123aabbcc::messaging::MessagingReader"),
            Some(MessagingPermission::MessagingReader)
        );
        assert_eq!(
            MessagingPermission::from_type_name("0x123aabbcc::messaging::MessagingEditor"),
            Some(MessagingPermission::MessagingEditor)
        );
        assert_eq!(
            MessagingPermission::from_type_name("0x123aabbcc::messaging::MessagingDeleter"),
            Some(MessagingPermission::MessagingDeleter)
        );
    }

    #[test]
    fn test_from_type_name_non_messaging() {
        assert_eq!(
            MessagingPermission::from_type_name("0x123aabbcc::permissioned_group::Administrator"),
            None
        );
    }

    #[test]
    fn test_from_type_name_invalid() {
        assert_eq!(MessagingPermission::from_type_name(""), None);
        assert_eq!(
            MessagingPermission::from_type_name("InvalidPermission"),
            None
        );
    }
}
