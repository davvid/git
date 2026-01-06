const S_IFMT: i32 = 0o0170000; // <sys/stat.h>
const S_IFLNK: i32 = 0o0120000; // <sys/stat.h>
const S_IFGITLINK: i32 = 0o0160000; // object.h
const S_IFREG: i32 = 0o0100000; // <sys/stat.h>

/// A "directory link" is a link to another git directory, AKA a submodule.
#[inline]
pub fn is_git_directory_link(mode: i32) -> bool {
    // The value 0160000 is not normally a valid mode, and
    // also just happens to be S_IFDIR + S_IFLNK.
    (mode & S_IFMT) == S_IFGITLINK
}

/// A symlink is represented using traditional stat mode bits.
#[inline]
pub fn is_link(mode: i32) -> bool {
    (mode & S_IFMT) == S_IFLNK
}

#[inline]
pub fn is_regular_file(mode: i32) -> bool {
    (mode & S_IFMT) == S_IFREG
}

#[repr(C)]
pub enum ObjectType {
    Bad = -1,
    None = 0,
    Commit = 1,
    Tree = 2,
    Blob = 3,
    Tag = 4,
    // 5 for future expansion
    OfsDelta = 6,
    RefDelta = 7,
    Any,
}
