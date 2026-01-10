use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CString, OsString};

use clap::{CommandFactory, Parser, ValueHint};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::config::c::{git_config_bool, git_default_config, ConfigFn};
use crate::entry::{c::checkout_entry_ca, Checkout};
use crate::hex::{c::get_oid_hex_algop, hex_from_oid};
use crate::lockfile::{hold_lock_file_for_update, LockFile};
use crate::object_file::c::index_fd;
use crate::odb::odb_read_object_into_utf8_string;
use crate::read_cache::{AddCache, IndexState, COMMIT_LOCK};
use crate::read_cache::c::{
    add_index_entry, discard_cache_entry, index_state_get_cache_entry_name,
    index_state_get_cache_nr, make_cache_entry, make_transient_cache_entry,
    write_locked_index,
};
use crate::hash::{ObjectID, c::git_hash_algo_get_hexsz, c::null_oid};
use crate::repository::c::{
    repo_config, repo_get_git_dir, repo_get_hash_algo, repo_get_index_state,
    repo_get_object_database, repo_get_work_tree,
};
use crate::setup::c::{git_get_startup_info, git_startup_info_have_repository, setup_work_tree};
use crate::sparse_index::c::ensure_full_index;
use crate::types::{
    maybe_os_string_from_c_char_ptr, maybe_str_from_c_char_ptr, os_string_from_c_char_ptr,
    pathbuf_from_c_char_ptr, ExitCode,
};
use crate::object::{is_git_directory_link, is_link, is_regular_file, ObjectType};

#[derive(Debug, Default)]
struct DifftoolConfig {
    has_symlinks: bool,
    trust_exit_code: bool,
}

impl DifftoolConfig {
    fn new() -> Self {
        Self {
            has_symlinks: true,
            trust_exit_code: false,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "git difftool",
    override_usage = "git difftool [<options>] [<commit> [<commit>]] [--] [<path>...]"
)]
struct DifftoolOptions {
    /// use `diff.guitool` instead of `diff.tool`.
    #[arg(
        short = 'g',
        long = "gui",
        num_args = 0,
        default_missing_value = "true",
        conflicts_with = "tool",
        conflicts_with = "extcmd"
    )]
    gui: Option<bool>,
    /// do not use `diff.guitool` instead of `diff.tool`.
    #[arg(
        long = "no-gui",
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        overrides_with = "gui"
    )]
    no_gui: Option<bool>,

    /// perform a full-directory diff.
    #[arg(short, long, conflicts_with = "no_index")]
    dir_diff: bool,
    /// do not perform a full-directory diff.
    #[arg(long = "no-dir-diff", hide = true, overrides_with = "dir_diff")]
    _no_dir_diff: bool,

    /// prompt before launching a diff tool
    #[arg(long, hide = true, num_args = 0, default_missing_value = "true")]
    prompt: Option<bool>,
    /// do not prompt before launching a diff tool.
    #[arg(
        short = 'y',
        long,
        num_args = 0,
        default_missing_value = "true",
        overrides_with = "prompt"
    )]
    no_prompt: Option<bool>,

    /// use symlinks in dir-diff mode.
    #[arg(long, num_args = 0, default_missing_value = "true")]
    symlinks: Option<bool>,
    /// do not use symlinks in dir-diff mode.
    #[arg(
        long = "no-symlinks",
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        overrides_with = "symlinks"
    )]
    no_symlinks: Option<bool>,

    /// use the specified diff tool.
    #[arg(
        short = 't',
        long,
        value_name = "tool",
        conflicts_with = "gui",
        conflicts_with = "extcmd"
    )]
    tool: Option<String>,

    /// print a list of diff tools that may be used with `--tool`.
    #[arg(long)]
    tool_help: bool,

    /// exit when an invoked diff tool returns a non-zero exit code.
    #[arg(long, num_args = 0, default_missing_value = "true")]
    trust_exit_code: Option<bool>,
    /// do not exit when an invoked diff tool returns a non-zero exit code.
    #[arg(
        long,
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        overrides_with = "trust_exit_code"
    )]
    no_trust_exit_code: Option<bool>,

    /// specify a custom command for viewing diffs.
    #[arg(
        short = 'x',
        long,
        value_name = "command",
        conflicts_with = "gui",
        conflicts_with = "tool"
    )]
    extcmd: Option<String>,

    /// make `git diff` compare non-git paths.
    #[arg(long, conflicts_with = "dir_diff")]
    no_index: bool,

    /// view changes that have been staged for the next commit.
    #[arg(long, alias = "cached")]
    staged: bool,

    /// swap input file pairs.
    #[arg(short = 'R')]
    reverse: bool,

    /// reorder diffs according to the <file>.
    #[arg(short = 'O')]
    order_file: Option<String>,

    /// find filepair whose only one side contains the string.
    #[arg(short = 'S')]
    search: Option<String>,

    /// show all files diff when -S is used and hit is found.
    #[arg(long)]
    pickaxe_all: bool,

    /// Discard the files before the named <file> from the diff.
    #[arg(long, value_name = "<file>")]
    skip_to: Option<String>,

    /// Move the files before the named <file> to the end of the diff.
    #[arg(long, value_name = "<file>")]
    rotate_to: Option<String>,

    /// options are forwarded to `git diff`
    #[arg(
        // These are actualy positional arguments but we call them "options" here and in the
        // docstring to make the automatic "-h" help text match the "Usage:" synopsis.
        value_name = "<options>",
        required = false,
        value_terminator = "--"
    )]
    arguments: Vec<OsString>,

    /// path specs are forwarded to `git diff`
    #[arg(value_name = "<path>", last = true, value_hint=ValueHint::AnyPath)]
    pathspecs: Vec<OsString>,
}

impl DifftoolOptions {
    /// Set default values from the configuration when unspecified.
    fn update(&mut self, config: &DifftoolConfig) {
        if self.symlinks.is_none() {
            let symlinks = self.no_symlinks.is_none() && config.has_symlinks;
            self.symlinks = Some(symlinks);
        }
        if self.trust_exit_code.is_none() {
            let trust_exit_code = self.no_trust_exit_code.is_none() && config.trust_exit_code;
            self.trust_exit_code = Some(trust_exit_code);
        }
    }
}

