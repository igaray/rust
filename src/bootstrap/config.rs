//! Serialized configuration of a build.
//!
//! This module implements parsing `config.toml` configuration files to tweak
//! how the build runs.

use std::cmp;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::cache::{Interned, INTERNER};
use crate::flags::Flags;
pub use crate::flags::Subcommand;
use build_helper::t;
use serde::Deserialize;

/// Global configuration for the entire build and/or bootstrap.
///
/// This structure is derived from a combination of both `config.toml` and
/// `config.mk`. As of the time of this writing it's unlikely that `config.toml`
/// is used all that much, so this is primarily filled out by `config.mk` which
/// is generated from `./configure`.
///
/// Note that this structure is not decoded directly into, but rather it is
/// filled out from the decoded forms of the structs below. For documentation
/// each field, see the corresponding fields in
/// `config.toml.example`.
#[derive(Default)]
pub struct Config {
    pub ccache: Option<String>,
    /// Call Build::ninja() instead of this.
    pub ninja_in_file: bool,
    pub verbose: usize,
    pub submodules: bool,
    pub fast_submodules: bool,
    pub compiler_docs: bool,
    pub docs: bool,
    pub locked_deps: bool,
    pub vendor: bool,
    pub target_config: HashMap<TargetSelection, Target>,
    pub full_bootstrap: bool,
    pub extended: bool,
    pub tools: Option<HashSet<String>>,
    pub sanitizers: bool,
    pub profiler: bool,
    pub ignore_git: bool,
    pub exclude: Vec<PathBuf>,
    pub rustc_error_format: Option<String>,
    pub json_output: bool,
    pub test_compare_mode: bool,
    pub llvm_libunwind: bool,

    pub skip_only_host_steps: bool,

    pub on_fail: Option<String>,
    pub stage: Option<u32>,
    pub keep_stage: Vec<u32>,
    pub src: PathBuf,
    pub jobs: Option<u32>,
    pub cmd: Subcommand,
    pub incremental: bool,
    pub dry_run: bool,

    pub deny_warnings: bool,
    pub backtrace_on_ice: bool,

    // llvm codegen options
    pub llvm_skip_rebuild: bool,
    pub llvm_assertions: bool,
    pub llvm_optimize: bool,
    pub llvm_thin_lto: bool,
    pub llvm_release_debuginfo: bool,
    pub llvm_version_check: bool,
    pub llvm_static_stdcpp: bool,
    pub llvm_link_shared: bool,
    pub llvm_clang_cl: Option<String>,
    pub llvm_targets: Option<String>,
    pub llvm_experimental_targets: Option<String>,
    pub llvm_link_jobs: Option<u32>,
    pub llvm_version_suffix: Option<String>,
    pub llvm_use_linker: Option<String>,
    pub llvm_allow_old_toolchain: Option<bool>,

    pub use_lld: bool,
    pub lld_enabled: bool,
    pub llvm_tools_enabled: bool,

    pub llvm_cflags: Option<String>,
    pub llvm_cxxflags: Option<String>,
    pub llvm_ldflags: Option<String>,
    pub llvm_use_libcxx: bool,

    // rust codegen options
    pub rust_optimize: bool,
    pub rust_codegen_units: Option<u32>,
    pub rust_codegen_units_std: Option<u32>,
    pub rust_debug_assertions: bool,
    pub rust_debug_assertions_std: bool,
    pub rust_debuginfo_level_rustc: u32,
    pub rust_debuginfo_level_std: u32,
    pub rust_debuginfo_level_tools: u32,
    pub rust_debuginfo_level_tests: u32,
    pub rust_rpath: bool,
    pub rustc_parallel: bool,
    pub rustc_default_linker: Option<String>,
    pub rust_optimize_tests: bool,
    pub rust_dist_src: bool,
    pub rust_codegen_backends: Vec<Interned<String>>,
    pub rust_verify_llvm_ir: bool,
    pub rust_thin_lto_import_instr_limit: Option<u32>,
    pub rust_remap_debuginfo: bool,
    pub rust_new_symbol_mangling: bool,

