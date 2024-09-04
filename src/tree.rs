use crate::{
    fs::{FileMode, FileSystem, SyncFile},
    CResult,
};
use chainerror::Context;
use std::{collections::HashSet, fmt::Debug, hash::Hash};
use typed_path::{UnixPath, UnixPathBuf};

#[derive(Eq)]
pub struct Node {
    pub sf: SyncFile,
    pub entries: HashSet<Node>,
    pub strip_path: UnixPathBuf,
}
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.strip_path == other.strip_path
    }
}
impl Hash for Node {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.strip_path.hash(state);
    }
}
impl Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("sf", &self.sf)
            .field("entries", &self.entries)
            .field("strip_path", &self.strip_path.display())
            .finish()
    }
}

impl Node {
    pub fn new(sf: SyncFile, prefix: &UnixPath) -> Self {
        let strip_path = sf
            .path
            .strip_prefix(prefix)
            .expect("has prefix")
            .to_path_buf();
        Self {
            sf,
            entries: HashSet::new(),
            strip_path,
        }
    }

    pub fn print_node(&self) {
        fn print_node_(depth: usize, entries: &HashSet<Node>) {
            for n in entries {
                for _ in 0..depth * 2 {
                    print!("  ");
                }
                println!("{}", n.sf.path.display());
                print_node_(depth + 1, &n.entries);
            }
        }
        print_node_(0, &self.entries)
    }
}

pub fn build_tree<FS: FileSystem>(fs: &mut FS, sf: SyncFile, prefix: &UnixPath) -> CResult<Node> {
    fn build_tree_<FS: FileSystem>(fs: &mut FS, root: &mut Node, prefix: &UnixPath) -> CResult<()> {
        for entry in fs.list_dir(&root.sf.path).annotate()? {
            let mode = entry.mode;
            let mut node = Node::new(entry, prefix);
            match mode {
                FileMode::File => {
                    root.entries.insert(node);
                }
                FileMode::Dir => {
                    build_tree_(fs, &mut node, prefix).annotate()?;
                    root.entries.insert(node);
                }
                FileMode::Symlink => unimplemented!("not supported for now!"),
            }
        }
        Ok(())
    }

    let mut root = Node::new(sf, prefix);
    build_tree_(fs, &mut root, prefix).annotate()?;
    Ok(root)
}

pub fn diff_trees<'n>(
    root1: &'n Node,
    root2: &'n Node,
) -> (
    Vec<&'n Node>,
    Vec<&'n Node>,
    Vec<(&'n SyncFile, &'n SyncFile)>,
) {
    fn diff_trees_<'n>(
        n1: &'n Node,
        n2: &'n Node,
        n1_doesnt_have: &mut Vec<&'n Node>,
        n2_doesnt_have: &mut Vec<&'n Node>,
        both_have: &mut Vec<(&'n SyncFile, &'n SyncFile)>,
    ) {
        n1_doesnt_have.extend(n2.entries.difference(&n1.entries));
        n2_doesnt_have.extend(n1.entries.difference(&n2.entries));
        for n in HashSet::intersection(&n1.entries, &n2.entries) {
            // SAFETY: i just checked their intersection so..
            let n1c = unsafe { n1.entries.get(n).unwrap_unchecked() };
            let n2c = unsafe { n2.entries.get(n).unwrap_unchecked() };
            if n.sf.mode == FileMode::File {
                both_have.push((&n1c.sf, &n2c.sf));
            }
            diff_trees_(n1c, n2c, n1_doesnt_have, n2_doesnt_have, both_have);
        }
    }

    let mut both_have = Vec::new();
    let mut n1_doesnt_have: Vec<&Node> = Vec::new();
    let mut n2_doesnt_have: Vec<&Node> = Vec::new();
    diff_trees_(
        root1,
        root2,
        &mut n1_doesnt_have,
        &mut n2_doesnt_have,
        &mut both_have,
    );
    (n1_doesnt_have, n2_doesnt_have, both_have)
}
