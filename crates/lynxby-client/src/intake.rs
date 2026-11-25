//! An assistant's chat form, asked one question at a time before the first
//! message. Mirrors `resources/js/intake/runner.ts`; the deployment checks the
//! answers again when they arrive with the first message, and its check wins.

use std::collections::HashMap;

use crate::types::{IntakeField, IntakeForm};

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Text(String),
    Choices(Vec<String>),
    Agreed,
    Skipped,
}

impl Answer {
    fn picked(&self) -> Vec<&str> {
        match self {
            Self::Text(text) => vec![text.as_str()],
            Self::Choices(choices) => choices.iter().map(String::as_str).collect(),
            _ => Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.trim().is_empty(),
            Self::Choices(choices) => choices.is_empty(),
            Self::Agreed => false,
            Self::Skipped => true,
        }
    }
}

/// Why an answer was refused, for the screen to say in the person's language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Required,
    Email,
    Phone,
    Number,
    Date,
    TooLong(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntakeRun {
    pub form: IntakeForm,
    answers: HashMap<String, Answer>,
    /// The field being asked, or None once every visible field is answered.
    current: Option<usize>,
    pub problem: Option<Problem>,
}

impl IntakeRun {
    pub fn new(form: IntakeForm) -> Self {
        let mut run = Self {
            form,
            answers: HashMap::new(),
            current: None,
            problem: None,
        };
        run.current = run.visible().first().copied();
        run
    }

    pub fn current(&self) -> Option<&IntakeField> {
        self.current.map(|index| &self.form.fields[index])
    }

    pub fn is_done(&self) -> bool {
        self.current.is_none()
    }

    /// 1-based position of the current question and how many are shown.
    pub fn progress(&self) -> (usize, usize) {
        let visible = self.visible();
        let position = self
            .current
            .and_then(|index| visible.iter().position(|candidate| *candidate == index))
            .map_or(visible.len(), |position| position + 1);

        (position, visible.len())
    }

    pub fn answer_of(&self, key: &str) -> Option<&Answer> {
        self.answers.get(key)
    }

    /// The questions already answered, in order, with what was said.
    pub fn answered(&self) -> Vec<(&IntakeField, &Answer)> {
        self.visible()
            .into_iter()
            .take_while(|index| Some(*index) != self.current)
            .filter_map(|index| {
                let field = &self.form.fields[index];
                self.answers.get(&field.key).map(|answer| (field, answer))
            })
            .collect()
    }

    pub fn can_go_back(&self) -> bool {
        match self.current {
            Some(index) => self.before(index).is_some(),
            None => !self.visible().is_empty(),
        }
    }

    /// Checks and keeps the answer, then moves on. A refused answer stays on
    /// the question with its problem set.
    pub fn answer(&mut self, answer: Answer) -> bool {
        let Some(index) = self.current else {
            return false;
        };
        let field = &self.form.fields[index];

        if let Some(problem) = check(field, &answer) {
            self.problem = Some(problem);
            return false;
        }

        let answer = if answer.is_empty() {
            Answer::Skipped
        } else {
            answer
        };

        self.answers.insert(field.key.clone(), answer);
        self.problem = None;
        self.current = self.after(index);
        true
    }

    pub fn skip(&mut self) -> bool {
        match self.current() {
            Some(field) if !field.required => self.answer(Answer::Skipped),
            _ => false,
        }
    }

    pub fn back(&mut self) {
        self.problem = None;
        self.current = match self.current {
            Some(index) => self.before(index).or(Some(index)),
            None => self.visible().last().copied(),
        };
    }

    /// The answers the deployment expects with the first message, by field
    /// key. Hidden and skipped fields are left out.
    pub fn payload(&self) -> serde_json::Value {
        let mut sent = serde_json::Map::new();

        for index in self.visible() {
            let field = &self.form.fields[index];
            let value = match self.answers.get(&field.key) {
                Some(Answer::Text(text)) => serde_json::json!(text.trim()),
                Some(Answer::Choices(choices)) => serde_json::json!(choices),
                Some(Answer::Agreed) => serde_json::json!(true),
                _ => continue,
            };
            sent.insert(field.key.clone(), value);
        }

        serde_json::Value::Object(sent)
    }

    fn shown(&self, field: &IntakeField) -> bool {
        let Some(condition) = &field.condition else {
            return true;
        };

        self.answers.get(&condition.field).is_some_and(|answer| {
            answer
                .picked()
                .iter()
                .any(|value| condition.one_of.iter().any(|wanted| wanted == value))
        })
    }

    fn visible(&self) -> Vec<usize> {
        (0..self.form.fields.len())
            .filter(|index| self.shown(&self.form.fields[*index]))
            .collect()
    }

    fn after(&self, index: usize) -> Option<usize> {
        self.visible()
            .into_iter()
            .find(|candidate| *candidate > index)
    }

    fn before(&self, index: usize) -> Option<usize> {
        self.visible()
            .into_iter()
            .rev()
            .find(|candidate| *candidate < index)
    }
}

/// The same rules as `checkAnswer()` in the web runner.
pub fn check(field: &IntakeField, answer: &Answer) -> Option<Problem> {
    if answer.is_empty() {
        return field.required.then_some(Problem::Required);
    }

    let Answer::Text(text) = answer else {
        return None;
    };
    let text = text.trim();
    let max = field.max.unwrap_or(2000);

    let valid = match field.kind.as_str() {
        "email" => is_email(text),
        "phone" => is_phone(text),
        "number" => text.parse::<f64>().is_ok(),
        "date" => is_date(text),
        _ => true,
    };

    if !valid {
        return Some(match field.kind.as_str() {
            "email" => Problem::Email,
            "phone" => Problem::Phone,
            "number" => Problem::Number,
            _ => Problem::Date,
        });
    }

    (text.chars().count() > max).then_some(Problem::TooLong(max))
}

fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    let Some((name, tld)) = domain.rsplit_once('.') else {
        return false;
    };

    [local, name, tld]
        .iter()
        .all(|part| !part.is_empty() && !part.contains(char::is_whitespace) && !part.contains('@'))
}