    pub build: TargetSelection,
    pub hosts: Vec<TargetSelection>,
    pub targets: Vec<TargetSelection>,
    pub local_rebuild: bool,
    pub jemalloc: bool,
    pub control_flow_guard: bool,

    // dist misc
    pub dist_sign_folder: Option<PathBuf>,
    pub dist_upload_addr: Option<String>,
    pub dist_gpg_password_file: Option<PathBuf>,

    // libstd features
    pub backtrace: bool, // support for RUST_BACKTRACE

    // misc
    pub low_priority: bool,
    pub channel: String,
    pub verbose_tests: bool,
    pub save_toolstates: Option<PathBuf>,
    pub print_step_timings: bool,
    pub missing_tools: bool,

    // Fallback musl-root for all targets
    pub musl_root: Option<PathBuf>,
    pub prefix: Option<PathBuf>,
    pub sysconfdir: Option<PathBuf>,
    pub datadir: Option<PathBuf>,
    pub docdir: Option<PathBuf>,
    pub bindir: PathBuf,
    pub libdir: Option<PathBuf>,
    pub mandir: Option<PathBuf>,
    pub codegen_tests: bool,
    pub nodejs: Option<PathBuf>,
    pub gdb: Option<PathBuf>,
    pub python: Option<PathBuf>,
    pub cargo_native_static: bool,
    pub configure_args: Vec<String>,

    // These are either the stage0 downloaded binaries or the locally installed ones.
    pub initial_cargo: PathBuf,
    pub initial_rustc: PathBuf,
    pub initial_rustfmt: Option<PathBuf>,
    pub out: PathBuf,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetSelection {
    pub triple: Interned<String>,
    file: Option<Interned<String>>,
}

impl TargetSelection {
    pub fn from_user(selection: &str) -> Self {
        let path = Path::new(selection);

        let (triple, file) = if path.exists() {
            let triple = path
                .file_stem()
                .expect("Target specification file has no file stem")
                .to_str()
                .expect("Target specification file stem is not UTF-8");

            (triple, Some(selection))
        } else {
            (selection, None)
        };

        let triple = INTERNER.intern_str(triple);
        let file = file.map(|f| INTERNER.intern_str(f));

        Self { triple, file }
    }

    pub fn rustc_target_arg(&self) -> &str {
        self.file.as_ref().unwrap_or(&self.triple)
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.triple.contains(needle)
    }

    pub fn starts_with(&self, needle: &str) -> bool {
        self.triple.starts_with(needle)
    }

    pub fn ends_with(&self, needle: &str) -> bool {
        self.triple.ends_with(needle)
    }
}

impl fmt::Display for TargetSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.triple)?;
        if let Some(file) = self.file {
            write!(f, "({})", file)?;
        }
        Ok(())
    }
}

impl PartialEq<&str> for TargetSelection {
    fn eq(&self, other: &&str) -> bool {
        self.triple == *other
    }
}

/// Per-target configuration stored in the global configuration structure.
#[derive(Default)]
pub struct Target {
    /// Some(path to llvm-config) if using an external LLVM.
    pub llvm_config: Option<PathBuf>,
    /// Some(path to FileCheck) if one was specified.
    pub llvm_filecheck: Option<PathBuf>,
    pub cc: Option<PathBuf>,
    pub cxx: Option<PathBuf>,
    pub ar: Option<PathBuf>,
    pub ranlib: Option<PathBuf>,
    pub linker: Option<PathBuf>,
    pub ndk: Option<PathBuf>,
    pub crt_static: Option<bool>,
    pub musl_root: Option<PathBuf>,
    pub musl_libdir: Option<PathBuf>,
    pub wasi_root: Option<PathBuf>,
    pub qemu_rootfs: Option<PathBuf>,
    pub no_std: bool,
}

impl Target {
    pub fn from_triple(triple: &str) -> Self {
        let mut target: Self = Default::default();
        if triple.contains("-none") || triple.contains("nvptx") {
            target.no_std = true;
        }
        target
    }
}
/// Structure of the `config.toml` file that configuration is read from.
///
/// This structure uses `Decodable` to automatically decode a TOML configuration
/// file into this format, and then this is traversed and written into the above
/// `Config` structure.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct TomlConfig {
    build: Option<Build>,
    install: Option<Install>,
    llvm: Option<Llvm>,
    rust: Option<Rust>,
    target: Option<HashMap<String, TomlTarget>>,
    dist: Option<Dist>,
}

