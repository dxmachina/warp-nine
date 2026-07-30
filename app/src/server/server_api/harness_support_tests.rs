// LOCAL FORK: the `Artifact` wire-format tests went out with `crate::ai::artifacts`.
// They pinned the serialization of agent run artifacts (pull requests, external
// references), which no longer exist. The upload-target and shutdown-report
// tests below cover types this fork still has.

#[test]
fn upload_target_deserializes_null_fields_as_empty() {
    use super::UploadTarget;

    let target: UploadTarget = serde_json::from_value(serde_json::json!({
        "url": "https://example.com/upload",
        "method": "PUT",
        "headers": {},
        "fields": null
    }))
    .unwrap();

    assert_eq!(target.fields.len(), 0);
}

#[test]
fn report_shutdown_clean_serializes_without_error() {
    use super::ReportShutdownRequest;

    let request = ReportShutdownRequest::clean();
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json, serde_json::json!({}));
}

#[test]
fn report_shutdown_abnormal_serializes_with_error() {
    use super::ReportShutdownRequest;

    let request = ReportShutdownRequest::abnormal("oom".to_string(), "out of memory".to_string());
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "error": {
                "category": "oom",
                "message": "out of memory"
            }
        })
    );
}
