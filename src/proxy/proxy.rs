use std::{collections::HashMap, io::{Read, Write}, net::{Shutdown, SocketAddr, TcpListener, TcpStream}, sync::{Arc, Mutex, MutexGuard}, time::Duration};
use http::{Request, Uri};
use serde::{Deserialize, Serialize};
use threadpool::ThreadPool;
use uuid::{NoContext, Timestamp, Uuid};

use crate::gateway::server::Server;
use super::connection::{ProxyTcpConnection, ProxyTcpConnectionsPool};

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
    T: Serialize + for<'de> Deserialize<'de>
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

    pub fn get_server_for_request_by_path(&self, request: &http::Request<&str>) -> Option<&Server> {
        for forward in self.forward.iter() {
            if let Some(paths) = forward.match_path.as_ref() {
                for path in paths.iter() {
                    if path.exactly {
                        if path.path.path() == request.uri().path() {
                            return Some(&forward.to);
                        }
                    } else if path.starts_with {
                        if request.uri().path().starts_with(path.path.path()) {
                            return Some(&forward.to);
                        }
                    }
                }
            }
        }

        None
    }

    pub fn get_server_for_request_by_all_matched_headers(&self, request: &http::Request<&str>) -> Option<&Server> {
        for forward in self.forward.iter() {
            if let Some(headers) = forward.match_headers.as_ref() {
                let all_headers_match = headers.iter().all(|header| {
                    request.headers().get(header).is_some()
                });
                
                if all_headers_match {
                    return Some(&forward.to);
                }
            }
        }

        None
    }

    pub fn get_server(&self, key: &T) -> Option<&Server>
    where 
        T: std::cmp::Eq + std::hash::Hash
    {
        self.servers.get(key)
    }

    pub fn add_server(&mut self, key: T, server: Server)
    where 
        T: std::cmp::Eq + std::hash::Hash
    {
        self.servers.insert(key, server);
    }

    pub fn remove_server(&mut self, key: T)
    where 
        T: std::cmp::Eq + std::hash::Hash
    {
        self.servers.remove(&key);
    }

    pub fn get_forward(&self) -> &Vec<ProxyForward> {
        &self.forward
    }

    pub fn add_forward(&mut self, forward: ProxyForward) {
        self.forward.push(forward.clone());
    }

    pub fn forward_conn(&self, to: &Server, conn: &mut MutexGuard<TcpStream>, req: &[u8]) {
        println!("Forwarding request to: {}", to.address);
        match TcpStream::connect(&to.address) {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(Duration::new(5, 0))).unwrap();
                stream.set_write_timeout(Some(Duration::new(5, 0))).unwrap();
                
                if let Err(e) = stream.write_all(req) {
                    eprintln!("Failed to forward request to server: {:?}", e);
                    return;
                }
    
                let mut server_response = Vec::new();
                match stream.read_to_end(&mut server_response) {
                    Ok(_) => {
                        if let Err(e) = conn.write_all(&server_response) {
                            eprintln!("Failed to forward response to client: {:?}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to read response from server: {:?}", e);
                    }
                }
    
                // Shutdown the connection to the server
                if let Err(e) = stream.shutdown(Shutdown::Both) {
                    eprintln!("Failed to shut down the connection to the server: {:?}", e);
                }
            },
            Err(e) => {
                eprintln!("Error connecting to server: {}", e);
                return;
            }
        }
    }

    pub fn listen(&mut self)
    where
        Proxy<T>: Clone,
        T: std::cmp::Eq + std::hash::Hash + Clone + Send + 'static
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
                    let ts = Timestamp::from_unix(NoContext, now.timestamp() as u64, now.timestamp_subsec_nanos() as u32);
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
                        super::connection::handle_connection(proxy_safe, connection);
                    })
                },
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
    
    pub to: Server,
}

#[derive(Debug, Clone)]
pub struct ProxyForwardPath {
    pub path: Uri,
    pub exactly: bool,
    pub starts_with: bool,
}