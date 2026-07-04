use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

const SCHEMA_PATH: &str = "config/openion.schema.toml";
const CONFIG_PATH: &str = ".config.toml";
const BACKUP_CONFIG_PATH: &str = ".config.old.toml";
const GENERATED_PATH: &str = "kernel/src/generated_config.rs";
const RISCV64IMA_TARGET_PATH: &str = "config/targets/riscv64ima-unknown-none-elf.json";
const RISCV32IMA_TARGET_PATH: &str = "config/targets/riscv32ima-unknown-none-elf.json";
const RISCV64IMAC_TARGET: &str = "riscv64imac-unknown-none-elf";
const RISCV32IMAC_TARGET: &str = "riscv32imac-unknown-none-elf";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArchKind {
    Riscv,
    Arm,
}

#[derive(Clone, Copy)]
struct PlatformSpec {
    name: &'static str,
    package: &'static str,
    default_target: &'static str,
    linker_script: &'static str,
    arch: ArchKind,
    supports_qemu_run: bool,
    supports_riscv_s_mode: bool,
    supports_riscv_m_mode: bool,
    default_riscv_s_mode: bool,
    supports_async_rt: bool,
    supports_hypervisor: bool,
    supports_ns16550a: bool,
    supports_cmsdk_uart: bool,
    supports_virtio_blk: bool,
    supports_virtio_gpu: bool,
    supports_virtio_rng: bool,
    supports_lan9118: bool,
    supports_mcu_profile: bool,
}

