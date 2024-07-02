use std::{fmt, sync::{Arc, Mutex}};

use log::error;

use crate::gateway::server::Server;

use super::{forwarder::ProxyForward, proxy::ProxyState};

/// # Load Balancer
///
/// This struct represents a Load Balancer Function.
#[derive(Clone)]
pub struct LoadBalancer (
    pub Arc<
        dyn Fn(Arc<Mutex<ProxyState>>, &ProxyForward, Vec<Server>) -> Server
            + Send
            + Sync
            + 'static,
    >,
);

/// # Load Balancer
///
/// The implementation of the Load Balancer Function
impl LoadBalancer {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Arc<Mutex<ProxyState>>, &ProxyForward, Vec<Server>) -> Server + Send + Sync + 'static,
    {
        LoadBalancer(Arc::new(f))
    }
}

/// # Debug for Load Balancer
///
/// The implementation of the Debug trait for Load Balancer
impl fmt::Debug for LoadBalancer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Closure")
    }
}

/// # Least Connections Load Balancer
/// 
/// This load balancer return the server with least connections
/// 
/// ## Example
/// 
/// ```rust
/// proxy.set_load_balancer(least_connections_load_balancer);
/// ```
pub fn least_connections_load_balancer(
    _: Arc<Mutex<ProxyState>>,
    _: &ProxyForward,
    preselected_servers: Vec<Server>,
) -> Server {
    let mut least_connections_server: Option<Server> = None;
    let mut least_connections = usize::max_value();

    for server in preselected_servers.clone() {
        let conn = server.active_connections;
        if conn < least_connections {
            least_connections = conn;
            least_connections_server = Some(server.clone());
        }
    }

    if let Some(server) = &least_connections_server {
        return server.clone();
    }

    preselected_servers[0].clone()
}

/// # Weighted Least Connections Load Balancer
/// 
/// This load balancer return the server depending on the weight and the least connections
/// 
/// ## Example
/// 
/// ```rust
/// proxy.set_load_balancer(weighted_least_connections_load_balancer);
/// ```
pub fn weighted_least_connections_load_balancer(
    _: Arc<Mutex<ProxyState>>,
    _: &ProxyForward,
    preselected_servers: Vec<Server>,
) -> Server {
    let mut weighted_least_connections_server: Option<Server> = None;
    let mut weighted_least_connections = usize::max_value();

    for server in preselected_servers.clone() {
        let conn = server.active_connections;
        let weight = server.weight;
        let weighted_conn = conn / weight;
        if weighted_conn < weighted_least_connections {
            weighted_least_connections = weighted_conn;
            weighted_least_connections_server = Some(server.clone());
        }
    }

    if let Some(server) = &weighted_least_connections_server {
        return server.clone();
    }

    preselected_servers[0].clone()
}

/// # Round Robin Load Balancer
/// 
/// This load balancer return the server in a round robin fashion
/// 
/// ## Example
/// 
/// ```rust
/// proxy.set_load_balancer(round_robin_load_balancer);
/// ```
pub fn round_robin_load_balancer(
    state: Arc<Mutex<ProxyState>>,
    forward: &ProxyForward,
    preselected_servers: Vec<Server>,
) -> Server {
    let mut state = match state.lock() {
        Ok(state) => state,
        Err(e) => {
            error!("Failed to lock the proxy state: {}", e);
            return preselected_servers[0].clone();
        }
    };

    let index = state.round_robin_index.entry(forward.id).or_insert(0);
    let server = preselected_servers[*index as usize].clone();

    *index += 1;
    if *index >= preselected_servers.len() as u32 {
        *index = 0;
    }

    server
}
