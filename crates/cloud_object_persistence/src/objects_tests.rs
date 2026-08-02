use cloud_objects::auth::UserUid;
use cloud_objects::cloud_object::{CloudObjectGuest, ServerObjectContainer};
use cloud_objects::drive::sharing::{
    LinkSharingSubjectType, SharingAccessLevel, Subject, TeamKind, UserKind,
};
use cloud_objects::ids::ServerId;
use lazy_static::lazy_static;

#[test]
fn test_roundtrip_guests() {
    let guests = vec![
        CloudObjectGuest {
            subject: Subject::User(UserKind::Account(UserUid::new("firebase_uid"))),
            access_level: SharingAccessLevel::Edit,
            source: None,
        },
        CloudObjectGuest {
            subject: Subject::PendingUser {
                email: Some("pending@warp.dev".to_string()),
            },
            access_level: SharingAccessLevel::View,
            source: Some(ServerObjectContainer::Folder {
                folder_uid: 123.into(),
            }),
        },
        CloudObjectGuest {
            subject: Subject::Team(TeamKind::Team {
                team_uid: ServerId::from(99),
            }),
            access_level: SharingAccessLevel::Edit,
            source: None,
        },
    ];

    let encoded = super::encode_guests(&guests).expect("encode should succeed");
    let decoded = super::decode_guests(&encoded).expect("decode should succeed");

    assert_eq!(guests, decoded);
}

#[test]
fn test_fail_unsupported_subjects() {
    let result = super::encode_guests(&[CloudObjectGuest {
        subject: Subject::AnyoneWithLink(LinkSharingSubjectType::Anyone),
        access_level: SharingAccessLevel::View,
        source: None,
    }]);
    assert!(result.is_err());
}
