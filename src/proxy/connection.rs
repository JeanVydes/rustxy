use http::{Method, Request, Response, StatusCode};
use log::error;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::str::from_utf8;
use std::sync::Mutex;
use std::{collections::HashMap, net::TcpStream, sync::Arc};
use uuid::Uuid;

use super::proxy::Proxy;

pub type TcpConnectionID = Uuid;

#[derive(Debug, Clone)]
pub struct ProxyTcpConnection {
    pub id: TcpConnectionID,
    pub stream: Arc<Mutex<TcpStream>>,
    pub peer_addr: String,
}

#[derive(Debug, Clone)]
pub struct ProxyTcpConnectionsPool {
    pub connections: Arc<Mutex<HashMap<TcpConnectionID, Arc<Mutex<TcpStream>>>>>,
    pub max_connections: usize,
}

impl ProxyTcpConnectionsPool {
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            max_connections,
        }
    }

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

pub fn handle_connection<T>(proxy: Arc<Mutex<Proxy<T>>>, connection: ProxyTcpConnection)
where T: Serialize + for<'de> Deserialize<'de>
{
    let mut stream = match connection.stream.lock() {
        Ok(stream) => stream,
        Err(_) => {
            return;
        }
    };

    let mut buffer = [0; 1024];
    match stream.read(&mut buffer) {
        Ok(0) => return,
        Ok(_) => match parse_http_request(&buffer) {
            Ok(req) => {
                match proxy.lock() {
                    Ok(proxy) => {
                        if let Some(server) = proxy.get_server_for_request_by_path(&req) {
                            proxy.forward_conn(&server, &mut stream, &buffer);
                        } else if let Some(server) =
                            proxy.get_server_for_request_by_all_matched_headers(&req)
                        {
                            proxy.forward_conn(&server, &mut stream, &buffer);
                        } else {
                            error!("No server found for request");
                        }
                    }
                    Err(er) => {
                        error!("Failed to lock proxy: {:?}", er);
                    }
                }
            }
            Err(err) => {
                error!("Failed to parse request: {:?}", err);
            }
        },
        Err(_) => {
            error!("Failed to read from stream");
        }
    }

    if let Err(e) = stream.shutdown(std::net::Shutdown::Both) {
        error!("Failed to shut down the stream: {:?}", e);
    }
}

fn parse_http_request(buffer: &[u8]) -> Result<Request<&str>, String> {
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut req = httparse::Request::new(&mut headers);

    let status = match req.parse(buffer) {
        Ok(status) => status,
        Err(e) => {
            return Err(e.to_string());
        }
    };
    
    if status.is_partial() {
        return Err("Request is partial".to_string());
    }

    let method: Method = match req.method {
        Some(t) => match t.parse() {
            Ok(method) => method,
            Err(e) => {
                return Err("Invalid Method".to_string());
            }
        },
        None => {
            return Err("Method not found".to_string());
        }
    };

    let path = match req.path {
        Some(path) => path,
        None => {
            return Err("Path not found".to_string());
        }
    };

    let version = match req.version {
        Some(0) => http::Version::HTTP_10,
        Some(1) => http::Version::HTTP_11,
        _ => http::Version::HTTP_11,
    };

    let uri: String = match path.parse() {
        Ok(uri) => uri,
        Err(e) => {
            return Err(e.to_string());
        }
    };

    let mut builder = Request::builder().method(method).uri(uri).version(version);

    for header in req.headers {
        builder = builder.header(header.name, header.value);
    }

    let header_len = match status {
        httparse::Status::Complete(len) => len,
        _ => {
            return Err("Failed to parse request".to_string());
        }
    };

    let body = &buffer[header_len..];

    let body_str = match from_utf8(body) {
        Ok(body_str) => body_str,
        Err(e) => {
            return Err(e.to_string());
        }
    };

    match builder.body(body_str) {
        Ok(request) => Ok(request),
        Err(e) => Err(e.to_string()),
    }
}

fn format_response(response: Response<String>) -> Vec<u8> {
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
