//! The messages queued, and the removals and take-backs that name them by
//! content, where the envelope Claude Code wraps a message in changes; and
//! the queue's state as the turns that take its messages go by.

use super::*;

/// The assistant's answer `uuid` of the message `id`, which ended its turn.
fn done(uuid: &str, parent: &str, id: &str) -> Value {
    assistant(
        uuid,
        parent,
        json!(id),
        json!([text(json!("Done"))]),
        json!("end_turn"),
    )
}

/// A user's turn that Claude Code says came from its queue.
fn from_queue(uuid: &str, parent: &str, content: &str) -> Value {
    having(
        user(uuid, json!(parent), json!(content)),
        json!({ "promptSource": "queued" }),
    )
}

/// A user's turn, typed.
fn typed(uuid: &str, parent: &str, content: &str) -> Value {
    user(uuid, json!(parent), json!(content))
}

#[test]
fn a_message_waits_until_the_turn_that_takes_it_has_begun_in_the_record() {
    let enqueue = |content: &str| queue("enqueue", json!(content));
    let dequeue = || queue("dequeue", Value::Null);
    let remove = |content: &str| queue("remove", json!(content));
    let pop = |content: &str| queue("popAll", json!(content));
    let (a1, a2) = (done("a1", "u1", "m1"), done("a2", "u2", "m2"));
    let end = |uuid: &str, parent: &str| duration(uuid, parent);
    // Node's answer for each record of a turn: has it settled?
    let cases: Vec<(&str, Vec<Value>, Settlement)> = vec![
        (
            "a message dequeued and begun as a queued prompt is done with",
            vec![
                hello(),
                enqueue("X"),
                a1.clone(),
                dequeue(),
                from_queue("u2", "a1", "X"),
                a2.clone(),
                end("d2", "a2"),
            ],
            Settlement::Settled,
        ),
        (
            "a queued prompt with no dequeue before it leaves its message queued",
            vec![
                hello(),
                enqueue("X"),
                a1.clone(),
                from_queue("u2", "a1", "X"),
                a2.clone(),
                end("d2", "a2"),
            ],
            Settlement::InFlight,
        ),
        (
            "a typed prompt of the queued text takes the queued message",
            vec![
                hello(),
                enqueue("X"),
                a1.clone(),
                typed("u2", "a1", "X"),
                a2.clone(),
                end("d2", "a2"),
            ],
            Settlement::Settled,
        ),
        (
            "a typed prompt of other text leaves it",
            vec![
                hello(),
                enqueue("X"),
                a1.clone(),
                typed("u2", "a1", "Y"),
                a2.clone(),
                end("d2", "a2"),
            ],
            Settlement::InFlight,
        ),
        (
            "a typed prompt takes the message dequeued, whatever its text",
            vec![
                hello(),
                enqueue("X"),
                a1.clone(),
                dequeue(),
                typed("u2", "a1", "Y"),
                a2.clone(),
                end("d2", "a2"),
            ],
            Settlement::Settled,
        ),
        (
            "a dequeue of nothing is a message taken all the same",
            vec![hello(), a1.clone(), dequeue(), end("d1", "a1")],
            Settlement::InFlight,
        ),
        (
            "and the next prompt begins it",
            vec![
                hello(),
                a1.clone(),
                dequeue(),
                typed("u2", "a1", "Z"),
                a2.clone(),
                end("d2", "a2"),
            ],
            Settlement::Settled,
        ),
        (
            "the oldest message is the one dequeued",
            vec![
                hello(),
                enqueue("X"),
                enqueue("Y"),
                a1.clone(),
                dequeue(),
                from_queue("u2", "a1", "X"),
                a2.clone(),
                remove("Y"),
                end("d2", "a2"),
            ],
            Settlement::Settled,
        ),
        (
            "a removal names a message dequeued as well as one queued",
            vec![
                hello(),
                enqueue("X"),
                enqueue("Y"),
                a1.clone(),
                dequeue(),
                remove("X"),
                remove("Y"),
                end("d1", "a1"),
            ],
            Settlement::Settled,
        ),
        (
            "a removal takes one message dequeued and one queued",
            vec![
                hello(),
                enqueue("X"),
                enqueue("X"),
                a1.clone(),
                dequeue(),
                remove("X"),
                end("d1", "a1"),
            ],
            Settlement::Settled,
        ),
        (
            "what was taken back is sent by an answer, or never",
            vec![
                hello(),
                enqueue("X"),
                a1.clone(),
                pop("X"),
                done("a2", "a1", "m2"),
                end("d2", "a2"),
            ],
            Settlement::Settled,
        ),
        (
            "what was taken back waits until an answer",
            vec![hello(), enqueue("X"), a1.clone(), pop("X"), end("d1", "a1")],
            Settlement::InFlight,
        ),
        (
            "what was taken back is begun by a prompt of its text",
            vec![
                hello(),
                enqueue("X"),
                a1.clone(),
                pop("X"),
                typed("u2", "a1", "X"),
                a2.clone(),
                end("d2", "a2"),
            ],
            Settlement::Settled,
        ),
        (
            "what was taken back and never queued is taken back all the same",
            vec![hello(), a1.clone(), pop("W"), end("d1", "a1")],
            Settlement::InFlight,
        ),
        (
            "a message queued after the answer holds the settlement",
            vec![hello(), a1.clone(), enqueue("X"), end("d1", "a1")],
            Settlement::InFlight,
        ),
        (
            "an operation of another name does nothing",
            vec![
                hello(),
                a1.clone(),
                queue("clear", json!("X")),
                end("d1", "a1"),
            ],
            Settlement::Settled,
        ),
    ];
    for (name, records, settlement) in cases {
        assert_eq!(settlement_of(&records), settlement, "{name}");
    }
}