const PLATFORMS: &[PlatformSpec] = &[
    PlatformSpec {
        name: "riscv-generic",
        package: "riscv-generic",
        default_target: RISCV64IMAC_TARGET,
        linker_script: "platform/riscv-generic/linker.ld",
        arch: ArchKind::Riscv,
        supports_qemu_run: true,
        supports_riscv_s_mode: true,
        supports_riscv_m_mode: true,
        default_riscv_s_mode: true,
        supports_async_rt: true,
        supports_hypervisor: true,
        supports_ns16550a: true,
        supports_cmsdk_uart: false,
        supports_virtio_blk: true,
        supports_virtio_gpu: true,
        supports_virtio_rng: true,
        supports_lan9118: false,
        supports_mcu_profile: false,
    },
    PlatformSpec {
        name: "qemu-virt-riscv",
        package: "riscv-generic",
        default_target: RISCV64IMAC_TARGET,
        linker_script: "platform/riscv-generic/linker.ld",
        arch: ArchKind::Riscv,
        supports_qemu_run: true,
        supports_riscv_s_mode: true,
        supports_riscv_m_mode: true,
        default_riscv_s_mode: true,
        supports_async_rt: true,
        supports_hypervisor: true,
        supports_ns16550a: true,
        supports_cmsdk_uart: false,
        supports_virtio_blk: true,
        supports_virtio_gpu: true,
        supports_virtio_rng: true,
        supports_lan9118: false,
        supports_mcu_profile: false,
    },
    PlatformSpec {
        name: "ionsoc-verilator",
        package: "ionsoc-verilator",
        default_target: RISCV64IMAC_TARGET,
        linker_script: "platform/ionsoc-verilator/linker.ld",
        arch: ArchKind::Riscv,
        supports_qemu_run: false,
        supports_riscv_s_mode: true,
        supports_riscv_m_mode: false,
        default_riscv_s_mode: true,
        supports_async_rt: true,
        supports_hypervisor: false,
        supports_ns16550a: true,
        supports_cmsdk_uart: false,
        supports_virtio_blk: false,
        supports_virtio_gpu: false,
        supports_virtio_rng: false,
        supports_lan9118: false,
        supports_mcu_profile: false,
    },
    PlatformSpec {
        name: "stm32f103-bluepill",
        package: "stm32f103-bluepill",
        default_target: "thumbv7m-none-eabi",
        linker_script: "platform/stm32f103-bluepill/linker.ld",
        arch: ArchKind::Arm,
        supports_qemu_run: false,
        supports_riscv_s_mode: false,
        supports_riscv_m_mode: false,
        default_riscv_s_mode: false,
        supports_async_rt: false,
        supports_hypervisor: false,
        supports_ns16550a: false,
        supports_cmsdk_uart: false,
        supports_virtio_blk: false,
        supports_virtio_gpu: false,
        supports_virtio_rng: false,
        supports_lan9118: false,
        supports_mcu_profile: true,
    },
    PlatformSpec {
        name: "qemu-an521",
        package: "an521",
        default_target: "thumbv8m.main-none-eabihf",
        linker_script: "platform/qemu-an521/linker.ld",
        arch: ArchKind::Arm,
        supports_qemu_run: true,
        supports_riscv_s_mode: false,
        supports_riscv_m_mode: false,
        default_riscv_s_mode: false,
        supports_async_rt: true,
        supports_hypervisor: false,
        supports_ns16550a: false,
        supports_cmsdk_uart: true,
        supports_virtio_blk: false,
        supports_virtio_gpu: false,
        supports_virtio_rng: false,
        supports_lan9118: true,
        supports_mcu_profile: false,
    },
];

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "OpenIon build and configuration tasks")]
struct Cli {
    /// Host target triple used to build host-side tools.
    #[arg(long, default_value = default_host_target())]
    host_target: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate kernel/src/generated_config.rs from the Ionix schema/config.
    Config {
        /// Override config file path.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Open the interactive Ionix menuconfig UI.
    Menuconfig {
        /// Override config file path.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Build a platform through Ionix-generated configuration.
    Build {
        /// Platform override: riscv-generic, qemu-virt-riscv, ionsoc-verilator, qemu-an521, or stm32f103-bluepill.
        #[arg(long, short = 'p')]
        platform: Option<String>,
        /// Config file override.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Build release artifacts.
        #[arg(long)]
        release: bool,
    },
    /// Launch QEMU after building. Avoid this in agent sessions.
    Run {
        /// Platform override: riscv-generic, qemu-virt-riscv, ionsoc-verilator, qemu-an521, or stm32f103-bluepill.
        #[arg(long, short = 'p')]
        platform: Option<String>,
        /// Config file override.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Build release artifacts.
        #[arg(long)]
        release: bool,
    },
}

#[derive(Clone)]
struct BuildConfig {
    platform: String,
    target: String,
    net_backend: String,
    riscv_xlen_64: bool,
    riscv_xlen_32: bool,
    riscv_kernel_base: usize,
    riscv_s_mode: bool,
    riscv_m_mode: bool,
    riscv_ext_m: bool,
    riscv_ext_a: bool,
    riscv_ext_c: bool,
    riscv_ext_f: bool,
    riscv_ext_d: bool,
    riscv_ext_b: bool,
    riscv_ext_v: bool,
    riscv_ext_zicsr: bool,
    riscv_ext_zifencei: bool,
    riscv_hypervisor: bool,
    async_rt: bool,
    builtin_shell: bool,
    driver_ns16550a: bool,
    driver_cmsdk_uart: bool,
    driver_virtio_blk: bool,
    driver_virtio_gpu: bool,
    driver_virtio_rng: bool,
    driver_lan9118: bool,
}

impl BuildConfig {
    fn spec(&self) -> Result<&'static PlatformSpec> {
        platform_spec(&self.platform)
    }

    fn qemu_command(&self, release: bool) -> Result<Command> {
        let spec = self.spec()?;
        if !spec.supports_qemu_run {
            bail!(
                "platform '{}' does not have an xtask run command",
                spec.name
            );
        }

        let profile = if release { "release" } else { "debug" };
        let target_dir = target_output_dir(&self.target);
        let kernel = format!("target/{}/{}/{}", target_dir, profile, spec.package);

        match spec.name {
            "riscv-generic" | "qemu-virt-riscv" => {
                let mut cmd = Command::new(if self.riscv_xlen_32 {
                    "qemu-system-riscv32"
                } else {
                    "qemu-system-riscv64"
                });
                cmd.args(["-machine", "virt", "-smp", "1"]);
                cmd.args(["-bios", "default"]);
                cmd.args([
                    "-kernel",
                    &kernel,
                    "-smp",
                    "1",
                    "-global",
                    "virtio-mmio.force-legacy=false",
                    "-device",
                    "virtio-blk-device,drive=hd0",
                    "-drive",
                    "if=none,file=sd.img,format=raw,id=hd0",
                    // "-device",
                    // "virtio-gpu-device",
                    "-serial",
                    "mon:stdio",
                    "-s",
                    "-nographic",
                ]);
                Ok(cmd)
            }
            "qemu-an521" => {
                let mut cmd = Command::new("qemu-system-arm");
                cmd.args(["-M", "mps2-an521", "-nographic", "-kernel", &kernel]);
                Ok(cmd)
            }
            other => bail!("unsupported platform '{}'", other),
        }
    }
}

fn target_output_dir(target: &str) -> String {
    Path::new(target)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(target)
        .to_string()
}

fn platform_spec(platform: &str) -> Result<&'static PlatformSpec> {
    PLATFORMS
        .iter()
        .find(|spec| spec.name == platform)
        .with_context(|| format!("unsupported platform '{}'", platform))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Config { config } => {
            generate_config(config.as_deref())?;
        }
        Commands::Menuconfig { config } => {
            menuconfig(config.as_deref(), &cli.host_target)?;
        }
        Commands::Build {
            platform,
            config,
            release,
        } => {
            let cfg = prepare_build(platform.as_deref(), config.as_deref())?;
            cargo_build(&cfg, release)?;
        }
        Commands::Run {
            platform,
            config,
            release,
        } => {
            let cfg = prepare_build(platform.as_deref(), config.as_deref())?;
            cargo_build(&cfg, release)?;
            let status = cfg
                .qemu_command(release)?
                .status()
                .context("failed to launch QEMU")?;
            if !status.success() {
                bail!("QEMU exited with status {}", status);
            }
        }
    }

    Ok(())
}