/// Create a temporary directory that is deleted when the struct is dropped.
struct TmpDir {
    tmpdir: String,
    pub cleanup: bool,
}
impl TmpDir {
    fn new(pattern: &str) -> Option<Self> {
        let tmpdir = create_tmpdir(pattern)?;

        Some(Self {
            tmpdir,
            cleanup: true,
        })
    }

    /// Return the temporary directory as an owned PathBuf.
    fn to_pathbuf(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.tmpdir)
    }

    /// Return a &str reference
    fn as_str(&self) -> &str {
        &self.tmpdir
    }
}
impl Drop for TmpDir {
    fn drop(&mut self) {
        if self.cleanup {
            std::fs::remove_dir_all(&self.tmpdir).ok();
        }
    }
}

/// Parsed index info from `git diff --raw`.
#[derive(Debug)]
struct IndexInfo {
    oid_left: ObjectID,
    oid_right: ObjectID,
    mode_left: i32,
    mode_right: i32,
    status: char,
}

extern "C" fn difftool_config_callback(
    key: *const c_char,
    value: *const c_char,
    ctx: *const c_void,
    data: *mut c_void,
) -> i32 {
    // Safety: key points to a null-terminated C string.
    let maybe_key_str = unsafe { maybe_str_from_c_char_ptr(key) };
    // Safety: data points to a DifftoolConfig struct.
    let difftool_config = unsafe { &mut *(data as *mut DifftoolConfig) };

    match maybe_key_str {
        Some("core.symlinks") => {
            // Safety: key/value arguments are not null.
            difftool_config.has_symlinks = unsafe { git_config_bool(key, value) };

            0
        }
        Some("difftool.trustexitcode") => {
            // Safety: key/value arguments are not null.
            difftool_config.trust_exit_code = unsafe { git_config_bool(key, value) };

            0
        }
        _ => {
            // Safety: arguments are not null.
            unsafe { git_default_config(key, value, ctx, data) }
        }
    }
}

/// The main entry point for "git difftool".
///
/// # Safety
///
/// Pointers are not null.
#[no_mangle]
pub unsafe fn cmd_difftool(
    argc: i32,
    argv: *const *const c_char,
    prefix_ptr: *const c_char,
    repo: *mut c_void,
) -> i32 {
    let difftool_config = DifftoolConfig::new();
    let difftool_config_addr = std::ptr::addr_of!(difftool_config);
    let difftool_config_void_ptr = difftool_config_addr as *mut c_void;
    let difftool_config_callback_ptr = difftool_config_callback as *const ConfigFn;
    // Safety: repo_config is not null.
    unsafe { repo_config(repo, difftool_config_callback_ptr, difftool_config_void_ptr) };

    // Copy argv into an owned vector so that we can parse it directly.
    let mut args = vec![OsString::from("git-difftool")];
    // The "difftool" argument in position 0 is handled above and skipped here.
    for i in 1..argc {
        // Safety: argc represents valid offsets within argv.
        let arg_ptr = unsafe { *(argv.offset(i as isize)) };
        let arg_os_string = os_string_from_c_char_ptr(arg_ptr);
        args.push(arg_os_string);
    }

    let Ok(mut difftool_options) = DifftoolOptions::try_parse_from(args) else {
        DifftoolOptions::command().print_help().ok();
        return ExitCode::UsageError as i32;
    };
    if difftool_options.tool_help {
        return print_tool_help();
    }
    difftool_options.update(&difftool_config);

    // Safety: git_get_startup_info() is a valid function.
    let startup_info = unsafe { git_get_startup_info() };
    // Safety: startup_info points to a struct startup_info.
    let have_repository = unsafe { git_startup_info_have_repository(startup_info) } > 0;
    if !difftool_options.no_index && !have_repository {
        eprintln!("error: difftool requires a worktree or --no-index");
        return ExitCode::Error as i32;
    }

    if !difftool_options.no_index {
        // Safety: setup_work_tree() is a valid function.
        unsafe { setup_work_tree(repo) };
        // Safety: repo is non-null and functions are valid.
        let (git_dir_ptr, git_work_tree_ptr) =
            unsafe { (repo_get_git_dir(repo), repo_get_work_tree(repo)) };

        // Safety: git_dir_ptr is not null.
        let mut git_dir_pathbuf = unsafe { pathbuf_from_c_char_ptr(git_dir_ptr) };
        if !git_dir_pathbuf.is_absolute() {
            git_dir_pathbuf =
                std::path::absolute(git_dir_pathbuf.clone()).unwrap_or(git_dir_pathbuf);
        }
        let mut git_work_tree_pathbuf = pathbuf_from_c_char_ptr(git_work_tree_ptr);
        if !git_work_tree_pathbuf.is_absolute() {
            git_work_tree_pathbuf =
                std::path::absolute(git_work_tree_pathbuf.clone()).unwrap_or(git_work_tree_pathbuf);
        }
        std::env::set_var("GIT_DIR", git_dir_pathbuf);
        std::env::set_var("GIT_WORK_TREE", git_work_tree_pathbuf);
    }

    if difftool_options.gui.is_some() {
        std::env::set_var("GIT_MERGETOOL_GUI", "true");
    } else if difftool_options.no_gui.is_some() {
        std::env::set_var("GIT_MERGETOOL_GUI", "false");
    }

    if let Some(tool) = &difftool_options.tool {
        std::env::set_var("GIT_DIFF_TOOL", tool);
    }

    if let Some(extcmd) = &difftool_options.extcmd {
        std::env::set_var("GIT_DIFFTOOL_EXTCMD", extcmd);
    }

    let trust_exit_code = difftool_options.trust_exit_code.unwrap_or(false);
    std::env::set_var(
        "GIT_DIFFTOOL_TRUST_EXIT_CODE",
        if trust_exit_code { "true" } else { "false" },
    );

    /*
     * In directory diff mode, 'git-difftool--helper' is called once
     * to compare the a / b directories. In file diff mode, 'git diff'
     * will invoke a separate instance of 'git-difftool--helper' for
     * each file that changed.
     */
    let mut git_cmd: Vec<OsString> = vec!["git".into(), "diff".into()];
    if difftool_options.no_index {
        git_cmd.push("--no-index".into());
    }
    if difftool_options.staged {
        git_cmd.push("--staged".into());
    }
    if difftool_options.reverse {
        git_cmd.push("-R".into());
    }
    if let Some(order_file) = &difftool_options.order_file {
        git_cmd.push(format!("-O{order_file}").into());
    }
    if let Some(rotate_to_file) = &difftool_options.rotate_to {
        git_cmd.push(format!("--rotate-to={rotate_to_file}").into());
    }
    if let Some(search) = &difftool_options.search {
        git_cmd.push(format!("-S{search}").into());
    }
    if difftool_options.pickaxe_all {
        git_cmd.push("--pickaxe-all".into());
    }
    if let Some(skip_to_file) = &difftool_options.skip_to {
        git_cmd.push(format!("--skip-to={skip_to_file}").into());
    }
    if difftool_options.dir_diff {
        git_cmd.extend(["--raw".into(), "--no-abbrev".into(), "-z".into()]);
    }
    for arg in &difftool_options.arguments {
        git_cmd.push(arg.to_os_string());
    }
    if !difftool_options.pathspecs.is_empty() {
        git_cmd.push("--".into());
        for path in &difftool_options.pathspecs {
            git_cmd.push(path.to_os_string());
        }
    }

    let prefix = maybe_os_string_from_c_char_ptr(prefix_ptr);
    if difftool_options.dir_diff {
        return run_dir_diff(repo, &difftool_config, &difftool_options, &prefix, &git_cmd);
    }

    run_file_diff(&difftool_options, &prefix, &git_cmd)
}

