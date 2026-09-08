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
    pub(super) fn parse(text: &str, width: u32, height: u32) -> Result<Self, Reason> {
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

    pub(crate) fn contains_marker(&self, marker: &str) -> bool {
        self.words.iter().any(|word| word.text.contains(marker))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_rejects_ambiguous_phrases_and_out_of_image_boxes() {
        let page =
            Page::parse("5\t1\t1\t1\t1\t1\t10\t10\t20\t10\t95\tMessage\n", 100, 100).unwrap();
        assert!(page.find_phrase("Message").is_some());
        assert!(!page.contains_marker("NAN_CHECK_FINAL_abc"));
        let duplicate = "5\t1\t1\t1\t1\t1\t10\t10\t20\t10\t95\tMessage\n".repeat(2);
        assert!(
            Page::parse(&duplicate, 100, 100)
                .unwrap()
                .find_phrase("Message")
                .is_none()
        );
        assert!(Page::parse("5\t1\t1\t1\t1\t1\t99\t10\t20\t10\t95\tMessage\n", 100, 100).is_err());
    }
}
