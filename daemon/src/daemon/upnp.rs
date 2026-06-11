use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use librqbit_upnp_serve::services::content_directory::ContentDirectoryBrowseProvider;
use librqbit_upnp_serve::services::content_directory::browse::response::{
    Container, Item, ItemOrContainer,
};
use librqbit_upnp_serve::{UpnpServer, UpnpServerOptions};
use parking_lot::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::media;

use magneto_core::config::Config;
use crate::daemon::session::SessionHandle;
use crate::metadata::MetadataStore;

const ROOT_ID: usize = 0;
const ID_MASK_31: usize = 0x7FFF_FFFF;

pub struct SharedBrowseProvider {
    session: Arc<SessionHandle>,
    metadata: Arc<RwLock<MetadataStore>>,
    lan_port: u16,
}

#[derive(Clone)]
enum TreeNode {
    Container(Container),
    Item(Item),
}

impl TreeNode {
    fn id(&self) -> usize {
        match self {
            Self::Container(c) => c.id,
            Self::Item(i) => i.id,
        }
    }
    fn parent(&self) -> Option<usize> {
        match self {
            Self::Container(c) => c.parent_id,
            Self::Item(i) => Some(i.parent_id),
        }
    }
    fn into_response(self) -> ItemOrContainer {
        match self {
            Self::Container(c) => ItemOrContainer::Container(c),
            Self::Item(i) => ItemOrContainer::Item(i),
        }
    }
}

impl SharedBrowseProvider {
    fn build_tree(&self, hostname: &str) -> Vec<TreeNode> {
        let meta = self.metadata.read();
        let mut nodes = Vec::new();
        for (info_hash, entry) in &meta.torrents {
            let shared: Vec<u32> = entry
                .files
                .iter()
                .filter(|(_, fm)| fm.shared)
                .map(|(idx, _)| *idx)
                .collect();
            if shared.is_empty() {
                continue;
            }
            let Some(handle) = self.session.get(info_hash) else { continue };
            let Ok(file_infos) = handle.with_metadata(|m| {
                shared
                    .iter()
                    .filter_map(|&idx| {
                        m.file_infos
                            .get(idx as usize)
                            .map(|fi| (idx, fi.relative_filename.clone(), fi.len))
                    })
                    .collect::<Vec<_>>()
            }) else {
                continue;
            };
            if file_infos.is_empty() {
                continue;
            }

            let t_id = torrent_node_id(info_hash);
            let title = handle.name().unwrap_or_else(|| info_hash.clone());
            nodes.push(TreeNode::Container(Container {
                id: t_id,
                parent_id: Some(ROOT_ID),
                title,
                children_count: None,
            }));

            let mut folder_paths: BTreeSet<String> = BTreeSet::new();
            for (_, path, _) in &file_infos {
                let mut cur = path.parent();
                while let Some(p) = cur {
                    if p.as_os_str().is_empty() {
                        break;
                    }
                    folder_paths.insert(p.to_string_lossy().into_owned());
                    cur = p.parent();
                }
            }
            for folder in &folder_paths {
                let f_id = folder_node_id(info_hash, folder);
                let parent = Path::new(folder)
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(|p| folder_node_id(info_hash, &p.to_string_lossy()))
                    .unwrap_or(t_id);
                let name = Path::new(folder)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                nodes.push(TreeNode::Container(Container {
                    id: f_id,
                    parent_id: Some(parent),
                    title: name,
                    children_count: None,
                }));
            }

            for (idx, path, size) in &file_infos {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(|p| folder_node_id(info_hash, &p.to_string_lossy()))
                    .unwrap_or(t_id);
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("file-{idx}"));
                let url = stream_url(hostname, self.lan_port, info_hash, *idx as usize, &name);
                let mime_type = item_mime_type(&name);
                nodes.push(TreeNode::Item(Item {
                    id: file_node_id(info_hash, *idx),
                    parent_id: parent,
                    title: name,
                    mime_type,
                    url,
                    size: *size,
                }));
            }
        }

        let mut counts: HashMap<usize, usize> = HashMap::new();
        for n in &nodes {
            if let Some(p) = n.parent() {
                *counts.entry(p).or_default() += 1;
            }
        }
        for n in &mut nodes {
            if let TreeNode::Container(c) = n {
                c.children_count = Some(counts.get(&c.id).copied().unwrap_or(0));
            }
        }
        nodes
    }
}