/// Run "git mergetool --tool-mode=diff"
fn print_tool_help() -> i32 {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["mergetool", "--tool-help=diff"]);

    let Ok(status) = cmd.status() else {
        eprintln!("error: unable to execute 'git mergetool --tool-help=diff'");
        return ExitCode::Error as i32;
    };

    status.code().unwrap_or(ExitCode::Error as i32)
}

/// Run git difftool in the default per-file diff mode.
fn run_file_diff(
    difftool_options: &DifftoolOptions,
    maybe_prefix: &Option<OsString>,
    git_cmd: &Vec<OsString>,
) -> i32 {
    let mut cmd = std::process::Command::new(&git_cmd[0]);
    cmd.args(&git_cmd[1..]);
    if let Some(prefix) = maybe_prefix {
        cmd.current_dir(prefix);
    }

    cmd.env("GIT_PAGER", "");
    cmd.env("GIT_EXTERNAL_DIFF", "git-difftool--helper");
    if difftool_options.prompt.is_some() {
        cmd.env("GIT_DIFFTOOL_PROMPT", "true");
    } else if difftool_options.no_prompt.is_some() {
        cmd.env("GIT_DIFFTOOL_NO_PROMPT", "true");
    }

    match cmd
        .status()
        .map(|status| status.code().unwrap_or(ExitCode::Error as i32))
    {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: unable to execute {git_cmd:?}: {err}");
            ExitCode::Error as i32
        }
    }
}

/// Run git difftool in the full-tree dir diff mode.
fn run_dir_diff(
    repo: *mut c_void,
    difftool_config: &DifftoolConfig,
    difftool_options: &DifftoolOptions,
    prefix: &Option<OsString>,
    git_cmd: &Vec<OsString>,
) -> i32 {
    match _run_dir_diff(repo, difftool_config, difftool_options, prefix, git_cmd) {
        Ok(exit_code) => exit_code,
        Err(string) => {
            eprintln!("error: {string}");
            ExitCode::Error as i32
        }
    }
}

