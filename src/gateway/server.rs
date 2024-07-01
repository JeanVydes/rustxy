use std::{collections::HashMap, net::SocketAddr};

use http::{Method, Uri};

#[derive(Debug, Clone, Copy)]
pub enum Schemes {
    Http,
    Https,
    Websocket,
}

#[derive(Debug, Clone)]
pub struct Server {
    pub address: SocketAddr,
    pub accepted_schemes: Vec<Schemes>,
    pub endpoints: HashMap<Uri, ServerEndpoint>
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub address: SocketAddr,
    pub accepted_schemes: Vec<Schemes>,
    pub endpoints: HashMap<Uri, ServerEndpoint>
}

impl Server {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            address: config.address,
            accepted_schemes: config.accepted_schemes,
            endpoints: config.endpoints,
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
}

#[derive(Debug, Clone)]
pub struct ServerEndpoint {
    pub scheme: Schemes,
    pub uri: Uri,
    pub method: Method,
    pub headers: Vec<ServerEndpointHeader>,
}

#[derive(Debug, Clone, Copy)]
pub enum ServerEndpointHeaderType {
    In,
    Out,
}

#[derive(Debug, Clone)]
pub struct ServerEndpointHeader {
    pub r#type: ServerEndpointHeaderType,
    pub key: String,
    pub value: String,
}

impl ServerEndpoint {
    pub fn set_in_header(&mut self, key: String, value: String) {
        self.headers.push(ServerEndpointHeader {
            r#type: ServerEndpointHeaderType::In,
            key,
            value,
        });
    }

    pub fn set_out_header(&mut self, key: String, value: String) {
        self.headers.push(ServerEndpointHeader {
            r#type: ServerEndpointHeaderType::Out,
            key,
            value,
        });
    }
}