fn default_host_target() -> &'static str {
    option_env!("HOST").unwrap_or("x86_64-pc-windows-msvc")
}

fn prepare_build(
    platform_override: Option<&str>,
    config_path: Option<&Path>,
) -> Result<BuildConfig> {
    generate_config(config_path)?;
    let platform = platform_override.unwrap_or("riscv-generic");
    let spec = platform_spec(platform)?;
    let mut cfg = load_build_config(config_path)?;
    cfg.platform = spec.name.to_string();
    cfg.target = target_for_config(spec, &cfg).to_string();
    apply_platform_constraints(spec, &mut cfg);

    validate_build_config(&cfg)?;
    Ok(cfg)
}

fn generate_config(config_path: Option<&Path>) -> Result<()> {
    let config_path = config_path.unwrap_or_else(|| Path::new(CONFIG_PATH));
    ionix::prepare(
        ionix::PrepareOptions::new(SCHEMA_PATH, config_path, GENERATED_PATH)
            .with_backup_path(BACKUP_CONFIG_PATH),
    )?;
    println!("generated {}", GENERATED_PATH);
    Ok(())
}

fn menuconfig(config_path: Option<&Path>, host_target: &str) -> Result<()> {
    let config_path = config_path.unwrap_or_else(|| Path::new(CONFIG_PATH));
    generate_config(Some(config_path))?;

    let mut cmd = Command::new("cargo");
    cmd.args([
        "run",
        "--release",
        "--manifest-path",
        "utils/ionix/Cargo.toml",
        "--target",
        host_target,
        "--",
        "--schema",
        SCHEMA_PATH,
        "--config",
    ]);
    cmd.arg(config_path);
    cmd.args(["--export", GENERATED_PATH]);

    let status = cmd.status().context("failed to run ionix menuconfig")?;
    if !status.success() {
        bail!("ionix menuconfig failed with status {}", status);
    }
    Ok(())
}

