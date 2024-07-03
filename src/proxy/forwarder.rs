use std::{collections::HashMap, fmt, sync::{Arc, Mutex}};

use http::{Request, Uri};
use uuid::Uuid;

use crate::gateway::server::Server;

use super::proxy::ProxyState;

#[derive(Debug, Clone)]
pub enum ProxyForwardPathRewrite {
    None,
    ForceRewrite(Uri),
    JoinStartWith(Uri),
    JoinEndWith(Uri),
    SkipTheMatchPath,
}

/// # Proxy Forward
///
/// This struct represents a Proxy Forward.
/// This is used to set rules to forward a request to a Server.
///
/// ## Fields
///
/// * `id` - A Uuid.
/// * `match_headers` - A Vec of Strings representing the headers to match.
/// * `match_query` - A Vec of Strings representing the query to match.
/// * `match_path` - A Vec of ProxyForwardPath representing the paths to match.
/// * `match_method` - A Vec of Strings representing the methods to match.
/// * `rewrite_to` - An Option of Uri to rewrite the request path to.
/// * `to` - A Vec of Uuid representing the servers to forward the request.
#[derive(Clone, Debug)]
pub struct ProxyForward {
    pub id: Uuid,

    pub match_headers: Option<Vec<String>>,
    pub match_query: Option<Vec<String>>,
    pub match_path: Option<Vec<ProxyForwardPath>>,
    pub match_method: Option<Vec<String>>,

    pub rewrite_path: Option<ProxyForwardPathRewrite>,

    pub middlewares: Arc<Mutex<HashMap<Uuid, Middleware>>>,

    pub to: Vec<Uuid>,
}

#[derive(Clone)]
pub struct Middleware(
    pub Arc<
        dyn Fn(Arc<Mutex<ProxyState>>, &ProxyForward, Server, Request<Vec<u8>>) -> Request<Vec<u8>>
            + Send
            + Sync
            + 'static,
    >,
);

/// # CloneableFn
///
/// The implementation of the Cloneable Function
impl Middleware {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Arc<Mutex<ProxyState>>, &ProxyForward, Server, Request<Vec<u8>>) -> Request<Vec<u8>> + Send + Sync + 'static,
    {
        Middleware(Arc::new(f))
    }
}

/// # Debug for CloneableFn
///
/// The implementation of the Debug trait for CloneableFn
impl fmt::Debug for Middleware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Closure")
    }
}

impl ProxyForward {
    /// # Proxy Forwarder Builder
    /// 
    /// This function will create a new forwarder builder.
    /// 
    /// ## Example
    /// 
    /// ```rust
    /// let forwarder = ProxyForward::builder()
    ///     .add_match_path(ProxyForwardPath {
    ///         path: Uri::from_static("/api/users"),
    ///         exactly: false,
    ///         starts_with: true,
    ///     })
    ///     .add_server(server_id)
    ///     .add_middleware(middleware)
    ///     .build();
    /// ```
    pub fn builder() -> Self {
        Self {
            id: Uuid::new_v4(),
            match_headers: None,
            match_query: None,
            match_path: None,
            match_method: None,
            rewrite_path: None,
            middlewares: Arc::new(Mutex::new(HashMap::new())),
            to: Vec::new(),
        }
    }

    /// # Add Server
    /// 
    /// This function will add a server to the forwarder by the server id.
    pub fn add_server(&mut self, server_id: Uuid) -> &mut Self {
        self.to.push(server_id);
        self
    }

    /// # Add Match Header
    /// 
    /// This function will add a header to the forwarder.
    pub fn add_match_headers(&mut self, match_header: String) -> &mut Self {
        
        match &mut self.match_headers {
            Some(headers) => {
                headers.push(match_header);
            }
            None => {
                self.match_headers = Some(vec![match_header]);
            }
        }

        self
    }

    /// # Add Match Method
    /// 
    /// This function will add a method to the forwarder.
    pub fn add_match_query(&mut self, match_query: String) -> &mut Self {
        
        match &mut self.match_query {
            Some(queries) => {
                queries.push(match_query);
            }
            None => {
                self.match_query = Some(vec![match_query]);
            }
        }

        self
    }

    /// # Add Match Method
    /// 
    /// This function will add a method to the forwarder.
    pub fn add_match_path(&mut self, match_path: ProxyForwardPath) -> &mut Self {

        match &mut self.match_path {
            Some(paths) => {
                paths.push(match_path);
                self.match_path = Some(paths.clone());
            }
            None => {
                self.match_path = Some(vec![match_path]);
            }
        }

        self
    }

