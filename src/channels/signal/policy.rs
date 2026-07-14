//! Allowlist and message-policy checks for the Signal channel: sender and
//! group allowlists, and the configured DM/group acceptance policies.

use super::{DataMessage, Envelope, SignalChannel};

impl SignalChannel {
    /// Normalize an allowlist entry to the bare identifier.
    ///
    /// Strips the `uuid:` prefix if present, so `uuid:<id>` and `<id>` both
    /// match against a bare UUID sender.
    pub(super) fn normalize_allow_entry(entry: &str) -> &str {
        entry.strip_prefix("uuid:").unwrap_or(entry)
    }

    /// Check whether a sender matches an allowlist entry (wildcard `*` or
    /// normalized identifier equality). An empty list denies all senders.
    fn sender_in_allowlist(list: &[String], sender: &str) -> bool {
        if list.is_empty() {
            return false;
        }
        list.iter().any(|entry| {
            entry == "*"
                || Self::normalize_allow_entry(entry) == Self::normalize_allow_entry(sender)
        })
    }

    /// Check whether a sender is in the allowed users list.
    pub(super) fn is_sender_allowed(&self, sender: &str) -> bool {
        Self::sender_in_allowlist(&self.config.allow_from, sender)
    }

    /// Get effective group allow_from list (inherits from allow_from if empty).
    fn effective_group_allow_from(&self) -> &[String] {
        if self.config.group_allow_from.is_empty() {
            &self.config.allow_from
        } else {
            &self.config.group_allow_from
        }
    }

    /// Check whether a group is in the allowed groups list.
    ///
    /// - Empty list — deny all groups (DMs only, secure by default).
    /// - `*` — allow all groups.
    /// - Specific IDs — allow only those groups.
    pub(super) fn is_group_allowed(&self, group_id: &str) -> bool {
        if self.config.allow_from_groups.is_empty() {
            return false;
        }
        self.config
            .allow_from_groups
            .iter()
            .any(|entry| entry == "*" || entry == group_id)
    }

    /// Check whether a sender is allowed for group messages.
    fn is_group_sender_allowed(&self, sender: &str) -> bool {
        Self::sender_in_allowlist(self.effective_group_allow_from(), sender)
    }

    /// Return `true` when an attachment-only message must be dropped per config.
    pub(super) fn should_drop_attachment_only(
        &self,
        has_attachments: bool,
        has_message_text: bool,
    ) -> bool {
        if !self.config.ignore_attachments {
            return false;
        }
        has_attachments && !has_message_text
    }

    /// Apply the configured group policy; returns `false` when the message must be dropped.
    pub(super) fn group_message_allowed(&self, data_msg: &DataMessage, sender: &str) -> bool {
        let group_id = data_msg
            .group_info
            .as_ref()
            .and_then(|g| g.group_id.as_deref());
        match self.config.group_policy.as_str() {
            "disabled" => {
                tracing::debug!("Signal: group messages disabled, dropping");
                false
            }
            // For "open" policy, check group allowlist but not sender allowlist
            "open" => self.open_group_policy_allows(group_id),
            // Default to allowlist - check group AND sender
            "allowlist" => self.allowlist_group_policy_allows(group_id, sender),
            _ => true,
        }
    }

    /// Apply the "open" group policy: senders are not vetted, but the group
    /// allowlist still applies when a group identifier is present.
    fn open_group_policy_allows(&self, group_id: Option<&str>) -> bool {
        let Some(group_id) = group_id else {
            return true;
        };
        if self.is_group_allowed(group_id) {
            return true;
        }
        tracing::debug!(
            group_id = %group_id,
            "Signal: group not in allow_from_groups, dropping"
        );
        false
    }

    /// Apply the "allowlist" group policy: both the group and the sender must
    /// be allowed before a group message is accepted.
    fn allowlist_group_policy_allows(&self, group_id: Option<&str>, sender: &str) -> bool {
        let Some(group_id) = group_id else {
            return true;
        };
        if !self.is_group_allowed(group_id) {
            tracing::debug!(
                group_id = %group_id,
                "Signal: group not in allow_from_groups, dropping"
            );
            return false;
        }
        // Also check sender is allowed for group
        if !self.is_group_sender_allowed(sender) {
            tracing::debug!(
                sender = %sender,
                group_id = %group_id,
                "Signal: sender not in group_allow_from, dropping"
            );
            return false;
        }
        true
    }

    /// Apply the configured DM policy; returns `false` when the message must be dropped.
    pub(super) fn dm_message_allowed(&self, sender: &str, envelope: &Envelope) -> bool {
        match self.config.dm_policy.as_str() {
            "open" => true,
            "pairing" if !self.is_sender_allowed_with_pairing(sender) => {
                // Pairing policy: check allow_from + pairing store.
                // Handle pairing request - this will create a request and send
                // reply if new; the message is dropped whether or not the
                // pairing request succeeds.
                let _ = self.handle_pairing_request(sender, envelope.source_name.as_deref());
                false
            }
            "allowlist" if !self.is_sender_allowed(sender) => {
                // Default: check allow_from list
                tracing::debug!(sender = %sender, "Signal: sender not in allow_from, dropping");
                false
            }
            _ => true,
        }
    }
}