fn load_build_config(config_path: Option<&Path>) -> Result<BuildConfig> {
    let schema = ionix::schema::ConfigSchema::from_path(SCHEMA_PATH)?;
    let config_path = config_path.unwrap_or_else(|| Path::new(CONFIG_PATH));
    let loaded = ionix::load_config(SCHEMA_PATH, Some(config_path))?;
    let values = ionix::ConfigLoader::merge_with_defaults(&loaded.values, &schema);
    let get_str = |key: &str| {
        values
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .with_context(|| format!("missing string config '{}'", key))
    };
    let get_bool = |key: &str| {
        values
            .get(key)
            .and_then(|v| v.as_bool())
            .with_context(|| format!("missing bool config '{}'", key))
    };
    let get_usize = |key: &str| {
        values
            .get(key)
            .and_then(|v| v.as_integer())
            .and_then(|v| usize::try_from(v).ok())
            .with_context(|| format!("missing usize config '{}'", key))
    };

    Ok(BuildConfig {
        platform: String::new(),
        target: String::new(),
        net_backend: get_str("OPENION_NET_BACKEND")?,
        riscv_xlen_64: get_bool("OPENION_RISCV_XLEN_64")?,
        riscv_xlen_32: get_bool("OPENION_RISCV_XLEN_32")?,
        riscv_kernel_base: get_usize("OPENION_RISCV_KERNEL_BASE")?,
        riscv_s_mode: get_bool("OPENION_RISCV_S_MODE")?,
        riscv_m_mode: get_bool("OPENION_RISCV_M_MODE")?,
        riscv_ext_m: get_bool("OPENION_RISCV_EXT_M")?,
        riscv_ext_a: get_bool("OPENION_RISCV_EXT_A")?,
        riscv_ext_c: get_bool("OPENION_RISCV_EXT_C")?,
        riscv_ext_f: get_bool("OPENION_RISCV_EXT_F")?,
        riscv_ext_d: get_bool("OPENION_RISCV_EXT_D")?,
        riscv_ext_b: get_bool("OPENION_RISCV_EXT_B")?,
        riscv_ext_v: get_bool("OPENION_RISCV_EXT_V")?,
        riscv_ext_zicsr: get_bool("OPENION_RISCV_EXT_ZICSR")?,
        riscv_ext_zifencei: get_bool("OPENION_RISCV_EXT_ZIFENCEI")?,
        riscv_hypervisor: get_bool("OPENION_RISCV_HYPERVISOR")?,
        async_rt: get_bool("OPENION_ASYNC_RT")?,
        builtin_shell: get_bool("OPENION_BUILTIN_SHELL")?,
        driver_ns16550a: get_bool("OPENION_DRIVER_NS16550A")?,
        driver_cmsdk_uart: get_bool("OPENION_DRIVER_CMSDK_UART")?,
        driver_virtio_blk: get_bool("OPENION_DRIVER_VIRTIO_BLK")?,
        driver_virtio_gpu: get_bool("OPENION_DRIVER_VIRTIO_GPU")?,
        driver_virtio_rng: get_bool("OPENION_DRIVER_VIRTIO_RNG")?,
        driver_lan9118: get_bool("OPENION_DRIVER_LAN9118")?,
    })
}

