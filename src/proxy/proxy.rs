use http::{Request, Response, Uri};
use log::error;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use threadpool::ThreadPool;
use uuid::{NoContext, Timestamp, Uuid};

use super::connection::{
    format_response, ProxyTcpConnection, ProxyTcpConnectionError, ProxyTcpConnectionsPool,
};
use crate::{gateway::server::Server, proxy::connection::write_http_request};

#[derive(Debug, Clone)]
pub struct Proxy<T> {
    pub address: SocketAddr,
    pub listener: Option<Arc<TcpListener>>,
    pub connections_pool: ProxyTcpConnectionsPool,
    pub thread_pool: ThreadPool,
    pub servers: HashMap<T, Server>,
    pub forward: Vec<ProxyForward>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProxyConfig {
    pub address: SocketAddr,
    pub max_connections: usize,
    pub threads: usize,
}

impl<T> Proxy<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            address: config.address,
            listener: None,
            connections_pool: ProxyTcpConnectionsPool::new(config.max_connections),
            thread_pool: ThreadPool::new(config.threads),
            servers: HashMap::new(),
            forward: Vec::new(),
        }
    }

    pub fn get_forwarder_for_request_by_path(
        &self,
        request: &http::Request<Vec<u8>>,
    ) -> Option<&ProxyForward> {
        for forward in self.forward.iter() {
            if let Some(paths) = forward.match_path.as_ref() {
                for path in paths.iter() {
                    if path.exactly {
                        if path.path.path() == request.uri().path() {
                            return Some(&forward);
                        }
                    } else if path.starts_with {
                        if request.uri().path().starts_with(path.path.path()) {
                            return Some(&forward);
                        }
                    }
                }
            }
        }

        None
    }

    pub fn get_forwarder_for_request_by_all_matched_headers(
        &self,
        request: &http::Request<Vec<u8>>,
    ) -> Option<&ProxyForward> {
        for forward in self.forward.iter() {
            if let Some(headers) = forward.match_headers.as_ref() {
                let all_headers_match = headers
                    .iter()
                    .all(|header| request.headers().get(header).is_some());

                if all_headers_match {
                    return Some(&forward);
                }
            }
        }

        None
    }

    pub fn get_server(&self, key: &T) -> Option<&Server>
    where
        T: std::cmp::Eq + std::hash::Hash,
    {
        self.servers.get(key)
    }

    pub fn add_server(&mut self, key: T, server: Server)
    where
        T: std::cmp::Eq + std::hash::Hash,
    {
        self.servers.insert(key, server);
    }

    pub fn remove_server(&mut self, key: T)
    where
        T: std::cmp::Eq + std::hash::Hash,
    {
        self.servers.remove(&key);
    }

    pub fn get_forward(&self) -> &Vec<ProxyForward> {
        &self.forward
    }

    pub fn add_forward(&mut self, forward: ProxyForward) {
        self.forward.push(forward.clone());
    }

    pub fn forward_conn(
        &self,
        forwarder: &ProxyForward,
        conn: &mut MutexGuard<TcpStream>,
        req: &mut Request<Vec<u8>>,
        req_buffer: &mut [u8],
    ) -> Result<(), ProxyTcpConnectionError> {
        println!("Forwarding request to: {}", forwarder.to.address);
        let mut stream = TcpStream::connect(&forwarder.to.address)
            .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        let _ = stream
            .set_read_timeout(Some(Duration::new(5, 0)))
            .map_err(|_| ProxyTcpConnectionError::InternalServerError);
        let _ = stream
            .set_write_timeout(Some(Duration::new(5, 0)))
            .map_err(|_| ProxyTcpConnectionError::InternalServerError);

        match forwarder.rewrite_to.as_ref() {
            Some(rewrite_to) => {
                if req.uri().path_and_query().is_some() {
                    let queries = req.uri().path_and_query().unwrap().query().unwrap_or("");
                    let full_new_path = format!("{}?{}", rewrite_to.path(), queries);
                    *req.uri_mut() = full_new_path.parse().unwrap_or(req.uri().clone());
                }
            }
            None => (),
        }

        let mut buffer = Vec::new();
        let _ =
            write_http_request(&mut buffer, &req).map_err(|_| ProxyTcpConnectionError::BadRequest);

        stream
            .write_all(&buffer)
            .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        let mut server_response = Vec::new();
        stream
            .read_to_end(&mut server_response)
            .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;
        conn.write_all(&server_response)
            .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        stop_stream(&mut stream).map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        Ok(())
    }

    pub fn listen(&mut self)
    where
        Proxy<T>: Clone,
        T: std::cmp::Eq + std::hash::Hash + Clone + Send + 'static,
    {
        self.listener = match TcpListener::bind(&self.address) {
            Ok(listener) => Some(listener.into()),
            Err(e) => {
                eprintln!("Error binding to address: {}", e);
                return;
            }
        };

        let listener = self.listener.as_ref().unwrap();

        println!("Listening on: {}", self.address);
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let peer_addr = match stream.peer_addr() {
                        Ok(addr) => addr.to_string(),
                        Err(e) => {
                            eprintln!("Error getting peer address: {}", e);
                            continue;
                        }
                    };

                    let safe_stream = Arc::new(Mutex::new(stream));
                    let now = chrono::Utc::now();
                    let ts = Timestamp::from_unix(
                        NoContext,
                        now.timestamp() as u64,
                        now.timestamp_subsec_nanos() as u32,
                    );
                    let connection = ProxyTcpConnection {
                        id: Uuid::new_v7(ts),
                        stream: safe_stream,
                        peer_addr,
                    };

                    match self.connections_pool.add_connection(connection.clone()) {
                        Ok(_) => (),
                        Err(e) => {
                            eprintln!("Error adding connection to pool: {}", e);
                            continue;
                        }
                    }

                    println!("Accepted connection from: {}", connection.peer_addr);
                    let proxy_safe = Arc::new(Mutex::new(self.clone()));
                    self.thread_pool.execute(|| {
                        match super::connection::handle_connection(proxy_safe, connection) {
                            Ok(_) => (),
                            Err(e) => {
                                error!("Error handling connection: {:?}", e);
                            }
                        }
                    })
                }
                Err(e) => {
                    eprintln!("Error accepting connection: {}", e);
                    continue;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProxyForward {
    pub match_headers: Option<Vec<String>>,
    pub match_query: Option<Vec<String>>,
    pub match_path: Option<Vec<ProxyForwardPath>>,
    pub match_method: Option<Vec<String>>,

    pub rewrite_to: Option<Uri>,

    pub to: Server,
}

#[derive(Debug, Clone)]
pub struct ProxyForwardPath {
    pub path: Uri,
    pub exactly: bool,
    pub starts_with: bool,
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

pub fn stop_stream(stream: &mut TcpStream) -> Result<(), ProxyTcpConnectionError> {
    stream
        .shutdown(Shutdown::Both)
        .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;
    Ok(())
}
