//! Questions with options, as a harness's own question tool asks them, and
//! their answers by choice: the checks that take them in and the text an inbox
//! or a window shows for them. Both come as JSON from a harness's door, and are
//! read as JavaScript read them.

use cf_base::js;
use cf_base::text::utf16_len;
use cf_proto::ledger::{Question, QuestionOption};
use serde_json::Value;

use crate::model::{self, LedgerError, MAX_BODY, MAX_TITLE};

const MAX_QUESTIONS: usize = 4;

fn bad_questions(why: &str) -> LedgerError {
    LedgerError::refused("bad-questions", format!("questions: {why}"))
}

/// A header or an option's label: what a picker shows, short.
fn short_text(value: Option<&Value>, field: &str) -> Result<String, LedgerError> {
    match value.and_then(Value::as_str) {
        Some(text) if !js::trim(text).is_empty() && utf16_len(text) <= MAX_TITLE * 10 => {
            Ok(js::trim(text).to_string())
        }
        _ => Err(bad_questions(&format!("{field} is a short text"))),
    }
}

/// Questions with options as a harness's question tool asks them: one to
/// four, each with its text, a short header, its options (a label, maybe a
/// description) and whether several may be picked.
pub(crate) fn require_questions(questions: &Value) -> Result<Vec<Question>, LedgerError> {
    let Some(questions) = questions
        .as_array()
        .filter(|questions| (1..=MAX_QUESTIONS).contains(&questions.len()))
    else {
        return Err(bad_questions(&format!("one to {MAX_QUESTIONS} questions")));
    };
    questions
        .iter()
        .map(|question| {
            let Some(options) = question.get("options").and_then(Value::as_array) else {
                return Err(bad_questions(
                    "each question is an object with an options array",
                ));
            };
            // The question itself may run as long as any message; its header
            // and labels are what a picker shows, and stay short.
            let text = question
                .get("question")
                .and_then(Value::as_str)
                .map(js::trim)
                .filter(|text| !text.is_empty())
                .ok_or_else(|| bad_questions("each question has its text"))?;
            model::require_text(text, "question", MAX_BODY)?;
            Ok(Question {
                question: text.to_string(),
                header: short_text(question.get("header"), "header")?,
                options: options
                    .iter()
                    .map(|option| {
                        Ok(QuestionOption {
                            label: short_text(option.get("label"), "an option label")?,
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .map(js::trim)
                                .filter(|description| !description.is_empty())
                                .map(str::to_string),
                        })
                    })
                    .collect::<Result<_, LedgerError>>()?,
                multiple: question.get("multiple") == Some(&Value::Bool(true)),
            })
        })
        .collect()
}

