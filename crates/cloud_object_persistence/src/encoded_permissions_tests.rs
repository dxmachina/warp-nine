use cloud_objects::cloud_object::{CloudObjectGuest, ServerObjectContainer};
use cloud_objects::drive::sharing::{
    LinkSharingSubjectType, SharingAccessLevel, Subject, TeamKind, UserKind,
};
use cloud_objects::ids::ServerId;
use lazy_static::lazy_static;

use super::{decode_guests, encode_guests};

#[test]
fn test_roundtrip_guests() {
    let guests = vec![
        CloudObjectGuest {
            subject: Subject::User(UserKind::Account(cloud_objects::UserUid::new(
                "firebase_uid",
            ))),
            access_level: SharingAccessLevel::Edit,
            source: None,
        },
        CloudObjectGuest {
            subject: Subject::PendingUser {
                email: Some("pending@warp.dev".to_string()),
            },
            access_level: SharingAccessLevel::View,
            source: Some(ServerObjectContainer::Folder {
                folder_uid: ServerId::from_string_lossy("1234567890123456789012"),
            }),
        },
        CloudObjectGuest {
            subject: Subject::Team(TeamKind::Team {
                team_uid: ServerId::from_string_lossy("abcdefghijklmnopqrstuv"),
            }),
            access_level: SharingAccessLevel::Edit,
            source: None,
        },
    ];

    let encoded = encode_guests(&guests).expect("encode should succeed");
    let decoded = decode_guests(&encoded).expect("decode should succeed");

    assert_eq!(guests, decoded);
}

#[test]
fn test_fail_unsupported_subjects() {
    let result = encode_guests(&[CloudObjectGuest {
        subject: Subject::AnyoneWithLink(LinkSharingSubjectType::Anyone),
        access_level: SharingAccessLevel::View,
        source: None,
    }]);
    assert!(result.is_err());
}
