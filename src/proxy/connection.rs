use http::{Method, Request, Response};
use log::error;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::Mutex;
use std::{collections::HashMap, net::TcpStream, sync::Arc};
use uuid::Uuid;

use super::proxy::{stop_stream, Proxy};

#[derive(Debug)]
pub enum ProxyTcpConnectionError {
    Error(String),
    InternalServerError,
    NotFound,
    Unauthorized,
    Forbidden,
    BadRequest,
    MethodNotAllowed,
}

/// The type of the connection id for the tcp connection is a UUID
pub type TcpConnectionID = Uuid;

/// # Proxy TCP Connection
/// 
/// The proxy tcp connection struct
/// 
/// # Fields
/// 
/// * `id` - The connection id
/// * `stream` - The connection stream
/// * `peer_addr` - The peer address
#[derive(Debug, Clone)]
pub struct ProxyTcpConnection {
    pub id: TcpConnectionID,
    pub stream: Arc<Mutex<TcpStream>>,
    pub peer_addr: String,
}

/// # Proxy TCP Connections Pool
///     
/// The proxy tcp connections pool struct
/// 
/// # Fields
/// 
/// * `connections` - The connections
/// * `max_connections` - The maximum number of connections
#[derive(Debug, Clone)]
pub struct ProxyTcpConnectionsPool {
    pub connections: Arc<Mutex<HashMap<TcpConnectionID, Arc<Mutex<TcpStream>>>>>,
    pub max_connections: usize,
}

/// # Proxy TCP Connections Pool
/// 
/// The implementation of the proxy tcp connections pool
/// 
/// # Methods
/// 
/// * `new` - Create a new connections pool
/// * `add_connection` - Add a connection to the pool
/// * `remove_connection` - Remove a connection from the pool
impl ProxyTcpConnectionsPool {
    /// # New Connections Pool
    /// 
    /// Create a new connections pool
    /// 
    /// # Arguments
    /// 
    /// * `max_connections` - The maximum number of connections
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            max_connections,
        }
    }

    /// # Add Connection
    /// 
    /// Add a connection to the pool
    /// 
    /// # Arguments
    /// 
    /// * `connection` - The connection to add
    pub fn add_connection(&mut self, connection: ProxyTcpConnection) -> Result<(), String> {
        match self.connections.lock() {
            Ok(mut connections) => {
                if connections.contains_key(&connection.id) {
                    return Err("Connection already exists".to_string());
                }

                if connections.len() >= self.max_connections {
                    return Err("Max connections reached".to_string());
                }

                connections.insert(connection.id, connection.stream);
            }
            Err(e) => {
                return Err(format!("Failed to lock connections: {:?}", e));
            }
        };

        Ok(())
    }

    /// # Remove Connection
    /// 
    /// Remove a connection from the pool
    /// 
    /// # Arguments
    /// 
    /// * `connection_id` - The connection id to remove
    pub fn remove_connection(&mut self, connection_id: TcpConnectionID) {
        match self.connections.lock() {
            Ok(mut connections) => {
                connections.remove(&connection_id);
            }
            Err(e) => {
                eprintln!("Failed to lock connections: {:?}", e);
            }
        };
    }
}


/// Handle the connection
/// 
/// # Arguments
/// 
/// * `proxy` - The proxy instance
/// * `connection` - The connection to handle
pub fn handle_connection<T>(
    proxy: Arc<Mutex<Proxy<T>>>,
    connection: ProxyTcpConnection,
) -> Result<(), ProxyTcpConnectionError>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    // Get the stream
    let mut stream = match connection.stream.lock() {
        Ok(stream) => stream,
        Err(_) => {
            return Err(ProxyTcpConnectionError::InternalServerError);
        }
    };

    // Read the buffer
    let mut buffer = [0; 1024];
    let buffer_readed = stream
        .read(&mut buffer)
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

    // Check if the buffer is empty
    if buffer_readed == 0 {
        error!("Failed to read from stream 0 buffer size");
        return Err(ProxyTcpConnectionError::BadRequest);
    }

    // Parse the http request into http::Request<Vec<u8>>
    let mut req = parse_http_request(&buffer).map_err(|e| e)?;

    // Get the proxy instance
    let proxy = proxy
        .lock()
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

    // Check if request hit path matching
    if let Some(forwarder) = proxy.get_forwarder_for_request_by_path(&req) {
        proxy.forward_conn(&forwarder, &mut stream, &mut req).map_err(|e| e)?;
    
    // Check if request hit all matched headers matching
    } else if let Some(forwarder) = proxy.get_forwarder_for_request_by_all_matched_headers(&req) {
        proxy.forward_conn(&forwarder, &mut stream, &mut req).map_err(|e| e)?;
    } else {
        // Request does not match any forwarder
        return Err(ProxyTcpConnectionError::NotFound);
    }

    // Stop the stream
    stop_stream(&mut stream).map_err(|_| ProxyTcpConnectionError::InternalServerError)
}

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
