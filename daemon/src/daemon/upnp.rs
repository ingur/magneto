use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
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

struct SharedTorrent {
    info_hash: String,
    title: String,
    // The torrent holds one file in total, so the root lists it as that file
    // instead of a container whose only child repeats the name.
    single_file: bool,
    files: Vec<SharedFile>,
}

struct SharedFile {
    index: u32,
    path: PathBuf,
    size: u64,
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
    /// The torrents the library lists: shared in the metadata, loaded in the
    /// session, and with at least one shared file the engine knows about.
    /// Each one becomes exactly one root child, so the root's child count is
    /// the length of this list.
    fn shared_torrents(&self) -> Vec<SharedTorrent> {
        let meta = self.metadata.read();
        let mut torrents = Vec::new();
        for (info_hash, entry) in &meta.torrents {
            if !entry.files.values().any(|fm| fm.shared) {
                continue;
            }
            let Some(handle) = self.session.get(info_hash) else { continue };
            let Ok((single_file, files)) = handle.with_metadata(|m| {
                let files = entry
                    .files
                    .iter()
                    .filter(|(_, fm)| fm.shared)
                    .filter_map(|(&index, _)| {
                        m.file_infos.get(index as usize).map(|fi| SharedFile {
                            index,
                            path: fi.relative_filename.clone(),
                            size: fi.len,
                        })
                    })
                    .collect::<Vec<_>>();
                (m.file_infos.len() == 1, files)
            }) else {
                continue;
            };
            if files.is_empty() {
                continue;
            }
            torrents.push(SharedTorrent {
                info_hash: info_hash.clone(),
                title: handle.name().unwrap_or_else(|| info_hash.clone()),
                single_file,
                files,
            });
        }
        torrents
    }

    fn tree(&self, hostname: &str) -> Vec<TreeNode> {
        build_tree(self.shared_torrents(), hostname, self.lan_port)
    }
}

/// Root entries in title order; inside a torrent, folders before files, each
/// in name order. Names collate case-insensitively.
fn build_tree(mut torrents: Vec<SharedTorrent>, hostname: &str, lan_port: u16) -> Vec<TreeNode> {
    torrents.sort_by(|a, b| title_cmp(&a.title, &b.title));
    let mut nodes = Vec::new();
    for torrent in &torrents {
        push_torrent(&mut nodes, torrent, hostname, lan_port);
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

fn push_torrent(
    nodes: &mut Vec<TreeNode>,
    torrent: &SharedTorrent,
    hostname: &str,
    lan_port: u16,
) {
    let info_hash = &torrent.info_hash;
    let item = |file: &SharedFile, parent: usize, title: String| {
        let name = file_name(file);
        let url = stream_url(hostname, lan_port, info_hash, file.index as usize, &name);
        TreeNode::Item(Item {
            id: file_node_id(info_hash, file.index),
            parent_id: parent,
            title,
            mime_type: item_mime_type(&name),
            url,
            size: file.size,
        })
    };
    if torrent.single_file {
        nodes.push(item(&torrent.files[0], ROOT_ID, torrent.title.clone()));
        return;
    }

    let t_id = torrent_node_id(info_hash);
    nodes.push(TreeNode::Container(Container {
        id: t_id,
        parent_id: Some(ROOT_ID),
        title: torrent.title.clone(),
        children_count: None,
    }));

    let mut folders: BTreeSet<&Path> = BTreeSet::new();
    for file in &torrent.files {
        let mut cur = file.path.parent();
        while let Some(p) = cur.filter(|p| !p.as_os_str().is_empty()) {
            folders.insert(p);
            cur = p.parent();
        }
    }
    let mut folders: Vec<&Path> = folders.into_iter().collect();
    folders.sort_by(|a, b| title_cmp(&folder_name(a), &folder_name(b)));
    for folder in folders {
        nodes.push(TreeNode::Container(Container {
            id: folder_node_id(info_hash, &folder.to_string_lossy()),
            parent_id: Some(parent_node_id(info_hash, folder, t_id)),
            title: folder_name(folder).into_owned(),
            children_count: None,
        }));
    }

    let mut files: Vec<&SharedFile> = torrent.files.iter().collect();
    files.sort_by(|a, b| title_cmp(&file_name(a), &file_name(b)));
    for file in files {
        let parent = parent_node_id(info_hash, &file.path, t_id);
        nodes.push(item(file, parent, file_name(file).into_owned()));
    }
}

fn title_cmp(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
        .then_with(|| a.cmp(b))
}

fn folder_name(path: &Path) -> Cow<'_, str> {
    path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
}

fn file_name(file: &SharedFile) -> Cow<'_, str> {
    match file.path.file_name() {
        Some(n) => n.to_string_lossy(),
        None => Cow::Owned(format!("file-{}", file.index)),
    }
}

fn parent_node_id(info_hash: &str, path: &Path, torrent_id: usize) -> usize {
    match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(p) => folder_node_id(info_hash, &p.to_string_lossy()),
        None => torrent_id,
    }
}

