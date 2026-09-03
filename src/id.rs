//! Persisted LAN-local identifier for this box's mDNS TXT record.
//!
//! **This is now the ruled shape, not a placeholder** — `data-architect`'s
//! Task #29 ruling (`context/projects/benixos.md` §9u, 2026-09-02) settled
//! the open question §9c routed here. The ruling, in short:
//!
//! - The `id` is a **per-install, opaque, random 128-bit token** — a
//!   discovery *handle*, not an identity. UUIDv4 is the ruled encoding
//!   (122 bits of randomness, collision-safe on a LAN, `/etc/machine-id`
//!   class). It carries **no** relationship, derivable or otherwise, to the
//!   fabric `device_id` — broadcasting that in clear multicast would hand
//!   every LAN observer the box's permanent global fabric identity
//!   (cross-domain correlation), so it stays excluded. Deriving the id from
//!   the fabric id was rejected as *structurally impossible* anyway: a
//!   clean-install box advertises while unclaimed, before any fabric
//!   `device_id` exists — that identity is minted during the `TPair` claim
//!   this discovery bootstraps.
//! - **Lifetime: stable for one claim/install, regenerated on every
//!   ownership-lifecycle boundary** (factory reset, unclaim/re-claim) — the
//!   `/etc/machine-id` *lifetime*, not persist-forever. Carrying it across a
//!   reset/unclaim would let a prior owner recognize the box under new
//!   ownership. Within a claim it must stay stable (reboots, responder
//!   restarts, IP changes) so Courier recognizes "the same box" across a
//!   re-scan or a mid-pairing restart.
//!
//! `load_or_create` below already implements the correct *generation and
//! per-install persistence* (regenerate when the file is absent, stable
//! while present). The rotation-on-boundary half is a **contract on the
//! reset/unclaim paths, not on this crate**: a factory reset MUST wipe
//! `<state_dir>/mdns-id` (wiping `/var/lib/benixos` satisfies it for free),
//! and `benix-claim-agent`'s unclaim path MUST delete `<state_dir>/mdns-id`
//! so the next claim advertises a fresh handle. Those paths don't exist yet
//! (see §9u's routed open items); this module needs no change for them —
//! it regenerates automatically once the file is gone.

use std::fs;
use std::io;
use std::path::Path;

use uuid::Uuid;

const ID_FILENAME: &str = "mdns-id";

/// Loads the persisted LAN-local id from `<state_dir>/mdns-id`, generating
/// and persisting a new one on first run. `state_dir` is created if it
/// doesn't exist.
pub fn load_or_create(state_dir: &Path) -> io::Result<String> {
    let id_path = state_dir.join(ID_FILENAME);

    if let Ok(existing) = fs::read_to_string(&id_path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
        // Fall through and regenerate: an empty file is treated the same as
        // a missing one rather than advertised as-is.
    }

    fs::create_dir_all(state_dir)?;

    let id = Uuid::new_v4().to_string();

    // Write-then-rename so a crash mid-write can never leave a
    // truncated/partial id file for the next boot to read back.
    let tmp_path = state_dir.join(format!(".{ID_FILENAME}.tmp"));
    fs::write(&tmp_path, &id)?;
    fs::rename(&tmp_path, &id_path)?;

    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_and_persists_on_first_run() {
        let dir = std::env::temp_dir().join(format!("benix-mdns-id-test-{}", Uuid::new_v4()));
        let id_first = load_or_create(&dir).expect("first load should create an id");
        assert!(!id_first.is_empty());

        let id_second = load_or_create(&dir).expect("second load should read the persisted id");
        assert_eq!(id_first, id_second, "id must be stable across restarts");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn treats_empty_file_as_missing() {
        let dir = std::env::temp_dir().join(format!("benix-mdns-id-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(ID_FILENAME), "").unwrap();

        let id = load_or_create(&dir).expect("should regenerate over an empty file");
        assert!(!id.is_empty());

        fs::remove_dir_all(&dir).ok();
    }
}