fn _run_dir_diff(
    repo: *mut c_void,
    difftool_config: &DifftoolConfig,
    difftool_options: &DifftoolOptions,
    maybe_prefix: &Option<OsString>,
    git_cmd: &Vec<OsString>,
) -> Result<i32, String> {
    // Setup temp directories
    let Some(mut tmpdir) = TmpDir::new("git-difftool.XXXXXX") else {
        return Err("unable to create tmpdir for git-difftool".to_string());
    };

    // Track submodule entries.
    let mut submodules = HashMap::new();
    // Track symlink entries.
    let mut symlinks = HashMap::new();
    // Track worktree paths to avoid processing duplicates.
    let mut work_tree_dups = HashSet::new();
    // Track files that have been modified in the worktree after the external tool exits.
    let mut work_tree_modified = HashSet::new();
    // Track files that have been modified in the tmpdir after the external tool exits.
    let mut tmp_tree_modified = HashSet::new();

    // Safety: repo is not null.
    let work_tree_ptr = unsafe { repo_get_work_tree(repo) };
    // Safety: work_tree_ptr is not null
    let mut work_tree_pathbuf = unsafe { pathbuf_from_c_char_ptr(work_tree_ptr) };
    if !work_tree_pathbuf.is_absolute() {
        work_tree_pathbuf =
            std::path::absolute(work_tree_pathbuf.clone()).unwrap_or(work_tree_pathbuf);
    }

    // Safety: repo is not null.
    let git_dir_ptr = unsafe { repo_get_git_dir(repo) };
    // Safety: git_dir_ptr is not null
    let mut git_dir_pathbuf = unsafe { pathbuf_from_c_char_ptr(git_dir_ptr) };
    if !git_dir_pathbuf.is_absolute() {
        git_dir_pathbuf = std::path::absolute(git_dir_pathbuf.clone()).unwrap_or(git_dir_pathbuf);
    }

    let mut left_dir = tmpdir.to_pathbuf();
    let mut right_dir = tmpdir.to_pathbuf();
    left_dir.push("left");
    right_dir.push("right");
    std::fs::create_dir(&left_dir)
        .map_err(|err| format!("unable to create {left_dir:?}: {err}"))?;
    std::fs::create_dir(&right_dir)
        .map_err(|err| format!("unable to create {right_dir:?}: {err}"))?;

    let mut left_base_dir = left_dir.to_string_lossy().to_string();
    let mut right_base_dir = right_dir.to_string_lossy().to_string();

    left_base_dir.push('/');
    right_base_dir.push('/');

    let left_base_dir_cstring = CString::new(left_base_dir.clone())
        .map_err(|err| format!("unable to create string left base dir C string: {err}"))?;
    let right_base_dir_cstring = CString::new(right_base_dir.clone())
        .map_err(|err| format!("unable to create string right base dir C string: {err}"))?;

    let mut left_checkout_state = Checkout {
        base_dir: left_base_dir_cstring.as_ptr(),
        base_dir_len: left_base_dir.len() as i32,
        ..Default::default()
    };
    left_checkout_state.fields.set_force(true);

    let mut right_checkout_state = Checkout {
        base_dir: right_base_dir_cstring.as_ptr(),
        base_dir_len: right_base_dir.len() as i32,
        ..Default::default()
    };
    right_checkout_state.fields.set_force(true);

    // Safety: repo is not null.
    let work_tree_index_state = unsafe { IndexState::new(repo) };

    #[cfg(unix)]
    {
        let owner_only = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&left_dir, owner_only.clone())
            .map_err(|err| format!("unable to set permissions on {left_dir:?}: {err}"))?;
        std::fs::set_permissions(&right_dir, owner_only.clone())
            .map_err(|err| format!("unable to set permissions on {right_dir:?}: {err}"))?;
    }

    let mut git_child = std::process::Command::new(&git_cmd[0]);
    git_child.args(&git_cmd[1..]);
    if let Some(prefix) = maybe_prefix {
        git_child.current_dir(prefix);
    }
    let git_spawn = git_child
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("unable to spawn: {git_cmd:?}: {err}"))?;

    let Some(stdout) = git_spawn.stdout else {
        return Err(format!("unable to capture stdout from: {git_cmd:?}"));
    };

    let mut buf = Vec::with_capacity(1024);
    let mut lpath_buf = Vec::with_capacity(256);
    let mut rpath_buf = Vec::with_capacity(256);
    let mut reader = std::io::BufReader::new(stdout);
    // Safety: repo is not null.
    let hash_algo = unsafe { repo_get_hash_algo(repo) };
    let hash_null_oid = unsafe { null_oid(hash_algo) as *const ObjectID };
    let mut count = 0;

    /* Build index info for left and right sides of the diff */
    loop {
        // Clear state from previous iteration.
        lpath_buf.clear();
        rpath_buf.clear();
        buf.clear();

        if !read_bytes_until_null(&mut reader, &mut buf)? {
            break;
        }
        let Ok(buf_str) = std::str::from_utf8(&buf) else {
            return Err(format!(
                "unable to convert from git diff output to utf8: {buf:?}"
            ));
        };
        let mut index_info = parse_index_info(hash_algo, buf_str)?;

        if !read_bytes_until_null(&mut reader, &mut lpath_buf)? {
            break;
        }
        let Ok(lpath_str) = std::str::from_utf8(&lpath_buf) else {
            return Err(format!(
                "unable to convert from git diff output to utf8: {lpath_buf:?}"
            ));
        };

        let src_path = lpath_str.to_string();
        count += 1;

        let dst_path = if index_info.status != 'C' && index_info.status != 'R' {
            src_path.clone()
        } else {
            if !read_bytes_until_null(&mut reader, &mut rpath_buf)? {
                break;
            }
            let Ok(rpath_str) = std::str::from_utf8(&rpath_buf) else {
                return Err(format!(
                    "unable to convert from git diff output to utf8: {rpath_buf:?}"
                ));
            };
            rpath_str.to_string()
        };

        if is_git_directory_link(index_info.mode_left)
            || is_git_directory_link(index_info.mode_right)
        {
            let left_value = format!("Subproject commit {}", hex_from_oid(&index_info.oid_left));
            let mut right_value =
                format!("Subproject commit {}", hex_from_oid(&index_info.oid_right));
            if index_info.oid_left != index_info.oid_right {
                right_value += "-dirty";
            }
            if src_path == dst_path {
                submodules.insert(src_path, (Some(left_value), Some(right_value)));
            } else {
                add_left_or_right(&mut submodules, &src_path, left_value, false);
                add_left_or_right(&mut submodules, &dst_path, right_value, true);
            }
            continue;
        }

        if is_link(index_info.mode_left) {
            let content = get_symlink(repo, difftool_config, &index_info.oid_left, &src_path)?;
            add_left_or_right(&mut symlinks, &src_path, content, false);
        }
        if is_link(index_info.mode_right) {
            let content = get_symlink(repo, difftool_config, &index_info.oid_right, &dst_path)?;
            add_left_or_right(&mut symlinks, &dst_path, content, true);
        }

        if index_info.mode_left != 0
            && index_info.status != 'C'
            && !checkout_path(
                index_info.mode_left,
                &index_info.oid_left,
                &src_path,
                &mut left_checkout_state,
            )
        {
            return Err(format!("could not write '{src_path}'"));
        }

        if index_info.mode_right != 0 && !is_link(index_info.mode_right) {
            if work_tree_dups.contains(&dst_path) {
                continue;
            }
            work_tree_dups.insert(dst_path.clone());

            if !use_work_tree_file(
                repo,
                &work_tree_pathbuf,
                &dst_path,
                &mut index_info.oid_right,
            ) {
                if !checkout_path(
                    index_info.mode_right,
                    &index_info.oid_right,
                    &dst_path,
                    &mut right_checkout_state,
                ) {
                    return Err(format!("could not write '{dst_path}'"));
                }
            } else if unsafe { // Safety: hash_null_oid is not null.
                index_info.oid_right != *hash_null_oid
            } {
                // Worktree edits are not part of the index and need special handling.
                let Ok(dst_path_cstring) = CString::new(dst_path.clone()) else {
                    return Err(format!("unable to create C string from {dst_path}"));
                };

                // Safety: pointers are not null.
                let cache_entry = unsafe {
                    make_cache_entry(
                        work_tree_index_state.as_mut_ptr(),
                        index_info.mode_right as u32,
                        &index_info.oid_right,
                        dst_path_cstring.as_ptr(),
                        0,
                        0,
                    )
                };

                // Safety: pointers are not null.
                unsafe {
                    add_index_entry(
                        work_tree_index_state.as_mut_ptr(),
                        cache_entry,
                        AddCache::JustAppend as i32,
                    );
                }

                let mut right_file = right_dir.to_path_buf();
                right_file.push(&dst_path);
                let Some(right_file_parent) = right_file.parent() else {
                    return Err(format!("unable to get parent directory for {right_file:?}"));
                };
                std::fs::create_dir_all(right_file_parent).map_err(|err| {
                    format!("could not create parent directory for {right_file:?}: {err}")
                })?;

                let mut worktree_file = work_tree_pathbuf.to_path_buf();
                worktree_file.push(&dst_path);

                if difftool_options.symlinks.unwrap_or(false) {
                    std::os::unix::fs::symlink(&worktree_file, &right_file).map_err(|err| {
                        format!(
                            "could not symlink '{}' to '{}': {err}",
                            worktree_file.to_string_lossy(),
                            right_file.to_string_lossy(),
                        )
                    })?;
                } else {
                    std::fs::copy(&worktree_file, &right_file).map_err(|err| {
                        format!(
                            "could not copy '{}' to '{}': {err}",
                            worktree_file.to_string_lossy(),
                            right_file.to_string_lossy(),
                        )
                    })?;
                }
            }
        }
    }

    if count == 0 {
        return Ok(ExitCode::Success as i32);
    }

    // Changes to submodules require special treatment. This loop writes a temporary file to both
    // the left and right directories to show the change in the recorded submodule object ID.
    write_standin_files(&submodules, &left_dir, &right_dir)?;

    // Symbolic links require special treatment. "git diff" shows only the link itself, not the
    // contents of the link target. This loop replicates that behavior.
    write_standin_files(&symlinks, &left_dir, &right_dir)?;

    let mut cmd: Vec<&str> = Vec::new();
    if let Some(extcmd) = &difftool_options.extcmd {
        cmd.push(extcmd);
    } else {
        cmd.extend(["git", "difftool--helper"]);
        std::env::set_var("GIT_DIFFTOOL_DIRDIFF", "true");
    }
    let left_dir_string = left_dir.to_string_lossy().to_string();
    let right_dir_string = right_dir.to_string_lossy().to_string();
    cmd.push(left_dir_string.as_str());
    cmd.push(right_dir_string.as_str());

    let ret = std::process::Command::new(cmd[0])
        .args(&cmd[1..])
        .status()
        .map_err(|err| format!("unable to execute {cmd:?}: {err}"))?;
    let mut exit_code = ret.code().unwrap_or(ExitCode::Error as i32);

    // TODO: audit for interaction with sparse-index
    // Safety: index_state pointer is not null.
    unsafe { ensure_full_index(work_tree_index_state.as_mut_ptr()) };

    let mut indices_loaded = false;
    let mut errored = false;

    // If the diff includes worktree files and those files were modified during the diff, then
    // the changes should be copied back to the worktree. Do not copy back files when symlinks
    // are used and the external tool did not replace the original link with a file.
    //
    // These hashes are loaded lazily since they aren't needed in the common case of --symlinks and
    // the difftool updating files through the symlink.

    // Safety: worktree_index_state is not null.
    let cache_nr = unsafe { index_state_get_cache_nr(work_tree_index_state.as_ptr()) } as usize;

    for idx in 0..cache_nr {
        let name = unsafe { index_state_get_cache_entry_name(work_tree_index_state.as_ptr(), idx) };
        // Safety: name is not null.
        let name_os_string = unsafe { os_string_from_c_char_ptr(name) };
        let name_string = name_os_string.to_string_lossy().to_string();

        let mut right_path = right_dir.to_path_buf();
        right_path.push(&name_os_string);

        let right_path_string = right_path.to_string_lossy().to_string();
        let right_path_cstring = CString::new(right_path_string.clone())
            .map_err(|err| format!("unable to create C string from {right_path_string}: {err}"))?;

        let mut file_stat: libc::stat = unsafe { std::mem::zeroed() };
        let lstat_ret = unsafe {
            libc::lstat(
                right_path_cstring.as_ptr(),
                std::ptr::addr_of_mut!(file_stat),
            )
        };
        if lstat_ret != 0 {
            continue;
        }

        if (difftool_options.symlinks.unwrap_or(false) && is_link(file_stat.st_mode as i32))
            || !is_regular_file(file_stat.st_mode as i32)
        {
            continue;
        }

        if !indices_loaded {
            let mut work_tree_index_pathbuf = tmpdir.to_pathbuf();
            work_tree_index_pathbuf.push("wtindex");
            let work_tree_index_string = work_tree_index_pathbuf.to_string_lossy().to_string();

            let mut lock_file = LockFile::default();
            if hold_lock_file_for_update(&mut lock_file, &work_tree_index_string, 0) < 0
                // Safety: arguments are not null.
                || unsafe {
                    write_locked_index(work_tree_index_state.as_mut_ptr(), lock_file.as_mut_ptr(), COMMIT_LOCK)
                } != 0
            {
                return Err(format!("could not write {work_tree_index_pathbuf:?}"));
            }

            query_changed_files(
                &mut work_tree_modified,
                &work_tree_index_string,
                &git_dir_pathbuf,
                &work_tree_pathbuf,
            )?;

            query_changed_files(
                &mut tmp_tree_modified,
                &work_tree_index_string,
                &git_dir_pathbuf,
                &right_dir,
            )?;

            indices_loaded = true;
        }

        if tmp_tree_modified.contains(&name_string) {
            let mut work_tree_path_pathbuf = work_tree_pathbuf.to_path_buf();
            work_tree_path_pathbuf.push(&name_os_string);

            if work_tree_modified.contains(&name_string) {
                errored = true;
                eprintln!(
                    "warning: both files modified: '{}' and '{}'.",
                    work_tree_path_pathbuf.to_string_lossy(),
                    right_path_string,
                );
            } else {
                if let Err(err) = std::fs::remove_file(&work_tree_path_pathbuf) {
                    errored = true;
                    eprintln!(
                        "warning: could not remove '{}': {err}",
                        work_tree_path_pathbuf.to_string_lossy(),
                    );
                    continue;
                }
                if let Err(err) = std::fs::copy(&right_path, &work_tree_path_pathbuf) {
                    errored = true;
                    eprintln!(
                        "could not copy '{}' to '{}': {err}",
                        right_path_string,
                        work_tree_path_pathbuf.to_string_lossy(),
                    );
                }
            }
        }
    }

    if errored {
        tmpdir.cleanup = false;
        exit_code = ExitCode::Failure as i32;
        eprintln!("warning: temporary files exist in '{}'.", tmpdir.as_str());
        eprintln!("warning: you may want to cleanup or recover these.");
    } else if exit_code != 0 {
        eprintln!("warning: failed: {exit_code}");
    }
    if exit_code < 0 {
        exit_code = ExitCode::Failure as i32;
    }

    Ok(exit_code)
}

