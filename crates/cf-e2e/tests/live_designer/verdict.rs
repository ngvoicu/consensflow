//! What the designer's work is held to, apart from how it was seen: the brief
//! says an image is saved where the task named, and what the task, the result
//! and the designer's window then are is the product's to have done. Every way
//! the work falls short is said, not the first.

use std::ops::RangeInclusive;

use cf_e2e::png;

use crate::report::cut;
use crate::result::Named;

/// What a file of a sane size is: not an empty stub, and not past what any
/// image a tool draws comes to.
const BYTES: RangeInclusive<usize> = 1_024..=64 * 1024 * 1024;

/// What a side of a sane picture is, in pixels.
const SIDE: RangeInclusive<u32> = 16..=16_384;

/// What the run saw of the designer's work.
pub struct Seen<'a> {
    /// Where the brief said to save the image, from the project's folder.
    pub image_at: &'a str,
    /// The file there, if there is one.
    pub image: Option<&'a [u8]>,
    /// What the project's folder holds, for the message of an image that is
    /// not where it should be.
    pub folder: &'a str,
    /// The result the chief was given, and how it names the file.
    pub result: &'a str,
    pub naming: Named,
    /// The task, and its state on the board.
    pub task: i64,
    pub state: &'a str,
    /// Where the designer's window was opened, and whether that is the
    /// project's folder.
    pub opened_in: &'a str,
    pub in_the_project: bool,
    /// Whether the window closed once its result was delivered, and in how many
    /// seconds it was given.
    pub closed: bool,
    pub closing: u64,
}

/// What the designer made.
#[derive(Debug, PartialEq, Eq)]
pub struct Made {
    pub bytes: usize,
    pub picture: png::Header,
}

/// What a file and a picture of the sizes given have wrong with them, if
/// anything: a sentence each.
fn insane(image_at: &str, bytes: usize, picture: &png::Header) -> Vec<String> {
    let mut problems = Vec::new();
    if !BYTES.contains(&bytes) {
        problems.push(format!("{image_at} is {bytes} bytes, not a sane size"));
    }
    if !SIDE.contains(&picture.width) || !SIDE.contains(&picture.height) {
        problems.push(format!(
            "{image_at} is {} by {} pixels, not a sane picture",
            picture.width, picture.height
        ));
    }
    problems
}

/// The work as it was seen: what the designer made, or every way it fell short.
pub fn judge(seen: &Seen) -> Result<Made, Vec<String>> {
    let mut problems = Vec::new();
    let made = match seen.image {
        None => {
            problems.push(format!(
                "no file was saved at {}: {}",
                seen.image_at, seen.folder
            ));
            None
        }
        Some(bytes) => match png::inspect(bytes) {
            Err(why) => {
                problems.push(format!("{} is no whole PNG: {why}", seen.image_at));
                None
            }
            Ok(picture) => {
                problems.extend(insane(seen.image_at, bytes.len(), &picture));
                Some(Made {
                    bytes: bytes.len(),
                    picture,
                })
            }
        },
    };
    if seen.naming == Named::Nowhere {
        problems.push(format!(
            "the result names no path to the image: {}",
            cut(seen.result, 300)
        ));
    }
    if seen.state != "done" {
        problems.push(format!(
            "T-{} is {} on the board, not done",
            seen.task, seen.state
        ));
    }
    if !seen.in_the_project {
        problems.push(format!(
            "the designer's window was opened in {}, not in the project's folder",
            seen.opened_in
        ));
    }
    if !seen.closed {
        problems.push(format!(
            "the designer's window did not close within {} s of its result's delivery",
            seen.closing
        ));
    }
    match made {
        Some(made) if problems.is_empty() => Ok(made),
        _ => Err(problems),
    }
}

#[cfg(test)]
mod tests {
    use cf_e2e::checkout;

    use super::*;

    /// A PNG of the repository's, 128 pixels on a side: a real encoder's work.
    fn picture() -> Vec<u8> {
        std::fs::read(checkout::path("app/src-tauri/icons/128x128.png")).unwrap()
    }