fn apply_platform_constraints(spec: &PlatformSpec, cfg: &mut BuildConfig) {
    if spec.arch == ArchKind::Riscv {
        if cfg.riscv_xlen_32 == cfg.riscv_xlen_64 {
            constrain_bool(spec, &mut cfg.riscv_xlen_64, true, "OPENION_RISCV_XLEN_64");
            constrain_bool(spec, &mut cfg.riscv_xlen_32, false, "OPENION_RISCV_XLEN_32");
        }
        if !spec.supports_riscv_s_mode {
            constrain_bool(spec, &mut cfg.riscv_s_mode, false, "OPENION_RISCV_S_MODE");
        }
        if !spec.supports_riscv_m_mode {
            constrain_bool(spec, &mut cfg.riscv_m_mode, false, "OPENION_RISCV_M_MODE");
            if !cfg.riscv_s_mode {
                constrain_bool(
                    spec,
                    &mut cfg.riscv_s_mode,
                    spec.default_riscv_s_mode,
                    "OPENION_RISCV_S_MODE",
                );
            }
        }
    } else {
        constrain_bool(spec, &mut cfg.riscv_xlen_64, false, "OPENION_RISCV_XLEN_64");
        constrain_bool(spec, &mut cfg.riscv_xlen_32, false, "OPENION_RISCV_XLEN_32");
        constrain_bool(spec, &mut cfg.riscv_s_mode, false, "OPENION_RISCV_S_MODE");
        constrain_bool(spec, &mut cfg.riscv_m_mode, false, "OPENION_RISCV_M_MODE");
    }

    if spec.arch == ArchKind::Riscv && cfg.riscv_xlen_32 && cfg.riscv_hypervisor {
        constrain_bool(
            spec,
            &mut cfg.riscv_hypervisor,
            false,
            "OPENION_RISCV_HYPERVISOR",
        );
    }

    if spec.arch == ArchKind::Riscv && cfg.riscv_s_mode == cfg.riscv_m_mode {
        if spec.default_riscv_s_mode {
            constrain_bool(spec, &mut cfg.riscv_s_mode, true, "OPENION_RISCV_S_MODE");
            constrain_bool(spec, &mut cfg.riscv_m_mode, false, "OPENION_RISCV_M_MODE");
        } else {
            constrain_bool(spec, &mut cfg.riscv_s_mode, false, "OPENION_RISCV_S_MODE");
            constrain_bool(spec, &mut cfg.riscv_m_mode, true, "OPENION_RISCV_M_MODE");
        }
    }

    if spec.supports_mcu_profile {
        constrain_bool(spec, &mut cfg.builtin_shell, false, "OPENION_BUILTIN_SHELL");
    }
    if !spec.supports_async_rt {
        constrain_bool(spec, &mut cfg.async_rt, false, "OPENION_ASYNC_RT");
    }
    if !spec.supports_hypervisor {
        constrain_bool(
            spec,
            &mut cfg.riscv_hypervisor,
            false,
            "OPENION_RISCV_HYPERVISOR",
        );
    }
    if !spec.supports_ns16550a {
        constrain_bool(
            spec,
            &mut cfg.driver_ns16550a,
            false,
            "OPENION_DRIVER_NS16550A",
        );
    }

    if !spec.supports_cmsdk_uart {
        constrain_bool(
            spec,
            &mut cfg.driver_cmsdk_uart,
            false,
            "OPENION_DRIVER_CMSDK_UART",
        );
    }
    if !spec.supports_virtio_blk {
        constrain_bool(
            spec,
            &mut cfg.driver_virtio_blk,
            false,
            "OPENION_DRIVER_VIRTIO_BLK",
        );
    }
    if !spec.supports_virtio_gpu {
        constrain_bool(
            spec,
            &mut cfg.driver_virtio_gpu,
            false,
            "OPENION_DRIVER_VIRTIO_GPU",
        );
    }
    if !spec.supports_virtio_rng {
        constrain_bool(
            spec,
            &mut cfg.driver_virtio_rng,
            false,
            "OPENION_DRIVER_VIRTIO_RNG",
        );
    }
    if !spec.supports_lan9118 {
        constrain_bool(
            spec,
            &mut cfg.driver_lan9118,
            false,
            "OPENION_DRIVER_LAN9118",
        );
    }
}

