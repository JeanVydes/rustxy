use std::sync::{Arc, Mutex};

use log::error;

use crate::gateway::server::Server;

use super::proxy::{ProxyForward, ProxyState};

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
        println!("Selected server with least connections: {:?}", server);
        return server.clone();
    }

    preselected_servers[0].clone()
}

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