/// Add an entry to the hashmap. The value being added is only one of the two
/// sides of the tuple. Which tuple position to update is controlled by the
/// is_right parameter.
fn add_left_or_right(
    hashmap: &mut HashMap<String, (Option<String>, Option<String>)>,
    key: &String,
    value: String,
    is_right: bool,
) {
    // If the entry exists then we will update the entry, otherwise
    // we will create a new (partial) entry with just one side set.
    if let Some(existing_entry) = hashmap.get_mut(key) {
        if is_right {
            existing_entry.1 = Some(value);
        } else {
            existing_entry.0 = Some(value);
        }
    } else {
        let new_entry = if is_right {
            (None, Some(value))
        } else {
            (Some(value), None)
        };
        hashmap.insert(key.clone(), new_entry);
    }
}

/// Create a temporary directory.
/// Params:
/// - pattern: a mkdtemp basename pattern.
fn create_tmpdir(pattern: &str) -> Option<String> {
    // Setup temp directories
    let tmpdir = std::env::var("TMPDIR").unwrap_or("/tmp".to_string());
    let Ok(mut tmpdir) = std::path::PathBuf::from(tmpdir).canonicalize() else {
        return None;
    };
    tmpdir.push(pattern);

    let strbuf = tmpdir.to_str()?.to_string();
    let cstr = CString::new(strbuf).ok()?;
    // Safety: cstr transferred ownershp to C.
    let tmpdir_ptr = unsafe { libc::mkdtemp(cstr.into_raw()) };
    if tmpdir_ptr.is_null() {
        return None;
    }
    // Safety: tmpdir_ptr ownership can be reclaimed.
    let cstr = unsafe { std::ffi::CString::from_raw(tmpdir_ptr) };

    Some(cstr.to_string_lossy().to_string())
}