fn constrain_bool(spec: &PlatformSpec, value: &mut bool, required: bool, key: &str) {
    if *value != required {
        println!(
            "platform {} constrains {}={} (config requested {})",
            spec.name, key, required, *value
        );
        *value = required;
    }
}

fn validate_build_config(cfg: &BuildConfig) -> Result<()> {
    let spec = cfg.spec()?;
    let expected_target = target_for_config(spec, cfg);
    if cfg.target != expected_target {
        bail!(
            "platform '{}' requires target '{}', config has '{}'",
            cfg.platform,
            expected_target,
            cfg.target
        );
    }

    if spec.arch == ArchKind::Riscv && cfg.riscv_s_mode == cfg.riscv_m_mode {
        bail!(
            "RISC-V config must enable exactly one of OPENION_RISCV_S_MODE or OPENION_RISCV_M_MODE"
        );
    }

    if spec.arch == ArchKind::Riscv && cfg.riscv_xlen_32 == cfg.riscv_xlen_64 {
        bail!(
            "RISC-V config must enable exactly one of OPENION_RISCV_XLEN_32 or OPENION_RISCV_XLEN_64"
        );
    }

    if spec.arch == ArchKind::Riscv && cfg.riscv_xlen_32 && cfg.riscv_hypervisor {
        bail!("OPENION_RISCV_HYPERVISOR requires RV64 in this tree");
    }

    if spec.arch == ArchKind::Riscv && cfg.riscv_ext_d && !cfg.riscv_ext_f {
        bail!("OPENION_RISCV_EXT_D requires OPENION_RISCV_EXT_F");
    }

    match cfg.net_backend.as_str() {
        "ionnet" | "smoltcp" => Ok(()),
        other => bail!("unsupported OPENION_NET_BACKEND '{}'", other),
    }
}

fn target_for_config(spec: &'static PlatformSpec, cfg: &BuildConfig) -> &'static str {
    if spec.arch != ArchKind::Riscv {
        return spec.default_target;
    }

    match (cfg.riscv_xlen_32, cfg.riscv_ext_c) {
        (true, true) => RISCV32IMAC_TARGET,
        (true, false) => RISCV32IMA_TARGET_PATH,
        (false, true) => RISCV64IMAC_TARGET,
        (false, false) => RISCV64IMA_TARGET_PATH,
    }
}

fn cargo_build(cfg: &BuildConfig, release: bool) -> Result<()> {
    let spec = cfg.spec()?;
    let package = spec.package;
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if cfg.target.ends_with(".json") {
        cmd.arg("-Zjson-target-spec");
        cmd.arg("-Zbuild-std=core,alloc");
    }

    cmd.args(["-p", package, "--target", &cfg.target]);

    if release {
        cmd.arg("--release");
    }

    cmd.args(["--no-default-features"]);
    cmd.env(
        "CARGO_ENCODED_RUSTFLAGS",
        cargo_rustflags(cfg)?.join("\u{1f}"),
    );

    let features = collect_features(spec, cfg);
    print_build_summary(spec, cfg, &features, release);

    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }

    let status = cmd.status().context("failed to run cargo build")?;
    if !status.success() {
        bail!("cargo build failed with status {}", status);
    }
    Ok(())
}

