# RUSTXY

The Rust Reverse Proxy, build your own reverse proxy in minutes with wonderful features, all customizable.

## Example

```rust
    fn main() {
        // Create the address for our proxy server
        let address = SocketAddr::from(([0, 0, 0, 0], 8080));
        let mut my_proxy = proxy::proxy::Proxy::new(ProxyConfig {
            address,
            max_connections: 100,
            threads: 1,
            max_buffer_size: 2048,
        });

        // set the load balancing algorithm, you can also create your own
        my_proxy.set_load_balancer(weighted_least_connections_load_balancer);

        // Create a new server
        let my_api_server = gateway::server::Server::new(ServerConfig {
            id: Uuid::new_v4(), // An internal ID
            address: SocketAddr::from(([127, 0, 0, 1], 8081)), // The external service address
            accepted_schemes: vec![Scheme::HTTP, Scheme::HTTPS], // The schemas that the server receive
            endpoints: Default::default(), // Not useful now, maybe in the future versions
            weight: 1, // Weight of the server, useful for the load balancing, more weight = more resources = more assigned requests
        });

        // Add the server to the proxy
        my_proxy.add_server(my_api_server.id.clone(), my_api_server);

        // Now lets make some rules for our proxy server
        my_proxy.add_forward(ProxyForward {
            id: Uuid::new_v4(), // An internal ID
            match_headers: None, // Match headers to redirect to this forwarder
            match_query: None, // Match query params to redirect to this forwarder
            match_method: None, // Match methods to redirect to this forwarder
            match_path: Some(vec![ProxyForwardPath {
                exactly: false,
                starts_with: true,
                path: Uri::from_static("/"),
            }]), // Match by path to redirect to this forwarder

            rewrite_to: None, // Rewrite path for the other side proxy, example the client type: /api and your backend server would receive in /internal-api-xxx

            to: vec![
                my_api_server.id.clone(),
            ], // A list of servers to forward the request, useful with load balancing
        });

        // Start the proxy listening in the provided address
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