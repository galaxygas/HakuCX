/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Engine-independent browser state for HakuCX.
//!
//! Types that cross the UI <-> core IPC boundary derive `ts_rs::TS`; running
//! `cargo test -p haku-core` regenerates their TypeScript definitions in `ui/src/ipc/generated`.

pub mod bookmarks;
pub mod extensions;
pub mod flow;
pub mod history;
pub mod ipc;
pub mod omnibox;
pub mod records;
pub mod selection;
pub mod session;
pub mod settings;
pub mod storage;
pub mod sync;
pub mod themes;
pub mod update;
pub mod vault;

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}
