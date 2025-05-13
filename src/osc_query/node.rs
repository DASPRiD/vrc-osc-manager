use serde::Serialize;
use serde_repr::Serialize_repr;
use std::collections::{HashMap, VecDeque};

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
pub(super) struct OscQueryNode {
    #[serde(rename = "FULL_PATH")]
    pub full_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "TYPE")]
    pub osc_type: Option<String>,
    #[serde(rename = "ACCESS")]
    pub access: OscAccess,
    #[serde(rename = "DESCRIPTION")]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "CONTENTS")]
    pub contents: Option<HashMap<String, OscQueryNode>>,
}

impl OscQueryNode {
    pub fn root() -> Self {
        Self {
            full_path: "/".to_string(),
            osc_type: None,
            access: OscAccess::NoAccess,
            description: "Root Node".to_string(),
            contents: None,
        }
    }

    pub fn new(
        full_path: String,
        osc_type: String,
        access: OscAccess,
        description: String,
    ) -> Self {
        Self {
            full_path,
            osc_type: Some(osc_type),
            access,
            description,
            contents: None,
        }
    }

    pub fn add_node(&mut self, node: OscQueryNode) {
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

    pub fn get(&self, path: String) -> Option<&OscQueryNode> {
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

        let contents = self.contents.as_ref()?;

        match contents.get(&next_key.to_string()) {
            Some(node) => node.get(path),
            None => None,
        }
    }
}
