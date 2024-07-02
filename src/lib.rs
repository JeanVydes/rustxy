pub mod gateway;
pub mod http_basic;
pub mod proxy;

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{Arc, Mutex},
    };

    use gateway::server::{ServerConfig, ServerEndpoint};
    use http::{uri::Scheme, Uri};
    use log::error;
    use proxy::{load_balancer::{round_robin_load_balancer, weighted_least_connections_load_balancer}, proxy::{ProxyConfig, ProxyForward, ProxyForwardPath}};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn it_works() {
        // Create the address for our proxy server
        let address = SocketAddr::from(([0, 0, 0, 0], 8080));
        let mut my_proxy = proxy::proxy::Proxy::<String>::new(ProxyConfig {
            address,
            max_connections: 100,
            threads: 1,
            max_buffer_size: 2048,
        });

        my_proxy.set_load_balancer(round_robin_load_balancer);

        // Create the address for our backend server
        let my_api_address = SocketAddr::from(([127, 0, 0, 1], 8081));

        // Create a new server
        let my_api_server = Arc::new(Mutex::new(gateway::server::Server::new(ServerConfig {
            address: my_api_address,
            accepted_schemes: vec![Scheme::HTTP, Scheme::HTTPS],
            endpoints: Default::default(),
            weight: 1,
        })));

        /*let uri = Uri::from_static("/");

        // Add an endpoint for our backend server
        match my_api_server.lock() {
            Ok(mut server) => {
                server.add_endpoint(ServerEndpoint {
                    scheme: Scheme::HTTP,
                    uri,
                    method: http::Method::GET,
                    headers: vec![],
                });
            }
            Err(e) => {
                error!("Failed to lock the server: {}", e);
            }
        }*/

        // Add a forward rule
        my_proxy.add_forward(ProxyForward {
            id: Uuid::new_v4(),
            match_headers: None,
            match_query: None,
            match_method: None,
            match_path: Some(vec![ProxyForwardPath {
                exactly: false,
                starts_with: true,
                path: Uri::from_static("/"),
            }]),

            rewrite_to: None,
            round_robin_index: Arc::new(0),

            to: vec![my_api_server.clone(), my_api_server.clone(), my_api_server.clone()],
        });

        // Add the server to the proxy
        my_proxy.add_server("my_api_1".to_string(), my_api_server);

        // Start the proxy
        my_proxy.listen()
    }
}
