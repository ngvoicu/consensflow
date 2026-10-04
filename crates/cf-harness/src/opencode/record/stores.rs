//! OpenCode's store, found where it is kept (`openOpencodeDb`,
//! `hosts/lib/completion/opencode.js`).

use std::path::Path;

use cf_base::env::Env;
use cf_base::file::{stat, Identity};

use crate::opencode::paths::stores;
use crate::shared::record::sqlite::Store;

/// Which store a look read: its path, and which file was there. Another
/// file in its place is another store.
pub(super) type Which = (String, Identity);

/// OpenCode's store, opened read-only: the first of its places that holds
/// `session`, else the first there is (whose answer is then that the session
/// is not in it yet), else none. A place with no file, or one that cannot be
/// opened, is passed over; one whose sessions cannot be read holds nothing.
pub(super) fn open(env: &Env, session: &str) -> Result<Option<(Store, Which)>, String> {
    let mut first = None;
    for file in stores(env)? {
        let Ok(found) = stat(Path::new(&file)) else {
            continue;
        };
        let Ok(store) = Store::open(Path::new(&file)) else {
            continue;
        };
        // Node asked outside a transaction; one read is one state either way.
        let holds = store
            .read(|reads| reads.get("select 1 from session where id = ?", [session]))
            .is_ok_and(|found| found.is_some());
        let which = (file, found.identity);
        if holds {
            return Ok(Some((store, which)));
        }
        if first.is_none() {
            first = Some((store, which));
        }
    }
    Ok(first)
}
