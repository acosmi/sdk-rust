//! Sources parsing contract ported from the public TypeScript SDK.
use acosmi::{
    classify_sources_event, parse_sources_event, SourcesEventIssueCode as Code,
    SourcesEventParseResult as Parsed, StreamEvent,
};

fn event(name: &str, data: &str) -> StreamEvent {
    StreamEvent {
        event: name.into(),
        data: data.into(),
        ..Default::default()
    }
}
#[test]
fn four_states_and_legacy_empty_semantics() {
    assert!(matches!(
        classify_sources_event(&event("delta", "{}")),
        Parsed::NotSources
    ));
    assert!(matches!(
        classify_sources_event(&event("delta", "not-json")),
        Parsed::NotSources
    ));
    let empty = event("sources", r#"{"sources":[],"session_id":"s"}"#);
    assert!(
        matches!(classify_sources_event(&empty),Parsed::EmptySources {session_id:Some(s)} if s=="s")
    );
    assert!(parse_sources_event(&empty).is_none());
    let good = event(
        "message",
        r#"{"type":"sources","sources":[{"title":"","url":"not-validated-as-url","snippet":"x","extra":1}],"extra":true}"#,
    );
    match classify_sources_event(&good) {
        Parsed::Sources { value } => assert_eq!(value.sources[0].url, "not-validated-as-url"),
        other => panic!("{other:?}"),
    }
}
#[test]
fn structural_error_codes_are_deterministic() {
    for (data, expected) in [
        ("!", Code::InvalidJson),
        ("null", Code::MissingSources),
        ("[]", Code::MissingSources),
        ("{}", Code::MissingSources),
        (r#"{"sources":null}"#, Code::SourcesNotArray),
        (
            r#"{"sources":[],"session_id":null}"#,
            Code::SessionIdInvalid,
        ),
        (r#"{"sources":[null]}"#, Code::SourceNotObject),
        (r#"{"sources":[[]]}"#, Code::SourceNotObject),
        (r#"{"sources":[{}]}"#, Code::SourceTitleInvalid),
        (r#"{"sources":[{"title":"t"}]}"#, Code::SourceUrlInvalid),
        (
            r#"{"sources":[{"title":"t","url":"u","snippet":null}]}"#,
            Code::SourceSnippetInvalid,
        ),
    ] {
        assert!(
            matches!(classify_sources_event(&event("sources",data)),Parsed::MalformedSources {code} if code==expected),
            "{data}"
        );
    }
}