impl ContentDirectoryBrowseProvider for SharedBrowseProvider {
    fn browse_direct_children(
        &self,
        parent_id: usize,
        http_hostname: &str,
    ) -> Vec<ItemOrContainer> {
        let result: Vec<ItemOrContainer> = self
            .build_tree(http_hostname)
            .into_iter()
            .filter(|n| matches!(n.parent(), Some(p) if p == parent_id))
            .map(TreeNode::into_response)
            .collect();
        debug!(parent_id, count = result.len(), "browse_direct_children");
        result
    }

    fn browse_metadata(
        &self,
        object_id: usize,
        http_hostname: &str,
    ) -> Vec<ItemOrContainer> {
        let result = if object_id == ROOT_ID {
            let count = self
                .build_tree(http_hostname)
                .iter()
                .filter(|n| matches!(n.parent(), Some(p) if p == ROOT_ID))
                .count();
            vec![ItemOrContainer::Container(Container {
                id: ROOT_ID,
                parent_id: None,
                title: "Magneto".into(),
                children_count: Some(count),
            })]
        } else {
            self.build_tree(http_hostname)
                .into_iter()
                .find(|n| n.id() == object_id)
                .map(|n| vec![n.into_response()])
                .unwrap_or_default()
        };
        debug!(object_id, count = result.len(), "browse_metadata");
        result
    }
}

fn torrent_part(info_hash: &str) -> u32 {
    u32::from_str_radix(info_hash.get(..8).unwrap_or("0"), 16).unwrap_or(0)
}

fn torrent_node_id(info_hash: &str) -> usize {
    (torrent_part(info_hash) as usize) & ID_MASK_31
}

fn folder_node_id(info_hash: &str, folder_path: &str) -> usize {
    let t = torrent_part(info_hash);
    let f = fnv1a_32(folder_path.as_bytes());
    ((t ^ f) as usize) & ID_MASK_31
}

fn file_node_id(info_hash: &str, file_index: u32) -> usize {
    ((torrent_part(info_hash) as usize) << 32) | (file_index as usize)
}

fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

fn item_mime_type(filename: &str) -> Option<mime_guess::Mime> {
    let curated = media::mime_for(filename);
    if curated.starts_with("video/")
        && let Ok(m) = mime_guess::Mime::from_str(curated)
    {
        return Some(m);
    }
    if let Some(m) = mime_guess::from_path(filename).first()
        && m.type_() == "video"
    {
        return Some(m);
    }
    mime_guess::Mime::from_str("video/x-unknown").ok()
}

fn host_has_port(hostname: &str) -> bool {
    !hostname.starts_with('[')
        && hostname
            .rsplit_once(':')
            .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
}

fn stream_url(
    hostname: &str,
    lan_port: u16,
    info_hash: &str,
    file_index: usize,
    filename: &str,
) -> String {
    let host = if host_has_port(hostname) {
        hostname.to_string()
    } else {
        format!("{hostname}:{lan_port}")
    };
    let encoded = urlencoding::encode(filename);
    format!("http://{host}/stream/{info_hash}/{file_index}/{encoded}")
}

pub async fn spawn(
    cancel: CancellationToken,
    session: Arc<SessionHandle>,
    metadata: Arc<RwLock<MetadataStore>>,
    config: &Config,
) -> Result<(JoinHandle<()>, Router)> {
    let provider = Box::new(SharedBrowseProvider {
        session,
        metadata,
        lan_port: config.network.lan_port,
    });
    let opts = UpnpServerOptions {
        friendly_name: config.network.server_name.clone(),
        http_listen_port: config.network.lan_port,
        http_prefix: "/upnp".into(),
        browse_provider: provider,
        cancellation_token: cancel.clone(),
    };
    let mut server = UpnpServer::new(opts)
        .await
        .context("constructing UpnpServer")?;
    let router = server.take_router().context("taking UPnP router")?;
    let ssdp = tokio::spawn(async move {
        if let Err(e) = server.run_ssdp_forever().await {
            warn!(error = %e, "ssdp loop exited");
        }
    });
    info!(port = config.network.lan_port, "upnp server started");
    Ok((ssdp, router))
}
