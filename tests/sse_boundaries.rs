use acosmi::core::http::iter_sse_lines_with_cap;
use bytes::Bytes;
use futures::{stream, StreamExt};

async fn lines(parts: Vec<&[u8]>, cap: usize) -> Vec<acosmi::Result<String>> {
    iter_sse_lines_with_cap(
        stream::iter(parts.into_iter().map(|p| Ok(Bytes::copy_from_slice(p)))),
        cap,
    )
    .collect()
    .await
}

#[tokio::test]
async fn complete_oversized_line_is_rejected() {
    let result = lines(vec![b"12345\n"], 4).await;
    assert!(result[0].is_err(), "complete line must obey the cap");
}

#[tokio::test]
async fn fragmented_oversized_line_is_rejected() {
    let result = lines(vec![b"1234", b"5\n"], 4).await;
    assert!(result[0].is_err());
}

#[tokio::test]
async fn invalid_utf8_is_rejected_without_replacement_characters() {
    for parts in [
        vec![&b"a\xff\n"[..]],
        vec![&b"\xe4"[..]],
        vec![&b"\xc0\xaf\n"[..]],
    ] {
        assert!(lines(parts, 10).await[0].is_err());
    }
}

#[tokio::test]
async fn exact_limit_crlf_multibyte_and_eof_are_preserved() {
    let result = lines(vec![b"1234\r", b"\n\xe4", b"\xb8\xad\nlast"], 4).await;
    let result: Vec<_> = result.into_iter().map(Result::unwrap).collect();
    assert_eq!(result, ["1234", "中", "last"]);
}

#[tokio::test]
async fn many_short_lines_in_one_chunk_do_not_count_as_one_line() {
    let result = lines(vec![b"a\nb\n\n:\r\n"], 1).await;
    let result: Vec<_> = result.into_iter().map(Result::unwrap).collect();
    assert_eq!(result, ["a", "b", "", ":"]);
}
