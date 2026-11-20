//! Wire types shared with the FastAPI backend (`backend/app/schemas`).
//! Field names match the JSON exactly; optional fields default so that older or
//! newer servers that omit them still deserialize.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type Id = String;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct User {
    pub id: Id,
    pub username: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub is_admin: bool,
}

impl User {
    /// Display name if set, otherwise the username (mirrors `getUserDisplayName`).
    pub fn name(&self) -> &str {
        match self.display_name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => name,
            _ => &self.username,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GroupRole {
    Admin,
    #[default]
    Member,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Member {
    #[serde(flatten)]
    pub user: User,
    #[serde(default)]
    pub group_role: GroupRole,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LastMessage {
    pub id: Id,
    pub sender_id: Id,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub video_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
    Private,
    Group,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Chat {
    pub id: Id,
    #[serde(rename = "type")]
    pub kind: ChatKind,
    #[serde(default)]
    pub peer: Option<User>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub members: Vec<Member>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_message: Option<LastMessage>,
    #[serde(default)]
    pub unread_count: u32,
    #[serde(default)]
    pub peer_last_read_at: Option<DateTime<Utc>>,
}

impl Chat {
    pub fn is_group(&self) -> bool {
        self.kind == ChatKind::Group
    }

    pub fn title(&self) -> &str {
        if self.is_group() {
            self.name.as_deref().unwrap_or("Group")
        } else {
            self.peer.as_ref().map(User::name).unwrap_or("Chat")
        }
    }

    /// Time used for ordering the chat list (`sortChats`).
    pub fn activity_at(&self) -> DateTime<Utc> {
        self.last_message
            .as_ref()
            .map(|message| message.created_at)
            .unwrap_or(self.created_at)
    }

    /// The web client hides private chats until they contain a message.
    pub fn is_listed(&self) -> bool {
        self.is_group() || self.last_message.is_some()
    }

    pub fn member(&self, user_id: &str) -> Option<&Member> {
        self.members.iter().find(|member| member.user.id == user_id)
    }

    pub fn is_admin(&self, user_id: &str) -> bool {
        self.member(user_id)
            .is_some_and(|member| member.group_role == GroupRole::Admin)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReplyMessage {
    pub id: Id,
    pub sender_id: Id,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub video_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Message {
    pub id: Id,
    pub chat_id: Id,
    pub sender_id: Id,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub video_url: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub edited_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub pinned_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub pinned_by_id: Option<Id>,
    #[serde(default)]
    pub reply_to_message_id: Option<Id>,
    #[serde(default)]
    pub reply_to_message: Option<ReplyMessage>,
    #[serde(default)]
    pub is_call: bool,
}

/// Short text shown for a message in previews (`messagePreviewText`).
pub fn preview_text(content: &str, image_url: Option<&str>, video_url: Option<&str>) -> String {
    if !content.is_empty() {
        content.to_owned()
    } else if image_url.is_some() {
        "Photo".to_owned()
    } else if video_url.is_some() {
        "Video".to_owned()
    } else {
        "Message".to_owned()
    }
}

impl Message {
    pub fn preview(&self) -> String {
        preview_text(&self.content, self.image_url.as_deref(), self.video_url.as_deref())
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    pub fn to_last_message(&self) -> LastMessage {
        LastMessage {
            id: self.id.clone(),
            sender_id: self.sender_id.clone(),
            content: self.content.clone(),
            image_url: self.image_url.clone(),
            video_url: self.video_url.clone(),
            created_at: self.created_at,
        }
    }
}

impl ReplyMessage {
    pub fn preview(&self) -> String {
        preview_text(&self.content, self.image_url.as_deref(), self.video_url.as_deref())
    }
}

impl LastMessage {
    pub fn preview(&self) -> String {
        preview_text(&self.content, self.image_url.as_deref(), self.video_url.as_deref())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionInfo {
    pub id: Id,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub remember_me: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AdminUser {
    pub id: Id,
    pub username: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub is_blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
pub struct AdminStats {
    pub users: u64,
    pub blocked_users: u64,
    pub online_users: u64,
    pub groups: u64,
    pub private_chats: u64,
    pub messages: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdminUsersPage {
    pub users: Vec<AdminUser>,
    pub total: u64,
}

/// `GET/PATCH /users/me/privacy`. `last_seen` is "everyone" or "nobody"; hiding your own
/// last seen also hides everyone else's from you.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Privacy {
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileUpdate {
    pub username: String,
    pub display_name: Option<String>,
    pub bio: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupDetails {
    pub name: String,
    pub username: Option<String>,
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chat_with_members_and_last_message() {
        let json = r#"{
            "id": "c1", "type": "group", "peer": null, "name": "Team", "username": "team",
            "description": null, "created_at": "2026-11-17T10:00:00Z",
            "members": [{"id": "u1", "username": "ann", "display_name": "Ann", "bio": null,
                         "avatar_url": null, "created_at": "2026-01-01T00:00:00Z", "group_role": "admin"}],
            "last_message": {"id": "m1", "sender_id": "u1", "content": "", "image_url": "/uploads/a.png",
                             "video_url": null, "created_at": "2026-11-17T11:00:00.123456+00:00"},
            "unread_count": 3, "peer_last_read_at": null
        }"#;
        let chat: Chat = serde_json::from_str(json).unwrap();
        assert!(chat.is_group());
        assert_eq!(chat.title(), "Team");
        assert!(chat.is_admin("u1"));
        assert_eq!(chat.last_message.as_ref().unwrap().preview(), "Photo");
        assert_eq!(chat.unread_count, 3);
    }

    #[test]
    fn user_name_falls_back_to_username() {
        let mut user: User = serde_json::from_str(
            r#"{"id":"u","username":"bob","display_name":"  ","bio":null,"avatar_url":null,"created_at":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(user.name(), "bob");
        user.display_name = Some("Bob B".into());
        assert_eq!(user.name(), "Bob B");
        assert!(!user.is_admin);
    }

    #[test]
    fn parses_call_message() {
        let json = r#"{"is_call": true, "id": "m", "chat_id": "c", "sender_id": "u",
            "content": "{\"reason\":\"ended\"}", "image_url": null, "video_url": null,
            "created_at": "2026-11-17T11:00:00Z", "reply_to_message_id": null, "reply_to_message": null,
            "edited_at": null, "deleted_at": null, "pinned_at": null, "pinned_by_id": null}"#;
        let message: Message = serde_json::from_str(json).unwrap();
        assert!(message.is_call);
        assert!(!message.is_deleted());
    }
}