/// TOML representation of various global build decisions.
#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Build {
    build: Option<String>,
    host: Option<Vec<String>>,
    target: Option<Vec<String>>,
    // This is ignored, the rust code always gets the build directory from the `BUILD_DIR` env variable
    build_dir: Option<String>,
    cargo: Option<String>,
    rustc: Option<String>,
    rustfmt: Option<String>, /* allow bootstrap.py to use rustfmt key */
    docs: Option<bool>,
    compiler_docs: Option<bool>,
    submodules: Option<bool>,
    fast_submodules: Option<bool>,
    gdb: Option<String>,
    nodejs: Option<String>,
    python: Option<String>,
    locked_deps: Option<bool>,
    vendor: Option<bool>,
    full_bootstrap: Option<bool>,
    extended: Option<bool>,
    tools: Option<HashSet<String>>,
    verbose: Option<usize>,
    sanitizers: Option<bool>,
    profiler: Option<bool>,
    cargo_native_static: Option<bool>,
    low_priority: Option<bool>,
    configure_args: Option<Vec<String>>,
    local_rebuild: Option<bool>,
    print_step_timings: Option<bool>,
}

/// TOML representation of various global install decisions.
#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Install {
    prefix: Option<String>,
    sysconfdir: Option<String>,
    docdir: Option<String>,
    bindir: Option<String>,
    libdir: Option<String>,
    mandir: Option<String>,
    datadir: Option<String>,

    // standard paths, currently unused
    infodir: Option<String>,
    localstatedir: Option<String>,
}

/// TOML representation of how the LLVM build is configured.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Llvm {
    skip_rebuild: Option<bool>,
    optimize: Option<bool>,
    thin_lto: Option<bool>,
    release_debuginfo: Option<bool>,
    assertions: Option<bool>,
    ccache: Option<StringOrBool>,
    version_check: Option<bool>,
    static_libstdcpp: Option<bool>,
    ninja: Option<bool>,
    targets: Option<String>,
    experimental_targets: Option<String>,
    link_jobs: Option<u32>,
    link_shared: Option<bool>,
    version_suffix: Option<String>,
    clang_cl: Option<String>,
    cflags: Option<String>,
    cxxflags: Option<String>,
    ldflags: Option<String>,
    use_libcxx: Option<bool>,
    use_linker: Option<String>,
    allow_old_toolchain: Option<bool>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Dist {
    sign_folder: Option<String>,
    gpg_password_file: Option<String>,
    upload_addr: Option<String>,
    src_tarball: Option<bool>,
    missing_tools: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrBool {
    String(String),
    Bool(bool),
}

impl Default for StringOrBool {
    fn default() -> StringOrBool {
        StringOrBool::Bool(false)
    }
}

/// TOML representation of how the Rust build is configured.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Rust {
    optimize: Option<bool>,
    debug: Option<bool>,
    codegen_units: Option<u32>,
    codegen_units_std: Option<u32>,
    debug_assertions: Option<bool>,
    debug_assertions_std: Option<bool>,
    debuginfo_level: Option<u32>,
    debuginfo_level_rustc: Option<u32>,
    debuginfo_level_std: Option<u32>,
    debuginfo_level_tools: Option<u32>,
    debuginfo_level_tests: Option<u32>,
    backtrace: Option<bool>,
    incremental: Option<bool>,
    parallel_compiler: Option<bool>,
    default_linker: Option<String>,
    channel: Option<String>,
    musl_root: Option<String>,
    rpath: Option<bool>,
    verbose_tests: Option<bool>,
    optimize_tests: Option<bool>,
    codegen_tests: Option<bool>,
    ignore_git: Option<bool>,
    dist_src: Option<bool>,
    save_toolstates: Option<String>,
    codegen_backends: Option<Vec<String>>,
    lld: Option<bool>,
    use_lld: Option<bool>,
    llvm_tools: Option<bool>,
    deny_warnings: Option<bool>,
    backtrace_on_ice: Option<bool>,
    verify_llvm_ir: Option<bool>,
    thin_lto_import_instr_limit: Option<u32>,
    remap_debuginfo: Option<bool>,
    jemalloc: Option<bool>,
    test_compare_mode: Option<bool>,
    llvm_libunwind: Option<bool>,
    control_flow_guard: Option<bool>,
    new_symbol_mangling: Option<bool>,
}