/// Parse an octal number from a str slice
fn parse_octal(octal_str: &str) -> Option<i32> {
    i32::from_str_radix(octal_str, 8).ok()
}

/// Parse index info from `git diff --raw` output
fn parse_index_info(hash_algo: *const c_void, buf_str: &str) -> Result<IndexInfo, String> {
    // The section being parsed looks like this:
    // :100644 100644 480a84738811a49290ffc8e72c56f0a6fbdb9f4b 8bd554f77af18d7b35fb71dfad8056007a79ab02 M
    let mut offset = 0;
    let buf_len = buf_str.len();

    // Safety: hash_algo is not null.
    let hexsz = unsafe { git_hash_algo_get_hexsz(hash_algo) } as usize;
    let buf_start = &buf_str[offset..offset + 1];
    offset += 1;
    if buf_start != ":" {
        return Err(format!("expected ':', got '{buf_start}'"));
    }
    if buf_len <= offset + 6 {
        let buf_offset = &buf_str[offset..];
        return Err(format!("expected octal mode, got truncated '{buf_offset}'"));
    }

    let left_mode_str = &buf_str[offset..offset + 6];
    offset += 6;
    let Some(left_mode) = parse_octal(left_mode_str) else {
        return Err(format!("expected octal mode, got '{left_mode_str}'"));
    };

    if buf_len <= offset + 1 {
        return Err(
            "expected space after first mode, got truncated value".to_string()
        );
    }
    let space_str = &buf_str[offset..offset + 1];
    offset += 1;
    if space_str != " " {
        return Err(format!("expected ' ', got '{space_str}'"));
    }

    if buf_len <= offset + 6 {
        let buf_offset = &buf_str[offset..];
        return Err(format!("expected octal mode, got truncated '{buf_offset}'"));
    }

    let right_mode_str = &buf_str[offset..offset + 6];
    offset += 6;
    let Some(right_mode) = parse_octal(right_mode_str) else {
        return Err(format!("expected octal mode, got '{right_mode_str}'"));
    };

    if buf_len <= offset + 1 {
        return Err(
            "expected space after 2nd mode, got truncated value".to_string()
        );
    }
    let space_str = &buf_str[offset..offset + 1];
    offset += 1;
    if space_str != " " {
        return Err(format!("expected ' ', got '{space_str}'"));
    }

    if buf_len <= offset + hexsz {
        let left_hex_str = &buf_str[offset..];
        return Err(format!(
            "expected hex object id, got truncated '{left_hex_str}'"
        ));
    }
    let left_hex_str = &buf_str[offset..offset + hexsz];
    offset += hexsz;

    let hash_null_oid = unsafe { null_oid(hash_algo) as *const ObjectID };
    let mut left_oid = unsafe { (*hash_null_oid).clone() };
    let left_hex_ptr = left_hex_str.as_ptr() as *const c_char;
    let left_oid_ptr = std::ptr::addr_of_mut!(left_oid);
    // Safety: pointers are not null.
    let ret = unsafe { get_oid_hex_algop(left_hex_ptr, left_oid_ptr, hash_algo) };
    if ret != 0 {
        return Err(format!("expected hex object id, got '{left_hex_str}'"));
    }

    if buf_len <= offset + 1 {
        return Err(
            "expected space after hex object id, got truncated value".to_string()
        );
    }
    let space_str = &buf_str[offset..offset + 1];
    offset += 1;
    if space_str != " " {
        return Err(format!("expected ' ', got '{space_str}'"));
    }

    if buf_len <= offset + hexsz {
        let right_hex_str = &buf_str[offset..];
        return Err(format!(
            "expected hex object id, got truncated '{right_hex_str}'"
        ));
    }
    let right_hex_str = &buf_str[offset..offset + hexsz];
    offset += hexsz;

    let right_hex_ptr = right_hex_str.as_ptr() as *const c_char;
    let right_null_oid = unsafe { null_oid(hash_algo) as *const ObjectID };
    let mut right_oid = unsafe { (*right_null_oid).clone() };
    let right_oid_ptr = std::ptr::addr_of_mut!(right_oid);

    // Safety: pointers are not null.
    let ret = unsafe { get_oid_hex_algop(right_hex_ptr, right_oid_ptr, hash_algo) };
    if ret != 0 {
        return Err(format!("expected hex object id, got '{right_hex_str}'"));
    }

    if buf_len <= offset + 1 {
        return Err("expected space after hex object id, got truncated value".to_string());
    }
    let space_str = &buf_str[offset..offset + 1];
    offset += 1;
    if space_str != " " {
        return Err("expected ' ', got '{space_str}'".to_string());
    }

    if buf_len < offset + 1 {
        return Err("expected status, got truncated value".to_string());
    }
    let status_str = &buf_str[offset..offset + 1];
    offset += 1;
    let Some(status_u8) = status_str.bytes().next() else {
        return Err("missing status: invalid bytes".to_string());
    };
    let status = status_u8 as char;
    if status == '\0' {
        return Err("missing status: null entry".to_string());
    }

    if buf_len > offset + 1 {
        let trailer = &buf_str[offset..offset + 1];
        let Some(trailer_char) = trailer.chars().next() else {
            return Err(format!("cannot index into trailer: '{trailer}'"));
        };
        // Allow e.g. "R80" (rename 80% similar)
        if !trailer_char.is_ascii_digit() {
            return Err(format!("unexpected trailer: '{trailer}'"));
        }
    }

    Ok(IndexInfo {
        mode_left: left_mode,
        mode_right: right_mode,
        oid_left: left_oid,
        oid_right: right_oid,
        status,
    })
}

