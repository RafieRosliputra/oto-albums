//! Web proof API **contract** (v1.1). No server ships in v1; these types pin the wire format so a
//! future server and the desktop client agree. See docs/web-proof-api.md.
//!
//! ```text
//! POST /v1/proofs                       create a proof (metadata only, no photos)
//! POST /v1/proofs/{id}/revisions        upload a revision (spread JPEGs + manifest)
//! GET  /v1/proofs/{id}                  proof + latest revision + comments
//! POST /v1/proofs/{id}/comments         add an anchored comment
//! POST /v1/proofs/{id}/approve          client approval (revision-bound)
//! ```
//! Security: client access uses an unguessable share token + optional PIN; PINs are stored only
//! as Argon2id hashes server-side. No password is ever stored in plaintext.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateProof {
    pub project_id: Uuid,
    pub title: String,
    pub client_name: String,
    /// Optional PIN the client must enter; hashed server-side (Argon2id), never stored in clear.
    pub pin: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proof {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub client_name: String,
    pub share_url: String,
    pub status: ProofStatus,
    pub created_at: String,
    pub revisions: Vec<Revision>,
    pub comments: Vec<Comment>,
    pub approval: Option<Approval>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofStatus {
    Draft,
    Sent,
    ChangesRequested,
    Approved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub id: Uuid,
    pub number: u32,
    pub created_at: String,
    pub note: String,
    pub spreads: Vec<RevisionSpread>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevisionSpread {
    pub spread_id: Uuid,
    pub index: u32,
    pub image_url: String,
    pub photos: Vec<ProofPhoto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProofPhoto {
    pub image_id: Uuid,
    pub number: u32,
    /// Frame rectangle on the spread, normalized 0..1.
    pub x_norm: f32,
    pub y_norm: f32,
    pub w_norm: f32,
    pub h_norm: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewComment {
    pub revision_id: Uuid,
    pub spread_id: Uuid,
    /// Anchor on the spread, normalized 0..1.
    pub x_norm: f32,
    pub y_norm: f32,
    pub image_id: Option<Uuid>,
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: Uuid,
    pub revision_id: Uuid,
    pub spread_id: Uuid,
    pub x_norm: f32,
    pub y_norm: f32,
    pub image_id: Option<Uuid>,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approve {
    pub revision_id: Uuid,
    pub approver_name: String,
    /// Client confirms they reviewed every spread of this revision.
    pub confirmed_all_spreads: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    pub revision_id: Uuid,
    pub approver_name: String,
    pub approved_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl NewComment {
    /// Client-side validation mirrored by the server.
    pub fn validate(&self) -> Result<(), ApiError> {
        let bad = |m: &str| Err(ApiError { code: "invalid_comment".into(), message: m.into() });
        if !(0.0..=1.0).contains(&self.x_norm) || !(0.0..=1.0).contains(&self.y_norm) {
            return bad("anchor must be inside the spread (0..1)");
        }
        if self.body.trim().is_empty() || self.body.len() > 4000 {
            return bad("comment must be 1–4000 characters");
        }
        if self.author.trim().is_empty() {
            return bad("author is required");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_roundtrip_and_validation() {
        let c = NewComment { revision_id: Uuid::now_v7(), spread_id: Uuid::now_v7(), x_norm: 0.4, y_norm: 0.7, image_id: None, author: "Aina".into(), body: "Swap #12 and #14".into() };
        let j = serde_json::to_string(&c).unwrap();
        assert!(j.contains("\"x_norm\":0.4"));
        let back: NewComment = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
        assert!(c.validate().is_ok());
        let bad = NewComment { x_norm: 1.5, ..c };
        assert!(bad.validate().is_err());
    }
}
