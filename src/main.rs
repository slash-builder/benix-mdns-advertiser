//! BenixOS box-side mDNS advertiser.
//!
//! Publishes `_benixos._tcp` via `mdns-sd` so Courier can discover a box on
//! the LAN without an account/hub round-trip, per messaging-architect's
//! ruling (`context/projects/benixos.md` §9c). This is deliberately a thin
//! advertiser and nothing else: register one service record, keep the
//! responder alive, exit non-zero (letting dinit's `restart`/
//! `smooth-recovery` bring it back) if the daemon ever reports an error or
//! its event channel closes. No claim logic, no credentials, no key
//! material — mDNS carries reachability only.
//!
//! Three env vars, all optional, all with a documented placeholder default:
//! - `BENIX_MDNS_STATE_DIR` — where the persisted LAN-local id lives
//!   (default `/var/lib/benixos`). See `id.rs` for the id's ruled shape and
//!   lifetime (data-architect Task #29, `benixos.md` §9u).
//! - `BENIX_MDNS_PORT` — the onboarding/connect port carried in the SRV
//!   record (default `8420`, a placeholder — no onboarding endpoint exists
//!   yet, pending gateway-pm's `qr-web` musl spike, §9d).
//! - `BENIX_MDNS_INSTANCE` — override the mDNS instance name (default: this
//!   box's hostname).
//! - `RUST_LOG` — standard `tracing-subscriber` env filter (default `info`).

mod id;

use std::env;
use std::path::PathBuf;

use mdns_sd::{DaemonEvent, ServiceDaemon, ServiceInfo};
use tracing_subscriber::EnvFilter;

const SERVICE_TYPE: &str = "_benixos._tcp.local.";

/// Placeholder onboarding port. No onboarding/connect endpoint is finalized
/// yet (context/projects/benixos.md §9d, gateway-pm's `qr-web` musl
/// feasibility spike still pending a real Linux run) — this is a stand-in so
/// the SRV record has *something* concrete, not a real contract.
const DEFAULT_PORT: u16 = 8420;

/// TXT record schema/protocol version this box speaks. Bump this whenever
/// the TXT record's field set or meaning changes, so a mismatched Courier
/// build can degrade gracefully instead of misparsing.
const PROTOCOL_VERSION: &str = "1";

const DEFAULT_STATE_DIR: &str = "/var/lib/benixos";

fn main() {
    init_tracing();

    let state_dir = PathBuf::from(
        env::var("BENIX_MDNS_STATE_DIR").unwrap_or_else(|_| DEFAULT_STATE_DIR.to_string()),
    );
    let port: u16 = env::var("BENIX_MDNS_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let lan_id = match id::load_or_create(&state_dir) {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(
                error = %e,
                state_dir = %state_dir.display(),
                "failed to load/create the LAN-local id, refusing to advertise"
            );
            std::process::exit(1);
        }
    };

    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "benixos".to_string());
    let instance_name = env::var("BENIX_MDNS_INSTANCE").unwrap_or_else(|_| host.clone());
    let service_hostname = format!("{host}.local.");

    // Exactly three TXT fields, per messaging-architect's ruling — no more
    // without a stated reason. The onboarding port is carried in the SRV
    // record (via `ServiceInfo::new`'s `port` argument below), not here.
    let txt: [(&str, &str); 3] = [
        ("id", lan_id.as_str()),
        // PROVISIONAL / advisory only, never authoritative: benix-claim-agent
        // (Task #23) doesn't exist as code yet, so there's no real claim
        // state to read here. Hardcoded to "not yet claimed" until that's
        // wired up. TODO(benix-claim-agent): replace with a live read of the
        // box's actual fail-closed claim state machine once it exists — this
        // field must remain a coarse UI hint only, never the security gate.
        ("claimed", "0"),
        ("pv", PROTOCOL_VERSION),
    ];

    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "failed to start the mdns-sd responder daemon");
            std::process::exit(1);
        }
    };

    // Empty addrs + enable_addr_auto(): let mdns-sd enumerate the box's own
    // interfaces rather than us hardcoding one, matching the crate's own
    // recommended pattern for a host with more than one interface/address.
    let service_info = match ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &service_hostname,
        "",
        port,
        &txt[..],
    ) {
        Ok(info) => info.enable_addr_auto(),
        Err(e) => {
            tracing::error!(error = %e, "failed to construct the mDNS ServiceInfo");
            std::process::exit(1);
        }
    };
    let fullname = service_info.get_fullname().to_string();

    let monitor = match daemon.monitor() {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(error = %e, "failed to attach to the mdns-sd daemon's event monitor");
            std::process::exit(1);
        }
    };

    if let Err(e) = daemon.register(service_info) {
        tracing::error!(error = %e, service = %fullname, "failed to register the mDNS service");
        std::process::exit(1);
    }

    tracing::info!(
        service = %fullname,
        port,
        lan_id = %lan_id,
        claimed = "0",
        pv = PROTOCOL_VERSION,
        "advertising _benixos._tcp — awaiting Courier discovery"
    );

    // Block on the daemon's own event channel rather than a bare sleep loop
    // (same idiom as mdns-sd's own `register` example): a real error (e.g.
    // the responder's socket dying) should exit non-zero so dinit's
    // `restart = true` / `smooth-recovery = true` (matching qr-gateway's and
    // dockerd's plain `process`-type units) actually re-registers the
    // service, instead of this process idling forever having silently gone
    // deaf.
    while let Ok(event) = monitor.recv() {
        match event {
            DaemonEvent::Error(e) => {
                tracing::error!(error = %e, "mdns-sd daemon reported an error, exiting for dinit to restart us");
                std::process::exit(1);
            }
            other => tracing::debug!(event = ?other, "mdns-sd daemon event"),
        }
    }

    tracing::error!("mdns-sd daemon event channel closed, exiting for dinit to restart us");
    std::process::exit(1);
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
