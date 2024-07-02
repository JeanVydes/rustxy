pub mod gateway;
pub mod http_basic;
pub mod proxy;

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use gateway::server::ServerConfig;
    use http::{uri::Scheme, Uri};
    use native_tls::Identity;
    use proxy::{
        forwarder::{ProxyForward, ProxyForwardPath}, load_balancer::weighted_least_connections_load_balancer, proxy::ProxyConfig
    };
    use uuid::Uuid;

    use super::*;

    #[test]
    fn it_works() {
        let identity = match Identity::from_pkcs8(
            include_bytes!("../cert.pem"),
            include_bytes!("../key.pem"),
        ) {
            Ok(identity) => identity,
            Err(e) => panic!("Error loading identity: {:?}", e),
        };

        // Create the address for our proxy server
        let address = SocketAddr::from(([0, 0, 0, 0], 8080));
        let mut my_proxy = proxy::proxy::Proxy::new(ProxyConfig {
            address,
            max_connections: 100,
            threads: 1,
            max_buffer_size: 2048,
            tls_identity: identity,
        });

        my_proxy.set_load_balancer(weighted_least_connections_load_balancer);

        // Create a new server
        let my_api_server = gateway::server::Server::new(ServerConfig {
            id: Uuid::new_v4(),
            address: SocketAddr::from(([127, 0, 0, 1], 8081)),
            accepted_schemes: vec![Scheme::HTTP, Scheme::HTTPS],
            endpoints: Default::default(),
            weight: 1,
            max_connections: 100,
        });

        // Example, for forwarding from API gateway to the server managing users endpoints
        let forward_1 = ProxyForward::builder()
            .add_server(my_api_server.id.clone())
            .add_server(my_api_server.id.clone())
            .add_server(my_api_server.id.clone())
            .add_match_path(ProxyForwardPath {
                exactly: false,
                starts_with: true,
                path: Uri::from_static("/"),
            })
            .set_rewrite_join_start_with_path(Uri::from_static("/src/"))
            .add_middleware(|_, _, _, mut req| {
                req.body_mut().push(b'1');
                req
            })
            .build();

        my_proxy.add_forward(forward_1);

        // Add the server to the proxy
        my_proxy.add_server(my_api_server.id.clone(), my_api_server);

        // Start the proxy
        my_proxy.listen()
    }
}