/// A question with options as text: what an inbox or a window shows.
pub(crate) fn render_questions(questions: &[Question]) -> String {
    questions
        .iter()
        .map(|question| {
            std::iter::once(format!("{}: {}", question.header, question.question))
                .chain(
                    question
                        .options
                        .iter()
                        .map(|option| match &option.description {
                            None => format!("- {}", option.label),
                            Some(description) => format!("- {}: {description}", option.label),
                        }),
                )
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn bad_choices(why: &str) -> LedgerError {
    LedgerError::refused("bad-choices", format!("answer: {why}"))
}

/// The choices for a question with options: one list of picks per question,
/// from explicit `choices` or from the text of `body`, the labels matched
/// regardless of case and free text kept as it is. An ask of several questions
/// is answered one line each. An ask of one question is answered by any text:
/// a label, or the answer's own words in as many lines as they take (a bulleted
/// list is one answer, not one answer per bullet: T-637, the field report of
/// 2026-10-26, where a chief's heredoc of several lines was refused).
pub(crate) fn require_choices(
    questions: &[Question],
    choices: Option<&Value>,
    body: Option<&Value>,
) -> Result<Vec<Vec<String>>, LedgerError> {
    let picks: Vec<Value> = match choices {
        Some(Value::Array(picks)) => picks.clone(),
        Some(_) => return Err(one_answer_each(questions)),
        None => {
            let text = match body {
                None | Some(Value::Null) => String::new(),
                body => js::text(body).into_owned(),
            };
            let lines: Vec<&str> = text
                .split('\n')
                .map(js::trim)
                .filter(|line| !line.is_empty())
                .collect();
            if questions.len() == 1 && lines.len() > 1 {
                // The one question's answer, whole: its lines are not answers of their own.
                vec![Value::from(vec![js::trim(&text)])]
            } else {
                lines
                    .into_iter()
                    .enumerate()
                    .map(|(at, line)| {
                        if questions.get(at).is_some_and(|question| question.multiple) {
                            Value::from(line.split(',').collect::<Vec<_>>())
                        } else {
                            Value::from(vec![line])
                        }
                    })
                    .collect()
            }
        }
    };
    if picks.len() != questions.len() {
        return Err(one_answer_each(questions));
    }
    picks
        .iter()
        .zip(questions)
        .map(|(pick, question)| {
            let pick = pick
                .as_array()
                .filter(|pick| !pick.is_empty() && (question.multiple || pick.len() == 1))
                .ok_or_else(|| {
                    bad_choices(&format!(
                        "{}: {}",
                        question.header,
                        if question.multiple {
                            "one or more picks"
                        } else {
                            "one pick"
                        }
                    ))
                })?;
            pick.iter()
                .map(|text| {
                    let wanted = js::trim(&js::text(Some(text))).to_string();
                    if wanted.is_empty() {
                        return Err(bad_choices("empty pick"));
                    }
                    // A pick in the human's own words ("Something else") is an answer like any other.
                    if utf16_len(&wanted) > MAX_BODY {
                        return Err(bad_choices(&format!(
                            "pick too long (at most {MAX_BODY} characters)"
                        )));
                    }
                    let lower = wanted.to_lowercase();
                    Ok(question
                        .options
                        .iter()
                        .find(|option| option.label.to_lowercase() == lower)
                        .map_or(wanted, |option| option.label.clone()))
                })
                .collect()
        })
        .collect()
}

/// The refusal of an answer that does not answer each question once: it says how
/// many questions the ask has and how they are answered.
fn one_answer_each(questions: &[Question]) -> LedgerError {
    bad_choices(&match questions.len() {
        1 => "this ask has 1 question; give one answer to it, a label or your own words (any number of lines)".to_owned(),
        count => format!(
            "this ask has {count} questions; give one answer to each, one line each in the order asked (a label or your own words)"
        ),
    })
}

/// An answer by choice as text: each question's header and its picks.
pub(crate) fn render_choices(questions: &[Question], choices: &[Vec<String>]) -> String {
    questions
        .iter()
        .zip(choices)
        .map(|(question, picks)| format!("{}: {}", question.header, picks.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn asked() -> Vec<Question> {
        require_questions(&json!([
            { "question": " Which way? ", "header": "Way", "options": [
                { "label": "Left", "description": " the short one " },
                { "label": "Right", "description": "  " },
            ] },
            { "question": "Which colours?", "header": "Colours", "multiple": true, "options": [
                { "label": "Red" }, { "label": "Blue" },
            ] },
        ]))
        .unwrap()
    }

    #[test]
    fn takes_questions_in_trimmed_with_descriptions_that_say_something() {
        let questions = asked();
        assert_eq!(questions[0].question, "Which way?");
        assert_eq!(
            questions[0].options[0].description.as_deref(),
            Some("the short one")
        );
        assert_eq!(questions[0].options[1].description, None);
        assert!(questions[1].multiple);
        assert_eq!(
            render_questions(&questions),
            "Way: Which way?\n- Left: the short one\n- Right\n\nColours: Which colours?\n- Red\n- Blue"
        );
    }

    #[test]
    fn refuses_questions_as_javascript_did() {
        let refused = |value: Value| require_questions(&value).unwrap_err().to_string();
        assert_eq!(refused(json!([])), "questions: one to 4 questions");
        assert_eq!(
            refused(json!([{ "question": "q" }])),
            "questions: each question is an object with an options array"
        );
        assert_eq!(
            refused(json!([{ "question": " ", "options": [] }])),
            "questions: each question has its text"
        );
        assert_eq!(
            refused(json!([{ "question": "q", "header": "h", "options": [null] }])),
            "questions: an option label is a short text"
        );
    }

    #[test]
    fn reads_choices_from_lines_matching_labels_whatever_their_case() {
        let questions = asked();
        let body = json!("left\nred, BLUE,green");
        let picks = require_choices(&questions, None, Some(&body)).unwrap();
        assert_eq!(picks, [vec!["Left"], vec!["Red", "Blue", "green"]]);
        assert_eq!(
            render_choices(&questions, &picks),
            "Way: Left\nColours: Red, Blue, green"
        );
    }

    /// An ask of one question with options, as a chief's `cf answer` meets it.
    fn asked_once() -> Vec<Question> {
        require_questions(&json!([
            { "question": "Go on?", "header": "Go", "options": [
                { "label": "Approve live tests" },
                { "label": "Offline preparation only" },
            ] },
        ]))
        .unwrap()
    }

    #[test]
    fn a_text_of_several_lines_is_the_one_answer_to_an_ask_of_one_question() {
        let questions = asked_once();
        let words =
            "Approve live tests, but:\n- only on the cheap models\n- stop at the first red\n";
        let body = json!(words);
        let picks = require_choices(&questions, None, Some(&body)).unwrap();
        assert_eq!(
            picks,
            [vec![
                "Approve live tests, but:\n- only on the cheap models\n- stop at the first red"
            ]],
            "the lines are not answers of their own: the whole text is one"
        );
        assert_eq!(
            render_choices(&questions, &picks),
            "Go: Approve live tests, but:\n- only on the cheap models\n- stop at the first red"
        );
        // A label alone is still the label, matched whatever its case.
        let body = json!(" offline preparation ONLY \n");
        let picks = require_choices(&questions, None, Some(&body)).unwrap();
        assert_eq!(picks, [vec!["Offline preparation only"]]);
    }

    #[test]
    fn a_text_of_several_lines_to_an_ask_of_several_questions_is_still_one_line_each() {
        let questions = asked();
        let body = json!("left\nred\nblue");
        let refused = require_choices(&questions, None, Some(&body)).unwrap_err();
        assert_eq!(
            refused.to_string(),
            "answer: this ask has 2 questions; give one answer to each, one line each in the order asked (a label or your own words)"
        );
    }

    #[test]
    fn an_ask_of_one_question_with_no_words_says_how_it_is_answered() {
        let questions = asked_once();
        for body in [json!(""), json!(" \n ")] {
            let refused = require_choices(&questions, None, Some(&body)).unwrap_err();
            assert_eq!(
                refused.to_string(),
                "answer: this ask has 1 question; give one answer to it, a label or your own words (any number of lines)"
            );
        }
    }

    #[test]
    fn refuses_choices_that_do_not_answer_each_question_once() {
        let questions = asked();
        let refused = |choices: Value| {
            require_choices(&questions, Some(&choices), None)
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            refused(json!([["Left"]])),
            "answer: this ask has 2 questions; give one answer to each, one line each in the order asked (a label or your own words)"
        );
        assert_eq!(
            refused(json!([["Left", "Right"], ["Red"]])),
            "answer: Way: one pick"
        );
        assert_eq!(refused(json!([["Left"], [" "]])), "answer: empty pick");
    }
}
