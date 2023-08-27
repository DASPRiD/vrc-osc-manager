use hyper::service::Service;
use hyper::{Body, Request, Response};
use serde::Serialize;
use serde_json::json;
use serde_repr::Serialize_repr;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

#[derive(Serialize_repr)]
#[repr(u8)]
#[allow(dead_code)]
pub enum OscAccess {
    NoAccess = 0,
    Read = 1,
    Write = 2,
    ReadWrite = 3,
}

#[derive(Serialize)]
struct OscQueryNode {
    #[serde(rename = "FULL_PATH")]
    full_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "TYPE")]
    osc_type: Option<String>,
    #[serde(rename = "ACCESS")]
    access: OscAccess,
    #[serde(rename = "DESCRIPTION")]
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "CONTENTS")]
    contents: Option<HashMap<String, OscQueryNode>>,
}

impl OscQueryNode {
    fn root() -> Self {
        Self {
            full_path: "/".to_string(),
            osc_type: None,
            access: OscAccess::NoAccess,
            description: "Root Node".to_string(),
            contents: None,
        }
    }

    fn new(full_path: String, osc_type: String, access: OscAccess, description: String) -> Self {
        Self {
            full_path,
            osc_type: Some(osc_type),
            access,
            description,
            contents: None,
        }
    }

    fn add_node(&mut self, node: OscQueryNode) {
        let mut address: VecDeque<String> =
            node.full_path.split('/').map(|s| s.to_string()).collect();
        address.pop_front();
        self.add_recursive_node(node, address);
    }

    fn add_recursive_node(&mut self, node: OscQueryNode, mut address: VecDeque<String>) {
        if self.contents.is_none() {
            self.contents = Some(HashMap::new());
        }

        let contents = self.contents.as_mut().unwrap();
        let key = address.pop_front().unwrap();

        if address.is_empty() {
            contents.insert(key.to_string(), node);
            return;
        }

        if !contents.contains_key(&key) {
            let next_address = self.full_path.to_string() + &key;

            let value = OscQueryNode {
                full_path: next_address,
                osc_type: None,
                access: OscAccess::NoAccess,
                description: "".to_string(),
                contents: None,
            };

            contents.insert(key.to_string(), value);
        }

        contents
            .get_mut(&key)
            .unwrap()
            .add_recursive_node(node, address);
    }

    fn get(&self, path: String) -> Option<&OscQueryNode> {
        let mut address: VecDeque<_> = path.split('/').collect();

        let next_key = match address.pop_front() {
            None => return Some(self),
            Some(key) => key,
        };

        let path: String = address.make_contiguous().join("/");

        if next_key.is_empty() {
            return if path.is_empty() {
                Some(self)
            } else {
                self.get(path)
            };
        }

        let contents = match self.contents.as_ref() {
            None => return None,
            Some(contents) => contents,
        };

        match contents.get(&next_key.to_string()) {
            Some(node) => node.get(path),
            None => None,
        }
    }
}

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

pub struct OscQueryService {
    root_node: OscQueryNode,
    host_info: OscHostInfo,
}

impl OscQueryService {
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
}

pub struct OscQueryStatic {
    service: Arc<OscQueryService>,
}

impl Service<Request<Body>> for OscQueryStatic {
    type Response = Response<Body>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        fn mk_response(s: String) -> Result<Response<Body>, hyper::Error> {
            Ok(Response::builder()
                .header("Content-Type", "application/json")
                .body(Body::from(s))
                .unwrap())
        }

        let response = match self.service.root_node.get(request.uri().path().to_string()) {
            None => Ok(Response::builder()
                .status(404)
                .body(Body::from("Not found"))
                .unwrap()),
            Some(node) => {
                let query = match request.uri().query() {
                    None => {
                        let response = mk_response(serde_json::to_string(&node).unwrap());
                        return Box::pin(async { response });
                    }
                    Some(query) => query,
                };

                match query {
                    "HOST_INFO" => {
                        mk_response(serde_json::to_string(&self.service.host_info).unwrap())
                    }
                    "TYPE" => mk_response(match serde_json::to_value(node).unwrap().get("TYPE") {
                        None => json!({}).to_string(),
                        Some(value) => json!({ "TYPE": value }).to_string(),
                    }),
                    "ACCESS" => {
                        mk_response(match serde_json::to_value(node).unwrap().get("ACCESS") {
                            None => json!({}).to_string(),
                            Some(value) => json!({ "ACCESS": value }).to_string(),
                        })
                    }
                    "DESCRIPTION" => mk_response(
                        match serde_json::to_value(node).unwrap().get("DESCRIPTION") {
                            None => json!({}).to_string(),
                            Some(value) => json!({ "DESCRIPTION": value }).to_string(),
                        },
                    ),
                    _ => Ok(Response::builder()
                        .status(204)
                        .body(Body::from("Not supported"))
                        .unwrap()),
                }
            }
        };

        Box::pin(async { response })
    }
}

pub struct MakeOscQueryStatic {
    service: Arc<OscQueryService>,
}

impl MakeOscQueryStatic {
    pub fn new(service: OscQueryService) -> Self {
        Self {
            service: Arc::new(service),
        }
    }
}

impl<T> Service<T> for MakeOscQueryStatic {
    type Response = OscQueryStatic;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        let service = self.service.clone();
        let fut = async move { Ok(OscQueryStatic { service }) };
        Box::pin(fut)
    }
}
