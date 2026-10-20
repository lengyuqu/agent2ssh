//! Manual real-machine verification for `container_terminal::launch` on
//! macOS (the `.command` script + `open -a Terminal` branch).
//!
//! These tests OPEN A REAL TERMINAL WINDOW on this Mac and require a running
//! container probe. They are `#[ignore]`d so CI and normal test runs skip
//! them; run explicitly with:
//!
//! ```sh
//! # probe first:
//! docker run -d --name a2s-probe alpine:latest sleep 3600
//! cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
//!   --test container_terminal_manual -- --ignored --nocapture
//! ```
//!
//! Apple container variant additionally needs the container system service
//! and a default kernel (`container system start`).

use agent2ssh::container_discovery::discover_containers;

fn first_target(runtime: &str) -> agent2ssh::ContainerDiscoveryTarget {
    let targets = discover_containers(false, Some(runtime)).expect("container discovery failed");
    assert!(
        !targets.is_empty(),
        "no running {runtime} containers; start a probe container first"
    );
    println!(
        "discovered {} target: {} ({})",
        runtime, targets[0].container_name, targets[0].id
    );
    targets[0].clone()
}

#[test]
#[ignore = "opens a real Terminal.app window; run manually with --ignored"]
fn launch_docker_container_in_macos_terminal() {
    let target = first_target("docker");
    agent2ssh::container_terminal::launch(&target.exec_binary, &target.exec_args)
        .expect("macOS terminal launch failed");
    println!(
        "Terminal.app handed docker exec for {}",
        target.container_name
    );
}

#[test]
#[ignore = "opens a real Terminal.app window; run manually with --ignored"]
fn launch_apple_container_in_macos_terminal() {
    let target = first_target("apple");
    agent2ssh::container_terminal::launch(&target.exec_binary, &target.exec_args)
        .expect("macOS terminal launch failed");
    println!(
        "Terminal.app handed container exec for {}",
        target.container_name
    );
}
