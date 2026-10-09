use axum::body::Bytes;

use crate::error::AppError;

/// 首块异常时写入诊断信息的最大字节数。
const DIAGNOSTIC_LIMIT: usize = 200;

/// 连续空块的上限：超过视为上游没有按事件流输出数据。
const MAX_EMPTY_CHUNKS: usize = 32;

/// 校验上游 SSE 响应的开头，并把首个非空数据块从流中取出。
///
/// 返回 `Ok(Some(head))` 时，`head` 是已经消费掉的首块，调用方必须把它
/// 拼回上游字节流（`upstream_bytes_stream`），否则会丢失数据。
/// 上游响应是网页（即使状态码是 2xx）或空响应时返回
/// `AppError::UpstreamInvalidResponse`，让候选循环继续切换供应商。
pub(crate) async fn verify_upstream_sse_head(
    response: &mut reqwest::Response,
) -> Result<Option<Bytes>, AppError> {
    if let Some(content_type) = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        let content_type = content_type.trim().to_ascii_lowercase();
        if content_type.starts_with("text/html") || content_type.starts_with("application/xhtml") {
            return Err(AppError::UpstreamInvalidResponse {
                message: format!("上游返回了网页而不是事件流（Content-Type: {content_type}）"),
            });
        }
    }

    let mut empty_chunks = 0usize;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if chunk.is_empty() {
                    empty_chunks += 1;
                    if empty_chunks > MAX_EMPTY_CHUNKS {
                        return Err(AppError::UpstreamInvalidResponse {
                            message: "上游持续返回空数据而不是事件流".to_string(),
                        });
                    }
                    continue;
                }
                if trim_ascii_start(&chunk).starts_with(b"<") {
                    return Err(AppError::UpstreamInvalidResponse {
                        message: html_body_message(&chunk),
                    });
                }
                return Ok(Some(chunk));
            }
            Ok(None) => {
                return Err(AppError::UpstreamInvalidResponse {
                    message: "上游返回了空响应".to_string(),
                });
            }
            Err(error) => {
                return Err(AppError::Upstream {
                    status: None,
                    message: format!("上游连接失败: {error}"),
                });
            }
        }
    }
}

fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    &bytes[start..]
}

/// 网页响应的诊断消息：尽量带上页面标题和正文片段。
fn html_body_message(chunk: &[u8]) -> String {
    let excerpt = String::from_utf8_lossy(&chunk[..chunk.len().min(DIAGNOSTIC_LIMIT)]);
    match page_title(&excerpt) {
        Some(title) => format!("上游返回了网页而不是事件流（页面标题: {title}）"),
        None => format!(
            "上游返回了网页而不是事件流（响应片段: {:?}）",
            excerpt.trim()
        ),
    }
}

fn page_title(excerpt: &str) -> Option<&str> {
    let lower = excerpt.to_ascii_lowercase();
    let title_start = lower.find("<title")?;
    let open = excerpt[title_start..].find('>')? + title_start + 1;
    let close = lower[open..].find("</title>")? + open;
    let title = excerpt[open..close].trim();
    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::verify_upstream_sse_head;
    use crate::{bridge::stream::upstream_bytes_stream, error::AppError};
    use axum::{
        body::{Body, Bytes},
        http::{header, StatusCode},
        response::Response,
        routing::get,
        Router,
    };
    use futures::{stream, StreamExt};
    use std::convert::Infallible;
    use tokio::net::TcpListener;

    /// 启动一个只服务一次请求的测试上游，返回可被 verify 消费的响应。
    async fn upstream_response(
        status: StatusCode,
        content_type: Option<&'static str>,
        chunks: Vec<&'static [u8]>,
    ) -> reqwest::Response {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("local addr");
        let app = Router::new().route(
            "/",
            get(move || {
                let chunks = chunks.clone();
                async move {
                    let mut builder = Response::builder().status(status);
                    if let Some(content_type) = content_type {
                        builder = builder.header(header::CONTENT_TYPE, content_type);
                    }
                    let body = Body::from_stream(stream::iter(
                        chunks
                            .into_iter()
                            .map(|chunk| Ok::<_, Infallible>(Bytes::from_static(chunk))),
                    ));
                    builder.body(body).expect("build response")
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        reqwest::Client::new()
            .get(format!("http://{addr}/"))
            .send()
            .await
            .expect("test response")
    }

    #[tokio::test]
    async fn html_content_type_is_refused() {
        let mut response = upstream_response(
            StatusCode::OK,
            Some("text/html; charset=utf-8"),
            vec![b"<html><title>challenge</title></html>"],
        )
        .await;

        let error = verify_upstream_sse_head(&mut response)
            .await
            .expect_err("html response is refused");
        assert!(matches!(error, AppError::UpstreamInvalidResponse { .. }));
    }

    #[tokio::test]
    async fn html_body_under_another_content_type_is_refused() {
        let mut response = upstream_response(
            StatusCode::OK,
            Some("text/plain"),
            vec![b"  <!doctype html><html><title>portal</title></html>"],
        )
        .await;

        let error = verify_upstream_sse_head(&mut response)
            .await
            .expect_err("html body is refused");
        match error {
            AppError::UpstreamInvalidResponse { message } => {
                assert!(
                    message.contains("portal"),
                    "diagnostic names the page: {message}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_body_is_refused() {
        let mut response =
            upstream_response(StatusCode::OK, Some("text/event-stream"), vec![]).await;

        let error = verify_upstream_sse_head(&mut response)
            .await
            .expect_err("empty response is refused");
        assert!(matches!(error, AppError::UpstreamInvalidResponse { .. }));
    }

    #[tokio::test]
    async fn sse_head_passes_and_head_is_reassembled() {
        let mut response = upstream_response(
            StatusCode::OK,
            Some("text/event-stream"),
            vec![b"data: first\n\n", b": ping\n\n", b"data: second\n\n"],
        )
        .await;

        let head = verify_upstream_sse_head(&mut response)
            .await
            .expect("sse response passes")
            .expect("head chunk is kept");

        let mut collected = Vec::new();
        let mut stream = upstream_bytes_stream(response, Some(head));
        while let Some(chunk) = stream.next().await {
            collected.extend_from_slice(&chunk.expect("upstream chunk"));
        }
        assert_eq!(
            collected, b"data: first\n\n: ping\n\ndata: second\n\n",
            "no bytes may be dropped between verification and forwarding"
        );
    }
}
