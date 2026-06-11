use agent2ssh::{
    add_host_core, exec_ssh_core, list_audit_core, list_hosts_core, remove_host_core, ExecRequest,
    HostProfile,
};
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "agent2ssh")]
#[command(about = "SSH capability layer for agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Host {
        #[command(subcommand)]
        command: HostCommands,
    },
    Exec {
        host: String,
        command: String,
        #[arg(long)]
        json: bool,
    },
    Audit {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum HostCommands {
    List {
        #[arg(long)]
        json: bool,
    },
    Add {
        name: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Rm {
        name: String,
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Host { command } => match command {
            HostCommands::List { json } => {
                let hosts = list_hosts_core()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&hosts)?);
                } else if hosts.is_empty() {
                    println!("No hosts configured.");
                } else {
                    for host in hosts {
                        println!(
                            "{}\t{}{}:{}",
                            host.name,
                            host.user.map(|u| format!("{u}@")).unwrap_or_default(),
                            host.host,
                            host.port.unwrap_or(22)
                        );
                    }
                }
            }
            HostCommands::Add {
                name,
                host,
                user,
                port,
                key,
                json,
            } => {
                let profile = add_host_core(HostProfile {
                    name,
                    host,
                    user,
                    port,
                    key_path: key,
                })?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&profile)?);
                } else {
                    println!("Saved host '{}'.", profile.name);
                }
            }
            HostCommands::Rm { name, json } => {
                remove_host_core(&name)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({ "removed": name }))?
                    );
                } else {
                    println!("Removed host '{name}'.");
                }
            }
        },
        Commands::Exec {
            host,
            command,
            json,
        } => {
            let result = exec_ssh_core(ExecRequest { host, command }).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print!("{}", result.stdout);
                eprint!("{}", result.stderr);
                std::process::exit(result.exit_code.unwrap_or(1));
            }
        }
        Commands::Audit { limit, json } => {
            let entries = list_audit_core(limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for entry in entries {
                    println!(
                        "{}\t{}\texit={:?}\t{}ms\t{}",
                        entry.ts, entry.host, entry.exit_code, entry.duration_ms, entry.command
                    );
                }
            }
        }
    }

    Ok(())
}
