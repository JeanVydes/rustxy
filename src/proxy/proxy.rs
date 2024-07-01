use http::{Request, Response, Uri};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use threadpool::ThreadPool;
use uuid::{NoContext, Timestamp, Uuid};

use super::connection::{
    ProxyTcpConnection, ProxyTcpConnectionError, ProxyTcpConnectionsPool,
};
use crate::{gateway::server::Server, http_basic::{request::write_http_request, response::format_response}};

/// # Proxy
///
/// This struct represents a Proxy.
///
/// ## Fields
///
/// * `address` - A SocketAddr.
/// * `listener` - A TcpListener.
/// * `connections_pool` - A ProxyTcpConnectionsPool.
/// * `thread_pool` - A ThreadPool.
/// * `servers` - A HashMap of type T and Server.
/// * `forward` - A Vec of ProxyForward.
#[derive(Debug, Clone)]
pub struct Proxy<T> {
    pub address: SocketAddr,
    pub listener: Option<Arc<TcpListener>>,
    pub connections_pool: Arc<Mutex<ProxyTcpConnectionsPool>>,
    pub thread_pool: ThreadPool,
    pub servers: HashMap<T, Arc<Mutex<Server>>>,
    pub forward: Vec<ProxyForward>,
    pub load_balancer: Option<CloneableFn>,
}

#[derive(Clone)]
pub struct CloneableFn(
    Arc<dyn Fn(Vec<Arc<Mutex<Server>>>) -> Arc<Mutex<Server>> + Send + Sync + 'static>,
);

impl CloneableFn {
    fn new<F>(f: F) -> Self
    where
        F: Fn(Vec<Arc<Mutex<Server>>>) -> Arc<Mutex<Server>> + Send + Sync + 'static,
    {
        CloneableFn(Arc::new(f))
    }
}

impl fmt::Debug for CloneableFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Closure")
    }
}

