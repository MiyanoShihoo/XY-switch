use std::error::Error;

/// Keep the transport cause without including request URLs or credentials.
pub(crate) fn describe_error(error: reqwest::Error) -> String {
    let hint = if error.is_timeout() {
        "连接超时，请检查网络、系统代理及上游服务"
    } else if error.is_connect() {
        "连接失败，请检查 DNS、TLS 证书及 Windows 系统代理"
    } else {
        "网络请求失败"
    };
    let error = error.without_url();
    let mut details = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !details.contains(&text) {
            details.push(text);
        }
        source = cause.source();
        if details.len() >= 6 {
            break;
        }
    }
    format!("{hint}：{}", details.join(" → "))
}