/// TOML representation of how each build target is configured.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct TomlTarget {
    cc: Option<String>,
    cxx: Option<String>,
    ar: Option<String>,
    ranlib: Option<String>,
    linker: Option<String>,
    llvm_config: Option<String>,
    llvm_filecheck: Option<String>,
    android_ndk: Option<String>,
    crt_static: Option<bool>,
    musl_root: Option<String>,
    musl_libdir: Option<String>,
    wasi_root: Option<String>,
    qemu_rootfs: Option<String>,
    no_std: Option<bool>,
}

impl Config {
    fn path_from_python(var_key: &str) -> PathBuf {
        match env::var_os(var_key) {
            Some(var_val) => Self::normalize_python_path(var_val),
            _ => panic!("expected '{}' to be set", var_key),
        }
    }

    /// Normalizes paths from Python slightly. We don't trust paths from Python (#49785).
    fn normalize_python_path(path: OsString) -> PathBuf {
        Path::new(&path).components().collect()
    }

    pub fn default_opts() -> Config {
        let mut config = Config::default();
        config.llvm_optimize = true;
        config.ninja_in_file = true;
        config.llvm_version_check = true;
        config.backtrace = true;
        config.rust_optimize = true;
        config.rust_optimize_tests = true;
        config.submodules = true;
        config.fast_submodules = true;
        config.docs = true;
        config.rust_rpath = true;
        config.channel = "dev".to_string();
        config.codegen_tests = true;
        config.ignore_git = false;
        config.rust_dist_src = true;
        config.rust_codegen_backends = vec![INTERNER.intern_str("llvm")];
        config.deny_warnings = true;
        config.missing_tools = false;

        // set by bootstrap.py
        config.build = TargetSelection::from_user(&env::var("BUILD").expect("'BUILD' to be set"));
        config.src = Config::path_from_python("SRC");
        config.out = Config::path_from_python("BUILD_DIR");

        config.initial_rustc = Config::path_from_python("RUSTC");
        config.initial_cargo = Config::path_from_python("CARGO");
        config.initial_rustfmt = env::var_os("RUSTFMT").map(Config::normalize_python_path);

        config
    }

