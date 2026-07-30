use std::collections::HashMap;
use std::fs;

use futures::executor::block_on;
use mockito::{Matcher, Server};
use tempfile::tempdir;

use super::*;
use crate::server::server_api::harness_support::{UploadField, UploadFieldValue};

/// Drive a future to completion on a fresh Tokio runtime. Required for tests
/// that exercise `FileUploadBody`, which hashes the file via `spawn_blocking`
/// and therefore needs an actual Tokio reactor - `futures::executor::block_on`
/// works for the other tests because they never touch Tokio's blocking pool.
#[cfg(feature = "local_fs")]
fn block_on_tokio<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(fut)
}

#[test]
fn encode_crc32c_base64_matches_spec_example() {
    assert_eq!(encode_crc32c_base64(0x1234_5678), "EjRWeA==");
}

#[test]
fn vec_upload_body_hashes_its_buffer() {
    block_on(async {
        let body: Vec<u8> = b"artifact payload".to_vec();
        let checksum = body.compute_crc32c_base64().await.unwrap();
        assert_eq!(
            checksum,
            encode_crc32c_base64(CRC32C.checksum(b"artifact payload"))
        );
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn file_upload_body_hashes_without_buffering() {
    block_on_tokio(async {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("artifact.bin");
        let payload = b"streamed artifact payload";
        fs::write(&path, payload).unwrap();
        let body = FileUploadBody::new(path);
        let checksum = body.compute_crc32c_base64().await.unwrap();
        assert_eq!(checksum, encode_crc32c_base64(CRC32C.checksum(payload)));
    });
}

#[test]
fn upload_to_target_replays_headers_for_byte_uploads() {
    block_on(async {
        let mut server = Server::new();
        let mock = server
            .mock("POST", "/upload")
            .match_header("x-test-header", "expected-header")
            .match_body("serialized body")
            .with_status(200)
            .create();

        let client = http_client::Client::new_for_test();
        let target = UploadTarget {
            url: format!("{}/upload", server.url()),
            method: "POST".to_string(),
            headers: HashMap::from([("x-test-header".to_string(), "expected-header".to_string())]),
            fields: Vec::new(),
        };

        upload_to_target(&client, &target, b"serialized body".to_vec())
            .await
            .unwrap();

        mock.assert();
    });
}

/// Mockito matcher that captures the raw request body so individual fields of
/// a multipart payload can be asserted without depending on reqwest's boundary.
fn matches_multipart_field(name: &'static str, expected: &'static str) -> Matcher {
    // mockito matches on the body as a raw byte slice; we look for the form-data
    // disposition header and the expected value within the same part.
    Matcher::Regex(format!(
        "name=\"{name}\"\r\n\r\n{expected}\r\n",
        name = regex::escape(name),
        expected = regex::escape(expected)
    ))
}

#[test]
fn upload_to_target_builds_multipart_post_with_static_crc_and_data_fields() {
    block_on(async {
        let mut server = Server::new();
        let body = b"multipart artifact payload";
        let expected_crc32c = encode_crc32c_base64(CRC32C.checksum(body));
        let expected_crc32c_escaped = expected_crc32c.clone();

        let mock = server
            .mock("POST", "/upload")
            .match_header(
                "content-type",
                Matcher::Regex("^multipart/form-data; boundary=.+".to_string()),
            )
            .match_body(Matcher::AllOf(vec![
                matches_multipart_field("key", "presigned/object/key"),
                matches_multipart_field("Content-Type", "application/octet-stream"),
                Matcher::Regex(format!(
                    r#"name="x-amz-checksum-crc32c"\r\n\r\n{crc}\r\n"#,
                    crc = regex::escape(&expected_crc32c_escaped)
                )),
                Matcher::Regex(r#"name="file"[\s\S]*multipart artifact payload"#.to_string()),
            ]))
            .with_status(204)
            .create();

        let client = http_client::Client::new_for_test();
        let target = UploadTarget {
            url: format!("{}/upload", server.url()),
            method: "POST".to_string(),
            headers: HashMap::new(),
            fields: vec![
                UploadField {
                    name: "key".to_string(),
                    value: UploadFieldValue::Static {
                        value: "presigned/object/key".to_string(),
                    },
                },
                UploadField {
                    name: "Content-Type".to_string(),
                    value: UploadFieldValue::Static {
                        value: "application/octet-stream".to_string(),
                    },
                },
                UploadField {
                    name: "x-amz-checksum-crc32c".to_string(),
                    value: UploadFieldValue::ContentCrc32C,
                },
                UploadField {
                    name: "file".to_string(),
                    value: UploadFieldValue::ContentData,
                },
            ],
        };

        upload_to_target(&client, &target, body.to_vec())
            .await
            .unwrap();

        mock.assert();
    });
}

#[test]
fn multipart_post_requires_content_data_field() {
    block_on(async {
        let target = UploadTarget {
            url: "https://example.com/upload".to_string(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            fields: vec![UploadField {
                name: "key".to_string(),
                value: UploadFieldValue::Static {
                    value: "presigned/object/key".to_string(),
                },
            }],
        };
        let normalized = NormalizedUploadTarget::from(&target);

        let err = build_multipart_form(&normalized, b"missing content data".to_vec(), None)
            .await
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("Multipart upload must contain exactly one ContentData field")
        );
    });
}
