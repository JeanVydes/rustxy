pub mod gateway;
pub mod http_basic;
pub mod proxy;

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, net::SocketAddr};

    use gateway::server::ServerConfig;
    use http::{uri::Scheme, Uri};
    use native_tls::Identity;
    use proxy::{
        forwarder::{ProxyForward, ProxyForwardPath}, load_balancer::weighted_least_connections_load_balancer, proxy::ProxyConfig
    };
    use uuid::Uuid;

    use super::*;

    #[test]
    fn identity_creation() {
        match Identity::from_pkcs8(
            include_bytes!("../test/cert.pem"),
            include_bytes!("../test/key.pem"),
        ) {
            Ok(identity) => identity,
            Err(e) => panic!("Error creating identity: {}", e),
        };
    }

    #[test]
    fn proxy_creation() {
        // Create the address for our proxy server
        let address = SocketAddr::from(([0, 0, 0, 0], 8080));
        let my_proxy = proxy::proxy::Proxy::new(ProxyConfig {
            address,
            max_connections: 100,
            threads: 1,
            max_buffer_size: 2048,
        });

        assert_eq!(my_proxy.address, address);
    }

    #[test]
    fn server_creation() {
        let address = SocketAddr::from(([0, 0, 0, 0], 8080));
        let server = gateway::server::Server::new(ServerConfig {
            id: Uuid::new_v4(),
            address,
            accepted_schemes: vec![Scheme::HTTP, Scheme::HTTPS],
            endpoints: HashMap::new(),
            weight: 1,
            max_connections: 100,
            tls_identity: None,
        });

        assert_eq!(server.address, address);
    }

    #[test]
    fn forwarder_creation() {
        let forwarder = ProxyForward::builder()
            .add_match_path(ProxyForwardPath {
                path: Uri::from_static("/api/users"),
                exactly: false,
                starts_with: true,
            })
            .add_server(Uuid::new_v4())
            .build();

        assert_eq!(forwarder.to.len(), 1);
    }

    #[test]
    fn general_test() {
        let identity = match Identity::from_pkcs8(
            include_bytes!("../test/cert.pem"),
            include_bytes!("../test/key.pem"),
        ) {
            Ok(identity) => identity,
            Err(e) => panic!("Error creating identity: {}", e),
        };

        // Create the address for our proxy server
        let address = SocketAddr::from(([0, 0, 0, 0], 8080));
        let mut my_proxy = proxy::proxy::Proxy::new(ProxyConfig {
            address,
            max_connections: 100,
            threads: 1,
            max_buffer_size: 2048,
        });

        my_proxy.set_load_balancer(weighted_least_connections_load_balancer);

        let mut forwarder = ProxyForward::builder()
            .add_match_path(ProxyForwardPath {
                path: Uri::from_static("/api/users"),
                exactly: false,
                starts_with: true,
            })
            .set_rewrite_skip_the_match_path()
            .build();

        let address = SocketAddr::from(([142, 250, 218, 132], 443));
        let server = gateway::server::Server::new(ServerConfig {
            id: Uuid::new_v4(),
            address,
            accepted_schemes: vec![Scheme::HTTP, Scheme::HTTPS],
            endpoints: HashMap::new(),
            weight: 1,
            max_connections: 100,
            tls_identity: Some(identity),
        });

        forwarder.add_server(server.id.clone());

        my_proxy.add_server(server.to_owned());
        my_proxy.add_forward(forwarder);

        eprintln!("{:?}", my_proxy.forwards);
        my_proxy.listen();
    }
}
