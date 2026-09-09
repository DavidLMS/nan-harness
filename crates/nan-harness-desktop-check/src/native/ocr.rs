use crate::report::Reason;
use xa11y::Rect;

pub(crate) struct Word {
    pub(crate) text: String,
    pub(crate) bounds: Rect,
}

pub(crate) struct Page {
    pub(crate) words: Vec<Word>,
}

impl Page {
    pub(crate) fn parse(text: &str, width: u32, height: u32) -> Result<Self, Reason> {
        let mut words = Vec::new();
        let image = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        for line in text.lines() {
            let fields = line.splitn(12, '\t').collect::<Vec<_>>();
            if fields.first() == Some(&"level") {
                continue;
            }
            if fields.len() != 12 {
                return Err(Reason::ResponseMismatch);
            }
            if fields[0] != "5" {
                continue;
            }
            let bounds = Rect {
                x: fields[6].parse().map_err(|_| Reason::ResponseMismatch)?,
                y: fields[7].parse().map_err(|_| Reason::ResponseMismatch)?,
                width: fields[8].parse().map_err(|_| Reason::ResponseMismatch)?,
                height: fields[9].parse().map_err(|_| Reason::ResponseMismatch)?,
            };
            let confidence: f32 = fields[10].parse().map_err(|_| Reason::ResponseMismatch)?;
            if !confidence.is_finite()
                || !super::window::contains(image, bounds)
                || words.len() >= 8192
            {
                return Err(Reason::ResponseMismatch);
            }
            if confidence >= 50.0 && !fields[11].trim().is_empty() {
                words.push(Word {
                    text: fields[11].to_owned(),
                    bounds,
                });
            }
        }
        Ok(Self { words })
    }

    pub(crate) fn find_phrase(&self, phrase: &str) -> Option<Rect> {
        let expected = phrase.split_whitespace().collect::<Vec<_>>();
        if expected.is_empty() {
            return None;
        }
        let matches = self
            .words
            .windows(expected.len())
            .filter(|words| {
                words
                    .iter()
                    .zip(&expected)
                    .all(|(word, expected)| word.text == *expected)
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return None;
        }
        let words = matches[0];
        let left = words.iter().map(|word| word.bounds.x).min()?;
        let top = words.iter().map(|word| word.bounds.y).min()?;
        let right = words
            .iter()
            .map(|word| i64::from(word.bounds.x) + i64::from(word.bounds.width))
            .max()?;
        let bottom = words
            .iter()
            .map(|word| i64::from(word.bounds.y) + i64::from(word.bounds.height))
            .max()?;
        Some(Rect {
            x: left,
            y: top,
            width: u32::try_from(right - i64::from(left)).ok()?,
            height: u32::try_from(bottom - i64::from(top)).ok()?,
        })
    }

    pub(crate) fn contains_phrase(&self, phrase: &str) -> bool {
        let expected = phrase.split_whitespace().collect::<Vec<_>>();
        !expected.is_empty()
            && self.words.windows(expected.len()).any(|words| {
                words
                    .iter()
                    .zip(&expected)
                    .all(|(word, expected)| word.text == *expected)
            })
    }

    pub(crate) fn contains_marker_above(&self, marker: &str, bottom: i32) -> bool {
        // A unique marker may wrap at the window edge into several OCR words.
        // Preserve every recognized character; never fuzzy-match the nonce.
        let marker = marker.split_whitespace().collect::<String>();
        !marker.is_empty()
            && self
                .words
                .iter()
                .filter(|word| {
                    i64::from(word.bounds.y) + i64::from(word.bounds.height) < i64::from(bottom)
                })
                .map(|word| word.text.as_str())
                .collect::<String>()
                .contains(&marker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_region_accepts_indentation_changes_but_excludes_the_composer() {
        let page = Page::parse(
            "5\t1\t1\t1\t1\t1\t5\t10\t80\t10\t95\tNAN_CHECK_RESPONSE_a\n\
             5\t1\t1\t1\t2\t1\t20\t40\t70\t10\t95\tNAN_CHECK_RESPONSE_b\n",
            100,
            100,
        )
        .unwrap();
        assert!(page.contains_marker_above("NAN_CHECK_RESPONSE_a", 40));
        assert!(!page.contains_marker_above("NAN_CHECK_RESPONSE_b", 40));
        assert!(!page.contains_marker_above("NAN_CHECK_RESPONSE_a", 20));
    }

    #[test]
    fn wrapped_markers_require_every_exact_character() {
        let page = Page::parse(
            "5\t1\t1\t1\t1\t1\t10\t10\t80\t10\t95\tNAN_CHECK_RESPONSE_\n\
             5\t1\t1\t1\t2\t1\t10\t30\t80\t10\t95\t0123456789abcdef\n",
            100,
            100,
        )
        .unwrap();
        assert!(page.contains_marker_above("NAN_CHECK_RESPONSE_0123456789abcdef", 100));
        assert!(!page.contains_marker_above("NAN_CHECK_RESPONSE_0123456789abcdee", 100));
        assert!(!page.contains_marker_above("", 100));
        assert!(!page.contains_marker_above(" \n\t", 100));
    }

    #[test]
    fn word_markers_accept_line_wrapping_but_not_changed_or_missing_words() {
        let page = Page::parse(
            "5\t1\t1\t1\t1\t1\t10\t10\t40\t10\t95\tapple\n\
             5\t1\t1\t1\t2\t1\t10\t30\t40\t10\t95\tbread\n\
             5\t1\t1\t1\t2\t2\t55\t30\t40\t10\t95\tchair\n",
            100,
            100,
        )
        .unwrap();
        assert!(page.contains_marker_above("apple bread chair", 100));
        assert!(!page.contains_marker_above("apple bread dream", 100));
        assert!(!page.contains_marker_above("apple chair", 100));
        assert!(!page.contains_marker_above("apple bread chair", 30));
    }

    #[test]
    fn ocr_rejects_ambiguous_phrases_and_out_of_image_boxes() {
        let page =
            Page::parse("5\t1\t1\t1\t1\t1\t10\t10\t20\t10\t95\tMessage\n", 100, 100).unwrap();
        assert!(page.find_phrase("Message").is_some());
        assert!(!page.contains_marker_above("NAN_CHECK_FINAL_abc", 100));
        let duplicate = "5\t1\t1\t1\t1\t1\t10\t10\t20\t10\t95\tMessage\n".repeat(2);
        assert!(
            Page::parse(&duplicate, 100, 100)
                .unwrap()
                .find_phrase("Message")
                .is_none()
        );
        assert!(Page::parse("5\t1\t1\t1\t1\t1\t99\t10\t20\t10\t95\tMessage\n", 100, 100).is_err());
    }

    #[test]
    fn duplicate_phrases_are_present_even_when_not_safe_click_targets() {
        let page = Page::parse(
            &"5\t1\t1\t1\t1\t1\t10\t10\t20\t10\t95\tMessage\n".repeat(2),
            100,
            100,
        )
        .unwrap();
        assert!(page.find_phrase("Message").is_none());
        assert!(page.contains_phrase("Message"));
        assert!(!page.contains_phrase("Missing"));
        assert!(!page.contains_phrase(""));
    }
}
