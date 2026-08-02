use cloud_objects::UserUid;
use cloud_objects::ids::ServerId;

/// Public struct for storing all the UserProfile data that's fed in from either sqlite or the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserProfileWithUID {
    pub firebase_uid: UserUid,
    pub display_name: Option<String>,
    pub email: String,
    pub photo_url: String,
}

// LOCAL FORK: `From<session_sharing_protocol::common::ProfileData>` built a profile from
// a shared-session participant's wire data, and went with session sharing.

impl From<warp_graphql::user::PublicUserProfile> for UserProfileWithUID {
    fn from(value: warp_graphql::user::PublicUserProfile) -> Self {
        UserProfileWithUID {
            firebase_uid: UserUid::new(&value.uid),
            display_name: value.display_name,
            email: value.email.unwrap_or_default(),
            photo_url: value.photo_url.unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserProfileIdAndName {
    pub user_uid: UserUid,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamProfileIdAndName {
    pub team_uid: ServerId,
    pub display_name: String,
}
