pub mod gateway;
pub mod http_basic;
pub mod proxy;

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
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
        let mut my_proxy = proxy::proxy::Proxy::new(ProxyConfig {
            address,
            max_connections: 100,
            threads: 1,
            max_buffer_size: 2048,
        });

        my_proxy.set_load_balancer(weighted_least_connections_load_balancer);

        // Create a new server
        let my_api_server = gateway::server::Server::new(ServerConfig {
            id: Uuid::new_v4(),
            address: SocketAddr::from(([127, 0, 0, 1], 8081)),
            accepted_schemes: vec![Scheme::HTTP, Scheme::HTTPS],
            endpoints: Default::default(),
            weight: 1,
        });

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

            to: vec![my_api_server.id.clone(), my_api_server.id.clone(), my_api_server.id.clone()],
        });

        // Add the server to the proxy
        my_proxy.add_server(my_api_server.id.clone(), my_api_server);

        // Start the proxy
        my_proxy.listen()
    }
}