/// # Proxy Config
///
/// This struct contains the configuration for the Proxy.
///
/// ## Fields
///
/// * `address` - A SocketAddr.
/// * `max_connections` - Max parallel connections.
/// * `threads` - Threads to use.
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
    /// # New Proxy
    ///
    /// This function will create a new Proxy.
    ///
    /// ## Arguments
    ///
    /// * `config` - A ProxyConfig.
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            address: config.address,
            listener: None,
            connections_pool: Arc::new(Mutex::new(ProxyTcpConnectionsPool::new(config.max_connections))),
            thread_pool: ThreadPool::new(config.threads),
            servers: HashMap::new(),
            forward: Vec::new(),
            load_balancer: None,
        }
    }

    /// # Get Forwarder for Request by Method
    ///
    /// This function will return the forwarder that matches the method of the request.
    /// The function will iterate over all forwarders and check if the method matches the request method.
    ///
    /// ## Arguments
    ///
    /// * `request` - A reference to a http::Request<Vec<u8>>.
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

    /// # Get Forwarder for Request by All Matched Headers
    ///
    /// This function will return the forwarder that matches all headers of the request.
    /// The function will iterate over all forwarders and check if all headers are present in the request.
    /// If all headers are present, it will return the forwarder.
    /// If not all headers are present, it will continue to the next forwarder.
    ///
    /// ## Arguments
    ///
    /// * `request` - A reference to a http::Request<Vec<u8>>.
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

    /// # Get Server
    ///
    /// This function will return a reference to a Server by key.
    ///
    /// ## Arguments
    ///
    /// * `key` - A key of type T.
    pub fn get_server(&self, key: &T) -> Option<&Arc<Mutex<Server>>>
    where
        T: std::cmp::Eq + std::hash::Hash,
    {
        self.servers.get(key)
    }

    /// # Add Server
    ///
    /// This function will add a Server to the Proxy.
    ///
    /// ## Arguments
    ///
    /// * `key` - A key of type T.
    /// * `server` - A Server.
    pub fn add_server(&mut self, key: T, server: Arc<Mutex<Server>>)
    where
        T: std::cmp::Eq + std::hash::Hash,
    {
        self.servers.insert(key, server);
    }

    /// # Remove Server
    ///     
    /// This function will remove a Server from the Proxy by key.
    ///
    /// ## Arguments
    ///
    /// * `key` - A key of type T.
    pub fn remove_server(&mut self, key: T)
    where
        T: std::cmp::Eq + std::hash::Hash,
    {
        self.servers.remove(&key);
    }

    /// # Get Forwards
    ///
    /// ¿What is a forward?
    ///
    /// A forward is a ProxyForward struct that contains the information to forward a request to a Server.
    ///
    /// This function will return a reference to the forward Vec.
    pub fn get_forwards(&self) -> &Vec<ProxyForward> {
        &self.forward
    }

    /// # Add Forward
    ///
    /// This function will add a ProxyForward to the forward Vec.
    ///
    /// ## Arguments
    ///
    /// * `forward` - A ProxyForward.
    pub fn add_forward(&mut self, forward: ProxyForward) {
        self.forward.push(forward.clone());
    }

    pub fn set_load_balancer<K>(&mut self, load_balancer: K)
    where
        K: Fn(Vec<Arc<Mutex<Server>>>) -> Arc<Mutex<Server>> + Send + Sync + 'static,
    {
        self.load_balancer = Some(CloneableFn::new(load_balancer));
    }

    /// # Forward Connection
    ///
    /// This function will forward a connection to a Server.
    ///
    /// ## Arguments
    ///
    /// * `forwarder` - A reference to a ProxyForward.
    /// * `conn` - A mutable reference to a MutexGuard<TcpStream>.
    /// * `req` - A mutable reference to a Request<Vec<u8>>.
    pub fn forward_conn(
        &self,
        forwarder: &ProxyForward,
        conn: &mut MutexGuard<TcpStream>,
        req: &mut Request<Vec<u8>>,
    ) -> Result<(), ProxyTcpConnectionError> {
        // we already have retrieve the matching server/s, but now, we have to select one, based on the load balancer, if any, or just the first one
        let selected_server_mutex = match self.load_balancer {
            // use the configured load balancer
            Some(ref lb) => {
                let servers = forwarder.to.clone();
                lb.0(servers)
            }
            // if no load balancer is set, just use the first one
            None => match forwarder.to.first().as_ref() {
                Some(&server) => server.clone(),
                None => return Err(ProxyTcpConnectionError::InternalServerError),
            },
        };

        let selected_server = match selected_server_mutex.lock() {
            Ok(server) => server,
            Err(e) => {
                error!("Error getting server: {:?}", e);
                return Err(ProxyTcpConnectionError::InternalServerError);
            }
        };

        // Connect to Server address
        let mut stream = TcpStream::connect(selected_server.address)
            .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        // Set timeouts
        let _ = stream
            .set_read_timeout(Some(Duration::new(5, 0)))
            .map_err(|_| ProxyTcpConnectionError::InternalServerError);
        let _ = stream
            .set_write_timeout(Some(Duration::new(5, 0)))
            .map_err(|_| ProxyTcpConnectionError::InternalServerError);

        // Rewrite request path
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

        // Write request again to buffer
        let mut buffer = Vec::new();
        let _ =
            write_http_request(&mut buffer, &req).map_err(|_| ProxyTcpConnectionError::BadRequest);

        stream
            .write_all(&buffer)
            .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        // Read response from server
        let mut server_response = Vec::new();
        stream
            .read_to_end(&mut server_response)
            .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        // Write response to client
        conn.write_all(&server_response)
            .map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        // Stop the stream
        stop_stream(&mut stream).map_err(|_| ProxyTcpConnectionError::InternalServerError)?;

        Ok(())
    }

    /// # Listen
    ///
    /// This function will listen for incoming connections.
    ///
    /// The function will bind to the address and listen for incoming connections.
    pub fn listen(&mut self)
    where
        Proxy<T>: Clone + Send + 'static,
        T: std::cmp::Eq + std::hash::Hash + Clone + Send + 'static,
    {
        // Bind to address
        self.listener = match TcpListener::bind(&self.address) {
            Ok(listener) => Some(listener.into()),
            Err(e) => {
                error!("Error binding to address: {}", e);
                return;
            }
        };

        // Clone the proxy to be able to pass it to the thread
        let listener = match self.listener.as_ref() {
            Some(listener) => listener.clone(),
            None => {
                error!("Error getting listener");
                return;
            }
        };

        info!("Listening on: {}", self.address);
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    // Get peer address
                    let peer_addr = match stream.peer_addr() {
                        Ok(addr) => addr.to_string(),
                        Err(e) => {
                            error!("Error getting peer address: {}", e);
                            continue;
                        }
                    };

                    // Clone the stream to be able to pass it to the thread
                    let safe_stream = Arc::new(Mutex::new(stream));
                    let now = chrono::Utc::now();
                    // Create timestamp for UUID
                    let ts = Timestamp::from_unix(
                        NoContext,
                        now.timestamp() as u64,
                        now.timestamp_subsec_nanos() as u32,
                    );

                    // Create connection
                    let connection = Arc::new(ProxyTcpConnection {
                        id: Uuid::new_v7(ts),
                        stream: safe_stream,
                        peer_addr,
                    });

                    // Add connection to pool
                    // First lock the pool, then add the connection
                    match self.connections_pool.lock() {
                        Ok(mut connection_pool) => {
                            match connection_pool.add_connection(connection.clone()) {
                                Ok(_) => (),
                                Err(e) => {
                                    error!("Error adding connection to pool: {:?}", e);
                                    continue;
                                }
                            }
                        },
                        Err(e) => {
                            error!("Error adding connection to pool: {:?}", e);
                            continue;
                        }
                    }

                    // Clone the proxy to be able to pass it to the thread
                    let proxy_safe = Arc::new(Mutex::new(self.clone()));
                    self.thread_pool.execute(move || {
                        // Handle connection
                        match super::connection::handle_connection(proxy_safe, connection) {
                            Ok(_) => (),
                            Err(e) => {
                                error!("Error handling connection: {:?}", e);
                            }
                        }
                    })
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
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

    pub to: Vec<Arc<Mutex<Server>>>,
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
