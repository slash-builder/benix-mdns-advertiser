//! Persisted LAN-local identifier for this box's mDNS TXT record.
//!
//! **Provisional, not a final design.** `context/projects/benixos.md` §9c
//! (messaging-architect's ruling) is explicit that the TXT `id` field must
//! NOT be the raw fabric `device_id` — broadcasting that in clear multicast
//! is a linkability/tracking beacon. The real shape of a LAN-local
//! identifier decoupled from the fabric ID is routed to `data-architect`
//! (Task #29), unstarted as of this writing. Until that ruling lands, this
//! module uses a reasonable machine-id-style placeholder: a UUIDv4
//! generated once at first run and persisted to disk, the same pattern
//! `/etc/machine-id` uses elsewhere. **Do not treat this as the final `id`
//! shape** — it exists so the advertiser has something concrete to ship
//! today, not as this crate unilaterally deciding data-architect's call.

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