/// Unconditional writing of a plain regular file is what
/// "git difftool --dir-diff" wants to do for symlinks.  We are preparing two
/// temporary directories to be fed to a Git-unaware tool that knows how to
/// show a diff of two directories (e.g. "diff -r A B").
///
/// Because the tool is Git-unaware, if a symbolic link appears in either of
/// these temporary directories, it will try to dereference and show the
/// difference of the target of the symbolic link, which is not what we want,
/// as the goal of the dir-diff mode is to produce an output that is logically
/// equivalent to what "git diff" produces.
///
/// Most importantly, we want to get textual comparison of the result of the
/// readlink(2).  get_symlink() provides that---it returns the contents of
/// the symlink that gets written to a regular file to force the external tool
/// to compare the readlink(2) result as text, even on a filesystem that is
/// capable of doing a symbolic link.
fn get_symlink(
    repo: *mut std::ffi::c_void,
    config: &DifftoolConfig,
    oid: &ObjectID,
    path: &str,
) -> Result<String, String> {
    // Safety: repo is not null.
    let hash_algo = unsafe { repo_get_hash_algo(repo) };
    // Safety: hash_algo is not null.
    let hash_null_oid = unsafe { null_oid(hash_algo) as *const ObjectID };

    // Safety: hash_null_oid is not null.
    if unsafe { oid == &*hash_null_oid } {
        if config.has_symlinks {
            let value = std::fs::read_link(path)
                .map_err(|err| format!("could not read symlink {path}: {err}"))?;

            Ok(value.to_string_lossy().to_string())
        } else {
            let value = std::fs::read_to_string(path)
                .map_err(|err| format!("could not read symlink file {path}: {err}"))?;

            Ok(value)
        }
    } else {
        // Safety: repo is not null.
        let odb = unsafe { repo_get_object_database(repo) };
        let mut object_type = ObjectType::None;
        if let Some(value) = unsafe { odb_read_object_into_utf8_string(odb, oid, &mut object_type) } {
            Ok(value)
        } else {
            let oid_hex = hex_from_oid(oid);

            Err(format!("could not read object {oid_hex} for symlink {path}"))
        }
    }
}