    pub fn parse(args: &[String]) -> Config {
        let flags = Flags::parse(&args);
        let file = flags.config.clone();
        let mut config = Config::default_opts();
        config.exclude = flags.exclude;
        config.rustc_error_format = flags.rustc_error_format;
        config.json_output = flags.json_output;
        config.on_fail = flags.on_fail;
        config.stage = flags.stage;
        config.jobs = flags.jobs.map(threads_from_config);
        config.cmd = flags.cmd;
        config.incremental = flags.incremental;
        config.dry_run = flags.dry_run;
        config.keep_stage = flags.keep_stage;
        config.bindir = "bin".into(); // default
        if let Some(value) = flags.deny_warnings {
            config.deny_warnings = value;
        }

        if config.dry_run {
            let dir = config.out.join("tmp-dry-run");
            t!(fs::create_dir_all(&dir));
            config.out = dir;
        }

        let toml = file
            .map(|file| {
                let contents = t!(fs::read_to_string(&file));
                match toml::from_str(&contents) {
                    Ok(table) => table,
                    Err(err) => {
                        println!(
                            "failed to parse TOML configuration '{}': {}",
                            file.display(),
                            err
                        );
                        process::exit(2);
                    }
                }
            })
            .unwrap_or_else(TomlConfig::default);

        let build = toml.build.clone().unwrap_or_default();

        // If --target was specified but --host wasn't specified, don't run any host-only tests.
        let has_hosts = build.host.is_some() || flags.host.is_some();
        let has_targets = build.target.is_some() || flags.target.is_some();
        config.skip_only_host_steps = !has_hosts && has_targets;

        config.hosts = if let Some(arg_host) = flags.host.clone() {
            arg_host
        } else if let Some(file_host) = build.host {
            file_host.iter().map(|h| TargetSelection::from_user(h)).collect()
        } else {
            vec![config.build]
        };
        config.targets = if let Some(arg_target) = flags.target.clone() {
            arg_target
        } else if let Some(file_target) = build.target {
            file_target.iter().map(|h| TargetSelection::from_user(h)).collect()
        } else {
            // If target is *not* configured, then default to the host
            // toolchains.
            config.hosts.clone()
        };

        config.nodejs = build.nodejs.map(PathBuf::from);
        config.gdb = build.gdb.map(PathBuf::from);
        config.python = build.python.map(PathBuf::from);
        set(&mut config.low_priority, build.low_priority);
        set(&mut config.compiler_docs, build.compiler_docs);
        set(&mut config.docs, build.docs);
        set(&mut config.submodules, build.submodules);
        set(&mut config.fast_submodules, build.fast_submodules);
        set(&mut config.locked_deps, build.locked_deps);
        set(&mut config.vendor, build.vendor);
        set(&mut config.full_bootstrap, build.full_bootstrap);
        set(&mut config.extended, build.extended);
        config.tools = build.tools;
        set(&mut config.verbose, build.verbose);
        set(&mut config.sanitizers, build.sanitizers);
        set(&mut config.profiler, build.profiler);
        set(&mut config.cargo_native_static, build.cargo_native_static);
        set(&mut config.configure_args, build.configure_args);
        set(&mut config.local_rebuild, build.local_rebuild);
        set(&mut config.print_step_timings, build.print_step_timings);
        config.verbose = cmp::max(config.verbose, flags.verbose);

        if let Some(ref install) = toml.install {
            config.prefix = install.prefix.clone().map(PathBuf::from);
            config.sysconfdir = install.sysconfdir.clone().map(PathBuf::from);
            config.datadir = install.datadir.clone().map(PathBuf::from);
            config.docdir = install.docdir.clone().map(PathBuf::from);
            set(&mut config.bindir, install.bindir.clone().map(PathBuf::from));
            config.libdir = install.libdir.clone().map(PathBuf::from);
            config.mandir = install.mandir.clone().map(PathBuf::from);
        }

        // We want the llvm-skip-rebuild flag to take precedence over the
        // skip-rebuild config.toml option so we store it separately
        // so that we can infer the right value
        let mut llvm_skip_rebuild = flags.llvm_skip_rebuild;

        // Store off these values as options because if they're not provided
        // we'll infer default values for them later
        let mut llvm_assertions = None;
        let mut debug = None;
        let mut debug_assertions = None;
        let mut debug_assertions_std = None;
        let mut debuginfo_level = None;
        let mut debuginfo_level_rustc = None;
        let mut debuginfo_level_std = None;
        let mut debuginfo_level_tools = None;
        let mut debuginfo_level_tests = None;
        let mut optimize = None;
        let mut ignore_git = None;

        if let Some(ref llvm) = toml.llvm {
            match llvm.ccache {
                Some(StringOrBool::String(ref s)) => config.ccache = Some(s.to_string()),
                Some(StringOrBool::Bool(true)) => {
                    config.ccache = Some("ccache".to_string());
                }
                Some(StringOrBool::Bool(false)) | None => {}
            }
            set(&mut config.ninja_in_file, llvm.ninja);
            llvm_assertions = llvm.assertions;
            llvm_skip_rebuild = llvm_skip_rebuild.or(llvm.skip_rebuild);
            set(&mut config.llvm_optimize, llvm.optimize);
            set(&mut config.llvm_thin_lto, llvm.thin_lto);
            set(&mut config.llvm_release_debuginfo, llvm.release_debuginfo);
            set(&mut config.llvm_version_check, llvm.version_check);
            set(&mut config.llvm_static_stdcpp, llvm.static_libstdcpp);
            set(&mut config.llvm_link_shared, llvm.link_shared);
            config.llvm_targets = llvm.targets.clone();
            config.llvm_experimental_targets = llvm.experimental_targets.clone();
            config.llvm_link_jobs = llvm.link_jobs;
            config.llvm_version_suffix = llvm.version_suffix.clone();
            config.llvm_clang_cl = llvm.clang_cl.clone();

            config.llvm_cflags = llvm.cflags.clone();
            config.llvm_cxxflags = llvm.cxxflags.clone();
            config.llvm_ldflags = llvm.ldflags.clone();
            set(&mut config.llvm_use_libcxx, llvm.use_libcxx);
            config.llvm_use_linker = llvm.use_linker.clone();
            config.llvm_allow_old_toolchain = llvm.allow_old_toolchain;
        }

        if let Some(ref rust) = toml.rust {
            debug = rust.debug;
            debug_assertions = rust.debug_assertions;
            debug_assertions_std = rust.debug_assertions_std;
            debuginfo_level = rust.debuginfo_level;
            debuginfo_level_rustc = rust.debuginfo_level_rustc;
            debuginfo_level_std = rust.debuginfo_level_std;
            debuginfo_level_tools = rust.debuginfo_level_tools;
            debuginfo_level_tests = rust.debuginfo_level_tests;
            optimize = rust.optimize;
            ignore_git = rust.ignore_git;
            set(&mut config.rust_new_symbol_mangling, rust.new_symbol_mangling);
            set(&mut config.rust_optimize_tests, rust.optimize_tests);
            set(&mut config.codegen_tests, rust.codegen_tests);
            set(&mut config.rust_rpath, rust.rpath);
            set(&mut config.jemalloc, rust.jemalloc);
            set(&mut config.test_compare_mode, rust.test_compare_mode);
            set(&mut config.llvm_libunwind, rust.llvm_libunwind);
            set(&mut config.backtrace, rust.backtrace);
            set(&mut config.channel, rust.channel.clone());
            set(&mut config.rust_dist_src, rust.dist_src);
            set(&mut config.verbose_tests, rust.verbose_tests);
            // in the case "false" is set explicitly, do not overwrite the command line args
            if let Some(true) = rust.incremental {
                config.incremental = true;
            }
            set(&mut config.use_lld, rust.use_lld);
            set(&mut config.lld_enabled, rust.lld);
            set(&mut config.llvm_tools_enabled, rust.llvm_tools);
            config.rustc_parallel = rust.parallel_compiler.unwrap_or(false);
            config.rustc_default_linker = rust.default_linker.clone();
            config.musl_root = rust.musl_root.clone().map(PathBuf::from);
            config.save_toolstates = rust.save_toolstates.clone().map(PathBuf::from);
            set(&mut config.deny_warnings, flags.deny_warnings.or(rust.deny_warnings));
            set(&mut config.backtrace_on_ice, rust.backtrace_on_ice);
            set(&mut config.rust_verify_llvm_ir, rust.verify_llvm_ir);
            config.rust_thin_lto_import_instr_limit = rust.thin_lto_import_instr_limit;
            set(&mut config.rust_remap_debuginfo, rust.remap_debuginfo);
            set(&mut config.control_flow_guard, rust.control_flow_guard);

            if let Some(ref backends) = rust.codegen_backends {
                config.rust_codegen_backends =
                    backends.iter().map(|s| INTERNER.intern_str(s)).collect();
            }

            config.rust_codegen_units = rust.codegen_units.map(threads_from_config);
            config.rust_codegen_units_std = rust.codegen_units_std.map(threads_from_config);
        }

        if let Some(ref t) = toml.target {
            for (triple, cfg) in t {
                let mut target = Target::from_triple(triple);

                if let Some(ref s) = cfg.llvm_config {
                    target.llvm_config = Some(config.src.join(s));
                }
                if let Some(ref s) = cfg.llvm_filecheck {
                    target.llvm_filecheck = Some(config.src.join(s));
                }
                if let Some(ref s) = cfg.android_ndk {
                    target.ndk = Some(config.src.join(s));
                }
                if let Some(s) = cfg.no_std {
                    target.no_std = s;
                }
                target.cc = cfg.cc.clone().map(PathBuf::from);
                target.cxx = cfg.cxx.clone().map(PathBuf::from);
                target.ar = cfg.ar.clone().map(PathBuf::from);
                target.ranlib = cfg.ranlib.clone().map(PathBuf::from);
                target.linker = cfg.linker.clone().map(PathBuf::from);
                target.crt_static = cfg.crt_static;
                target.musl_root = cfg.musl_root.clone().map(PathBuf::from);
                target.musl_libdir = cfg.musl_libdir.clone().map(PathBuf::from);
                target.wasi_root = cfg.wasi_root.clone().map(PathBuf::from);
                target.qemu_rootfs = cfg.qemu_rootfs.clone().map(PathBuf::from);

                config.target_config.insert(TargetSelection::from_user(triple), target);
            }
        }

        if let Some(ref t) = toml.dist {
            config.dist_sign_folder = t.sign_folder.clone().map(PathBuf::from);
            config.dist_gpg_password_file = t.gpg_password_file.clone().map(PathBuf::from);
            config.dist_upload_addr = t.upload_addr.clone();
            set(&mut config.rust_dist_src, t.src_tarball);
            set(&mut config.missing_tools, t.missing_tools);
        }

        // Now that we've reached the end of our configuration, infer the
        // default values for all options that we haven't otherwise stored yet.

        set(&mut config.initial_rustc, build.rustc.map(PathBuf::from));
        set(&mut config.initial_cargo, build.cargo.map(PathBuf::from));

        config.llvm_skip_rebuild = llvm_skip_rebuild.unwrap_or(false);

        let default = false;
        config.llvm_assertions = llvm_assertions.unwrap_or(default);

        let default = true;
        config.rust_optimize = optimize.unwrap_or(default);

        let default = debug == Some(true);
        config.rust_debug_assertions = debug_assertions.unwrap_or(default);
        config.rust_debug_assertions_std =
            debug_assertions_std.unwrap_or(config.rust_debug_assertions);

        let with_defaults = |debuginfo_level_specific: Option<u32>| {
            debuginfo_level_specific.or(debuginfo_level).unwrap_or(if debug == Some(true) {
                1
            } else {
                0
            })
        };
        config.rust_debuginfo_level_rustc = with_defaults(debuginfo_level_rustc);
        config.rust_debuginfo_level_std = with_defaults(debuginfo_level_std);
        config.rust_debuginfo_level_tools = with_defaults(debuginfo_level_tools);
        config.rust_debuginfo_level_tests = debuginfo_level_tests.unwrap_or(0);

        let default = config.channel == "dev";
        config.ignore_git = ignore_git.unwrap_or(default);

        config
    }

