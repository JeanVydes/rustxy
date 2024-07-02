use std::io::Write;

use http::{Method, Request};
use log::error;

use crate::proxy::connection::ProxyTcpConnectionError;

/// # Parse HTTP Request
/// 
/// Parse the http request
/// 
/// # Arguments
/// 
/// * `buffer` - The buffer to parse
pub fn parse_http_request(buffer: &[u8]) -> Result<Request<Vec<u8>>, ProxyTcpConnectionError> {
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut req = httparse::Request::new(&mut headers);

    // Parse the request
    let status = match req.parse(buffer) {
        Ok(status) => status,
        Err(e) => {
            error!("Failed to parse request: {}", e);
            return Err(ProxyTcpConnectionError::BadRequest);
        }
    };

    // Check if the request is partial
    if status.is_partial() {
        error!("Failed to parse request");
        return Err(ProxyTcpConnectionError::BadRequest);
    }

    // Get the method, path and version
    let method: Method = match req.method {
        Some(t) => match t.parse() {
            Ok(method) => method,
            Err(e) => {
                error!("Failed to parse method: {}", e);
                return Err(ProxyTcpConnectionError::MethodNotAllowed);
            }
        },
        None => {
            error!("Failed to parse method");
            return Err(ProxyTcpConnectionError::BadRequest);
        }
    };

    // Get the path
    let path = match req.path {
        Some(path) => path,
        None => {
            error!("Failed to parse path");
            return Err(ProxyTcpConnectionError::BadRequest);
        }
    };

    // Get the http version
    let version = match req.version {
        Some(0) => http::Version::HTTP_10,
        Some(1) => http::Version::HTTP_11,
        _ => http::Version::HTTP_11,
    };

    // Parse the uri
    let uri: String = match path.parse() {
        Ok(uri) => uri,
        Err(e) => {
            error!("Failed to parse uri: {}", e);
            return Err(ProxyTcpConnectionError::BadRequest);
        }
    };
    
    // Create the request
    let mut builder = Request::builder().method(method).uri(uri).version(version);

    // Add the headers
    for header in req.headers {
        builder = builder.header(header.name, header.value);
    }

    // Get the header length
    let header_len = match status {
        httparse::Status::Complete(len) => len,
        _ => {
            error!("Failed to parse headers");
            return Err(ProxyTcpConnectionError::BadRequest);
        }
    };

    // Build the request
    match builder.body(buffer[header_len..].to_vec()) {
        Ok(request) => Ok(request),
        Err(_) => Err(ProxyTcpConnectionError::BadRequest),
    }
}


/// # Write HTTP Request
/// 
/// Write the http request to the writer as bytes
/// 
/// # Arguments
/// 
/// * `writer` - The writer
/// * `req` - The request
pub fn write_http_request<W: Write>(writer: &mut W, req: &Request<Vec<u8>>) -> std::io::Result<()> {
    // Write the request line
    write!(
        writer,
        "{} {} {:?}\r\n",
        req.method(),
        req.uri(),
        req.version()
    )?;

    // Write the headers
    for (name, value) in req.headers() {
        write!(writer, "{}: {}\r\n", name, value.to_str().unwrap())?;
    }

    // End headers section
    write!(writer, "\r\n")?;

    // Write the body
    writer.write_all(req.body())?;

    Ok(())
}