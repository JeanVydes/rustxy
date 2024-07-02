use http::Request;
use log::{error, info};
use native_tls::{Identity, TlsAcceptor};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    time::Duration,
};
use threadpool::ThreadPool;
use uuid::{NoContext, Timestamp, Uuid};

use super::{
    connection::{ProxyTcpConnection, ProxyTcpConnectionError, ProxyTcpConnectionsPool},
    forwarder::ProxyForward, load_balancer::LoadBalancer,
};
use crate::{
    gateway::server::Server,
    http_basic::{request::write_http_request, response::stop_stream, rewrite_path},
};

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
#[derive(Clone)]
pub struct Proxy {
    pub address: SocketAddr,
    pub listener: Option<Arc<TcpListener>>,
    pub connections_pool: Arc<Mutex<ProxyTcpConnectionsPool>>,
    pub thread_pool: ThreadPool,
    pub servers: Arc<Mutex<HashMap<Uuid, Server>>>,
    pub forwards: Vec<ProxyForward>,
    pub load_balancer: Option<LoadBalancer>,
    pub max_buffer_size: usize,
    pub shared_state: Arc<Mutex<ProxyState>>,
    pub tls_identity: Arc<Identity>,
    pub tls_acceptor: Arc<TlsAcceptor>,
}

#[derive(Debug, Clone)]
pub struct ProxyState {
    // Forwarder -> Round Robin Index
    pub round_robin_index: HashMap<Uuid, u32>,
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
#[derive(Clone)]
pub struct ProxyConfig {
    pub address: SocketAddr,
    pub max_connections: usize,
    pub threads: usize,
    pub max_buffer_size: usize,
    pub tls_identity: Identity,
}

impl Proxy {
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
            connections_pool: Arc::new(Mutex::new(ProxyTcpConnectionsPool::new(
                config.max_connections,
            ))),
            thread_pool: ThreadPool::new(config.threads),
            servers: Arc::new(Mutex::new(HashMap::new())),
            forwards: Vec::new(),
            load_balancer: None,
            max_buffer_size: config.max_buffer_size,
            shared_state: Arc::new(Mutex::new(ProxyState {
                round_robin_index: HashMap::new(),
            })),
            tls_identity: Arc::new(config.tls_identity.clone()),
            tls_acceptor: Arc::new(TlsAcceptor::new(config.tls_identity).unwrap()),
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
        for forward in self.forwards.iter() {
            if let Some(paths) = forward.match_path.as_ref() {
                for path in paths.iter() {
                    if path.exactly {
                        if path.path.path() == request.uri().path() {
                            return Some(forward);
                        }
                    } else if path.starts_with {
                        if request.uri().path().starts_with(path.path.path()) {
                            return Some(forward);
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
        for forward in self.forwards.iter() {
            if let Some(headers) = forward.match_headers.as_ref() {
                let all_headers_match = headers
                    .iter()
                    .all(|header| request.headers().get(header).is_some());

                if all_headers_match {
                    return Some(forward);
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
    pub fn get_server(&self, key: Uuid) -> Option<Server> {
        match self.servers.lock() {
            Ok(servers) => match servers.get(&key) {
                Some(server) => Some(server.clone()),
                None => None,
            },
            Err(e) => {
                error!("Error getting server: {:?}", e);
                None
            }
        }
    }

    /// # Add Server
    ///
    /// This function will add a Server to the Proxy.
    ///
    /// ## Arguments
    ///
    /// * `key` - A key of type T.
    /// * `server` - A Server.
    pub fn add_server(&mut self, key: Uuid, server: Server) {
        match self.servers.lock() {
            Ok(mut servers) => {
                servers.insert(key, server);
            }
            Err(e) => {
                error!("Error adding server: {:?}", e);
            }
        }
    }

    /// # Remove Server
    ///     
    /// This function will remove a Server from the Proxy by key.
    ///
    /// ## Arguments
    ///
    /// * `key` - A key of type T.
    pub fn remove_server(&mut self, key: Uuid) {
        match self.servers.lock() {
            Ok(mut servers) => {
                servers.remove(&key);
            }
            Err(e) => {
                error!("Error removing server: {:?}", e);
            }
        }
    }

    /// # Get Forwards
    ///
    /// ¿What is a forward?
    ///
    /// A forward is a ProxyForward struct that contains the information to forward a request to a Server.
    ///
    /// This function will return a reference to the forward Vec.
    pub fn get_forwards(&self) -> &Vec<ProxyForward> {
        &self.forwards
    }

    /// # Add Forward
    ///
    /// This function will add a ProxyForward to the forward Vec.
    ///
    /// ## Arguments
    ///
    /// * `forward` - A ProxyForward.
    pub fn add_forward(&mut self, forward: ProxyForward) {
        self.forwards.push(forward.clone());
    }

    /// # Set Load Balancer
    ///
    /// This function will set the load balancer for the Proxy.
    ///
    /// /// ## Arguments
    ///
    /// * `load_balancer` - A closure that receives the internal state of the proxy, the forwarder and the preselected servers and returns a Server.
    ///
    /// ## Example
    ///
    /// ```rust
    /// my_proxy.set_load_balancer(|proxy_state, forwarder, preselected_servers| {
    ///     // proxy_state: internal state of the proxy
    ///     // forwarder: the forwarder for the request, selected from matching certain criteria like path, methods or other things
    ///     // preselected_servers: the servers that are preselected for the forwarder

    ///     // You have to return a server to process the request
    ///     // Check in src/proxy/load_balancer.rs for pre-built load balancers
    ///     preselected_servers[0].clone()
    /// });
    /// ```
    pub fn set_load_balancer<K>(&mut self, load_balancer: K)
    where
        K: Fn(Arc<Mutex<ProxyState>>, &ProxyForward, Vec<Server>) -> Server + Send + Sync + 'static,
    {
        self.load_balancer = Some(LoadBalancer::new(load_balancer));
    }

    /// # Add Connection to Server
    ///
    /// This function will add a connection (the count) to a Server.
    ///
    /// ## Arguments
    ///
    /// * `server_id` - A Uuid.
    pub fn add_connection_count_to_server(&self, server_id: Uuid) -> Result<(), ()> {
        match self.servers.lock() {
            Ok(mut servers) => {
                match servers.get_mut(&server_id) {
                    Some(server) => {
                        // Increment active connections
                        server.increment_active_connections();
                        Ok(())
                    }
                    None => Err(()),
                }
            }
            Err(e) => {
                error!("Error adding connection to server: {:?}", e);
                Err(())
            }
        }
    }

    /// # Remove Connection from Server
    ///
    /// This function will remove a connection (the count) from a Server.
    ///
    /// ## Arguments
    ///
    /// * `server_id` - A Uuid.
    pub fn remove_connection_count_from_server(&self, server_id: Uuid) -> Result<(), ()> {
        match self.servers.lock() {
            Ok(mut servers) => match servers.get_mut(&server_id) {
                Some(server) => {
                    server.decrement_active_connections();
                    Ok(())
                }
                None => Err(()),
            },
            Err(e) => {
                error!("Error removing connection from server: {:?}", e);
                Err(())
            }
        }
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
        proxy_state: Arc<Mutex<ProxyState>>,
        forwarder: &ProxyForward,
        nottls_client_stream: &mut TcpStream,
        req: &mut Request<Vec<u8>>,
    ) -> Result<Server, (ProxyTcpConnectionError, Option<Server>)> {
        let selected_server = match self.get_selected_server(forwarder, proxy_state.clone()) {
            Ok(server) => server,
            Err((e, server)) => return Err((e, server)),
        };

        // Connect to Server address
        let mut server_stream = match TcpStream::connect(selected_server.address.clone()) {
            Ok(stream) => stream,
            Err(e) => {
                error!("Error connecting to server: {}", e);
                return Err((ProxyTcpConnectionError::InternalServerError, None));
            }
        };

        self.set_server_stream_timeout(&mut server_stream)
            .map_err(|e| {
                error!("Error setting server stream timeout: {:?}", e);
                (
                    ProxyTcpConnectionError::InternalServerError,
                    Some(selected_server.clone()),
                )
            })?;

        let middlewares = match forwarder.middlewares.lock() {
            Ok(middlewares) => middlewares,
            Err(e) => {
                error!("Error getting middlewares: {:?}", e);
                return Err((ProxyTcpConnectionError::InternalServerError, None));
            }
        };

        // Rewrite request path
        if let Some(rewrite_to) = forwarder.rewrite_path.as_ref() {
            if req.uri().path_and_query().is_some() {
                let new_path = rewrite_path(req.uri(), rewrite_to, forwarder);
                *req.uri_mut() = new_path.parse().unwrap_or(req.uri().clone());
            }
        }

        for middleware in middlewares.values() {
            *req = middleware.0(proxy_state.clone(), forwarder, selected_server.clone(), req.clone());
        }

        // Write request again to buffer
        let mut buffer = Vec::new();
        write_http_request(&mut buffer, &req).map_err(|e| {
            error!("Error writing to buffer: {}", e);
            (
                ProxyTcpConnectionError::InternalServerError,
                Some(selected_server.clone()),
            )
        })?;

        // Write request to server
        server_stream.write_all(&buffer).map_err(|e| {
            error!("Error writing to stream: {}", e);
            (
                ProxyTcpConnectionError::InternalServerError,
                Some(selected_server.clone()),
            )
        })?;

        // Read response from server
        let mut server_response = Vec::new();
        server_stream
            .read_to_end(&mut server_response)
            .map_err(|e| {
                error!("Error reading from stream: {}", e);
                (
                    ProxyTcpConnectionError::InternalServerError,
                    Some(selected_server.clone()),
                )
            })?;

        // Write response from server to client
        nottls_client_stream
            .write_all(&server_response)
            .map_err(|e| {
                error!("Error writing to stream: {}", e);
                (
                    ProxyTcpConnectionError::InternalServerError,
                    Some(selected_server.clone()),
                )
            })?;

        // Stop the stream with the proxied server
        stop_stream(&mut server_stream).map_err(|e| {
            error!("Error stopping stream: {:?}", e);
            (
                ProxyTcpConnectionError::InternalServerError,
                Some(selected_server.clone()),
            )
        })?;

        let _ = self.remove_connection_count_from_server(selected_server.id.clone());
        Ok(selected_server.clone())
    }

    pub fn get_selected_server(
        &self,
        forwarder: &ProxyForward,
        proxy_state: Arc<Mutex<ProxyState>>,
    ) -> Result<Server, (ProxyTcpConnectionError, Option<Server>)> {
        // we already have retrieve the matching server/s, but now, we have to select one, based on the load balancer, if any, or just the first one
        let selected_server = match self.load_balancer {
            // use the configured load balancer
            Some(ref lb) => {
                let servers_ids = forwarder.to.clone();
                let mut servers: Vec<Server> = Vec::new();

                for server_id in servers_ids.iter() {
                    match self.get_server(*server_id) {
                        Some(server) => servers.push(server),
                        None => continue,
                    };
                }

                lb.0(proxy_state.clone(), forwarder, servers)
            }
            // if no load balancer is set, just use the first one
            None => {
                let server_id = forwarder.to.first().unwrap();
                let server = match self.get_server(*server_id) {
                    Some(server) => server,
                    None => {
                        error!("Error getting server");
                        return Err((ProxyTcpConnectionError::InternalServerError, None));
                    }
                };
                server
            }
        };

        if selected_server.active_connections >= selected_server.max_connections {
            return Err((
                ProxyTcpConnectionError::TooManyConnections,
                Some(selected_server.clone()),
            ));
        }

        match self.add_connection_count_to_server(selected_server.id.clone()) {
            Ok(_) => (),
            Err(e) => {
                error!("Error adding connection to server: {:?}", e);
                return Err((
                    ProxyTcpConnectionError::InternalServerError,
                    Some(selected_server.clone()),
                ));
            }
        }

        Ok(selected_server)
    }

    pub fn set_server_stream_timeout(
        &self,
        stream: &mut TcpStream,
    ) -> Result<(), ProxyTcpConnectionError> {
        match stream.set_read_timeout(Some(Duration::from_secs(5))) {
            Ok(_) => (),
            Err(e) => {
                error!("Error setting read timeout: {}", e);
                return Err(ProxyTcpConnectionError::InternalServerError);
            }
        };

        match stream.set_write_timeout(Some(Duration::from_secs(5))) {
            Ok(_) => (),
            Err(e) => {
                error!("Error setting write timeout: {}", e);
                return Err(ProxyTcpConnectionError::InternalServerError);
            }
        };

        Ok(())
    }

    /// # Listen
    ///
    /// This function will listen for incoming connections.
    ///
    /// The function will bind to the address and listen for incoming connections.
    pub fn listen(&mut self)
    where
        Proxy: Clone + 'static,
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
                        }
                        Err(e) => {
                            error!("Error adding connection to pool: {:?}", e);
                            continue;
                        }
                    }

                    // Clone the proxy to be able to pass it to the thread
                    let proxy_safe = Arc::new(Mutex::new(self.clone()));
                    self.thread_pool.execute(move || {
                        super::connection::handle_connection(proxy_safe, connection.clone());
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
