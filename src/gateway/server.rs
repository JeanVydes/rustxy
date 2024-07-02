use std::{collections::HashMap, net::SocketAddr};

use http::{uri::Scheme, Method, Uri};


/// # Server
/// 
/// The server struct
/// 
/// # Fields
/// 
/// * `address` - The address
/// * `accepted_schemes` - The accepted schemes
/// * `endpoints` - The endpoints
#[derive(Debug, Clone)]
pub struct Server {
    pub address: SocketAddr,
    pub accepted_schemes: Vec<Scheme>,
    pub endpoints: HashMap<Uri, ServerEndpoint>,
    pub active_connections: usize,
    pub weight: usize,
}

/// # Server Config
/// 
/// The server config struct
/// 
/// # Fields
/// 
/// * `address` - The address
/// * `accepted_schemes` - The accepted schemes
/// * `endpoints` - The endpoints
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub address: SocketAddr,
    pub accepted_schemes: Vec<Scheme>,
    pub endpoints: HashMap<Uri, ServerEndpoint>,
    pub weight: usize,
}

/// # Server
/// 
/// The implementation of the server
/// 
/// # Methods
/// 
/// * `new` - The server constructor
/// * `get_endpoint` - Get an endpoint
/// * `add_endpoint` - Add an endpoint
/// * `remove_endpoint` - Remove an endpoint
impl Server {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            address: config.address,
            accepted_schemes: config.accepted_schemes,
            endpoints: config.endpoints,
            active_connections: 0,
            weight: config.weight,
        }
    }

    pub fn get_endpoint(&self, uri: &Uri) -> Option<&ServerEndpoint> {
        self.endpoints.get(uri)
    }

    pub fn add_endpoint(&mut self, endpoint: ServerEndpoint) {
        self.endpoints.insert(endpoint.uri.clone(), endpoint);
    }

    pub fn remove_endpoint(&mut self, uri: Uri) {
        self.endpoints.remove(&uri);
    }

    pub fn get_active_connections(&self) -> usize {
        self.active_connections
    }

    pub fn increment_active_connections(&mut self) {
        self.active_connections += 1;
    }

    pub fn decrement_active_connections(&mut self) {
        self.active_connections -= 1;
    }
}

/// # Server Endpoint
/// 
/// The server endpoint struct
/// 
/// # Fields
/// 
/// * `scheme` - The scheme
/// * `uri` - The uri
/// * `method` - The method
/// * `headers` - The headers
#[derive(Debug, Clone)]
pub struct ServerEndpoint {
    pub scheme: Scheme,
    pub uri: Uri,
    pub method: Method,
    pub headers: Vec<ServerEndpointHeader>,
}

/// # Server Endpoint Header Type
/// 
/// The server endpoint header type enum
/// 
/// # Fields
/// 
/// * `In` - The incoming header
/// * `Out` - The outgoing header
#[derive(Debug, Clone, Copy)]
pub enum ServerEndpointHeaderType {
    In,
    Out,
}

/// # Server Endpoint Header
/// 
/// The server endpoint header struct
/// 
/// # Fields
/// 
/// * `r#type` - The header type
/// * `key` - The header key
/// * `value` - The header value
#[derive(Debug, Clone)]
pub struct ServerEndpointHeader {
    pub r#type: ServerEndpointHeaderType,
    pub key: String,
    pub value: String,
}


/// # Server Endpoint
/// 
/// The implementation of the server endpoint
/// 
/// # Methods
/// 
/// * `set_in_header` - Set an incoming header
/// * `set_out_header` - Set an outgoing header
impl ServerEndpoint {
    /// # Set In Header
    /// 
    /// Set an incoming header, that will hit the real server
    /// 
    /// # Arguments
    /// 
    /// * `key` - The header key
    /// * `value` - The header value
    pub fn set_in_header(&mut self, key: String, value: String) {
        self.headers.push(ServerEndpointHeader {
            r#type: ServerEndpointHeaderType::In,
            key,
            value,
        });
    }

    /// # Set Out Header
    /// 
    /// Set an outgoing header, that will be sent to the client
    /// 
    /// # Arguments
    /// 
    /// * `key` - The header key
    /// * `value` - The header value
    pub fn set_out_header(&mut self, key: String, value: String) {
        self.headers.push(ServerEndpointHeader {
            r#type: ServerEndpointHeaderType::Out,
            key,
            value,
        });
    }
}