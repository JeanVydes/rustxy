use http::Response;

/// # Format Response
/// 
/// Format the response as bytes
/// 
/// # Arguments
/// 
/// * `response` - The response
pub fn format_response(response: Response<String>) -> Vec<u8> {
    let status_line = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status().as_u16(),
        response.status().canonical_reason().unwrap_or("")
    );

    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| format!("{}: {}\r\n", k, v.to_str().unwrap()))
        .collect::<Vec<_>>()
        .join("");

    let body = response.body();

    format!("{}{}\r\n{}", status_line, headers, body).into_bytes()
}
