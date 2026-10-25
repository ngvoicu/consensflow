//! A line of the transcript torn and passed over (T-223, T-248): the reading
//! says where, by the place among the records the line stood at, so that what
//! reads it (a result, the window's activity) can say a record was lost.

use super::*;

#[test]
fn a_reading_past_a_torn_line_says_where_it_was_and_reads_what_follows() {
    let mut stage = Stage::new();
    stage.write(&[hello(), answer("a1", "u1")]);
    let first = stage.reader.look(&Options::default(), 0);
    assert!(matches!(&*first, Reading::Known(record) if record.lines.lost.is_empty()));
    // A short write left half of a record, and the next append made one line of it and itself.
    let half = Stage::lines(&[duration("d1", "a1")]);
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(stage.file())
        .unwrap();
    std::io::Write::write_all(&mut file, &half.as_bytes()[..half.len() / 2]).unwrap();
    drop(file);
    stage.append(&[duration("d1", "a1"), duration("d2", "a1")]);
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => {
            assert_eq!(record.lines.lost, [2]);
            // Each item with its record's place: the user's at 0, the answer at 1.
            assert_eq!(record.lines.places, Some(vec![0, 1]));
            assert_eq!(
                record.settlement,
                Settlement::Settled,
                "the record after it read"
            );
        }
        Reading::Unknown(reason) => panic!("unknown: {reason}"),
    }
}
