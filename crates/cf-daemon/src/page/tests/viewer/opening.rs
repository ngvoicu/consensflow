//! An HTML file of a specs folder opened in the browser, over the bridge: only a
//! regular `.html` or `.htm` file of one name, in the folder and reached through
//! no link, is read, and the opener is handed a copy of what was read, with the
//! assets it names, in ConsensFlow's home: never a path of the project's, which
//! may name something else by the time the browser reads it. Anything else is
//! refused in words, nothing is opened for a page that may have stopped
//! waiting, and the opener's own failure reaches the page. The opener here
//! records what it is asked: no test opens a browser.

use super::*;

impl Asked {
    /// Where the copies for the browser are made.
    fn copies(&self) -> PathBuf {
        self.home.path().join("tmp").join("specs-open")
    }

    /// The text of the file the opener was handed `at` (in order).
    fn opened_text(&self, at: usize) -> String {
        fs::read_to_string(&self.opened.borrow()[at]).unwrap()
    }
}

#[tokio::test]
async fn an_html_file_of_the_folder_is_opened_in_the_browser_and_nothing_else_is() {
    LocalSet::new()
        .run_until(async {
            let held = asked(
                Value::Null,
                &[
                    (".specs/checkout/SPEC.html", "<title>Checkout</title>"),
                    (".specs/checkout/mockup-cart.htm", "<p>cart</p>"),
                    (".specs/checkout/SPEC.md", "# Checkout"),
                    ("secret.html", "<p>outside</p>"),
                ],
            );
            assert_eq!(
                held.open("checkout/SPEC.html").await,
                json!({ "ok": true, "opened": "checkout/SPEC.html" })
            );
            assert_eq!(
                held.open("checkout/mockup-cart.htm").await,
                json!({ "ok": true, "opened": "checkout/mockup-cart.htm" })
            );
            let opened = held.opened.borrow().clone();
            assert_eq!(opened.len(), 2);
            for (file, name) in opened.iter().zip(["SPEC.html", "mockup-cart.htm"]) {
                assert!(file.starts_with(held.copies()), "{}", file.display());
                assert!(file.ends_with(Path::new("checkout").join(name)), "{}", file.display());
            }
            assert_eq!(held.opened_text(0), "<title>Checkout</title>");
            assert_eq!(held.opened_text(1), "<p>cart</p>");
            for (path, words) in [
                (
                    "checkout/SPEC.md",
                    "checkout/SPEC.md is no HTML file: only an .html or .htm file opens in the browser",
                ),
                (
                    "../secret.html",
                    "../secret.html climbs out of the specs folder (..)",
                ),
                (
                    "/etc/hosts.html",
                    "/etc/hosts.html is absolute: a path in the specs folder is relative to it",
                ),
                ("checkout", "checkout is a folder, not a file"),
                ("gone.html", "no file gone.html in the specs folder"),
            ] {
                assert_eq!(
                    held.open(path).await,
                    json!({ "ok": false, "error": words }),
                    "{path}"
                );
            }
            assert_eq!(held.opened.borrow().len(), 2, "nothing else was opened");
            assert_eq!(held.rig.kicks.get(), 0, "opening a file wakes nothing");
        })
        .await;
}

/// The race the copy closes: the browser reads its file after the daemon has
/// checked it, and the file, or the folder it is in, may be swapped for a link
/// out meanwhile. The browser is given the bytes that were checked, with the
/// assets they name, in a folder of the daemon's own: whatever the original
/// becomes after (here, while the opener runs), what is opened is unchanged.
#[tokio::test]
async fn the_browser_is_given_a_copy_of_what_was_checked_whatever_the_original_becomes() {
    LocalSet::new()
        .run_until(async {
            let spec = "<link href=\"../assets/spec-styles.css\"><p>checked</p>";
            let (project, swapped) = (
                Rc::new(RefCell::new(None::<PathBuf>)),
                Rc::new(RefCell::new(None)),
            );
            let (folder, seen) = (Rc::clone(&project), Rc::clone(&swapped));
            let swapping: Opener = Rc::new(move |_, file: PathBuf| {
                let root = folder.borrow().clone().unwrap();
                let specs = root.join(".specs");
                fs::remove_file(specs.join("checkout/SPEC.html")).unwrap();
                fs::write(specs.join("checkout/SPEC.html"), "<p>swapped</p>").unwrap();
                fs::write(specs.join("assets/spec-styles.css"), "swapped {}").unwrap();
                fs::rename(specs.join("checkout"), root.join("moved")).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(root.join("secret"), specs.join("checkout")).unwrap();
                *seen.borrow_mut() = Some((
                    fs::read_to_string(&file).unwrap(),
                    fs::read_to_string(file.parent().unwrap().join("../assets/spec-styles.css"))
                        .unwrap(),
                    file,
                ));
                Box::pin(async { Ok(()) })
            });
            let held = asked_with(
                Value::Null,
                &[
                    (".specs/checkout/SPEC.html", spec),
                    (".specs/assets/spec-styles.css", "p {}"),
                    ("secret/SPEC.html", "<p>TOKEN=1</p>"),
                ],
                |_| Specs::opening_with(swapping),
            );
            *project.borrow_mut() = Some(held.folder.path().to_path_buf());
            assert_eq!(
                held.open("checkout/SPEC.html").await,
                json!({ "ok": true, "opened": "checkout/SPEC.html" })
            );
            let (html, styles, file) = swapped.borrow_mut().take().unwrap();
            assert_eq!(html, spec, "the bytes that were checked");
            assert_eq!(styles, "p {}", "and the assets read with them");
            assert!(file.starts_with(held.copies()), "{}", file.display());
            assert!(!file.starts_with(held.folder.path()));
        })
        .await;
}