    /// # Force Rewrite Path
    /// 
    /// This function will force the rewrite path to the argument.
    /// 
    /// ## Example
    /// 
    /// ```rust
    /// let path = "/api/users" // The argument
    /// let request_path = "/whatever" // URI that user see
    /// let path_that_server_will_receive = "/api/users" // URI that server see
    /// ```
    pub fn set_rewrite_forced_path(&mut self, path: Uri) -> &mut Self {
        self.rewrite_path = Some(ProxyForwardPathRewrite::ForceRewrite(path));
        self
    }

    /// # Set Rewrite Join Start With Path
    /// 
    /// This function will set the rewrite path to join the start with path.
    /// 
    /// ## Example
    /// 
    /// ```rust
    /// let path = "/api" // The argument
    /// let request_path = "/users" // URI that user see
    /// let path_that_server_will_receive = "/api/users" // URI that server see
    /// ```
    pub fn set_rewrite_join_start_with_path(&mut self, path: Uri) -> &mut Self {
        self.rewrite_path = Some(ProxyForwardPathRewrite::JoinStartWith(path));
        self
    }

    /// # Set Rewrite Join End With Path
    /// 
    /// This function will set the rewrite path to join end with the path.
    /// 
    /// ## Example
    /// 
    /// ```rust
    /// let path = "/users" // The argument
    /// let request_path = "/api" // URI that user see
    /// let path_that_server_will_receive = "/api/users" // URI that server see
    /// ```
    pub fn set_rewrite_join_end_with_path(&mut self, path: Uri) -> &mut Self {
        self.rewrite_path = Some(ProxyForwardPathRewrite::JoinEndWith(path));
        self
    }

    /// # Set Rewrite Skip The Match Path
    /// 
    /// This function will set the rewrite path to skip the match path.
    /// 
    /// ## Example
    /// 
    /// ```rust
    /// let match_path = "/api/users"; // Forwarder Matching Path
    /// let request_path = "/api/users/fetch/by/id"; // URI that user see
    /// let path_that_server_will_receive = "/fetch/by/id"; // URI that server see
    /// ```
    pub fn set_rewrite_skip_the_match_path(&mut self) -> &mut Self {
        self.rewrite_path = Some(ProxyForwardPathRewrite::SkipTheMatchPath);
        self
    }

    /// # Add Middleware
    /// 
    /// This function will add a middleware to the forwarder.
    /// The middlewares run before sending the request to the server and after opening the stream and rewrite the path.
    /// 
    /// ## Example
    /// 
    /// ```rust
    /// let middleware = |proxy_state, forwarder, selected_server, mut req: http::Request<Vec<u8>>| {
    ///    // Do something with the request
    ///    req.body_mut().push(b'1');
    ///    req
    /// };
    /// 
    /// forwarder.add_middleware(middleware);
    /// ```
    /// 
    /// ## Parameters
    /// 
    /// * `middleware` - A closure that takes 4 parameters and returns a Request<Vec<u8>>.
    pub fn add_middleware<K>(&mut self, middleware: K) -> &mut Self 
    where
        K: Fn(Arc<Mutex<ProxyState>>, &ProxyForward, Server, Request<Vec<u8>>) -> Request<Vec<u8>> + Send + Sync + 'static,
    {
        match self.middlewares.lock() {
            Ok(mut middlewares) => {
                let id = Uuid::new_v4();
                middlewares.insert(id, Middleware::new(middleware));
            }
            Err(_) => {}
        }

        self
    }

    /// # Build
    /// 
    /// This function will build the forwarder.
    pub fn build(&self) -> ProxyForward {
        ProxyForward {
            id: self.id.clone(),
            match_headers: self.match_headers.clone(),
            match_query: self.match_query.clone(),
            match_path: self.match_path.clone(),
            match_method: self.match_method.clone(),
            rewrite_path: self.rewrite_path.clone(),
            middlewares: self.middlewares.clone(),
            to: self.to.clone(),
        }
    }
}

/// # Proxy Forward Path
///
/// This struct represents a Proxy Forward Path.
/// This is used to set rules to match a path.
///
/// ## Fields
///
/// * `path` - A Uri.
/// * `exactly` - A bool to match exactly.
/// * `starts_with` - A bool to match if the path starts with.
#[derive(Debug, Clone)]
pub struct ProxyForwardPath {
    // The path to match
    pub path: Uri,
    // The path will exactly match
    pub exactly: bool,
    // Check if request path starts with our path
    pub starts_with: bool,
}
