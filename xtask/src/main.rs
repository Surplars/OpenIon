use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;
use toml::Value;

const SCHEMA_PATH: &str = "config/openion.schema.toml";
const CONFIG_PATH: &str = ".config.toml";
const BACKUP_CONFIG_PATH: &str = ".config.old.toml";
const GENERATED_PATH: &str = "kernel/src/generated_config.rs";

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
        /// Platform override: qemu-virt-riscv or qemu-an521.
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
        /// Platform override: qemu-virt-riscv or qemu-an521.
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
    builtin_shell: bool,
}

impl BuildConfig {
    fn package(&self) -> Result<&'static str> {
        match self.platform.as_str() {
            "qemu-virt-riscv" => Ok("qemu-virt-riscv"),
            "qemu-an521" => Ok("an521"),
            other => bail!("unsupported platform '{}'", other),
        }
    }

    fn qemu_command(&self, release: bool) -> Result<Command> {
        let profile = if release { "release" } else { "debug" };
        let package = self.package()?;
        let kernel = format!("target/{}/{}/{}", self.target, profile, package);

        match self.platform.as_str() {
            "qemu-virt-riscv" => {
                let mut cmd = Command::new("qemu-system-riscv64");
                cmd.args([
                    "-machine",
                    "virt",
                    "-smp",
                    "1",
                    "-nographic",
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
                    "-s",
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
            if platform.is_some() {
                generate_config(config.as_deref())?;
            }
        }
        Commands::Run {
            platform,
            config,
            release,
        } => {
            let cfg = prepare_build(platform.as_deref(), config.as_deref())?;
            cargo_build(&cfg, release)?;
            if platform.is_some() {
                generate_config(config.as_deref())?;
            }
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
    generate_config_for_build(platform_override, config_path)?;
    let mut cfg = load_build_config(config_path)?;

    if let Some(platform) = platform_override {
        cfg.platform = platform.to_string();
        cfg.target = default_target_for_platform(&cfg.platform)?.to_string();
    }

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

fn generate_config_for_build(
    platform_override: Option<&str>,
    config_path: Option<&Path>,
) -> Result<()> {
    let config_path = config_path.unwrap_or_else(|| Path::new(CONFIG_PATH));
    generate_config(Some(config_path))?;

    let schema = ionix::schema::ConfigSchema::from_path(SCHEMA_PATH)?;
    let loaded = ionix::load_config(SCHEMA_PATH, Some(config_path))?;
    let mut values = ionix::ConfigLoader::merge_with_defaults(&loaded.values, &schema);

    if let Some(platform) = platform_override {
        values.insert(
            "OPENION_PLATFORM".to_string(),
            Value::String(platform.to_string()),
        );
        values.insert(
            "OPENION_TARGET".to_string(),
            Value::String(default_target_for_platform(platform)?.to_string()),
        );
    }

    ionix::validate_values(&values, &schema)?;
    let generator = ionix::schema::CodeGenerator::new(&schema, &values);
    generator.write_to_file(Path::new(GENERATED_PATH))?;
    println!("generated {}", GENERATED_PATH);
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
        platform: get_str("OPENION_PLATFORM")?,
        target: get_str("OPENION_TARGET")?,
        net_backend: get_str("OPENION_NET_BACKEND")?,
        riscv_s_mode: get_bool("OPENION_RISCV_S_MODE")?,
        riscv_m_mode: get_bool("OPENION_RISCV_M_MODE")?,
        builtin_shell: get_bool("OPENION_BUILTIN_SHELL")?,
    })
}

fn validate_build_config(cfg: &BuildConfig) -> Result<()> {
    let expected_target = default_target_for_platform(&cfg.platform)?;
    if cfg.target != expected_target {
        bail!(
            "platform '{}' requires target '{}', config has '{}'",
            cfg.platform,
            expected_target,
            cfg.target
        );
    }

    if cfg.platform == "qemu-virt-riscv" && cfg.riscv_s_mode == cfg.riscv_m_mode {
        bail!(
            "RISC-V config must enable exactly one of OPENION_RISCV_S_MODE or OPENION_RISCV_M_MODE"
        );
    }

    match cfg.net_backend.as_str() {
        "ionnet" | "smoltcp" => Ok(()),
        other => bail!("unsupported OPENION_NET_BACKEND '{}'", other),
    }
}

fn default_target_for_platform(platform: &str) -> Result<&'static str> {
    match platform {
        "qemu-virt-riscv" => Ok("riscv64imac-unknown-none-elf"),
        "qemu-an521" => Ok("thumbv8m.main-none-eabihf"),
        other => bail!("unsupported platform '{}'", other),
    }
}

fn cargo_build(cfg: &BuildConfig, release: bool) -> Result<()> {
    let package = cfg.package()?;
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-p", package, "--target", &cfg.target]);

    if release {
        cmd.arg("--release");
    }

    cmd.args(["--no-default-features"]);

    let mut features = Vec::new();

    if package == "qemu-virt-riscv" {
        if cfg.riscv_m_mode {
            features.push("m-mode");
        } else {
            features.push("s-mode");
        }
    }

    if cfg.builtin_shell {
        features.push("builtin_shell");
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

    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }

    let status = cmd.status().context("failed to run cargo build")?;
    if !status.success() {
        bail!("cargo build failed with status {}", status);
    }
    Ok(())
}
