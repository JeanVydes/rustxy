pub mod proxy;
pub mod gateway;

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use gateway::server::{ServerConfig, ServerEndpoint};
    use http::Uri;
    use proxy::proxy::{ProxyConfig, ProxyForward, ProxyForwardPath};

    use super::*;

    #[test]
    fn it_works() {
        let address = SocketAddr::from(([0, 0, 0, 0], 8080));
        let mut my_proxy = proxy::proxy::Proxy::<String>::new(ProxyConfig {
            address,
            max_connections: 100,
            threads: 1,
        });

        let my_api_address = SocketAddr::from(([127, 0, 0, 1], 8081));
        let mut my_api_server = gateway::server::Server::new(ServerConfig {
            address: my_api_address,
            accepted_schemes: vec![gateway::server::Schemes::Http],
            endpoints: Default::default(),
        });

        let uri = Uri::from_static("/");
        my_api_server.add_endpoint(ServerEndpoint {
            scheme: gateway::server::Schemes::Http,
            uri,
            method: http::Method::GET,
            headers: vec![],
        });
        my_proxy.add_forward(ProxyForward {
            match_headers: None,
            match_query: None,
            match_method: None,
            match_path: Some(vec![ProxyForwardPath {
                exactly: false,
                starts_with: true,
                path: Uri::from_static("/"),
            }]),

            rewrite_to: None,

            to: my_api_server.clone(),
        });

        println!("{:?}", my_proxy.forward);

        my_proxy.add_server("my_api_1".to_string(), my_api_server.clone());

        my_proxy.listen()
    }
}