fn is_phone(text: &str) -> bool {
    let rest = text.strip_prefix('+').unwrap_or(text);

    (6..=30).contains(&rest.len())
        && rest
            .chars()
            .all(|character| character.is_ascii_digit() || " ().-".contains(character))
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();

    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IntakeCondition;

    fn field(key: &str, kind: &str, required: bool) -> IntakeField {
        IntakeField {
            key: key.to_string(),
            kind: kind.to_string(),
            label: key.to_string(),
            required,
            ..IntakeField::default()
        }
    }

    fn form() -> IntakeForm {
        IntakeForm {
            fields: vec![
                IntakeField {
                    options: vec!["Tax".into(), "Law".into()],
                    ..field("topic", "single_choice", true)
                },
                IntakeField {
                    condition: Some(IntakeCondition {
                        field: "topic".into(),
                        one_of: vec!["Tax".into()],
                    }),
                    ..field("year", "number", false)
                },
                field("email", "email", true),
                field("consent", "consent", false),
            ],
        }
    }

    #[test]
    fn a_condition_hides_a_question_until_its_answer_is_picked() {
        let mut run = IntakeRun::new(form());
        assert_eq!(run.progress(), (1, 3));

        run.answer(Answer::Text("Law".into()));
        assert_eq!(run.current().unwrap().key, "email");

        run.back();
        run.answer(Answer::Text("Tax".into()));
        assert_eq!(run.current().unwrap().key, "year");
        assert_eq!(run.progress(), (2, 4));
    }

    #[test]
    fn a_required_question_cannot_be_skipped_and_a_bad_answer_stays() {
        let mut run = IntakeRun::new(form());

        assert!(!run.skip());
        assert!(!run.answer(Answer::Text(String::new())));
        assert_eq!(run.problem, Some(Problem::Required));

        run.answer(Answer::Text("Law".into()));
        assert!(!run.answer(Answer::Text("not an address".into())));
        assert_eq!(run.problem, Some(Problem::Email));
        assert_eq!(run.current().unwrap().key, "email");
    }

    #[test]
    fn the_payload_leaves_out_hidden_and_skipped_answers() {
        let mut run = IntakeRun::new(form());

        run.answer(Answer::Text("Tax".into()));
        run.answer(Answer::Text("2025".into()));
        run.back();
        run.back();
        run.answer(Answer::Text("Law".into()));
        run.answer(Answer::Text(" ada@example.com ".into()));
        assert!(run.skip());

        assert!(run.is_done());
        assert_eq!(
            run.payload(),
            serde_json::json!({"topic": "Law", "email": "ada@example.com"})
        );
        assert_eq!(run.answered().len(), 3);
    }

    #[test]
    fn consent_and_multiple_choice_are_sent_in_their_own_shape() {
        let mut run = IntakeRun::new(IntakeForm {
            fields: vec![
                IntakeField {
                    options: vec!["A".into(), "B".into()],
                    ..field("pick", "multiple_choice", true)
                },
                field("ok", "consent", true),
            ],
        });

        run.answer(Answer::Choices(vec!["A".into(), "B".into()]));
        run.answer(Answer::Agreed);

        assert_eq!(
            run.payload(),
            serde_json::json!({"pick": ["A", "B"], "ok": true})
        );
    }

    #[test]
    fn the_checks_match_the_web_runner() {
        assert_eq!(
            check(
                &field("p", "phone", true),
                &Answer::Text("+31 (0)20-123".into())
            ),
            None
        );
        assert_eq!(
            check(&field("p", "phone", true), &Answer::Text("12".into())),
            Some(Problem::Phone)
        );
        assert_eq!(
            check(&field("n", "number", true), &Answer::Text("4.5".into())),
            None
        );
        assert_eq!(
            check(&field("n", "number", true), &Answer::Text("four".into())),
            Some(Problem::Number)
        );
        assert_eq!(
            check(
                &field("d", "date", true),
                &Answer::Text("2026-11-25".into())
            ),
            None
        );
        assert_eq!(
            check(
                &field("d", "date", true),
                &Answer::Text("25-11-2026".into())
            ),
            Some(Problem::Date)
        );

        let short = IntakeField {
            max: Some(3),
            ..field("s", "single_line", true)
        };
        assert_eq!(
            check(&short, &Answer::Text("four".into())),
            Some(Problem::TooLong(3))
        );
    }
}