impl ContentDirectoryBrowseProvider for SharedBrowseProvider {
    fn browse_direct_children(
        &self,
        parent_id: usize,
        http_hostname: &str,
    ) -> Vec<ItemOrContainer> {
        let result: Vec<ItemOrContainer> = self
            .tree(http_hostname)
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
            vec![ItemOrContainer::Container(Container {
                id: ROOT_ID,
                parent_id: None,
                title: "Magneto".into(),
                children_count: Some(self.shared_torrents().len()),
            })]
        } else {
            self.tree(http_hostname)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn file(index: u32, path: &str) -> SharedFile {
        SharedFile { index, path: PathBuf::from(path), size: 1 }
    }

    fn torrent(info_hash: &str, title: &str, single_file: bool, files: Vec<SharedFile>) -> SharedTorrent {
        SharedTorrent { info_hash: info_hash.into(), title: title.into(), single_file, files }
    }

    fn titles(nodes: &[TreeNode], parent: usize) -> Vec<&str> {
        nodes
            .iter()
            .filter(|n| n.parent() == Some(parent))
            .map(|n| match n {
                TreeNode::Container(c) => c.title.as_str(),
                TreeNode::Item(i) => i.title.as_str(),
            })
            .collect()
    }

    fn child_count(nodes: &[TreeNode], id: usize) -> Option<usize> {
        nodes.iter().find_map(|n| match n {
            TreeNode::Container(c) if c.id == id => c.children_count,
            _ => None,
        })
    }

    #[test]
    fn root_is_ordered_by_title_ignoring_case() {
        let tree = build_tree(
            vec![
                torrent("cccccccc", "cherry", false, vec![file(0, "c.mkv"), file(1, "d.mkv")]),
                torrent("aaaaaaaa", "banana.mkv", true, vec![file(0, "banana.mkv")]),
                torrent("bbbbbbbb", "Apple", false, vec![file(0, "a.mkv"), file(1, "b.mkv")]),
            ],
            "tv",
            1,
        );
        assert_eq!(titles(&tree, ROOT_ID), ["Apple", "banana.mkv", "cherry"]);
    }

    #[test]
    fn single_file_torrent_is_one_item_at_the_root() {
        let tree = build_tree(
            vec![torrent("aaaaaaaa", "Movie", true, vec![file(0, "movie.mkv")])],
            "tv",
            1,
        );
        assert_eq!(tree.len(), 1);
        let TreeNode::Item(item) = &tree[0] else { panic!("expected an item") };
        assert_eq!(item.parent_id, ROOT_ID);
        assert_eq!(item.id, file_node_id("aaaaaaaa", 0));
        assert_eq!(item.title, "Movie");
        assert_eq!(item.url, "http://tv:1/stream/aaaaaaaa/0/movie.mkv");
    }

    #[test]
    fn one_shared_file_of_many_keeps_its_container() {
        let tree = build_tree(
            vec![torrent("aaaaaaaa", "Show", false, vec![file(3, "Season 01/e03.mkv")])],
            "tv",
            1,
        );
        assert_eq!(titles(&tree, ROOT_ID), ["Show"]);
        assert_eq!(titles(&tree, torrent_node_id("aaaaaaaa")), ["Season 01"]);
        assert_eq!(titles(&tree, folder_node_id("aaaaaaaa", "Season 01")), ["e03.mkv"]);
    }

    #[test]
    fn folders_come_before_files_each_in_name_order() {
        let tree = build_tree(
            vec![torrent(
                "aaaaaaaa",
                "Show",
                false,
                vec![
                    file(0, "zeta.mkv"),
                    file(1, "Season 01/E02.mkv"),
                    file(2, "Season 01/e01.mkv"),
                    file(3, "Alpha.mkv"),
                    file(4, "extras/bloopers.mkv"),
                    file(5, "Bonus/x.mkv"),
                ],
            )],
            "tv",
            1,
        );
        let t_id = torrent_node_id("aaaaaaaa");
        assert_eq!(titles(&tree, t_id), ["Bonus", "extras", "Season 01", "Alpha.mkv", "zeta.mkv"]);
        assert_eq!(child_count(&tree, t_id), Some(5));
        let season = folder_node_id("aaaaaaaa", "Season 01");
        assert_eq!(titles(&tree, season), ["e01.mkv", "E02.mkv"]);
        assert_eq!(child_count(&tree, season), Some(2));
    }

    #[test]
    fn every_listed_torrent_is_exactly_one_root_child() {
        let torrents = vec![
            torrent("aaaaaaaa", "a", true, vec![file(0, "a.mkv")]),
            torrent("bbbbbbbb", "b", false, vec![file(0, "x/y.mkv"), file(1, "z.mkv")]),
            torrent("cccccccc", "c", false, vec![file(0, "c.mkv")]),
        ];
        let count = torrents.len();
        let tree = build_tree(torrents, "tv", 1);
        assert_eq!(titles(&tree, ROOT_ID).len(), count);
    }
}
