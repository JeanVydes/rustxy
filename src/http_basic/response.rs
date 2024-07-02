use std::{net::{Shutdown, TcpStream}, sync::MutexGuard};
use std::io::Write;
use http::Response;

use crate::proxy::connection::ProxyTcpConnectionError;

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

pub fn not_found(conn: &mut MutexGuard<TcpStream>) -> Result<(), ProxyTcpConnectionError> {
    let res: Response<String> = Response::builder()
        .status(404)
        .body("Not Found".to_string())
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;
    let formatted_response = format_response(res);
    conn.write_all(&formatted_response)
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

    stop_stream(conn)?;

    Ok(())
}

pub fn unauthorized(conn: &mut MutexGuard<TcpStream>) -> Result<(), ProxyTcpConnectionError> {
    let res: Response<String> = Response::builder()
        .status(401)
        .body("Unauthorized".to_string())
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;
    let formatted_response = format_response(res);
    conn.write_all(&formatted_response)
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

    stop_stream(conn)?;

    Ok(())
}

pub fn internal_server_error(
    conn: &mut MutexGuard<TcpStream>,
) -> Result<(), ProxyTcpConnectionError> {
    let res: Response<String> = Response::builder()
        .status(500)
        .body("Internal Server Error".to_string())
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;
    let formatted_response = format_response(res);
    conn.write_all(&formatted_response)
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

    stop_stream(conn)?;

    Ok(())
}

pub fn bad_request(conn: &mut MutexGuard<TcpStream>) -> Result<(), ProxyTcpConnectionError> {
    let res: Response<String> = Response::builder()
        .status(400)
        .body("Bad Request".to_string())
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;
    let formatted_response = format_response(res);
    conn.write_all(&formatted_response)
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

    stop_stream(conn)?;

    Ok(())
}

pub fn err_response(
    conn: &mut MutexGuard<TcpStream>,
    error: ProxyTcpConnectionError,
) -> Result<(), ProxyTcpConnectionError> {
    let _ = match error {
        ProxyTcpConnectionError::NotFound => not_found(conn),
        ProxyTcpConnectionError::Unauthorized => unauthorized(conn),
        ProxyTcpConnectionError::InternalServerError => internal_server_error(conn),
        ProxyTcpConnectionError::BadRequest => bad_request(conn),
        _ => internal_server_error(conn),
    };

    stop_stream(conn)?;
    
    Ok(())
}

pub fn stop_stream(stream: &mut TcpStream) -> Result<(), ProxyTcpConnectionError> {
    stream
        .shutdown(Shutdown::Both)
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;
    Ok(())
}