    /// Try to find the relative path of `bindir`, otherwise return it in full.
    pub fn bindir_relative(&self) -> &Path {
        let bindir = &self.bindir;
        if bindir.is_absolute() {
            // Try to make it relative to the prefix.
            if let Some(prefix) = &self.prefix {
                if let Ok(stripped) = bindir.strip_prefix(prefix) {
                    return stripped;
                }
            }
        }
        bindir
    }

    /// Try to find the relative path of `libdir`.
    pub fn libdir_relative(&self) -> Option<&Path> {
        let libdir = self.libdir.as_ref()?;
        if libdir.is_relative() {
            Some(libdir)
        } else {
            // Try to make it relative to the prefix.
            libdir.strip_prefix(self.prefix.as_ref()?).ok()
        }
    }

    pub fn verbose(&self) -> bool {
        self.verbose > 0
    }

    pub fn very_verbose(&self) -> bool {
        self.verbose > 1
    }

    pub fn llvm_enabled(&self) -> bool {
        self.rust_codegen_backends.contains(&INTERNER.intern_str("llvm"))
    }
}

fn set<T>(field: &mut T, val: Option<T>) {
    if let Some(v) = val {
        *field = v;
    }
}

fn threads_from_config(v: u32) -> u32 {
    match v {
        0 => num_cpus::get() as u32,
        n => n,
    }
}
