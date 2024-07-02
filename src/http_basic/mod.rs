use http::Uri;

use crate::proxy::forwarder::{ProxyForward, ProxyForwardPathRewrite};
pub mod request;
pub mod response;

// Function to normalize paths by removing trailing slashes
pub fn normalize_path(path: &str) -> String {
    let trimmed_path = path.trim_end_matches('/');
    if trimmed_path.is_empty() {
        "/".to_string()
    } else {
        trimmed_path.to_string()
    }
}

// Helper function to construct the final path with query
fn construct_full_path(path: String, query: &str) -> String {
    if query.is_empty() {
        if !path.ends_with('/') {
            format!("{}/", path)
        } else {
            path
        }
    } else {
        path // If there's a query, no need to append '/'
    }
}

// Function to rewrite paths based on the given rewrite rule
pub fn rewrite_path(req_uri: &Uri, rewrite: &ProxyForwardPathRewrite, forwarder: &ProxyForward) -> String {
    let query = req_uri.path_and_query().and_then(|pq| pq.query()).unwrap_or("");

    // Normalize the request path
    let req_path = normalize_path(req_uri.path());

    let full_new_path = match rewrite {
        ProxyForwardPathRewrite::ForceRewrite(uri) => {
            construct_full_path(normalize_path(uri.path()), query)
        }
        ProxyForwardPathRewrite::JoinStartWith(uri) => {
            construct_full_path(format!("{}{}", normalize_path(uri.path()), req_path), query)
        }
        ProxyForwardPathRewrite::JoinEndWith(uri) => {
            construct_full_path(format!("{}{}", req_path, normalize_path(uri.path())), query)
        }
        ProxyForwardPathRewrite::SkipTheMatchPath => {
            if let Some(match_paths) = &forwarder.match_path {
                let match_path = &normalize_path(&match_paths[0].path.path());
                if req_path.starts_with(match_path) {
                    let path_without_forward_start_with_path = req_path.replace(match_path, "");
                    if !path_without_forward_start_with_path.starts_with('/') {
                        construct_full_path(format!("/{}", path_without_forward_start_with_path), query)
                    } else {
                        construct_full_path(path_without_forward_start_with_path, query)
                    }
                } else {
                    construct_full_path(req_path, query)
                }
            } else {
                // Handle the case where forwarder.match_path is None
                construct_full_path(req_path, query)
            }
        }
        _ => {
            // Keep the original path
            construct_full_path(req_path, query)
        }
    };

    eprintln!("Full new path: {}", full_new_path);

    full_new_path
}