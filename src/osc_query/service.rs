use axum::body::Body;
use axum::http::{Request, Response};
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use serde_json::json;
use std::convert::Infallible;
use std::future::{ready, Ready};
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::Service;

use crate::osc_query::node::{OscAccess, OscQueryNode};

#[derive(Serialize)]
struct OscHostInfoExtension {
    #[serde(rename = "ACCESS")]
    access: bool,
    #[serde(rename = "DESCRIPTION")]
    description: bool,
}

#[derive(Serialize)]
pub struct OscHostInfo {
    #[serde(rename = "NAME")]
    name: String,
    #[serde(rename = "OSC_IP")]
    osc_ip: String,
    #[serde(rename = "OSC_PORT")]
    osc_port: u16,
    #[serde(rename = "EXTENSIONS")]
    extension: OscHostInfoExtension,
}

impl OscHostInfo {
    pub fn new(name: String, osc_ip: String, osc_port: u16) -> Self {
        Self {
            name,
            osc_ip,
            osc_port,
            extension: OscHostInfoExtension {
                description: true,
                access: true,
            },
        }
    }
}

pub struct OscQueryServiceBuilder {
    root_node: OscQueryNode,
    host_info: OscHostInfo,
}

impl OscQueryServiceBuilder {
    pub fn new(host_info: OscHostInfo) -> Self {
        Self {
            root_node: OscQueryNode::root(),
            host_info,
        }
    }

    pub fn add_endpoint(
        &mut self,
        full_path: String,
        osc_type: String,
        access: OscAccess,
        description: String,
    ) {
        let node = OscQueryNode::new(full_path, osc_type, access, description);
        self.root_node.add_node(node);
    }

    pub fn build(self) -> OscQueryService {
        OscQueryService {
            root_node: Arc::new(self.root_node),
            host_info: Arc::new(self.host_info),
        }
    }
}

#[derive(Clone)]
pub struct OscQueryService {
    root_node: Arc<OscQueryNode>,
    host_info: Arc<OscHostInfo>,
}

impl Service<Request<Body>> for OscQueryService {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let node = match self.root_node.get(req.uri().path().to_string()) {
            None => {
                return ready(Ok(Response::builder()
                    .status(404)
                    .body(Body::from("Not found"))
                    .unwrap()))
            }
            Some(node) => node,
        };

        let query = match req.uri().query() {
            None => return ready(Ok(Json(&node).into_response())),
            Some(query) => query,
        };

        let response = match query {
            "HOST_INFO" => Json(self.host_info.as_ref()).into_response(),
            "TYPE" => Json(
                node.osc_type
                    .clone()
                    .map(|value| json!({"TYPE": value}))
                    .unwrap_or(json!({})),
            )
            .into_response(),
            "ACCESS" => Json(json!({"ACCESS": node.access})).into_response(),
            "DESCRIPTION" => Json(json!({"DESCRIPTION": node.description})).into_response(),
            _ => Response::builder()
                .status(204)
                .body(Body::from("Not supported"))
                .unwrap(),
        };

        ready(Ok(response))
    }
}
