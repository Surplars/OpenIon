use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

const SCHEMA_PATH: &str = "config/openion.schema.toml";
const CONFIG_PATH: &str = ".config.toml";
const BACKUP_CONFIG_PATH: &str = ".config.old.toml";
const GENERATED_PATH: &str = "kernel/src/generated_config.rs";
const RISCV64IMA_TARGET_PATH: &str = "config/targets/riscv64ima-unknown-none-elf.json";

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
}

const PLATFORMS: &[PlatformSpec] = &[
    PlatformSpec {
        name: "qemu-virt-riscv",
        package: "qemu-virt-riscv",
        default_target: "riscv64imac-unknown-none-elf",
        linker_script: "platform/qemu-virt-riscv/linker.ld",
        arch: ArchKind::Riscv,
        supports_qemu_run: true,
    },
    PlatformSpec {
        name: "ionsoc-verilator",
        package: "ionsoc-verilator",
        default_target: "riscv64imac-unknown-none-elf",
        linker_script: "platform/ionsoc-verilator/linker.ld",
        arch: ArchKind::Riscv,
        supports_qemu_run: false,
    },
    PlatformSpec {
        name: "qemu-an521",
        package: "an521",
        default_target: "thumbv8m.main-none-eabihf",
        linker_script: "platform/qemu-an521/linker.ld",
        arch: ArchKind::Arm,
        supports_qemu_run: true,
    },
    PlatformSpec {
        name: "qemu-stm32l475",
        package: "qemu-stm32l475",
        default_target: "thumbv7m-none-eabi",
        linker_script: "platform/qemu-stm32l475/linker.ld",
        arch: ArchKind::Arm,
        supports_qemu_run: true,
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
        /// Platform override: qemu-virt-riscv, ionsoc-verilator, qemu-an521, or qemu-stm32l475.
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
        /// Platform override: qemu-virt-riscv, ionsoc-verilator, qemu-an521, or qemu-stm32l475.
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
    driver_stm32l4x5_usart: bool,
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
        let kernel = format!("target/{}/{}/{}", self.target, profile, spec.package);

        match spec.name {
            "qemu-virt-riscv" => {
                let mut cmd = Command::new("qemu-system-riscv64");
                cmd.args([
                    "-machine",
                    "virt",
                    "-smp",
                    "1",
                    "-bios",
                    "platform/qemu-virt-riscv/rustsbi-prototyper-jump.elf",
                    "-kernel",
                    &kernel,
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
            "qemu-stm32l475" => {
                let mut cmd = Command::new("qemu-system-arm");
                cmd.args(["-M", "b-l475e-iot01a", "-nographic", "-kernel", &kernel]);
                Ok(cmd)
            }
            other => bail!("unsupported platform '{}'", other),
        }
    }
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
    let platform = platform_override.unwrap_or("qemu-virt-riscv");
    let spec = platform_spec(platform)?;
    let mut cfg = load_build_config(config_path)?;
    cfg.platform = spec.name.to_string();
    cfg.target = target_for_config(spec, &cfg).to_string();
    apply_platform_defaults(&mut cfg);

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

    Ok(BuildConfig {
        platform: String::new(),
        target: String::new(),
        net_backend: get_str("OPENION_NET_BACKEND")?,
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
        driver_stm32l4x5_usart: get_bool("OPENION_DRIVER_STM32L4X5_USART")?,
        driver_virtio_blk: get_bool("OPENION_DRIVER_VIRTIO_BLK")?,
        driver_virtio_gpu: get_bool("OPENION_DRIVER_VIRTIO_GPU")?,
        driver_virtio_rng: get_bool("OPENION_DRIVER_VIRTIO_RNG")?,
        driver_lan9118: get_bool("OPENION_DRIVER_LAN9118")?,
    })
}

fn apply_platform_defaults(cfg: &mut BuildConfig) {
    // Platform constraints narrow schema-level options to what each board crate
    // actually wires today.
    match cfg.platform.as_str() {
        "ionsoc-verilator" => {
            cfg.riscv_s_mode = true;
            cfg.riscv_m_mode = false;
            cfg.riscv_hypervisor = false;
            cfg.driver_cmsdk_uart = false;
            cfg.driver_stm32l4x5_usart = false;
            cfg.driver_virtio_blk = false;
            cfg.driver_virtio_gpu = false;
            cfg.driver_virtio_rng = false;
            cfg.driver_lan9118 = false;
        }
        "qemu-stm32l475" => {
            cfg.async_rt = false;
            cfg.riscv_s_mode = false;
            cfg.riscv_m_mode = false;
            cfg.riscv_hypervisor = false;
            cfg.driver_ns16550a = false;
            cfg.driver_cmsdk_uart = false;
            cfg.driver_virtio_blk = false;
            cfg.driver_virtio_gpu = false;
            cfg.driver_virtio_rng = false;
            cfg.driver_lan9118 = false;
        }
        _ => {}
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

    if spec.arch == ArchKind::Riscv && cfg.riscv_ext_d && !cfg.riscv_ext_f {
        bail!("OPENION_RISCV_EXT_D requires OPENION_RISCV_EXT_F");
    }

    match cfg.net_backend.as_str() {
        "ionnet" | "smoltcp" => Ok(()),
        other => bail!("unsupported OPENION_NET_BACKEND '{}'", other),
    }
}

fn target_for_config(spec: &'static PlatformSpec, cfg: &BuildConfig) -> &'static str {
    if spec.arch == ArchKind::Riscv && !cfg.riscv_ext_c {
        RISCV64IMA_TARGET_PATH
    } else {
        spec.default_target
    }
}

fn cargo_build(cfg: &BuildConfig, release: bool) -> Result<()> {
    let spec = cfg.spec()?;
    let package = spec.package;
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if spec.arch == ArchKind::Riscv && !cfg.riscv_ext_c {
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

    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }

    let status = cmd.status().context("failed to run cargo build")?;
    if !status.success() {
        bail!("cargo build failed with status {}", status);
    }
    Ok(())
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
        if package == "qemu-virt-riscv" && cfg.riscv_hypervisor {
            features.push("hypervisor");
        }
    }

    if cfg.builtin_shell {
        features.push("builtin_shell");
    }

    if cfg.async_rt {
        features.push("async_rt");
    }

    if spec.arch == ArchKind::Riscv {
        if cfg.driver_ns16550a {
            features.push("driver_ns16550a");
        }
    }

    if package == "qemu-virt-riscv" {
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

    if package == "qemu-stm32l475" && cfg.driver_stm32l4x5_usart {
        features.push("mcu_profile");
        features.push("driver_stm32l4x5_usart");
    }

    match cfg.net_backend.as_str() {
        "smoltcp" => {
            features.push("kernel/use_smoltcp");
        }
        "ionnet" => {
            features.push("kernel/use_ionnet");
        }
        _ => {}
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
