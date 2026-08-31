use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostConfig {
    /// Destination passed verbatim as one argument to `ssh`.
    pub ssh: String,
    /// Optional tree label; the ssh destination is used when absent.
    #[serde(default)]
    pub name: Option<String>,
}

impl super::Config {
    pub(crate) fn add_ad_hoc_hosts(&mut self, destinations: Vec<String>) {
        for ssh in destinations {
            if !self.hosts.iter().any(|host| host.ssh == ssh) {
                self.hosts.push(HostConfig { ssh, name: None });
            }
        }
    }
}
