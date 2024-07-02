# RUSTXY

The Rust Reverse Proxy, build your own reverse proxy in minutes with wonderful features, all customizable.

You need to create certificates

```
openssl genpkey -algorithm RSA -out key.pem
openssl req -x509 -new -key key.pem -out cert.pem -days 365
```

## Example

```rust
    fn main() {
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

        my_proxy.set_load_balancer(weighted_least_connections_load_balancer); // pre-built load balancer

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
            .add_middleware(|proxy_state, forwarder, selected_server, mut req| {
                req.body_mut().push(b'1');
                req
            })
            .build();

        my_proxy.add_forward(forward_1); // Add forward rule to proxy

        // Add the server to the proxy
        my_proxy.add_server(my_api_server.id.clone(), my_api_server);

        // Start the proxy
        my_proxy.listen()
    }
```

## Custom Load Balance

If you need it, you can build with your own criteria load balancer.

```rust
my_proxy.set_load_balancer(|proxy_state, forwarder, preselected_servers| {
    // proxy_state: internal state of the proxy
    // forwarder: the forwarder for the request, selected from matching certain criteria like path, methods or other things
    // preselected_servers: the servers that are preselected for the forwarder

    // You have to return a server to process the request
    // Check in src/proxy/load_balancer.rs for pre-built load balancers
    preselected_servers[0].clone()
});
```