/// Reading an HTML file that took longer than the page waits opens nothing:
/// the page may have said it failed, and the browser would open it after.
#[tokio::test]
async fn an_html_file_read_too_late_for_the_page_is_not_opened() {
    LocalSet::new()
        .run_until(async {
            let slow: Arc<dyn Fn() + Send + Sync> =
                Arc::new(|| std::thread::sleep(Duration::from_millis(60)));
            let held = asked_with(
                Value::Null,
                &[(".specs/a/SPEC.html", "<p>a</p>")],
                |specs| {
                    specs
                        .before_each_look(slow)
                        .opening_within(Duration::from_millis(20))
                },
            );
            assert_eq!(
                held.open("a/SPEC.html").await,
                json!({
                    "ok": false,
                    "error": "a/SPEC.html took too long to read: it was not opened, for the page \
                              may have stopped waiting. Open it again",
                })
            );
            assert!(held.opened.borrow().is_empty(), "nothing was opened");
        })
        .await;
}

/// A copy is kept a day, for the browser may read it late; the copies older
/// are removed at the next opening.
#[tokio::test]
async fn the_copies_kept_their_time_are_removed_at_the_next_opening() {
    LocalSet::new()
        .run_until(async {
            let held = asked_with(
                Value::Null,
                &[(".specs/a/SPEC.html", "<p>a</p>")],
                |specs| specs.keeping_copies_for(Duration::ZERO),
            );
            for _ in 0..2 {
                assert_eq!(held.open("a/SPEC.html").await["ok"], true);
            }
            let opened = held.opened.borrow().clone();
            assert!(
                !opened[0].exists(),
                "the first copy, removed at the second opening"
            );
            assert!(opened[1].exists(), "the second, made after");
        })
        .await;
}

/// An agent left the copies' folder a link to a folder of the human's: the
/// opening says it cannot copy, opens nothing, and removes nothing there.
#[cfg(unix)]
#[tokio::test]
async fn a_copies_folder_left_a_link_is_refused_and_nothing_where_it_leads_is_removed() {
    use std::os::unix::fs::symlink;

    LocalSet::new()
        .run_until(async {
            let held = asked(Value::Null, &[(".specs/a/SPEC.html", "<p>a</p>")]);
            let outside = tempfile::tempdir().unwrap();
            let sentinel = outside.path().join("old.txt");
            fs::write(&sentinel, "the human's").unwrap();
            fs::File::options()
                .write(true)
                .open(&sentinel)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_577_836_800))
                .unwrap();
            fs::create_dir_all(held.home.path().join("tmp")).unwrap();
            symlink(outside.path(), held.copies()).unwrap();
            assert_eq!(
                held.open("a/SPEC.html").await,
                json!({
                    "ok": false,
                    "error": format!(
                        "no copy for the browser was made: {} is a link, and no copy is made \
                         where a link leads",
                        held.copies().display()
                    ),
                })
            );
            assert!(held.opened.borrow().is_empty(), "nothing was opened");
            assert!(sentinel.exists(), "the old file where it leads");
            assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
        })
        .await;
}

/// What the system's opener says when it could not open a file reaches the page.
#[tokio::test]
async fn an_opener_that_fails_says_why() {
    LocalSet::new()
        .run_until(async {
            let failing: Opener = Rc::new(|_, file| {
                Box::pin(
                    async move { Err(format!("{} could not be opened: no app", file.display())) },
                )
            });
            let held = asked_with(Value::Null, &[(".specs/a/SPEC.html", "<p>a</p>")], |_| {
                Specs::opening_with(failing)
            });
            let answer = held.open("a/SPEC.html").await;
            assert_eq!(answer["ok"], false);
            assert!(
                answer["error"]
                    .as_str()
                    .unwrap()
                    .ends_with("SPEC.html could not be opened: no app"),
                "{answer}"
            );
        })
        .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_out_a_fifo_and_a_hard_link_are_never_opened_in_the_browser() {
    use nix::sys::stat::Mode as Permissions;
    use nix::unistd::mkfifo;
    use std::os::unix::fs::symlink;

    LocalSet::new()
        .run_until(async {
            let held = asked(
                Value::Null,
                &[
                    (".specs/a/SPEC.html", "<p>a</p>"),
                    ("secret.html", "<p>s</p>"),
                ],
            );
            let specs = held.specs();
            symlink(
                held.folder.path().join("secret.html"),
                specs.join("a/out.html"),
            )
            .unwrap();
            symlink(held.folder.path(), specs.join("a/up")).unwrap();
            mkfifo(
                &specs.join("a/pipe.html"),
                Permissions::S_IRUSR | Permissions::S_IWUSR,
            )
            .unwrap();
            fs::hard_link(
                held.folder.path().join("secret.html"),
                specs.join("a/twin.html"),
            )
            .unwrap();
            for (path, words) in [
                (
                    "a/out.html",
                    "a/out.html is a link: the viewer opens no link",
                ),
                (
                    "a/up/secret.html",
                    "a/up/secret.html goes through a/up, a link: the viewer follows none",
                ),
                ("a/pipe.html", "a/pipe.html is a FIFO, not a regular file"),
                (
                    "a/twin.html",
                    "a/twin.html has 2 names (hard links): the viewer reads only a file of one \
                     name, which no path outside the folder can share",
                ),
            ] {
                assert_eq!(
                    held.open(path).await,
                    json!({ "ok": false, "error": words }),
                    "{path}"
                );
            }
            assert!(held.opened.borrow().is_empty(), "nothing was opened");
        })
        .await;
}
