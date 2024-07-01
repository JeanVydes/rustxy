use log::error;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::Mutex;
use std::{collections::HashMap, net::TcpStream, sync::Arc};
use uuid::Uuid;

use crate::http_basic::request::parse_http_request;

use super::proxy::{internal_server_error, stop_stream, Proxy, ProxyForward};

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
    pub fn add_connection(&mut self, connection: Arc<ProxyTcpConnection>) -> Result<(), String> {
        match self.connections.lock() {
            Ok(mut connections) => {
                if connections.contains_key(&connection.id) {
                    return Err("Connection already exists".to_string());
                }

                if connections.len() >= self.max_connections {
                    return Err("Max connections reached".to_string());
                }

                connections.insert(connection.id, connection.stream.clone());
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
                error!("Failed to lock connections: {:?}", e);
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
    connection: Arc<ProxyTcpConnection>,
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

    let max_buffer_size = match proxy.lock() {
        Ok(proxy) => proxy.max_buffer_size,
        Err(_) => {
            return Err(ProxyTcpConnectionError::InternalServerError);
        }
    };

    // Read the buffer
    let mut buffer = vec![0; max_buffer_size];
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

    let selected_forwarder: Option<&ProxyForward>;
    // Check if request hit path matching
    if let Some(forwarder) = proxy.get_forwarder_for_request_by_path(&req) {    
        selected_forwarder = Some(forwarder);
    // Check if request hit all matched headers matching
    } else if let Some(forwarder) = proxy.get_forwarder_for_request_by_all_matched_headers(&req) {
        selected_forwarder = Some(forwarder);
    } else {
        // Request does not match any forwarder
        return Err(ProxyTcpConnectionError::NotFound);
    }

    match selected_forwarder {
        Some(forwarder) => {
            match proxy
                .forward_conn(&forwarder, &mut stream, &mut req)
                .map_err(|e| e)
            {
                Ok(server) => {
                    match server.lock() {
                        Ok(mut server) => {
                            server.decrement_active_connections();
                        }
                        Err(e) => {
                            error!("Failed to lock the server: {}", e);
                        }
                    }
                }
                Err((err, server)) => {
                    match server {
                        Some(server) => match server.lock() {
                            Ok(mut server) => {
                                server.decrement_active_connections();
                            }
                            Err(e) => {
                                error!("Failed to lock the server: {}", e);
                            }
                        },
                        None => {
                            error!("Failed to forward request");
                        }
                    }

                    let _ = internal_server_error(&mut stream);

                    return Err(err);
                }
            }
        }
        None => {
            return Err(ProxyTcpConnectionError::NotFound);
        }
    }

    // Stop the stream
    stop_stream(&mut stream).map_err(|_| ProxyTcpConnectionError::InternalServerError)
}