#[test]
fn a_prompt_consumes_the_message_that_was_taken_back_of_its_text() {
    // The next answer would clear what was taken back, so the turn a `/clear`
    // ends says whether the prompt that is its command consumed it. Node's
    // answer for each.
    let enqueue = |content: &str| queue("enqueue", json!(content));
    let pop = |content: &str| queue("popAll", json!(content));
    let clear = command("", "");
    let said = |records: Vec<Value>| {
        let mut all = vec![hello()];
        all.extend(records);
        all.extend([typed("u2", "a1", &clear), output("u2")]);
        settlement_of(&all)
    };
    let a1 = || answer("a1", "u1");
    assert_eq!(
        said(vec![enqueue(&clear), a1(), pop(&clear)]),
        Settlement::Settled
    );
    assert_eq!(
        said(vec![enqueue("X"), a1(), pop("X")]),
        Settlement::InFlight
    );
    assert_eq!(said(vec![enqueue(&clear), a1()]), Settlement::Settled);
    assert_eq!(said(vec![a1()]), Settlement::Settled);
    assert_eq!(said(vec![a1(), pop(&clear)]), Settlement::Settled);
}

#[test]
fn a_message_is_named_by_its_content_with_the_attributes_of_its_envelope_set_aside() {
    // Node's answer for each pair: is the queued message gone once an
    // operation names the second text, so that the turn settles?
    let pairs: &[(&str, &str, bool)] = &[
        // Attributes on one side only, either side, and different on both.
        (r#"<m a="1">x</m>"#, "<m>x</m>", true),
        ("<m>x</m>", r#"<m a="1">x</m>"#, true),
        (r#"<m a="1">x</m>"#, r#"<m b="2">x</m>"#, true),
        ("<m>x", "<m>x", true),
        ("<m a>x<n b>", "<m>x<n b>", true),
        // Only the opening tag's attributes are set aside, never its name or the rest.
        (r#"<m a="1">x</m>"#, r#"<n a="1">x</n>"#, false),
        (r#"<m a="1">x</m>"#, r#"<m a="1">y</m>"#, false),
        ("<M a>x", "<m a>x", false),
        ("<m a>x<n b>", "<m>x<n>", false),
        (r#"plain a="1""#, "plain", false),
        // The tag ends at the first `>`, even one in an attribute's value.
        (r#"<m a=">">x</m>"#, r#"<m>">x</m>"#, true),
        // A name is ASCII letters, digits, `_` and `-`, and begins with a letter.
        ("<a-b_1 k>x", "<a-b_1>x", true),
        ("<é a>x", "<é>x", false),
        // After the first letter too, where JavaScript's `\w` is ASCII: an
        // accented letter, an Arabic-Indic digit, a long s and the Kelvin sign.
        ("<a\u{e9} b>x", "<a\u{e9}>x", false),
        ("<a\u{663} b>x", "<a\u{663}>x", false),
        ("<a\u{17f} b>x", "<a\u{17f}>x", false),
        ("<a\u{212a} b>x", "<a\u{212a}>x", false),
        ("<ab b>x", "<ab>x", true),
        ("<a_ b>x", "<a_>x", true),
        ("<a- b>x", "<a->x", true),
        ("<a-1 b>x", "<a-1>x", true),
        ("<Z9 b>x", "<Z9>x", true),
        ("<a. b>x", "<a.>x", false),
        ("<a:b c>x", "<a:b>x", false),
        ("<1 a>x", "<1>x", false),
        (" <m a>x", " <m>x", false),
        ("<m/>x", "<m a>x", false),
        ("<m a", "<m>", false),
        // JavaScript's white space opens the attributes: a line break, a no-break space and a byte order mark do, U+0085 does not.
        ("<m\n a=\"1\">x</m>", "<m>x</m>", true),
        ("<m\u{a0}a=\"1\">x</m>", "<m>x</m>", true),
        ("<m\u{feff}a=\"1\">x</m>", "<m>x</m>", true),
        ("<m\u{85}a=\"1\">x</m>", "<m>x</m>", false),
    ];
    let items = r#"[["u1","user","Hello",true,0],["m1","assistant","Hi",true,3]]"#;
    for operation in ["remove", "popAll"] {
        for &(queued, named, gone) in pairs {
            let read = once(&[
                hello(),
                queue("enqueue", json!(queued)),
                queue(operation, json!(named)),
                answer("a1", "u1"),
                duration("d1", "a1"),
            ]);
            let expected = if gone {
                settled(items)
            } else {
                in_flight(items)
            };
            assert_eq!(read, expected, "{operation} {queued:?} then {named:?}");
        }
    }
}
