//! The staff's roles as the page reads them: one field, `roles`, of `harnesses.missing`,
//! the answer the page reads as it starts and again whenever a picker opens, in the roles'
//! table's order (`cf_ledger::roles`), so the page holds no list of its own. The page's
//! tests read the same field from `app/tests/fixtures/roles.json`, which this test holds to
//! the table: `CF_WRITE_PAGE_ROLES=1` writes it.

use std::path::PathBuf;

use super::*;

/// What `harnesses.missing` answers of the roles.
async fn roles_answered() -> Value {
    let rig = rig();
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let answer = ask(&app, "harnesses.missing", json!({})).await;
    assert_eq!(answer["ok"], true, "{answer}");
    answer["roles"].clone()
}

#[tokio::test]
async fn the_page_reads_the_roles_from_the_table_in_its_order_and_its_tests_read_the_same() {
    LocalSet::new()
        .run_until(async {
            let roles = roles_answered().await;
            assert_eq!(
                roles[8],
                json!({ "id": "designer", "name": "image designer", "label": "Image designer",
                    "article": "an", "tiered": false, "imageAgent": true })
            );
            assert_eq!(
                roles[1],
                json!({ "id": "frontend", "name": "frontend designer", "label": "Frontend designer",
                    "article": "a", "tiered": false, "imageAgent": false })
            );
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../app/tests/fixtures/roles.json");
            let text = format!("{}\n", serde_json::to_string_pretty(&roles).unwrap());
            if cf_base::env::Env::from_process()
                .text("CF_WRITE_PAGE_ROLES")
                .is_some()
            {
                std::fs::write(&path, &text).unwrap();
            }
            assert!(
                std::fs::read_to_string(&path).unwrap_or_default() == text,
                "{} is not the roles' table as the page reads it: CF_WRITE_PAGE_ROLES=1 writes it",
                path.display()
            );
        })
        .await;
}

/// A sixth role, put in the table for this test alone, is in the page's list in its place.
#[tokio::test]
async fn a_role_in_the_table_alone_is_in_the_pages_list_in_its_place() {
    LocalSet::new()
        .run_until(async {
            let _scout = cf_ledger::roles::seam::put(&cf_ledger::roles::seam::WITH_A_SCOUT);
            let roles = roles_answered().await;
            let ids: Vec<&str> = roles
                .as_array()
                .unwrap()
                .iter()
                .map(|role| role["id"].as_str().unwrap())
                .collect();
            assert_eq!(
                ids,
                ["worker", "advisor", "reviewer", "scout", "tester", "designer"]
            );
            assert_eq!(
                roles[3],
                json!({ "id": "scout", "name": "field scout", "label": "Field scout",
                    "article": "a", "tiered": false, "imageAgent": false })
            );
        })
        .await;
}
