use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, ensure};
use fuser::{
    Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo,
    LockOwner, MountOption, OpenAccMode, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEntry, ReplyOpen, Request, Session,
};

const IMAGE_INODE: INodeNo = INodeNo(2);
const TTL: Duration = Duration::from_secs(60);

struct PlaceholderFs {
    image: Vec<u8>,
    created: SystemTime,
}

impl PlaceholderFs {
    fn attr(&self, ino: INodeNo) -> Option<FileAttr> {
        let (kind, size, nlink) = match ino {
            INodeNo::ROOT => (FileType::Directory, 0, 2),
            IMAGE_INODE => (FileType::RegularFile, self.image.len() as u64, 1),
            _ => return None,
        };
        Some(FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: self.created,
            mtime: self.created,
            ctime: self.created,
            crtime: self.created,
            kind,
            perm: 0o555,
            nlink,
            uid: rustix::process::geteuid().as_raw(),
            gid: rustix::process::getegid().as_raw(),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        })
    }
}

impl Filesystem for PlaceholderFs {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        if parent != INodeNo::ROOT {
            reply.error(Errno::ENOTDIR);
            return;
        }
        let ino = if name == "." || name == ".." {
            INodeNo::ROOT
        } else {
            IMAGE_INODE
        };
        // All names are aliases of one immutable image; no per-name allocation is needed.
        reply.entry(&TTL, &self.attr(ino).expect("known inode"), Generation(0));
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        match self.attr(ino) {
            Some(attr) => reply.attr(&TTL, &attr),
            None => reply.error(Errno::ENOENT),
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        match ino {
            INodeNo::ROOT => reply.error(Errno::EISDIR),
            IMAGE_INODE if flags.acc_mode() == OpenAccMode::O_RDONLY => {
                reply.opened(FileHandle(0), FopenFlags::FOPEN_KEEP_CACHE);
            }
            IMAGE_INODE => reply.error(Errno::EROFS),
            _ => reply.error(Errno::ENOENT),
        }
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        if ino != IMAGE_INODE {
            reply.error(Errno::ENOENT);
            return;
        }
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.image.len());
        let end = start.saturating_add(size as usize).min(self.image.len());
        reply.data(&self.image[start..end]);
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        if ino == INodeNo::ROOT {
            reply.opened(FileHandle(0), FopenFlags::empty());
        } else {
            reply.error(Errno::ENOTDIR);
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        if ino != INodeNo::ROOT {
            reply.error(Errno::ENOTDIR);
            return;
        }
        // The namespace has no finite listing. Names become accessible through lookup.
        for (index, name) in [".", ".."].into_iter().enumerate() {
            if (index as u64) < offset {
                continue;
            }
            if reply.add(INodeNo::ROOT, index as u64 + 1, FileType::Directory, name) {
                break;
            }
        }
        reply.ok();
    }
}

pub(crate) fn mount(mountpoint: &Path, helper: Option<&Path>) -> Result<()> {
    let mountpoint = mountpoint.canonicalize().with_context(|| {
        format!(
            "open mountpoint {}; create the directory first",
            mountpoint.display()
        )
    })?;
    ensure!(mountpoint.is_dir(), "mountpoint must be a directory");
    ensure!(
        fs::read_dir(&mountpoint)?.next().is_none(),
        "mountpoint must be empty: {}",
        mountpoint.display()
    );
    let helper = match helper {
        Some(path) => path.to_path_buf(),
        None => env::current_exe()?.with_file_name("path-to-fs-helper"),
    };
    let image = fs::read(&helper).with_context(|| {
        format!("read helper {}; build both binaries with `cargo build --release --bins` or use --helper", helper.display())
    })?;
    ensure!(
        image.starts_with(b"\x7fELF"),
        "helper must be a Linux ELF executable"
    );
    let filesystem = PlaceholderFs {
        image,
        created: SystemTime::now(),
    };
    let mut config = Config::default();
    config.mount_options = vec![
        MountOption::RO,
        MountOption::Exec,
        MountOption::NoSuid,
        MountOption::NoDev,
        MountOption::DefaultPermissions,
        MountOption::FSName("path-to-fs".to_owned()),
    ];
    let mut session = Session::new(filesystem, &mountpoint, &config)
        .context("mount filesystem; check /dev/fuse and fusermount3 availability")?;
    let mut unmounter = session.unmount_callable();
    ctrlc::set_handler(move || {
        if let Err(error) = unmounter.unmount() {
            eprintln!("path-to-fs: could not unmount: {error}");
        }
    })
    .context("install shutdown handler")?;
    eprintln!("Mounted {}. Press Ctrl-C to unmount.", mountpoint.display());
    session.run().context("serve filesystem")
}