    /// The work as it should be seen: nothing to say against it.
    fn well<'a>(image: &'a [u8]) -> Seen<'a> {
        Seen {
            image_at: "images/honey.png",
            image: Some(image),
            folder: "the folder holds: README.md",
            result: "/project/images/honey.png\nA honey jar.",
            naming: Named::Whole,
            task: 1,
            state: "done",
            opened_in: "/project",
            in_the_project: true,
            closed: true,
            closing: 60,
        }
    }

    fn said(seen: &Seen) -> Vec<String> {
        judge(seen).unwrap_err()
    }

    #[test]
    fn work_that_is_all_the_brief_asked_for_is_what_the_designer_made() {
        let image = picture();
        let made = judge(&well(&image)).unwrap();
        assert_eq!(made.bytes, image.len());
        assert_eq!((made.picture.width, made.picture.height), (128, 128));
        // A result that names the path the task gave is as good for finding the file.
        let relative = Seen {
            naming: Named::Relative,
            ..well(&image)
        };
        assert!(judge(&relative).is_ok());
    }

    #[test]
    fn an_image_that_is_not_there_says_what_the_folder_holds() {
        let seen = Seen {
            image: None,
            ..well(&[])
        };
        assert_eq!(
            said(&seen),
            ["no file was saved at images/honey.png: the folder holds: README.md"]
        );
    }

    #[test]
    fn an_image_that_is_another_kind_of_image_or_cut_short_is_no_whole_png() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F'];
        let cut_short = picture();
        let cases: [(&[u8], &str); 2] = [
            (
                &jpeg,
                "images/honey.png is no whole PNG: it is no PNG: it starts with ff d8",
            ),
            (
                &cut_short[..cut_short.len() - 20],
                "images/honey.png is no whole PNG: ",
            ),
        ];
        for (bytes, said_so) in cases {
            let problems = said(&Seen {
                image: Some(bytes),
                ..well(bytes)
            });
            assert_eq!(problems.len(), 1, "{problems:?}");
            assert!(problems[0].starts_with(said_so), "{problems:?}");
        }
    }

    #[test]
    fn a_file_or_a_picture_of_an_insane_size_is_told() {
        let header = |width, height| png::Header { width, height };
        assert!(insane("i.png", 50_000, &header(1024, 1024)).is_empty());
        assert!(insane("i.png", 1_024, &header(16, 16_384)).is_empty());
        assert_eq!(
            insane("i.png", 100, &header(1024, 1024)),
            ["i.png is 100 bytes, not a sane size"]
        );
        assert_eq!(
            insane("i.png", 65 * 1024 * 1024, &header(1024, 1024)).len(),
            1
        );
        for (width, height) in [(15, 1024), (1024, 15), (16_385, 1024), (1, 1)] {
            assert_eq!(
                insane("i.png", 50_000, &header(width, height)),
                [format!(
                    "i.png is {width} by {height} pixels, not a sane picture"
                )]
            );
        }
    }

    #[test]
    fn a_result_with_no_path_a_task_not_done_a_window_elsewhere_and_one_left_open_are_each_told() {
        let image = picture();
        let cases = [
            (
                Seen {
                    naming: Named::Nowhere,
                    result: "Done.",
                    ..well(&image)
                },
                "the result names no path to the image: Done.",
            ),
            (
                Seen {
                    state: "open",
                    ..well(&image)
                },
                "T-1 is open on the board, not done",
            ),
            (
                Seen {
                    in_the_project: false,
                    opened_in: "/elsewhere",
                    ..well(&image)
                },
                "the designer's window was opened in /elsewhere, not in the project's folder",
            ),
            (
                Seen {
                    closed: false,
                    ..well(&image)
                },
                "the designer's window did not close within 60 s of its result's delivery",
            ),
        ];
        for (seen, expected) in cases {
            assert_eq!(said(&seen), [expected]);
        }
    }

    #[test]
    fn every_way_the_work_falls_short_is_said_together_in_the_order_the_brief_is_read() {
        let seen = Seen {
            image: None,
            naming: Named::Nowhere,
            state: "failed",
            in_the_project: false,
            closed: false,
            ..well(&[])
        };
        let problems = said(&seen);
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert!(problems[0].starts_with("no file was saved"));
        assert!(problems[1].starts_with("the result names no path"));
        assert!(problems[2].starts_with("T-1 is failed"));
        assert!(problems[3].starts_with("the designer's window was opened in"));
        assert!(problems[4].starts_with("the designer's window did not close"));
    }
}
