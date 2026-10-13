use super::*;

#[test]
fn test_known_valid_regex_behaves_like_a_compiled_one() {
    let pattern = format!("ls{OPTIONAL_ARGS_PATTERN}");
    let lazy = AgentModeCommandExecutionPredicate::new_known_valid_regex(&pattern);
    let compiled = AgentModeCommandExecutionPredicate::new_regex(&pattern).unwrap();

    for cmd in ["ls", "ls -la", "ls\t/tmp", "lsof", "echo ls", ""] {
        assert_eq!(lazy.matches(cmd), compiled.matches(cmd), "{cmd:?}");
    }
    assert!(lazy.matches("ls -la"));
    assert!(!lazy.matches("lsof"));
    assert_eq!(lazy, compiled);
    assert_eq!(lazy.to_string(), compiled.to_string());
    assert_eq!(lazy.to_string(), r"ls(\s.*)?");
}

#[test]
fn test_predicate_serializes_as_its_anchored_pattern() {
    let lazy = AgentModeCommandExecutionPredicate::new_known_valid_regex("which .*");
    let compiled = AgentModeCommandExecutionPredicate::new_regex("which .*").unwrap();

    let json = serde_json::to_string(&lazy).unwrap();
    assert_eq!(json, r#"{"AnchoredRegex":"^which .*$"}"#);
    assert_eq!(serde_json::to_string(&compiled).unwrap(), json);

    let roundtripped: AgentModeCommandExecutionPredicate = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtripped, compiled);
    assert!(roundtripped.matches("which git"));
}

#[test]
fn test_invalid_patterns_are_rejected() {
    assert!(AgentModeCommandExecutionPredicate::new_regex("(unclosed").is_err());
    assert!(
        serde_json::from_str::<AgentModeCommandExecutionPredicate>(r#"{"AnchoredRegex":"^(x$"}"#)
            .is_err()
    );
}