fn print_build_summary(
    spec: &PlatformSpec,
    cfg: &BuildConfig,
    features: &[&'static str],
    release: bool,
) {
    println!("build platform: {}", spec.name);
    println!("build package: {}", spec.package);
    println!("build target: {}", cfg.target);
    if spec.arch == ArchKind::Riscv {
        println!(
            "build riscv xlen: {}",
            if cfg.riscv_xlen_32 { "rv32" } else { "rv64" }
        );
        println!("build riscv base: {:#x}", cfg.riscv_kernel_base);
    }
    println!("build profile: {}", if release { "release" } else { "dev" });
    println!("build linker: {}", spec.linker_script);
    if features.is_empty() {
        println!("build features: <none>");
    } else {
        println!("build features: {}", features.join(","));
    }
}

fn collect_features(spec: &PlatformSpec, cfg: &BuildConfig) -> Vec<&'static str> {
    let package = spec.package;
    let mut features = Vec::new();

    if spec.arch == ArchKind::Riscv {
        if cfg.riscv_m_mode {
            features.push("m-mode");
        } else {
            features.push("s-mode");
        }
        if package == "riscv-generic" && cfg.riscv_hypervisor {
            features.push("hypervisor");
        }
    }

    if cfg.builtin_shell {
        features.push("builtin_shell");
    }

    if cfg.async_rt {
        features.push("async_rt");
    }

    if spec.supports_mcu_profile {
        features.push("kernel/mcu_profile");
    }

    if spec.arch == ArchKind::Riscv {
        if cfg.driver_ns16550a {
            features.push("driver_ns16550a");
        }
    }

    if package == "riscv-generic" {
        if cfg.driver_virtio_blk {
            features.push("driver_virtio_blk");
        }
        if cfg.driver_virtio_gpu {
            features.push("driver_virtio_gpu");
        }
        if cfg.driver_virtio_rng {
            features.push("driver_virtio_rng");
        }
    }

    if package == "an521" {
        if cfg.driver_cmsdk_uart {
            features.push("driver_cmsdk_uart");
        }
        if cfg.driver_lan9118 {
            features.push("driver_lan9118");
        }
    }

    if !spec.supports_mcu_profile {
        match cfg.net_backend.as_str() {
            "smoltcp" => {
                features.push("kernel/use_smoltcp");
            }
            "ionnet" => {
                features.push("kernel/use_ionnet");
            }
            _ => {}
        }
    }

    features
}

fn cargo_rustflags(cfg: &BuildConfig) -> Result<Vec<String>> {
    let spec = cfg.spec()?;
    let mut flags = Vec::new();

    if spec.arch == ArchKind::Riscv {
        let target_features = riscv_target_features(cfg);
        if !target_features.is_empty() {
            flags.push(String::from("-C"));
            flags.push(format!("target-feature={}", target_features.join(",")));
        }

        flags.push(String::from("-C"));
        flags.push(format!(
            "link-arg=--defsym=BASE_ADDRESS={:#x}",
            cfg.riscv_kernel_base
        ));
    }

    flags.push(String::from("-C"));
    flags.push(format!("link-arg=-T{}", spec.linker_script));
    Ok(flags)
}

fn riscv_target_features(cfg: &BuildConfig) -> Vec<String> {
    let mut features = Vec::new();

    if cfg.riscv_ext_c {
        for (feature, enabled) in [
            ("m", cfg.riscv_ext_m),
            ("a", cfg.riscv_ext_a),
            ("c", cfg.riscv_ext_c),
            ("zicsr", cfg.riscv_ext_zicsr),
            ("zifencei", cfg.riscv_ext_zifencei),
        ] {
            if !enabled {
                features.push(format!("-{feature}"));
            }
        }
    } else {
        for (feature, enabled) in [("m", cfg.riscv_ext_m), ("a", cfg.riscv_ext_a)] {
            if !enabled {
                features.push(format!("-{feature}"));
            }
        }

        for (feature, enabled) in [
            ("zicsr", cfg.riscv_ext_zicsr),
            ("zifencei", cfg.riscv_ext_zifencei),
        ] {
            if enabled {
                features.push(format!("+{feature}"));
            }
        }
    }

    for (feature, enabled) in [
        ("f", cfg.riscv_ext_f),
        ("d", cfg.riscv_ext_d),
        ("b", cfg.riscv_ext_b),
        ("v", cfg.riscv_ext_v),
    ] {
        if enabled {
            features.push(format!("+{feature}"));
        }
    }

    features
}