/// Checkout a path to disk.
fn checkout_path(mode: i32, oid: &ObjectID, path: &str, state: &mut Checkout) -> bool {
    let Ok(path_cstr) = CString::new(path) else {
        return false;
    };
    // Safety: cache_entry is allocated by C.
    let cache_entry = unsafe {
        make_transient_cache_entry(
            mode as u32,
            std::ptr::addr_of!(*oid),
            path_cstr.as_ptr(),
            0,
            std::ptr::null_mut(),
        )
    };
    // Safety: The path must not exist and cache_entry is not null.
    let ret = unsafe {
        checkout_entry_ca(
            cache_entry,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(*state),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    // Safety: cache_entry must be freed by C.
    unsafe {
        discard_cache_entry(cache_entry);
    }

    ret == 0
}

/// Can we use the worktree file directly when in dir-diff mode?
fn use_work_tree_file(
    repo: *mut std::ffi::c_void,
    work_tree_path: &std::path::Path,
    path: &str,
    oid: &mut ObjectID,
) -> bool {
    // Safety: repo is not null.
    let hash_algo = unsafe { repo_get_hash_algo(repo) };
    // Safety: hash_algo is not null.
    let hash_null_oid = unsafe { null_oid(hash_algo) as *const ObjectID };
    let mut file_path = work_tree_path.to_path_buf();
    file_path.push(path);

    let Some(file_path_str) = file_path.to_str() else {
        return false;
    };
    let Ok(file_path_cstr) = CString::new(file_path_str) else {
        return false;
    };

    // Safety: struct_stat will be initialized by lstat().
    let mut file_stat: libc::stat = unsafe { std::mem::zeroed() };
    // Safety: struct_stat is a valid libc struct.
    let lstat_ret =
        unsafe { libc::lstat(file_path_cstr.as_ptr(), std::ptr::addr_of_mut!(file_stat)) };

    if lstat_ret == 0 && !is_link(file_stat.st_mode as i32) {
        let mut full_path = work_tree_path.to_path_buf();
        full_path.push(path);
        let full_path_str = full_path.to_string_lossy().to_string();
        let Ok(full_path_cstring) = CString::new(full_path_str) else {
            return false;
        };
        let Ok(path_cstring) = CString::new(path) else {
            return false;
        };

        // Safety: pointer is not null.
        let fd = unsafe { libc::open(full_path_cstring.as_ptr(), libc::O_RDONLY) };
        if fd < 0 {
            return false;
        }

        let obj_blob = ObjectType::Blob as i32;
        let mut work_tree_oid = unsafe { (*hash_null_oid).clone() };
        // Safety: repo is not null.
        let index = unsafe { repo_get_index_state(repo) };
        // Safety: index is not null.
        let ret = unsafe {
            index_fd(
                index,
                std::ptr::addr_of_mut!(work_tree_oid),
                fd,
                std::ptr::addr_of_mut!(file_stat),
                obj_blob,
                path_cstring.as_ptr(),
                0,
            )
        };
        if ret == 0 {
            // Safety: hash_null_oid is not null.
            if unsafe { oid == &*hash_null_oid } {
                *oid = work_tree_oid;
                return true;
            } else if oid == &work_tree_oid {
                return true;
            }
        }
    }

    false
}

/// Write the left and right standin file entries to the specified base directories.
fn write_standin_files(
    entries: &HashMap<String, (Option<String>, Option<String>)>,
    left_dir: &std::path::Path,
    right_dir: &std::path::Path,
) -> Result<(), String> {
    for entry in entries {
        let path = &entry.0;
        let (left_value_opt, right_value_opt) = &entry.1;
        maybe_write_standin_file(left_dir, path, left_value_opt)?;
        maybe_write_standin_file(right_dir, path, right_value_opt)?;
    }

    Ok(())
}

/// Write a standin file if the value is Some.
fn maybe_write_standin_file(
    base_dir: &std::path::Path,
    path: &str,
    value_opt: &Option<String>,
) -> Result<(), String> {
    if let Some(raw_value) = value_opt {
        let mut value_with_newline;
        let value = if raw_value.ends_with('\n') {
            raw_value
        } else {
            value_with_newline = raw_value.to_string();
            value_with_newline.push('\n');
            &value_with_newline
        };
        let mut filename = base_dir.to_path_buf();
        filename.push(path);
        let Some(parent_dir) = filename.parent() else {
            return Err(format!("unable to get parent directory for {filename:?}"));
        };
        std::fs::create_dir_all(parent_dir).map_err(|err| {
            format!("could not create directory for '{path}' in {parent_dir:?}: {err}")
        })?;
        std::fs::remove_file(&filename).ok();
        std::fs::write(&filename, value)
            .map_err(|err| format!("could not write '{path}' content to {filename:?}: {err}"))?;
    }

    Ok(())
}

fn query_changed_files(
    modified_files: &mut HashSet<String>,
    index_file: &str,
    git_dir: &std::path::Path,
    work_tree: &std::path::Path,
) -> Result<(), String> {
    let git_dir_string = git_dir.to_string_lossy().to_string();
    let work_tree_string = work_tree.to_string_lossy().to_string();
    // Populate the index file using "git update-index".
    let update_index_cmd_vec = [
        "git",
        "--git-dir",
        &git_dir_string,
        "--work-tree",
        &work_tree_string,
        "update-index",
        "--really-refresh",
        "-q",
        "--unmerged",
    ];
    let mut update_index_cmd = std::process::Command::new(update_index_cmd_vec[0]);
    update_index_cmd
        .args(&update_index_cmd_vec[1..])
        .current_dir(work_tree)
        .env("GIT_INDEX_FILE", index_file)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    update_index_cmd.status().ok(); // Errors are intentionally ignored.

    // Use the newly-populated index to query modified files.
    let mut buf = Vec::with_capacity(256);
    let diff_files_cmd_vec = [
        "git",
        "--git-dir",
        &git_dir_string,
        "--work-tree",
        &work_tree_string,
        "diff-files",
        "--name-only",
        "-z",
    ];
    let mut diff_files_cmd = std::process::Command::new(diff_files_cmd_vec[0]);
    diff_files_cmd
        .args(&diff_files_cmd_vec[1..])
        .current_dir(work_tree)
        .env("GIT_INDEX_FILE", index_file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped());

    let diff_files_spawn = diff_files_cmd
        .spawn()
        .map_err(|err| format!("unable to spawn: {diff_files_cmd:?}: {err}"))?;

    let Some(stdout) = diff_files_spawn.stdout else {
        return Err(format!("unable to capture stdout from: {diff_files_cmd:?}"));
    };
    let mut reader = std::io::BufReader::new(stdout);
    loop {
        buf.clear(); // Clear state from previous iteration.
        if !read_bytes_until_null(&mut reader, &mut buf)? {
            break;
        }
        let path = String::from_utf8_lossy(&buf).to_string();
        modified_files.insert(path);
    }

    Ok(())
}

fn read_bytes_until_null<R: std::io::BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> Result<bool, String> {
    let num_bytes = reader
        .read_until(0u8, buf)
        .map_err(|err| format!("unable to read from stdout: {err}"))?;
    if buf.last() == Some(&0u8) {
        buf.pop();
    }

    Ok(num_bytes > 0 && !buf.is_empty())
}
